//! Log level filtering and a fixed-capacity log ring.
//!
//! On target, `defmt` is the wire format; this module is the part that decides
//! *what* is kept and keeps the most recent records for a crash dump. That
//! decision is pure logic and is unit-tested on the host.
//!
//! A record carries a numeric code and one integer argument rather than a
//! formatted string: formatting happens on the host when the ring is drained,
//! so the kernel never allocates and an ISR never formats.

use core::fmt;

/// Severity of a log record, ordered from most to least severe.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
#[repr(u8)]
pub enum Level {
    /// A failure that needs attention.
    Error = 0,
    /// Something unexpected but recoverable.
    Warn = 1,
    /// Normal lifecycle events.
    Info = 2,
    /// Development detail.
    Debug = 3,
    /// Very verbose tracing.
    Trace = 4,
}

impl Level {
    /// Short textual form for the console.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Error => "ERROR",
            Self::Warn => "WARN",
            Self::Info => "INFO",
            Self::Debug => "DEBUG",
            Self::Trace => "TRACE",
        }
    }

    /// Whether a record at this level passes `max`.
    #[must_use]
    pub const fn enabled(self, max: Self) -> bool {
        (self as u8) <= (max as u8)
    }
}

impl fmt::Display for Level {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Runtime log-level filter (the console can change it without a rebuild).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Filter {
    max: Level,
}

impl Filter {
    /// Allow records at `max` and above (i.e. at most as verbose).
    #[must_use]
    pub const fn new(max: Level) -> Self {
        Self { max }
    }

    /// The current threshold.
    #[must_use]
    pub const fn max(&self) -> Level {
        self.max
    }

    /// Replace the threshold.
    pub fn set(&mut self, max: Level) {
        self.max = max;
    }

    /// Whether `level` passes the filter.
    #[must_use]
    pub fn enabled(&self, level: Level) -> bool {
        level.enabled(self.max)
    }
}

impl Default for Filter {
    fn default() -> Self {
        Self::new(Level::Info)
    }
}

/// One retained log record.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Record {
    /// Microseconds since boot, truncated to 32 bits (~71 minutes).
    pub uptime_us: u32,
    /// Severity.
    pub level: Level,
    /// Stable numeric code; the host maps it to a format string.
    pub code: u16,
    /// The record's single integer argument.
    pub arg: i32,
}

impl Record {
    /// Empty placeholder used to initialise the ring.
    pub const EMPTY: Self = Self {
        uptime_us: 0,
        level: Level::Trace,
        code: 0,
        arg: 0,
    };
}

/// Fixed-capacity ring that keeps the most recent `N` records.
///
/// When full, the oldest record is overwritten. `N = 0` is allowed and simply
/// stores nothing, so a build can compile logging out without changing call
/// sites.
#[derive(Clone)]
pub struct Ring<const N: usize> {
    slots: [Record; N],
    write: usize,
    len: usize,
}

impl<const N: usize> Default for Ring<N> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const N: usize> Ring<N> {
    /// Create an empty ring.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            slots: [Record::EMPTY; N],
            write: 0,
            len: 0,
        }
    }

    /// Number of retained records.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.len
    }

    /// Whether no record is retained.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Append a record, dropping the oldest one when full.
    pub fn push(&mut self, record: Record) {
        if N == 0 {
            return;
        }
        self.slots[self.write] = record;
        self.write = (self.write + 1) % N;
        self.len = (self.len + 1).min(N);
    }

    /// Record at `index`, oldest first.
    #[must_use]
    pub fn get(&self, index: usize) -> Option<&Record> {
        if index >= self.len || N == 0 {
            return None;
        }
        let start = (self.write + N - self.len) % N;
        Some(&self.slots[(start + index) % N])
    }

    /// Iterate oldest to newest.
    pub fn iter(&self) -> impl Iterator<Item = &Record> {
        (0..self.len).filter_map(|index| self.get(index))
    }

    /// Write the retained records oldest-first, for a panic dump.
    ///
    /// # Errors
    /// Propagates writer errors.
    pub fn flush<W: fmt::Write>(&self, out: &mut W) -> fmt::Result {
        for record in self.iter() {
            writeln!(
                out,
                "[{}] t={}us code={} arg={}",
                record.level, record.uptime_us, record.code, record.arg
            )?;
        }
        Ok(())
    }

    /// Drop all records.
    pub fn clear(&mut self) {
        self.write = 0;
        self.len = 0;
    }
}

