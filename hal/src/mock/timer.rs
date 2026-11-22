//! Mock timer with a manually advanced clock and a recorded alarm.

use crate::Timer;

/// A timer whose clock only moves when [`MockTimer::advance`] is called.
pub struct MockTimer {
    now_us: u64,
    armed_us: Option<u64>,
    arms: u32,
}

impl Default for MockTimer {
    fn default() -> Self {
        Self::new()
    }
}

impl MockTimer {
    /// Start at zero with no alarm.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            now_us: 0,
            armed_us: None,
            arms: 0,
        }
    }

    /// Advance the clock.
    pub fn advance(&mut self, us: u64) {
        self.now_us = self.now_us.saturating_add(us);
    }

    /// Currently armed deadline, if any.
    #[must_use]
    pub const fn armed(&self) -> Option<u64> {
        self.armed_us
    }

    /// Number of `arm` calls.
    #[must_use]
    pub const fn arms(&self) -> u32 {
        self.arms
    }
}

impl Timer for MockTimer {
    fn now_us(&self) -> u64 {
        self.now_us
    }

    fn arm(&mut self, deadline_us: u64) {
        self.armed_us = Some(deadline_us);
        self.arms = self.arms.saturating_add(1);
    }

    fn disarm(&mut self) {
        self.armed_us = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clock_is_manual_and_monotonic() {
        let mut timer = MockTimer::new();
        assert_eq!(timer.now_us(), 0);
        timer.advance(1_500);
        assert_eq!(timer.now_us(), 1_500);
        timer.advance(u64::MAX);
        assert_eq!(timer.now_us(), u64::MAX);
    }

    #[test]
    fn arm_and_disarm_are_recorded() {
        let mut timer = MockTimer::new();
        assert_eq!(timer.armed(), None);
        timer.arm(5_000);
        assert_eq!(timer.armed(), Some(5_000));
        assert_eq!(timer.arms(), 1);
        timer.disarm();
        assert_eq!(timer.armed(), None);
    }
}
