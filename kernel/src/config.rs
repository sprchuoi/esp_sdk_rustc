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
