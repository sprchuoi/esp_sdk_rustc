//! Interrupt dispatch table.
//!
//! The SoC owns the vector hardware; this module owns the *booking*: which
//! driver claimed which IRQ at which priority, and how a test can dispatch one.
//! It is deliberately pure so the interrupt contract can be exercised on the
//! host, where the real vectors do not exist.
//!
//! The ISR contract (see docs/ARCHITECTURE.md §3.2): handlers run with
//! interrupts masked, must not allocate, log or block, and use [`crate::sync`]
//! primitives to wake tasks. The table counts dispatches so diagnostics can
//! show a silent interrupt storm.

/// A chip interrupt number.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct IrqNumber(u16);

impl IrqNumber {
    /// Construct from the raw controller number.
    #[must_use]
    pub const fn new(raw: u16) -> Self {
        Self(raw)
    }

    /// Raw controller number.
    #[must_use]
    pub const fn raw(self) -> u16 {
        self.0
    }
}

/// Interrupt priority, 1 (lowest) to 7 (highest).
///
/// The SoC maps these onto its own levels; the table only needs the ordering
/// and range so a driver cannot claim an impossible priority.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Priority(u8);

impl Priority {
    /// Lowest usable priority.
    pub const LOW: Self = Self(1);
    /// Highest usable priority.
    pub const HIGH: Self = Self(7);

    /// Validate a raw priority.
    ///
    /// # Errors
    /// [`IrqError::InvalidPriority`] outside `1..=7`.
    pub const fn new(raw: u8) -> Result<Self, IrqError> {
        if raw < 1 || raw > 7 {
            return Err(IrqError::InvalidPriority);
        }
        Ok(Self(raw))
    }

    /// Raw value.
    #[must_use]
    pub const fn raw(self) -> u8 {
        self.0
    }
}

/// An interrupt handler.
///
/// A plain `fn()`: no captures, no allocation, no `dyn` dispatch. Anything the
/// handler needs lives in a `static` it owns.
pub type Handler = fn();

/// Why a claim was rejected.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IrqError {
    /// The table has no free slot.
    Capacity,
    /// Another driver already claimed this IRQ.
    AlreadyClaimed,
    /// Priority outside `1..=7`.
    InvalidPriority,
}

/// One claimed interrupt.
#[derive(Clone, Copy, Debug)]
pub struct IrqEntry {
    /// Interrupt number.
    pub irq: IrqNumber,
    /// Claimed priority.
    pub priority: Priority,
    /// Handler.
    pub handler: Handler,
    /// Number of dispatches observed.
    pub count: u32,
}

/// Static table of claimed interrupts.
#[derive(Clone, Debug)]
pub struct IrqTable<const N: usize> {
    entries: [Option<IrqEntry>; N],
    count: usize,
}

