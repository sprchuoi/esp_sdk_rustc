//! Watchdog and heartbeat supervision.
//!
//! A node that hangs must reset itself, and a node that resets in a loop must
//! stop trying. This module owns both policies:
//!
//! - **heartbeats** — every long-running service declares a maximum period; a
//!   missed deadline is reported once (not every poll) so a hung task produces
//!   one fault, not a log flood;
//! - **watchdog feed** — the `Ctrl` executor feeds it; when the deadline passes
//!   the event tells the SoC to reset;
//! - **boot attempts** — three failed boots in a row enter safe mode, which the
//!   board handles by starting only the console and provisioning.
//!
//! The module is pure: the SoC arms the hardware, the caller performs the reset.

/// A supervisor configuration or registration error.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SupervisorError {
    /// No free heartbeat slot.
    Capacity,
    /// The task already has a heartbeat.
    Duplicate,
    /// The task has no heartbeat registered.
    UnknownTask,
    /// The period was zero.
    InvalidPeriod,
}

/// Something the supervisor noticed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Event {
    /// A task missed its heartbeat deadline.
    HeartbeatMissed {
        /// Task identifier.
        task: u8,
    },
    /// The watchdog deadline passed without a feed.
    WatchdogExpired,
}

#[derive(Clone, Copy, Debug)]
struct Heartbeat {
    task: u8,
    period_us: u64,
    last_us: u64,
    missed: bool,
}

/// Watchdog and heartbeat tracker.
#[derive(Clone, Debug)]
pub struct Supervisor<const N: usize> {
    heartbeats: [Option<Heartbeat>; N],
    count: usize,
    watchdog_us: u64,
    watchdog_deadline_us: u64,
    boot_attempts: u8,
    max_boot_attempts: u8,
    safe_mode: bool,
}

impl<const N: usize> Supervisor<N> {
    /// Create a supervisor with a watchdog timeout and a safe-mode threshold.
    #[must_use]
    pub const fn new(watchdog_us: u64, max_boot_attempts: u8) -> Self {
        Self {
            heartbeats: [const { None }; N],
            count: 0,
            watchdog_us,
            watchdog_deadline_us: watchdog_us,
            boot_attempts: 0,
            max_boot_attempts,
            safe_mode: false,
        }
    }

    /// Register a heartbeat for `task`, due every `period_us`.
    ///
    /// # Errors
    /// [`SupervisorError::InvalidPeriod`] for zero,
    /// [`SupervisorError::Duplicate`] if already registered,
    /// [`SupervisorError::Capacity`] when the table is full.
    pub fn register(
        &mut self,
        task: u8,
        period_us: u64,
        now_us: u64,
    ) -> Result<(), SupervisorError> {
        if period_us == 0 {
            return Err(SupervisorError::InvalidPeriod);
        }
        if self.find(task).is_some() {
            return Err(SupervisorError::Duplicate);
        }
        if self.count >= N {
            return Err(SupervisorError::Capacity);
        }
        self.heartbeats[self.count] = Some(Heartbeat {
            task,
            period_us,
            last_us: now_us,
            missed: false,
        });
        self.count += 1;
        Ok(())
    }

    fn find(&self, task: u8) -> Option<&Heartbeat> {
        self.heartbeats[..self.count]
            .iter()
            .flatten()
            .find(|heartbeat| heartbeat.task == task)
    }

    fn find_mut(&mut self, task: u8) -> Option<&mut Heartbeat> {
        self.heartbeats[..self.count]
            .iter_mut()
            .flatten()
            .find(|heartbeat| heartbeat.task == task)
    }

    /// Record a heartbeat. Returns `false` for an unknown task.
    pub fn beat(&mut self, task: u8, now_us: u64) -> bool {
        let Some(heartbeat) = self.find_mut(task) else {
            return false;
        };
        heartbeat.last_us = now_us;
        heartbeat.missed = false;
        true
    }

    /// Feed the watchdog, moving its deadline to `now + timeout`.
    pub fn feed(&mut self, now_us: u64) {
        self.watchdog_deadline_us = now_us.saturating_add(self.watchdog_us);
    }

    /// Check deadlines. At most one event is returned per call.
    ///
    /// A missed heartbeat is reported once per miss; [`Supervisor::beat`] re-arms
    /// it.
    pub fn poll(&mut self, now_us: u64) -> Option<Event> {
        for heartbeat in self.heartbeats[..self.count].iter_mut().flatten() {
            if now_us.saturating_sub(heartbeat.last_us) > heartbeat.period_us && !heartbeat.missed {
                heartbeat.missed = true;
                return Some(Event::HeartbeatMissed {
                    task: heartbeat.task,
                });
            }
        }

        if now_us >= self.watchdog_deadline_us {
            self.watchdog_deadline_us = now_us.saturating_add(self.watchdog_us);
            return Some(Event::WatchdogExpired);
        }

        None
    }