impl<const N: usize> fmt::Debug for Ring<N> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Ring")
            .field("len", &self.len)
            .field("capacity", &N)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(code: u16, level: Level) -> Record {
        Record {
            uptime_us: u32::from(code),
            level,
            code,
            arg: i32::from(code),
        }
    }

    #[test]
    fn filter_admits_more_severe_levels() {
        let filter = Filter::new(Level::Info);
        assert!(filter.enabled(Level::Error));
        assert!(filter.enabled(Level::Warn));
        assert!(filter.enabled(Level::Info));
        assert!(!filter.enabled(Level::Debug));
        assert!(!filter.enabled(Level::Trace));
    }

    #[test]
    fn filter_can_be_raised_at_runtime() {
        let mut filter = Filter::default();
        assert!(!filter.enabled(Level::Debug));
        filter.set(Level::Trace);
        assert!(filter.enabled(Level::Debug));
        assert!(filter.enabled(Level::Trace));
        assert_eq!(filter.max(), Level::Trace);
    }

    #[test]
    fn ring_keeps_insertion_order() {
        let mut ring = Ring::<4>::new();
        ring.push(record(1, Level::Info));
        ring.push(record(2, Level::Warn));
        ring.push(record(3, Level::Error));

        let codes: std::vec::Vec<u16> = ring.iter().map(|entry| entry.code).collect();
        assert_eq!(codes, [1, 2, 3]);
        assert_eq!(ring.len(), 3);
    }

    #[test]
    fn ring_overwrites_oldest_when_full() {
        let mut ring = Ring::<3>::new();
        for code in 1..=5 {
            ring.push(record(code, Level::Info));
        }
        let codes: std::vec::Vec<u16> = ring.iter().map(|entry| entry.code).collect();
        assert_eq!(codes, [3, 4, 5]);
        assert_eq!(ring.get(0).map(|entry| entry.code), Some(3));
        assert_eq!(ring.get(2).map(|entry| entry.code), Some(5));
        assert_eq!(ring.get(3), None);
    }

    #[test]
    fn zero_capacity_ring_stores_nothing() {
        let mut ring = Ring::<0>::new();
        ring.push(record(1, Level::Info));
        assert!(ring.is_empty());
        assert_eq!(ring.len(), 0);
        assert_eq!(ring.get(0), None);
        assert_eq!(ring.iter().count(), 0);
    }

    #[test]
    fn clear_drops_everything() {
        let mut ring = Ring::<4>::new();
        ring.push(record(1, Level::Info));
        ring.push(record(2, Level::Info));
        ring.clear();
        assert!(ring.is_empty());
        assert_eq!(ring.iter().count(), 0);
    }

    #[test]
    fn flush_renders_retained_records() {
        let mut ring = Ring::<4>::new();
        ring.push(record(1, Level::Warn));
        ring.push(record(2, Level::Error));
        let mut out = std::string::String::new();
        ring.flush(&mut out).unwrap();
        assert!(out.contains("[WARN]"));
        assert!(out.contains("[ERROR]"));
        assert!(out.contains("code=2"));
    }

    #[test]
    fn level_display_is_stable() {
        let rendered = std::format!("{} {} {}", Level::Error, Level::Info, Level::Trace);
        assert_eq!(rendered, "ERROR INFO TRACE");
    }
}