impl<const N: usize> Default for IrqTable<N> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const N: usize> IrqTable<N> {
    /// Create an empty table.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            entries: [None; N],
            count: 0,
        }
    }

    /// Number of claimed interrupts.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.count
    }

    /// Whether nothing is claimed.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.count == 0
    }

    /// Claim an interrupt for a handler.
    ///
    /// # Errors
    /// [`IrqError::Capacity`] when the table is full,
    /// [`IrqError::AlreadyClaimed`] for a duplicate IRQ.
    pub fn claim(
        &mut self,
        irq: IrqNumber,
        priority: Priority,
        handler: Handler,
    ) -> Result<(), IrqError> {
        if self.find(irq).is_some() {
            return Err(IrqError::AlreadyClaimed);
        }
        if self.count >= N {
            return Err(IrqError::Capacity);
        }
        self.entries[self.count] = Some(IrqEntry {
            irq,
            priority,
            handler,
            count: 0,
        });
        self.count += 1;
        Ok(())
    }

    /// Find a claimed interrupt.
    #[must_use]
    pub fn find(&self, irq: IrqNumber) -> Option<&IrqEntry> {
        self.entries[..self.count]
            .iter()
            .flatten()
            .find(|entry| entry.irq == irq)
    }

    /// Dispatch an interrupt: run its handler and count it.
    ///
    /// Returns `false` when the IRQ is unclaimed (the SoC would disable it).
    pub fn dispatch(&mut self, irq: IrqNumber) -> bool {
        let Some(entry) = self.entries[..self.count]
            .iter_mut()
            .flatten()
            .find(|entry| entry.irq == irq)
        else {
            return false;
        };
        entry.count = entry.count.saturating_add(1);
        (entry.handler)();
        true
    }

    /// Dispatches observed for an IRQ.
    #[must_use]
    pub fn count(&self, irq: IrqNumber) -> u32 {
        self.find(irq).map_or(0, |entry| entry.count)
    }

    /// Iterate claimed interrupts.
    pub fn iter(&self) -> impl Iterator<Item = &IrqEntry> + '_ {
        self.entries[..self.count].iter().flatten()
    }

    /// Highest claimed priority, for diagnostics.
    #[must_use]
    pub fn max_priority(&self) -> Option<Priority> {
        self.iter().map(|entry| entry.priority).max()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::sync::atomic::{AtomicU32, Ordering};

    static A: AtomicU32 = AtomicU32::new(0);
    static B: AtomicU32 = AtomicU32::new(0);

    fn handler_a() {
        A.fetch_add(1, Ordering::Relaxed);
    }

    fn handler_b() {
        B.fetch_add(1, Ordering::Relaxed);
    }

    #[test]
    fn priority_is_range_checked() {
        assert_eq!(Priority::new(0), Err(IrqError::InvalidPriority));
        assert_eq!(Priority::new(8), Err(IrqError::InvalidPriority));
        assert_eq!(Priority::new(1).map(|p| p.raw()), Ok(1));
        assert_eq!(Priority::new(7).map(|p| p.raw()), Ok(7));
    }

    #[test]
    fn claims_and_dispatches() {
        let mut table = IrqTable::<4>::new();
        A.store(0, Ordering::Relaxed);
        table
            .claim(IrqNumber::new(10), Priority::new(3).unwrap(), handler_a)
            .unwrap();
        assert_eq!(table.len(), 1);
        assert!(table.find(IrqNumber::new(10)).is_some());
        assert!(table.dispatch(IrqNumber::new(10)));
        assert!(table.dispatch(IrqNumber::new(10)));
        assert_eq!(A.load(Ordering::Relaxed), 2);
        assert_eq!(table.count(IrqNumber::new(10)), 2);
    }

    #[test]
    fn unclaimed_dispatch_is_refused() {
        let mut table = IrqTable::<2>::new();
        assert!(!table.dispatch(IrqNumber::new(99)));
        assert_eq!(table.count(IrqNumber::new(99)), 0);
    }

    #[test]
    fn rejects_duplicate_and_overflow() {
        let mut table = IrqTable::<1>::new();
        table
            .claim(IrqNumber::new(1), Priority::LOW, handler_a)
            .unwrap();
        assert_eq!(
            table.claim(IrqNumber::new(1), Priority::LOW, handler_b),
            Err(IrqError::AlreadyClaimed)
        );
        assert_eq!(
            table.claim(IrqNumber::new(2), Priority::LOW, handler_b),
            Err(IrqError::Capacity)
        );
    }

    #[test]
    fn tracks_highest_priority() {
        let mut table = IrqTable::<4>::new();
        table
            .claim(IrqNumber::new(1), Priority::new(2).unwrap(), handler_a)
            .unwrap();
        table
            .claim(IrqNumber::new(2), Priority::new(6).unwrap(), handler_b)
            .unwrap();
        assert_eq!(table.max_priority(), Some(Priority::new(6).unwrap()));
        assert_eq!(table.iter().count(), 2);
    }
}