    /// Next deadline the supervisor needs to be polled at.
    #[must_use]
    pub fn next_deadline(&self) -> u64 {
        let heartbeat = self.heartbeats[..self.count]
            .iter()
            .flatten()
            .filter(|heartbeat| !heartbeat.missed)
            .map(|heartbeat| heartbeat.last_us.saturating_add(heartbeat.period_us))
            .min();
        match heartbeat {
            Some(deadline) => deadline.min(self.watchdog_deadline_us),
            None => self.watchdog_deadline_us,
        }
    }

    /// Count a boot attempt; returns `true` when safe mode should be entered.
    pub fn note_boot_attempt(&mut self) -> bool {
        self.boot_attempts = self.boot_attempts.saturating_add(1);
        if self.boot_attempts >= self.max_boot_attempts {
            self.safe_mode = true;
        }
        self.safe_mode
    }

    /// Mark this boot as healthy, clearing the attempt counter.
    pub fn confirm_boot(&mut self) {
        self.boot_attempts = 0;
        self.safe_mode = false;
    }

    /// Number of consecutive failed boot attempts.
    #[must_use]
    pub const fn boot_attempts(&self) -> u8 {
        self.boot_attempts
    }

    /// Whether the node is in safe mode.
    #[must_use]
    pub const fn safe_mode(&self) -> bool {
        self.safe_mode
    }

    /// Number of registered heartbeats.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.count
    }

    /// Whether no heartbeat is registered.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.count == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn heartbeat_within_period_is_quiet() {
        let mut supervisor = Supervisor::<4>::new(10_000, 3);
        supervisor.register(1, 1_000, 0).unwrap();
        assert!(supervisor.beat(1, 500));
        assert_eq!(supervisor.poll(900), None);
    }

    #[test]
    fn missed_heartbeat_is_reported_once_until_rearmed() {
        let mut supervisor = Supervisor::<4>::new(10_000, 3);
        supervisor.register(1, 1_000, 0).unwrap();
        assert_eq!(
            supervisor.poll(1_001),
            Some(Event::HeartbeatMissed { task: 1 })
        );
        assert_eq!(supervisor.poll(1_500), None);
        assert!(supervisor.beat(1, 1_600));
        assert_eq!(
            supervisor.poll(2_700),
            Some(Event::HeartbeatMissed { task: 1 })
        );
    }

    #[test]
    fn watchdog_expires_and_rearms() {
        let mut supervisor = Supervisor::<4>::new(1_000, 3);
        assert_eq!(supervisor.poll(999), None);
        assert_eq!(supervisor.poll(1_000), Some(Event::WatchdogExpired));
        assert_eq!(supervisor.poll(1_000), None);
        supervisor.feed(2_000);
        assert_eq!(supervisor.poll(2_999), None);
        assert_eq!(supervisor.poll(3_000), Some(Event::WatchdogExpired));
    }

    #[test]
    fn boot_attempts_enter_safe_mode() {
        let mut supervisor = Supervisor::<1>::new(1_000, 3);
        assert!(!supervisor.note_boot_attempt());
        assert!(!supervisor.note_boot_attempt());
        assert!(supervisor.note_boot_attempt());
        assert!(supervisor.safe_mode());
        assert_eq!(supervisor.boot_attempts(), 3);
        supervisor.confirm_boot();
        assert!(!supervisor.safe_mode());
        assert_eq!(supervisor.boot_attempts(), 0);
    }

    #[test]
    fn registration_is_validated() {
        let mut supervisor = Supervisor::<1>::new(1_000, 3);
        assert_eq!(
            supervisor.register(1, 0, 0),
            Err(SupervisorError::InvalidPeriod)
        );
        supervisor.register(1, 100, 0).unwrap();
        assert_eq!(
            supervisor.register(1, 100, 0),
            Err(SupervisorError::Duplicate)
        );
        assert_eq!(
            supervisor.register(2, 100, 0),
            Err(SupervisorError::Capacity)
        );
        assert!(!supervisor.beat(9, 0));
    }

    #[test]
    fn next_deadline_is_the_earliest() {
        let mut supervisor = Supervisor::<4>::new(10_000, 3);
        supervisor.register(1, 500, 0).unwrap();
        supervisor.register(2, 2_000, 0).unwrap();
        assert_eq!(supervisor.next_deadline(), 500);
        supervisor.beat(1, 100);
        assert_eq!(supervisor.next_deadline(), 600);
    }
}
