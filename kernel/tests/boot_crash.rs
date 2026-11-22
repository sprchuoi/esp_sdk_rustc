//! Integration test: a panic leaves a crash record, the next boot reads it, and
//! three failed boots end in safe mode.
//!
//! This exercises the public boot-facing API only: `metrics::CrashRecord` (the
//! bytes the panic handler writes to RTC RAM) and `supervisor::Supervisor` (the
//! boot-attempt/heartbeat policy).

use esp_sdk_kernel::metrics::{CRASH_RECORD_LEN, CrashRecord};
use esp_sdk_kernel::supervisor::{Event, Supervisor};

fn panic_record() -> CrashRecord {
    CrashRecord {
        reset_reason: 3,
        fault: 0x0101,
        task: 2,
        uptime_us: 12_345,
        boot_count: 4,
    }
}

#[test]
fn a_panic_record_survives_the_reboot() {
    let raw: [u8; CRASH_RECORD_LEN] = panic_record().to_bytes();

    // Next boot: the bytes are still in RTC RAM.
    let decoded = CrashRecord::from_bytes(&raw).expect("valid crash record");
    assert_eq!(decoded, panic_record());
    assert_eq!(decoded.fault, 0x0101);
    assert_eq!(decoded.task, 2);
}

#[test]
fn corrupt_rtc_bytes_are_ignored() {
    let mut raw = panic_record().to_bytes();
    raw[5] ^= 0xFF;
    assert!(CrashRecord::from_bytes(&raw).is_none());

    let mut raw = panic_record().to_bytes();
    raw[0] = 0;
    assert!(CrashRecord::from_bytes(&raw).is_none());
}

#[test]
fn three_failed_boots_enter_safe_mode_and_confirmation_clears_it() {
    let mut supervisor = Supervisor::<4>::new(10_000_000, 3);

    assert!(!supervisor.note_boot_attempt());
    assert!(!supervisor.note_boot_attempt());
    assert!(supervisor.note_boot_attempt());
    assert!(supervisor.safe_mode());
    assert_eq!(supervisor.boot_attempts(), 3);

    // The board would now start only console + provisioning.
    supervisor.confirm_boot();
    assert!(!supervisor.safe_mode());
    assert_eq!(supervisor.boot_attempts(), 0);
}

#[test]
fn a_hung_service_is_reported_once_per_miss() {
    let mut supervisor = Supervisor::<4>::new(10_000_000, 3);
    supervisor.register(7, 1_000, 0).unwrap();

    assert!(supervisor.beat(7, 500));
    assert_eq!(supervisor.poll(900), None);
    assert_eq!(
        supervisor.poll(1_600),
        Some(Event::HeartbeatMissed { task: 7 })
    );
    // Still missed: no second event until the task beats again.
    assert_eq!(supervisor.poll(2_000), None);
    assert!(supervisor.beat(7, 2_100));
    assert_eq!(
        supervisor.poll(3_200),
        Some(Event::HeartbeatMissed { task: 7 })
    );
    assert_eq!(supervisor.poll(3_300), None);
}

#[test]
fn watchdog_expiry_is_independent_of_heartbeats() {
    let mut supervisor = Supervisor::<4>::new(5_000, 3);
    supervisor.register(1, 100_000, 0).unwrap();

    assert_eq!(supervisor.poll(4_999), None);
    assert_eq!(supervisor.poll(5_000), Some(Event::WatchdogExpired));
    supervisor.feed(5_000);
    assert_eq!(supervisor.poll(9_999), None);
    assert_eq!(supervisor.poll(10_000), Some(Event::WatchdogExpired));
}
