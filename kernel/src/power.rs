//! Sleep policy and power profiles.
//!
//! The kernel owns *when* to sleep; the SoC owns *how*. This module turns the
//! scheduler's next deadline, the board profile and a few thresholds into a
//! concrete [`Action`] the idle loop can execute. Keeping the policy pure makes
//! it testable and keeps the `WFI`/light-sleep/deep-sleep register sequences in
//! `soc/`.
//!
//! Two profiles exist:
//!
//! - [`Profile::Mains`] — lightsleep on idle, never deep-sleeps.
//! - [`Profile::Battery`] — reports every `report_period_us`, deep-sleeps between
//!   reports when the radio is down and no nearer deadline exists.

/// Product power profile.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Profile {
    /// Mains-powered: stay reachable, sleep lightly when idle.
    Mains,
    /// Battery-powered: wake, report, sleep deeply.
    Battery {
        /// How often the node must report, in microseconds.
        report_period_us: u64,
    },
}

/// A sleep mode the SoC can enter.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SleepMode {
    /// Wait for interrupt; the CPU stops, peripherals keep running.
    Wfi,
    /// Light sleep; RAM retained, most clocks gated.
    LightSleep,
    /// Deep sleep; only RTC domain retained, restart on wake.
    DeepSleep,
}

/// Bit set of sources that may wake the chip.
#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub struct WakeSources(u8);

impl WakeSources {
    /// No source.
    pub const NONE: Self = Self(0);
    /// Timer expiry.
    pub const TIMER: Self = Self(1 << 0);
    /// GPIO edge.
    pub const GPIO: Self = Self(1 << 1);
    /// UART activity.
    pub const UART: Self = Self(1 << 2);
    /// Network activity (modem wake).
    pub const WIFI: Self = Self(1 << 3);

    /// Combine two sets.
    #[must_use]
    pub const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    /// Whether every bit of `other` is present.
    #[must_use]
    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    /// Whether the set is empty.
    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }
}

impl core::fmt::Debug for WakeSources {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let mut first = true;
        for (bit, name) in [
            (Self::TIMER, "TIMER"),
            (Self::GPIO, "GPIO"),
            (Self::UART, "UART"),
            (Self::WIFI, "WIFI"),
        ] {
            if self.contains(bit) {
                if !first {
                    f.write_str("|")?;
                }
                f.write_str(name)?;
                first = false;
            }
        }
        if first {
            f.write_str("NONE")?;
        }
        Ok(())
    }
}

/// Thresholds for entering each sleep mode.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Policy {
    /// Idle time before `WFI`.
    pub wfi_after_us: u64,
    /// Idle time before light sleep.
    pub light_sleep_after_us: u64,
    /// Idle time before deep sleep (battery profile only).
    pub deep_sleep_after_us: u64,
    /// Whether UART activity should wake the chip.
    pub uart_wake: bool,
}

impl Default for Policy {
    fn default() -> Self {
        Self {
            wfi_after_us: 1_000,
            light_sleep_after_us: 20_000,
            deep_sleep_after_us: 300_000,
            uart_wake: true,
        }
    }
}

/// What the idle loop should do now.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    /// Work is pending.
    StayAwake,
    /// Nothing to do, but a deadline is near: stop the CPU until an interrupt.
    Wfi,
    /// Enter a sleep mode, waking on `wake`.
    Sleep {
        /// Mode to enter.
        mode: SleepMode,
        /// Sources allowed to wake the chip.
        wake: WakeSources,
    },
}

/// Tracks idle time, report scheduling and sleep statistics.
#[derive(Clone, Debug)]
pub struct PowerManager {
    profile: Profile,
    policy: Policy,
    idle_since: Option<u64>,
    last_report_us: u64,
    sleeps: u32,
    sleep_time_us: u64,
    last_sleep: Option<(SleepMode, u64)>,
}

