//! Compile-time kernel configuration.
//!
//! These are the knobs a product build may change. Today they are constants;
//! Phase 2 replaces them with values generated from Kconfig
//! (`sdkconfig.rs`) so that `menuconfig` can set them without touching source.
//!
//! Keeping the limits in one module means the scheduler, the log ring and the
//! future timer wheel cannot disagree about them.

use crate::log::Level;

/// Maximum number of schedulable tasks.
pub const MAX_TASKS: usize = 32;

/// Maximum number of pending timers.
pub const MAX_TIMERS: usize = 32;

/// Capacity of the retained log ring.
pub const LOG_RING_CAPACITY: usize = 64;

/// Default runtime log threshold; the console can raise or lower it.
pub const DEFAULT_LOG_LEVEL: Level = Level::Info;

/// Default number of services the manager can hold.
pub const MAX_SERVICES: usize = 16;

/// Default number of named counters in the diagnostics bundle.
pub const METRIC_CAPACITY: usize = 24;

/// Default number of tracked memory regions.
pub const MEMORY_REGIONS: usize = 4;

/// Default number of distinct fault codes retained.
pub const FAULT_CAPACITY: usize = 16;

/// How long a ready task may wait for a lower class to be boosted.
///
/// After this, a ready `App`/`Net` task is promoted one class for one run, so a
/// busy high-priority class cannot starve telemetry forever.
pub const AGING_THRESHOLD_US: u64 = 100_000;
