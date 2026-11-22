#![no_std]
#![forbid(unsafe_code)]
#![deny(missing_docs)]
// Async methods are used as bounds on concrete drivers, never as `dyn Trait`,
// so the auto-trait leakage this lint warns about cannot occur here.
#![allow(async_fn_in_trait)]

//! Hardware abstraction layer (L1).
//!
//! The HAL is the contract between the kernel/services (upper layers) and a
//! chip implementation (lower layers). It contains **traits only**: no chip
//! constants, no register access, no `esp-hal` import. That is what allows the
//! kernel to compile and be unit-tested on the host, and what makes a new SoC a
//! new implementation rather than a fork.
//!
//! Implementations live in `soc/` (silicon) or [`mock`] (host tests).
//!
//! Each peripheral lives in its own module and is re-exported at the crate root,
//! so drivers use `esp_sdk_hal::I2cBus` regardless of file layout.
//!
//! # Contract for implementors
//!
//! - All methods are non-blocking in the async sense: they return a future that
//!   completes when the transfer is done.
//! - Errors are typed; no implementation may panic on a bus error.
//! - `read`/`write` lengths are bounded by the caller; implementations must not
//!   allocate.
//! - Addresses passed to buses are already validated by the driver (7-bit for
//!   I2C); an implementation may still reject them with [`Error::InvalidAddress`].

pub mod delay;
pub mod error;
pub mod flash;
pub mod gpio;
pub mod i2c;
pub mod net;
pub mod rng;
pub mod serial;
pub mod spi;
pub mod timer;

pub use delay::Delay;
pub use error::{Error, Result};
pub use flash::Flash;
pub use gpio::GpioPin;
pub use i2c::I2cBus;
pub use net::{LinkState, MacAddr, NetDevice, NetStats};
pub use rng::Rng;
pub use serial::Serial;
pub use spi::SpiBus;
pub use timer::Timer;

#[cfg(any(test, feature = "mock"))]
pub mod mock;
