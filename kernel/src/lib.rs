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
//! the executor drives [`sched::Scheduler::next`]. That split is what lets the
//! scheduler be unit-tested on the host with no hardware and no `std`.
//!
//! Later phases add, behind the same style of pure logic:
//! interrupt dispatch tables, the sync primitives, the event bus, the KV store,
//! the driver core and the power policy.

pub mod sched;
