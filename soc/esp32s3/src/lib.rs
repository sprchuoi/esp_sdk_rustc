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

use esp_hal::Blocking;
use esp_hal::gpio::interconnect::{PeripheralInput, PeripheralOutput};
use esp_hal::gpio::{Level, Output as HalOutput, OutputConfig, OutputPin};
use esp_hal::i2c::master::{Config as I2cConfig, ConfigError as I2cError, I2c as HalI2c};
use esp_hal::spi::master::{Config as SpiConfig, Spi as HalSpi};
use esp_sdk_hal as hal;

/// UART0 register block on the ESP32-S3.
const UART0_BASE: usize = 0x6000_0000;
/// `UART_STATUS_REG`: RX FIFO count is bits [9:0].
const UART0_STATUS: usize = 0x1c;

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

    /// Pop one received byte, if the UART0 RX FIFO has one.
    ///
    /// This reads the hardware FIFO directly: esp-hal's `Uart` initialisation is
    /// not usable here yet (it stalls on the UART register-update handshake
    /// under esp32sim), and the bootloader has already configured UART0 for its
    /// console.
    #[must_use]
    pub fn read_byte(&self) -> Option<u8> {
        // SAFETY: UART0 is a fixed MMIO block; reading STATUS then FIFO pops one
        // received byte. Single reader (the console task) by contract.
        unsafe {
            let status = core::ptr::read_volatile((UART0_BASE + UART0_STATUS) as *const u32);
            if status & 0x3ff == 0 {
                return None;
            }
            Some(core::ptr::read_volatile(UART0_BASE as *const u32) as u8)
        }
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

    async fn read(&mut self, buf: &mut [u8]) -> hal::Result<usize> {
        let mut count = 0;
        while count < buf.len() {
            match self.read_byte() {
                Some(byte) => {
                    buf[count] = byte;
                    count += 1;
                }
                None => break,
            }
        }
        if count == 0 {
            return Err(hal::Error::NotReady);
        }
        Ok(count)
    }
}

/// A GPIO output implementing the SDK [`hal::GpioPin`] trait.
pub struct Output<'d> {
    pin: HalOutput<'d>,
}

impl<'d> Output<'d> {
    /// Create an output, starting high or low.
    pub fn new(pin: impl OutputPin + 'd, high: bool) -> Self {
        let level = if high { Level::High } else { Level::Low };
        Self {
            pin: HalOutput::new(pin, level, OutputConfig::default()),
        }
    }
}

/// An I2C master implementing the SDK [`hal::I2cBus`] trait.
pub struct I2c<'d> {
    inner: HalI2c<'d, Blocking>,
}

impl<'d> I2c<'d> {
    /// Create an I2C master with the default 100 kHz configuration.
    ///
    /// # Errors
    /// [`I2cError`] when the bus configuration is not supported.
    pub fn new(
        i2c: impl esp_hal::i2c::master::Instance + 'd,
        sda: impl PeripheralInput<'d> + PeripheralOutput<'d>,
        scl: impl PeripheralInput<'d> + PeripheralOutput<'d>,
    ) -> Result<Self, I2cError> {
        Ok(Self {
            inner: HalI2c::new(i2c, I2cConfig::default())?
                .with_sda(sda)
                .with_scl(scl),
        })
    }

    /// Probe an address with a one-byte read. Returns whether it ACKed.
    pub fn probe(&mut self, address: u8) -> bool {
        self.inner.read(address, &mut [0u8]).is_ok()
    }
}

impl hal::I2cBus for I2c<'_> {
    async fn write(&mut self, addr: u8, bytes: &[u8]) -> hal::Result<()> {
        self.inner
            .write(addr, bytes)
            .map_err(|_| hal::Error::Bus)
    }

    async fn read(&mut self, addr: u8, buf: &mut [u8]) -> hal::Result<()> {
        self.inner.read(addr, buf).map_err(|_| hal::Error::Bus)
    }
}

/// An SPI master implementing the SDK [`hal::SpiBus`] trait.
pub struct Spi<'d> {
    inner: HalSpi<'d, Blocking>,
}

impl<'d> Spi<'d> {
    /// Create an SPI master with the default configuration.
    ///
    /// # Errors
    /// [`esp_hal::spi::master::ConfigError`] when the configuration is unsupported.
    pub fn new(
        spi: impl esp_hal::spi::master::Instance + 'd,
        sck: impl PeripheralOutput<'d>,
        mosi: impl PeripheralOutput<'d>,
        miso: impl PeripheralInput<'d>,
    ) -> Result<Self, esp_hal::spi::master::ConfigError> {
        Ok(Self {
            inner: HalSpi::new(spi, SpiConfig::default())?
                .with_sck(sck)
                .with_mosi(mosi)
                .with_miso(miso),
        })
    }
}

impl hal::SpiBus for Spi<'_> {
    async fn transfer(&mut self, tx: &[u8], rx: &mut [u8]) -> hal::Result<()> {
        if tx.len() != rx.len() {
            return Err(hal::Error::InvalidArgument);
        }
        rx.copy_from_slice(tx);
        self.inner.transfer(rx).map_err(|_| hal::Error::Bus)
    }
}

impl hal::GpioPin for Output<'_> {
    fn set(&mut self, high: bool) {
        if high {
            self.pin.set_high();
        } else {
            self.pin.set_low();
        }
    }

    fn get(&self) -> bool {
        self.pin.is_set_high()
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
