#![no_std]
#![forbid(unsafe_code)]
#![deny(missing_docs)]
#![allow(async_fn_in_trait)]

//! Device drivers and the driver model.
//!
//! The model is Linux-like but static: every driver is linked in, declares the
//! `compatible` strings it handles, and the board's generated device tree is
//! resolved against that list. There are no modules, no hotplug and no `dyn Any`
//! — probing a concrete driver stays typed and board-owned, which keeps the
//! image small and failures compile-time.
//!
//! Implemented drivers:
//!
//! - [`eeprom::At24`] — AT24Cxx I2C EEPROM, page-aware writes, write-cycle delay.

/// Re-export so drivers can name HAL types without a second dependency.
pub use esp_sdk_hal as hal;

pub mod eeprom;
pub mod model;

/// Errors produced by drivers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DriverError {
    /// The underlying bus or peripheral failed.
    Bus(hal::Error),
    /// A configuration value (address, capacity, page size) is invalid.
    InvalidConfig(&'static str),
    /// The requested offset/length is outside the device.
    OutOfRange,
}

impl From<hal::Error> for DriverError {
    fn from(error: hal::Error) -> Self {
        Self::Bus(error)
    }
}