impl PowerManager {
    /// Create a manager for a profile and policy.
    #[must_use]
    pub const fn new(profile: Profile, policy: Policy) -> Self {
        Self {
            profile,
            policy,
            idle_since: None,
            last_report_us: 0,
            sleeps: 0,
            sleep_time_us: 0,
            last_sleep: None,
        }
    }

    /// The active profile.
    #[must_use]
    pub const fn profile(&self) -> Profile {
        self.profile
    }

    /// Decide what to do after the scheduler found no ready task.
    ///
    /// Call repeatedly; the manager tracks how long the idle period has lasted.
    pub fn on_idle(&mut self, now_us: u64, next_deadline: Option<u64>, wifi_up: bool) -> Action {
        let idle_since = *self.idle_since.get_or_insert(now_us);
        let idle = now_us.saturating_sub(idle_since);

        let report_deadline = self.next_report_us();
        let deadline = match (next_deadline, report_deadline) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        };
        let until_deadline = deadline.map(|deadline| deadline.saturating_sub(now_us));

        if matches!(self.profile, Profile::Battery { .. })
            && !wifi_up
            && idle >= self.policy.deep_sleep_after_us
            && until_deadline.is_none_or(|until| until >= self.policy.deep_sleep_after_us)
        {
            return Action::Sleep {
                mode: SleepMode::DeepSleep,
                wake: WakeSources::TIMER.union(WakeSources::GPIO),
            };
        }

        if idle >= self.policy.light_sleep_after_us {
            let mut wake = WakeSources::TIMER.union(WakeSources::GPIO);
            if self.policy.uart_wake {
                wake = wake.union(WakeSources::UART);
            }
            return Action::Sleep {
                mode: SleepMode::LightSleep,
                wake,
            };
        }

        if idle >= self.policy.wfi_after_us {
            return Action::Wfi;
        }

