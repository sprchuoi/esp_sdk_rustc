//! Metrics, memory accounting, fault records and a text dump.
//!
//! The kernel cannot log every event and cannot allocate, so it keeps a small,
//! fixed set of observable state:
//!
//! - [`Metrics`] — named counters (packets, reconnects, sensor reads, …);
//! - [`MemoryTracker`] — per-region used/free with a **low-water mark**, which is
//!   what catches a slow leak that a point-in-time free-heap reading misses;
//! - [`FaultTable`] — deduplicated error codes with first/last timestamp and
//!   count, so a flapping fault shows up as one row, not a log flood;
//! - [`CrashRecord`] — the bytes the panic handler leaves in RTC RAM;
//! - [`Diagnostics`] — the bundle and its [`Diagnostics::dump`] text form.
//!
//! Everything is `core::fmt::Write`-based: no allocation, works over a UART, a
//! defmt sink wrapper or an in-memory buffer in tests.

use core::fmt;

/// Handle to a registered counter.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct MetricId(u8);

impl MetricId {
    /// Raw index.
    #[must_use]
    pub const fn raw(self) -> u8 {
        self.0
    }
}

/// A fixed-capacity table of named counters.
#[derive(Clone)]
pub struct Metrics<const N: usize> {
    names: [&'static str; N],
    values: [u64; N],
    used: usize,
}

impl<const N: usize> Default for Metrics<N> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const N: usize> Metrics<N> {
    /// Create an empty table.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            names: [""; N],
            values: [0; N],
            used: 0,
        }
    }

    /// Register a counter; returns `None` when the table is full.
    pub fn define(&mut self, name: &'static str) -> Option<MetricId> {
        if self.used >= N {
            return None;
        }
        self.names[self.used] = name;
        self.values[self.used] = 0;
        let id = MetricId(self.used as u8);
        self.used += 1;
        Some(id)
    }

    /// Increment a counter by one.
    pub fn incr(&mut self, id: MetricId) {
        self.add(id, 1);
    }

    /// Add `value` to a counter, saturating at `u64::MAX`.
    pub fn add(&mut self, id: MetricId, value: u64) {
        if let Some(slot) = self.values.get_mut(usize::from(id.0)) {
            *slot = slot.saturating_add(value);
        }
    }

    /// Set a counter to an absolute value.
    pub fn set(&mut self, id: MetricId, value: u64) {
        if let Some(slot) = self.values.get_mut(usize::from(id.0)) {
            *slot = value;
        }
    }

    /// Current value.
    #[must_use]
    pub fn get(&self, id: MetricId) -> Option<u64> {
        self.values.get(usize::from(id.0)).copied()
    }

    /// Number of registered counters.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.used
    }

    /// Whether no counter is registered.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.used == 0
    }

    /// Iterate name/value pairs in registration order.
    pub fn iter(&self) -> impl Iterator<Item = (&'static str, u64)> + '_ {
        (0..self.used).map(move |index| (self.names[index], self.values[index]))
    }
}

/// Handle to a registered memory region.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RegionId(u8);

#[derive(Clone)]
struct Region {
    name: &'static str,
    capacity: usize,
    used: usize,
    low_water_free: usize,
}

/// Tracks used/free bytes per memory region (heap, DMA pool, stack, …).
#[derive(Clone)]
pub struct MemoryTracker<const N: usize> {
    regions: [Option<Region>; N],
    used: usize,
}

