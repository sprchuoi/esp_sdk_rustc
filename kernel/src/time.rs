//! Monotonic time types.
//!
//! The kernel keeps time as a 64-bit microsecond counter since boot. That is
//! enough for 584 942 years, does not drift by conversion, and maps directly to
//! the hardware comparator/SYSTIMER value the SoC layer provides.
//!
//! Wall-clock time (SNTP offset) is deliberately a service concern, not part of
//! these types.

use core::time::Duration;

/// A monotonic instant, microseconds since boot.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Instant(u64);

impl Instant {
    /// The instant the kernel started.
    pub const ZERO: Self = Self(0);

    /// Construct from microseconds.
    #[must_use]
    pub const fn from_micros(micros: u64) -> Self {
        Self(micros)
    }

    /// Construct from milliseconds, saturating on overflow.
    #[must_use]
    pub const fn from_millis(millis: u64) -> Self {
        Self(millis.saturating_mul(1_000))
    }

    /// Construct from seconds, saturating on overflow.
    #[must_use]
    pub const fn from_secs(secs: u64) -> Self {
        Self(secs.saturating_mul(1_000_000))
    }

    /// Microseconds since boot.
    #[must_use]
    pub const fn as_micros(self) -> u64 {
        self.0
    }

    /// Whole milliseconds since boot.
    #[must_use]
    pub const fn as_millis(self) -> u64 {
        self.0 / 1_000
    }

    /// Whole seconds since boot.
    #[must_use]
    pub const fn as_secs(self) -> u64 {
        self.0 / 1_000_000
    }

    /// Add a duration, returning `None` on overflow.
    #[must_use]
    pub fn checked_add(self, duration: Duration) -> Option<Self> {
        let micros = u64::try_from(duration.as_micros()).ok()?;
        self.0.checked_add(micros).map(Self)
    }

    /// Add a duration, saturating on overflow.
    #[must_use]
    pub fn saturating_add(self, duration: Duration) -> Self {
        let micros = u64::try_from(duration.as_micros()).unwrap_or(u64::MAX);
        Self(self.0.saturating_add(micros))
    }

    /// Subtract a duration, returning `None` if it would wrap below zero.
    #[must_use]
    pub fn checked_sub(self, duration: Duration) -> Option<Self> {
        let micros = u64::try_from(duration.as_micros()).ok()?;
        self.0.checked_sub(micros).map(Self)
    }

    /// Subtract a duration, saturating at [`Instant::ZERO`].
    #[must_use]
    pub fn saturating_sub(self, duration: Duration) -> Self {
        let micros = u64::try_from(duration.as_micros()).unwrap_or(u64::MAX);
        Self(self.0.saturating_sub(micros))
    }

    /// Duration from `earlier` to `self`, or `None` if `self < earlier`.
    #[must_use]
    pub fn checked_duration_since(self, earlier: Self) -> Option<Duration> {
        self.0.checked_sub(earlier.0).map(Duration::from_micros)
    }

    /// Duration from `earlier` to `self`, saturating at zero.
    #[must_use]
    pub fn saturating_duration_since(self, earlier: Self) -> Duration {
        Duration::from_micros(self.0.saturating_sub(earlier.0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constructors_scale() {
        assert_eq!(Instant::from_micros(1).as_micros(), 1);
        assert_eq!(Instant::from_millis(2).as_micros(), 2_000);
        assert_eq!(Instant::from_secs(3).as_micros(), 3_000_000);
        assert_eq!(Instant::from_millis(1_500).as_secs(), 1);
        assert_eq!(Instant::from_millis(1_500).as_millis(), 1_500);
    }

    #[test]
    fn constructors_saturate() {
        assert_eq!(Instant::from_secs(u64::MAX).as_micros(), u64::MAX);
        assert_eq!(Instant::from_millis(u64::MAX).as_micros(), u64::MAX);
    }

    #[test]
    fn arithmetic_handles_overflow() {
        let near_max = Instant::from_micros(u64::MAX - 1);
        assert_eq!(near_max.checked_add(Duration::from_micros(2)), None);
        assert_eq!(
            near_max
                .saturating_add(Duration::from_micros(2))
                .as_micros(),
            u64::MAX
        );

        let zero = Instant::ZERO;
        assert_eq!(zero.checked_sub(Duration::from_micros(1)), None);
        assert_eq!(zero.saturating_sub(Duration::from_micros(1)), Instant::ZERO);
    }

    #[test]
    fn elapsed_uses_checked_and_saturating_paths() {
        let start = Instant::from_secs(1);
        let later = Instant::from_secs(3);
        assert_eq!(
            later.checked_duration_since(start),
            Some(Duration::from_secs(2))
        );
        assert_eq!(start.checked_duration_since(later), None);
        assert_eq!(start.saturating_duration_since(later), Duration::ZERO);
    }

    #[test]
    fn ordering_is_monotonic() {
        assert!(Instant::from_millis(2) > Instant::from_millis(1));
        assert_eq!(Instant::ZERO, Instant::default());
    }
}
