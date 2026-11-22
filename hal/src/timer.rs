//! Monotonic timer with a one-shot alarm.
//!
//! The kernel's scheduler decides *when*; this trait is how a SoC arms the
//! hardware comparator for that deadline (tickless). `now_us` must be monotonic
//! since boot.

/// A monotonic microsecond clock with a one-shot alarm.
pub trait Timer {
    /// Microseconds since boot.
    fn now_us(&self) -> u64;

    /// Arm the alarm for `deadline_us` (monotonic).
    fn arm(&mut self, deadline_us: u64);

    /// Disarm the alarm.
    fn disarm(&mut self);
}