impl<const N: usize> Default for MemoryTracker<N> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const N: usize> MemoryTracker<N> {
    /// Create an empty tracker.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            regions: [const { None }; N],
            used: 0,
        }
    }

    /// Register a region of `capacity` bytes. Zero capacity is rejected.
    pub fn register(&mut self, name: &'static str, capacity: usize) -> Option<RegionId> {
        if capacity == 0 || self.used >= N {
            return None;
        }
        self.regions[self.used] = Some(Region {
            name,
            capacity,
            used: 0,
            low_water_free: capacity,
        });
        let id = RegionId(self.used as u8);
        self.used += 1;
        Some(id)
    }

    fn region(&self, id: RegionId) -> Option<&Region> {
        self.regions.get(usize::from(id.0)).and_then(Option::as_ref)
    }

    fn region_mut(&mut self, id: RegionId) -> Option<&mut Region> {
        self.regions
            .get_mut(usize::from(id.0))
            .and_then(Option::as_mut)
    }

    /// Reserve `bytes` from a region; returns `false` when it does not fit.
    pub fn take(&mut self, id: RegionId, bytes: usize) -> bool {
        let Some(region) = self.region(id) else {
            return false;
        };
        let Some(used) = region.used.checked_add(bytes) else {
            return false;
        };
        if used > region.capacity {
            return false;
        }
        self.set_used_locked(id, used)
    }

    /// Return `bytes` to a region, saturating at zero usage.
    pub fn give(&mut self, id: RegionId, bytes: usize) {
        let Some(region) = self.region(id) else {
            return;
        };
        let used = region.used.saturating_sub(bytes);
        self.set_used_locked(id, used);
    }

    /// Set absolute usage, updating the low-water mark.
    pub fn set_used(&mut self, id: RegionId, used: usize) -> bool {
        if self.region(id).is_none_or(|region| used > region.capacity) {
            return false;
        }
        self.set_used_locked(id, used)
    }

    fn set_used_locked(&mut self, id: RegionId, used: usize) -> bool {
        let Some(region) = self.region_mut(id) else {
            return false;
        };
        region.used = used;
        let free = region.capacity.saturating_sub(used);
        region.low_water_free = region.low_water_free.min(free);
        true
    }

    /// Free bytes currently available.
    #[must_use]
    pub fn free(&self, id: RegionId) -> Option<usize> {
        self.region(id)
            .map(|region| region.capacity.saturating_sub(region.used))
    }

    /// Lowest free-byte count observed since registration.
    #[must_use]
    pub fn low_water_free(&self, id: RegionId) -> Option<usize> {
        self.region(id).map(|region| region.low_water_free)
    }

    /// Total capacity across regions.
    #[must_use]
    pub fn total_capacity(&self) -> usize {
        (0..self.used)
            .filter_map(|index| self.regions[index].as_ref())
            .map(|region| region.capacity)
            .sum()
    }

    /// Total used bytes across regions.
    #[must_use]
    pub fn total_used(&self) -> usize {
        (0..self.used)
            .filter_map(|index| self.regions[index].as_ref())
            .map(|region| region.used)
            .sum()
    }

    /// Worst health across all regions.
    #[must_use]
    pub fn health(&self) -> Health {
        let mut worst = Health::Ok;
        for index in 0..self.used {
            let Some(region) = self.regions[index].as_ref() else {
                continue;
            };
            let free_percent = region
                .capacity
                .saturating_sub(region.used)
                .saturating_mul(100)
                / region.capacity;
            let health = if free_percent < 5 {
                Health::Critical
            } else if free_percent < 15 {
                Health::Degraded
            } else {
                Health::Ok
            };
            if health > worst {
                worst = health;
            }
        }
        worst
    }

    /// Iterate regions for the dump.
    pub fn iter(&self) -> impl Iterator<Item = (RegionId, &'static str, usize, usize, usize)> + '_ {
        (0..self.used).filter_map(move |index| {
            self.regions[index].as_ref().map(|region| {
                (
                    RegionId(index as u8),
                    region.name,
                    region.capacity,
                    region.used,
                    region.low_water_free,
                )
            })
        })
    }
}

/// Overall health, ordered from good to bad.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Health {
    /// Everything within thresholds.
    Ok,
    /// Low memory or a recorded fault.
    Degraded,
    /// Memory exhausted or nearly so.
    Critical,
}

impl Health {
    /// Short text form used by the dump.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Ok => "ok",
            Self::Degraded => "degraded",
            Self::Critical => "critical",
        }
    }
}

/// One deduplicated fault.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FaultRecord {
    /// Fault code.
    pub code: u16,
    /// Number of occurrences.
    pub count: u32,
    /// First occurrence, microseconds since boot.
    pub first_us: u64,
    /// Most recent occurrence.
    pub last_us: u64,
    /// Argument of the most recent occurrence (line, channel, …).
    pub last_arg: u32,
}

/// Fixed-capacity table of deduplicated faults.
#[derive(Clone)]
pub struct FaultTable<const N: usize> {
    entries: [Option<FaultRecord>; N],
    used: usize,
}

