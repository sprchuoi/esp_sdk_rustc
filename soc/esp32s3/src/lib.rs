#![no_std]
#![deny(missing_docs)]

//! ESP32-S3 platform layer: the first concrete implementation of the SDK HAL.
//!
//! This crate is where `esp-hal` is allowed: it turns the chip's drivers into
//! the traits the kernel and services use, so nothing above sees `esp-hal`
//! types. The seed covers the console path (UART0) and blocking delays; GPIO,
//! I2C, SPI and flash follow the same shape.
//!
//! Board wiring stays outside: the caller passes the UART instance and TX pin,
//! so a different board is a different call, not a different crate.

use core::fmt;

use esp_sdk_hal as hal;

/// UART0 TX on the ESP32-S3-DevKitC-1.
pub const UART0_TX_GPIO: u8 = 43;
/// UART0 RX on the ESP32-S3-DevKitC-1.
pub const UART0_RX_GPIO: u8 = 44;

/// The UART0 console.
///
/// Writes go through esp-println's UART0 backend (GPIO43 TX / GPIO44 RX on the
/// DevKitC-1), which is already initialised by the boot path. Reading is not
/// wired yet: the console is write-only until the RX interrupt path lands.
pub struct Serial;

impl Serial {
    /// Create the console.
    #[must_use]
    pub const fn uart0() -> Self {
        Self
    }
}

impl fmt::Write for Serial {
    fn write_str(&mut self, text: &str) -> fmt::Result {
        esp_println::Printer::write_bytes(text.as_bytes());
        Ok(())
    }
}

impl hal::Serial for Serial {
    async fn write(&mut self, bytes: &[u8]) -> hal::Result<()> {
        esp_println::Printer::write_bytes(bytes);
        Ok(())
    }

    async fn read(&mut self, _buf: &mut [u8]) -> hal::Result<usize> {
        Err(hal::Error::Unsupported)
    }
}

/// Busy-wait delay backed by `esp-hal`.
pub struct Delay {
    inner: esp_hal::delay::Delay,
}

impl Default for Delay {
    fn default() -> Self {
        Self::new()
    }
}

impl Delay {
    /// Create a delay.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            inner: esp_hal::delay::Delay::new(),
        }
    }
}

impl hal::Delay for Delay {
    fn delay_us(&mut self, us: u32) {
        self.inner.delay_micros(us);
    }
}