        Action::StayAwake
    }

    /// Record that the chip woke from `mode` after `slept_us`.
    pub fn on_wake(&mut self, mode: SleepMode, slept_us: u64) {
        self.idle_since = None;
        if mode != SleepMode::Wfi {
            self.sleeps = self.sleeps.saturating_add(1);
            self.sleep_time_us = self.sleep_time_us.saturating_add(slept_us);
        }
        self.last_sleep = Some((mode, slept_us));
    }

    /// Mark that the node just reported; resets the battery report schedule.
    pub fn mark_reported(&mut self, now_us: u64) {
        self.last_report_us = now_us;
    }

    /// Whether a battery node owes a report at `now_us`.
    ///
    /// Always `false` for [`Profile::Mains`].
    #[must_use]
    pub fn report_due(&self, now_us: u64) -> bool {
        match self.profile {
            Profile::Mains => false,
            Profile::Battery { report_period_us } => {
                now_us >= self.last_report_us.saturating_add(report_period_us)
            }
        }
    }

    /// Next report deadline, if the profile has one.
    #[must_use]
    pub fn next_report_us(&self) -> Option<u64> {
        match self.profile {
            Profile::Mains => None,
            Profile::Battery { report_period_us } => {
                Some(self.last_report_us.saturating_add(report_period_us))
            }
        }
    }

    /// Number of non-WFI sleeps entered.
    #[must_use]
    pub const fn sleeps(&self) -> u32 {
        self.sleeps
    }

    /// Total time spent in light/deep sleep.
    #[must_use]
    pub const fn sleep_time_us(&self) -> u64 {
        self.sleep_time_us
    }

    /// The last wake event, for diagnostics.
    #[must_use]
    pub const fn last_sleep(&self) -> Option<(SleepMode, u64)> {
        self.last_sleep
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manager(profile: Profile) -> PowerManager {
        PowerManager::new(profile, Policy::default())
    }

    #[test]
    fn busy_node_stays_awake() {
        let mut power = manager(Profile::Mains);
        assert_eq!(power.on_idle(0, Some(100), true), Action::StayAwake);
        assert_eq!(power.on_idle(500, Some(100), true), Action::StayAwake);
    }

    #[test]
    fn short_idle_uses_wfi() {
        let mut power = manager(Profile::Mains);
        assert_eq!(power.on_idle(0, Some(50_000), true), Action::StayAwake);
        assert_eq!(power.on_idle(1_500, Some(50_000), true), Action::Wfi);
    }

    #[test]
    fn long_idle_enters_light_sleep() {
        let mut power = manager(Profile::Mains);
        power.on_idle(0, Some(60_000), true);
        match power.on_idle(25_000, Some(60_000), true) {
            Action::Sleep { mode, wake } => {
                assert_eq!(mode, SleepMode::LightSleep);
                assert!(wake.contains(WakeSources::TIMER));
                assert!(wake.contains(WakeSources::GPIO));
            }
            other => panic!("expected light sleep, got {other:?}"),
        }
    }

    #[test]
    fn mains_never_deep_sleeps() {
        let mut power = manager(Profile::Mains);
        power.on_idle(0, None, false);
        match power.on_idle(10_000_000, None, false) {
            Action::Sleep { mode, .. } => assert_eq!(mode, SleepMode::LightSleep),
            other => panic!("expected sleep, got {other:?}"),
        }
    }

    #[test]
    fn battery_deep_sleeps_when_radio_is_down() {
        let mut power = manager(Profile::Battery {
            report_period_us: 300_000_000,
        });
        power.on_idle(0, None, false);
        match power.on_idle(400_000, None, false) {
            Action::Sleep { mode, wake } => {
                assert_eq!(mode, SleepMode::DeepSleep);
                assert!(wake.contains(WakeSources::TIMER));
            }
            other => panic!("expected deep sleep, got {other:?}"),
        }
    }

    #[test]
    fn battery_stays_light_when_radio_is_up() {
        let mut power = manager(Profile::Battery {
            report_period_us: 300_000_000,
        });
        power.on_idle(0, None, true);
        match power.on_idle(400_000, None, true) {
            Action::Sleep { mode, .. } => assert_eq!(mode, SleepMode::LightSleep),
            other => panic!("expected sleep, got {other:?}"),
        }
    }

    #[test]
    fn near_deadline_prevents_deep_sleep() {
        let mut power = manager(Profile::Battery {
            report_period_us: 300_000_000,
        });
        power.on_idle(0, Some(350_000), false);
        match power.on_idle(400_000, Some(350_000), false) {
            Action::Sleep { mode, .. } => assert_eq!(mode, SleepMode::LightSleep),
            other => panic!("expected sleep, got {other:?}"),
        }
    }

    #[test]
    fn wake_clears_idle_and_records_stats() {
        let mut power = manager(Profile::Mains);
        power.on_idle(0, Some(60_000), true);
        assert_eq!(power.sleeps(), 0);
        power.on_wake(SleepMode::LightSleep, 25_000);
        assert_eq!(power.sleeps(), 1);
        assert_eq!(power.sleep_time_us(), 25_000);
        assert_eq!(power.on_idle(25_000, Some(60_000), true), Action::StayAwake);
    }

    #[test]
    fn wfi_does_not_count_as_sleep() {
        let mut power = manager(Profile::Mains);
        power.on_wake(SleepMode::Wfi, 10);
        assert_eq!(power.sleeps(), 0);
        assert_eq!(power.sleep_time_us(), 0);
    }

    #[test]
    fn battery_reports_on_schedule() {
        let mut power = manager(Profile::Battery {
            report_period_us: 60_000_000,
        });
        assert!(!power.report_due(0));
        assert!(!power.report_due(59_999_999));
        assert!(power.report_due(60_000_000));
        power.mark_reported(60_000_000);
        assert!(!power.report_due(60_000_001));
        assert_eq!(power.next_report_us(), Some(120_000_000));
    }

    #[test]
    fn mains_has_no_report_schedule() {
        let power = manager(Profile::Mains);
        assert!(!power.report_due(u64::MAX));
        assert_eq!(power.next_report_us(), None);
    }
}