impl<const N: usize> Default for FaultTable<N> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const N: usize> FaultTable<N> {
    /// Create an empty table.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            entries: [const { None }; N],
            used: 0,
        }
    }

    /// Record a fault at `now_us`. Returns `false` when the table is full and the
    /// code is new.
    pub fn record(&mut self, code: u16, now_us: u64, arg: u32) -> bool {
        for record in self.entries.iter_mut().take(self.used).flatten() {
            if record.code == code {
                record.count = record.count.saturating_add(1);
                record.last_us = now_us;
                record.last_arg = arg;
                return true;
            }
        }
        if self.used >= N {
            return false;
        }
        self.entries[self.used] = Some(FaultRecord {
            code,
            count: 1,
            first_us: now_us,
            last_us: now_us,
            last_arg: arg,
        });
        self.used += 1;
        true
    }

    /// Look up a code.
    #[must_use]
    pub fn find(&self, code: u16) -> Option<&FaultRecord> {
        self.entries
            .iter()
            .take(self.used)
            .flatten()
            .find(|record| record.code == code)
    }

    /// Number of distinct fault codes.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.used
    }

    /// Whether no fault is recorded.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.used == 0
    }

    /// Total number of fault occurrences.
    #[must_use]
    pub fn total_events(&self) -> u32 {
        self.entries
            .iter()
            .take(self.used)
            .flatten()
            .map(|record| record.count)
            .sum()
    }

    /// Drop all records.
    pub fn clear(&mut self) {
        self.used = 0;
    }

    /// Iterate records.
    pub fn iter(&self) -> impl Iterator<Item = &FaultRecord> + '_ {
        self.entries.iter().take(self.used).flatten()
    }
}

/// Size of the encoded crash record.
pub const CRASH_RECORD_LEN: usize = 24;

const CRASH_MAGIC: u16 = 0x5A5A;

/// What the panic handler leaves in RTC RAM for the next boot.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct CrashRecord {
    /// Reset reason code from the SoC.
    pub reset_reason: u8,
    /// Fault code active at the time of the crash.
    pub fault: u16,
    /// Task id that crashed.
    pub task: u8,
    /// Uptime at the crash.
    pub uptime_us: u64,
    /// Boot counter.
    pub boot_count: u32,
}

impl CrashRecord {
    /// Encode into the fixed 24-byte layout.
    #[must_use]
    pub fn to_bytes(&self) -> [u8; CRASH_RECORD_LEN] {
        let mut bytes = [0u8; CRASH_RECORD_LEN];
        bytes[0..2].copy_from_slice(&CRASH_MAGIC.to_le_bytes());
        bytes[2] = self.reset_reason;
        bytes[3] = self.task;
        bytes[4..6].copy_from_slice(&self.fault.to_le_bytes());
        bytes[6..10].copy_from_slice(&self.boot_count.to_le_bytes());
        bytes[10..18].copy_from_slice(&self.uptime_us.to_le_bytes());
        bytes[18] = checksum(&bytes[..18]);
        bytes
    }

    /// Decode, rejecting a bad magic or checksum (a half-written record).
    #[must_use]
    pub fn from_bytes(bytes: &[u8; CRASH_RECORD_LEN]) -> Option<Self> {
        if u16::from_le_bytes([bytes[0], bytes[1]]) != CRASH_MAGIC {
            return None;
        }
        if bytes[18] != checksum(&bytes[..18]) {
            return None;
        }
        Some(Self {
            reset_reason: bytes[2],
            task: bytes[3],
            fault: u16::from_le_bytes([bytes[4], bytes[5]]),
            boot_count: u32::from_le_bytes([bytes[6], bytes[7], bytes[8], bytes[9]]),
            uptime_us: u64::from_le_bytes([
                bytes[10], bytes[11], bytes[12], bytes[13], bytes[14], bytes[15], bytes[16],
                bytes[17],
            ]),
        })
    }
}

fn checksum(bytes: &[u8]) -> u8 {
    bytes.iter().fold(0u8, |sum, byte| sum.wrapping_add(*byte))
}

