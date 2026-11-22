#![no_std]
#![forbid(unsafe_code)]
#![deny(missing_docs)]

//! TinyRTOS kernel — host-testable core.
//!
//! This is the Phase 1 seed of the kernel. It currently contains the
//! deterministic scheduler core ([`sched`]): priority classes, round-robin
//! ready queues and a fixed-capacity timer heap.
//!
//! The module deliberately knows nothing about a CPU, a clock or an executor.
//! The architecture layer feeds it wake events and a monotonic timestamp, and
//! the executor drives [`sched::Scheduler::next_ready`]. That split is what lets the
//! scheduler be unit-tested on the host with no hardware and no `std`.
//!
//! Beyond the scheduler, the implemented pieces are [`time`] (monotonic
//! instants), [`sync::Signal`] (ISR-to-task wake), [`log`] (level filter and a
//! retained-record ring), [`service`] (lifecycle and restart policy), [`crypto`]
//! (SHA-256/HMAC), [`power`] (sleep policy), [`metrics`] (counters, memory
//! accounting, faults and the diagnostics dump) and [`config`] (compile-time
//! limits).
//!
//! Later phases add, behind the same style of pure logic: interrupt dispatch
//! tables, the executor integration, the event bus, the KV store, the driver
//! core and the power policy.

pub mod config;
pub mod crypto;
pub mod log;
pub mod metrics;
pub mod power;
pub mod sched;
pub mod service;
pub mod sync;
pub mod time;

#[cfg(test)]
extern crate std;
