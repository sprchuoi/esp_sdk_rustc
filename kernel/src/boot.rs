//! Boot state that survives a reset.
//!
//! The panic handler writes a [`CrashRecord`] into RTC RAM; the next boot reads
//! it and decides whether to continue or enter safe mode. Keeping that decision
//! here (rather than in the panic handler) makes it testable on the host.

use crate::metrics::{CRASH_RECORD_LEN, CrashRecord};

/// Consecutive-boot accounting and the previous crash record.
#[derive(Clone, Copy, Debug, Default)]
pub struct BootManager {
    crash: Option<CrashRecord>,
    attempts: u8,
    max_attempts: u8,
    safe_mode: bool,
}

impl BootManager {
    /// Create a manager that enters safe mode after `max_attempts` bad boots.
    #[must_use]
    pub const fn new(max_attempts: u8) -> Self {
        Self {
            crash: None,
            attempts: 0,
            max_attempts,
            safe_mode: false,
        }
    }

    /// Decode the crash record left in RTC RAM by the panic handler.
    ///
    /// A corrupt or absent record is ignored; the return value is the decoded
    /// record, if any.
    pub fn read_previous(&mut self, rtc: Option<&[u8; CRASH_RECORD_LEN]>) -> Option<CrashRecord> {
        self.crash = rtc.and_then(CrashRecord::from_bytes);
        self.crash
    }

    /// The decoded crash record, if the previous boot crashed.
    #[must_use]
    pub const fn crash(&self) -> Option<CrashRecord> {
        self.crash
    }

    /// Count this boot; returns `true` when safe mode should be entered.
    pub fn note_boot(&mut self) -> bool {
        self.attempts = self.attempts.saturating_add(1);
        if self.attempts >= self.max_attempts {
            self.safe_mode = true;
        }
        self.safe_mode
    }

    /// This boot is healthy: clear the counter and safe mode.
    pub fn confirm(&mut self) {
        self.attempts = 0;
        self.safe_mode = false;
    }

    /// Whether the node is in safe mode.
    #[must_use]
    pub const fn safe_mode(&self) -> bool {
        self.safe_mode
    }

    /// Consecutive boot attempts.
    #[must_use]
    pub const fn attempts(&self) -> u8 {
        self.attempts
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn crashed() -> CrashRecord {
        CrashRecord {
            reset_reason: 3,
            fault: 0x0101,
            task: 2,
            uptime_us: 4_000,
            boot_count: 9,
        }
    }

    #[test]
    fn reads_the_previous_crash() {
        let mut manager = BootManager::new(3);
        let bytes = crashed().to_bytes();
        assert_eq!(manager.read_previous(Some(&bytes)), Some(crashed()));
        assert_eq!(manager.crash(), Some(crashed()));
    }

    #[test]
    fn ignores_corrupt_and_absent_records() {
        let mut manager = BootManager::new(3);
        assert_eq!(manager.read_previous(None), None);
        let mut bytes = crashed().to_bytes();
        bytes[5] ^= 0xFF;
        assert_eq!(manager.read_previous(Some(&bytes)), None);
    }

    #[test]
    fn three_bad_boots_enter_safe_mode() {
        let mut manager = BootManager::new(3);
        assert!(!manager.note_boot());
        assert!(!manager.note_boot());
        assert!(manager.note_boot());
        assert!(manager.safe_mode());
        manager.confirm();
        assert!(!manager.safe_mode());
        assert_eq!(manager.attempts(), 0);
    }
}