/// The bundle published as a diagnostic dump.
#[derive(Clone)]
pub struct Diagnostics<const M: usize, const R: usize, const F: usize> {
    /// Named counters.
    pub metrics: Metrics<M>,
    /// Memory regions.
    pub memory: MemoryTracker<R>,
    /// Deduplicated faults.
    pub faults: FaultTable<F>,
    /// Crash record from the previous boot, if any.
    pub crash: Option<CrashRecord>,
    /// Current uptime.
    pub uptime_us: u64,
    /// Boot counter.
    pub boot_count: u32,
}

impl<const M: usize, const R: usize, const F: usize> Default for Diagnostics<M, R, F> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const M: usize, const R: usize, const F: usize> Diagnostics<M, R, F> {
    /// Create an empty bundle.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            metrics: Metrics::new(),
            memory: MemoryTracker::new(),
            faults: FaultTable::new(),
            crash: None,
            uptime_us: 0,
            boot_count: 0,
        }
    }

    /// Aggregate health: memory dominates, then faults/crash degrade.
    #[must_use]
    pub fn health(&self) -> Health {
        let memory = self.memory.health();
        if memory == Health::Critical {
            return Health::Critical;
        }
        if memory == Health::Degraded || !self.faults.is_empty() || self.crash.is_some() {
            return Health::Degraded;
        }
        Health::Ok
    }

    /// Write a human-readable dump.
    ///
    /// # Errors
    /// Propagates write errors from `writer`.
    pub fn dump<W: fmt::Write>(&self, writer: &mut W) -> fmt::Result {
        writeln!(
            writer,
            "uptime_ms={} boot={} health={}",
            self.uptime_us / 1_000,
            self.boot_count,
            self.health().as_str()
        )?;

        for (_, name, capacity, used, low_water) in self.memory.iter() {
            writeln!(
                writer,
                "mem {name} cap={capacity} used={used} free={} low={low_water}",
                capacity.saturating_sub(used)
            )?;
        }

        for (name, value) in self.metrics.iter() {
            writeln!(writer, "metric {name}={value}")?;
        }

        for fault in self.faults.iter() {
            writeln!(
                writer,
                "fault 0x{:04x} count={} last_ms={}",
                fault.code,
                fault.count,
                fault.last_us / 1_000
            )?;
        }

        if let Some(crash) = self.crash {
            writeln!(
                writer,
                "crash reset={} fault=0x{:04x} task={} uptime_ms={}",
                crash.reset_reason,
                crash.fault,
                crash.task,
                crash.uptime_us / 1_000
            )?;
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::string::String;

    #[test]
    fn metrics_count_and_iterate() {
        let mut metrics = Metrics::<4>::new();
        let packets = metrics.define("packets").unwrap();
        let drops = metrics.define("drops").unwrap();
        metrics.incr(packets);
        metrics.add(packets, 10);
        metrics.set(drops, 3);

        assert_eq!(metrics.get(packets), Some(11));
        assert_eq!(metrics.get(drops), Some(3));
        assert_eq!(metrics.len(), 2);
        let rows: std::vec::Vec<(&str, u64)> = metrics.iter().collect();
        assert_eq!(rows, [("packets", 11), ("drops", 3)]);
    }

    #[test]
    fn metrics_saturate_and_report_capacity() {
        let mut metrics = Metrics::<1>::new();
        let only = metrics.define("only").unwrap();
        metrics.set(only, u64::MAX);
        metrics.incr(only);
        assert_eq!(metrics.get(only), Some(u64::MAX));
        assert_eq!(metrics.define("second"), None);
    }

    #[test]
    fn memory_tracks_low_water_mark() {
        let mut memory = MemoryTracker::<2>::new();
        let heap = memory.register("heap", 1_000).unwrap();
        let dma = memory.register("dma", 500).unwrap();
        assert!(memory.register("zero", 0).is_none());

        assert!(memory.take(heap, 800));
        assert_eq!(memory.free(heap), Some(200));
        assert_eq!(memory.low_water_free(heap), Some(200));

        memory.give(heap, 300);
        assert_eq!(memory.free(heap), Some(500));
        assert_eq!(memory.low_water_free(heap), Some(200));

        assert!(!memory.take(heap, 600));
        assert!(memory.set_used(dma, 250));
        assert_eq!(memory.total_capacity(), 1_500);
        assert_eq!(memory.total_used(), 750);
    }

    #[test]
    fn memory_health_thresholds() {
        let mut memory = MemoryTracker::<1>::new();
        let heap = memory.register("heap", 1_000).unwrap();
        assert_eq!(memory.health(), Health::Ok);

        assert!(memory.take(heap, 860));
        assert_eq!(memory.health(), Health::Degraded); // 14% free

        assert!(memory.take(heap, 100));
        assert_eq!(memory.health(), Health::Critical); // 4% free
    }

    #[test]
    fn faults_are_deduplicated() {
        let mut faults = FaultTable::<4>::new();
        assert!(faults.record(0x0101, 100, 7));
        assert!(faults.record(0x0101, 200, 8));
        assert!(faults.record(0x0202, 300, 0));

        let record = faults.find(0x0101).unwrap();
        assert_eq!(record.count, 2);
        assert_eq!(record.first_us, 100);
        assert_eq!(record.last_us, 200);
        assert_eq!(record.last_arg, 8);
        assert_eq!(faults.len(), 2);
        assert_eq!(faults.total_events(), 3);
        assert!(!faults.is_empty());

        faults.clear();
        assert!(faults.is_empty());
        assert_eq!(faults.total_events(), 0);
    }

    #[test]
    fn fault_table_reports_full_only_for_new_codes() {
        let mut faults = FaultTable::<1>::new();
        assert!(faults.record(1, 0, 0));
        assert!(faults.record(1, 1, 0));
        assert!(!faults.record(2, 2, 0));
        assert_eq!(faults.total_events(), 2);
    }

    #[test]
    fn crash_record_roundtrips() {
        let record = CrashRecord {
            reset_reason: 3,
            fault: 0x0101,
            task: 2,
            uptime_us: 12_345,
            boot_count: 7,
        };
        let bytes = record.to_bytes();
        assert_eq!(CrashRecord::from_bytes(&bytes), Some(record));
    }

    #[test]
    fn corrupt_crash_record_is_rejected() {
        let record = CrashRecord {
            reset_reason: 1,
            ..CrashRecord::default()
        };
        let mut bytes = record.to_bytes();
        bytes[5] ^= 0xFF;
        assert_eq!(CrashRecord::from_bytes(&bytes), None);

        let mut bytes = record.to_bytes();
        bytes[0] = 0;
        assert_eq!(CrashRecord::from_bytes(&bytes), None);
    }

    #[test]
    fn diagnostics_dump_contains_state() {
        let mut diagnostics = Diagnostics::<2, 2, 2>::new();
        diagnostics.uptime_us = 12_345_000;
        diagnostics.boot_count = 3;
        let heap = diagnostics.memory.register("heap", 1_000).unwrap();
        diagnostics.memory.take(heap, 900);
        let packets = diagnostics.metrics.define("packets").unwrap();
        diagnostics.metrics.add(packets, 42);
        diagnostics.faults.record(0x0101, 1000, 9);
        diagnostics.crash = Some(CrashRecord {
            reset_reason: 3,
            fault: 0x0101,
            task: 1,
            uptime_us: 900_000,
            boot_count: 2,
        });

        let mut output = String::new();
        diagnostics.dump(&mut output).unwrap();

        assert!(output.contains("uptime_ms=12345 boot=3 health=degraded"));
        assert!(output.contains("mem heap cap=1000 used=900 free=100 low=100"));
        assert!(output.contains("metric packets=42"));
        assert!(output.contains("fault 0x0101 count=1"));
        assert!(output.contains("crash reset=3 fault=0x0101 task=1 uptime_ms=900"));
    }

    #[test]
    fn diagnostics_health_aggregates() {
        let mut diagnostics = Diagnostics::<1, 1, 1>::new();
        assert_eq!(diagnostics.health(), Health::Ok);

        diagnostics.faults.record(1, 0, 0);
        assert_eq!(diagnostics.health(), Health::Degraded);

        let heap = diagnostics.memory.register("heap", 100).unwrap();
        diagnostics.memory.take(heap, 99);
        assert_eq!(diagnostics.health(), Health::Critical);
    }
}
