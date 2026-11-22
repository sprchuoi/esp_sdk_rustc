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
//! # Contract for implementors
//!
//! - All methods are non-blocking in the async sense: they return a future that
//!   completes when the transfer is done.
//! - Errors are typed; no implementation may panic on a bus error.
//! - `read`/`write` lengths are bounded by the caller; implementations must not
//!   allocate.
//! - Addresses passed to buses are already validated by the driver (7-bit for
//!   I2C); an implementation may still reject them with [`Error::InvalidAddress`].

/// Result type used across the HAL.
pub type Result<T> = core::result::Result<T, Error>;

/// Errors a peripheral implementation may report.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Error {
    /// The bus reported an error (NACK, framing, CRC, ...).
    Bus,
    /// The operation did not complete before its deadline.
    Timeout,
    /// The addressed device does not exist on this bus.
    InvalidAddress,
    /// An argument was out of range for the peripheral.
    InvalidArgument,
    /// The peripheral cannot service the request yet (busy, not initialised).
    NotReady,
    /// A flash write hit a non-erased region.
    NotErased,
    /// The requested range is outside the device.
    OutOfRange,
    /// The peripheral exists in the SoC but this build does not support it.
    Unsupported,
}

/// Blocking, short delays. Implementations must not sleep the executor.
pub trait Delay {
    /// Spin or busy-wait for `us` microseconds.
    fn delay_us(&mut self, us: u32);

    /// Delay for `ms` milliseconds.
    fn delay_ms(&mut self, ms: u32) {
        self.delay_us(ms.saturating_mul(1_000));
    }
}

/// A digital output/input pin.
pub trait GpioPin {
    /// Drive the pin high or low.
    fn set(&mut self, high: bool);

    /// Read the pin level.
    fn get(&self) -> bool;
}

/// Asynchronous I2C controller.
pub trait I2cBus {
    /// Write `bytes` to the device at `addr`.
    async fn write(&mut self, addr: u8, bytes: &[u8]) -> Result<()>;

    /// Read into `buf` from the device at `addr`.
    async fn read(&mut self, addr: u8, buf: &mut [u8]) -> Result<()>;
}

/// Asynchronous SPI controller. Chip select is owned by the device driver.
pub trait SpiBus {
    /// Full-duplex transfer: `tx` is clocked out, `rx` is filled.
    ///
    /// `tx` and `rx` must have the same length; implementations return
    /// [`Error::InvalidArgument`] otherwise.
    async fn transfer(&mut self, tx: &[u8], rx: &mut [u8]) -> Result<()>;
}

/// Asynchronous byte-oriented serial port (console, log sink).
pub trait Serial {
    /// Write all bytes.
    async fn write(&mut self, bytes: &[u8]) -> Result<()>;

    /// Read at least one byte, returning how many were read.
    async fn read(&mut self, buf: &mut [u8]) -> Result<usize>;
}

/// Byte-addressable NOR flash.
///
/// Writes require a prior erase of the affected region; implementations must
/// return [`Error::NotErased`] rather than silently ANDing bits.
pub trait Flash {
    /// Total capacity in bytes.
    fn capacity(&self) -> u64;

    /// Erase granularity in bytes (4 KiB on SPI NOR).
    fn erase_size(&self) -> u32;

    /// Read into `buf`.
    async fn read(&mut self, offset: u64, buf: &mut [u8]) -> Result<()>;

    /// Erase `len` bytes starting at `offset`. `offset` and `len` must be
    /// multiples of [`Flash::erase_size`].
    async fn erase(&mut self, offset: u64, len: u64) -> Result<()>;

    /// Program `data` at `offset`.
    async fn write(&mut self, offset: u64, data: &[u8]) -> Result<()>;
}

/// Hardware random number generator.
pub trait Rng {
    /// Fill `buf` with random bytes.
    fn fill(&mut self, buf: &mut [u8]);
}

#[cfg(any(test, feature = "mock"))]
pub mod mock;

#[cfg(test)]
mod tests {
    use super::mock::*;
    use super::*;

    #[test]
    fn i2c_register_protocol_roundtrip() {
        let mut bus = MockI2c::new().with_device(0x50);
        block_on(bus.write(0x50, &[0x10, 0xAA, 0xBB])).unwrap();

        let mut buf = [0u8; 2];
        block_on(bus.write(0x50, &[0x10])).unwrap();
        block_on(bus.read(0x50, &mut buf)).unwrap();
        assert_eq!(buf, [0xAA, 0xBB]);

        assert_eq!(
            bus.memory(0x50).map(|m| &m[0x10..0x12]),
            Some(&[0xAA, 0xBB][..])
        );
    }

    #[test]
    fn i2c_unknown_address_is_an_error() {
        let mut bus = MockI2c::new();
        assert_eq!(
            block_on(bus.write(0x42, &[0x00])),
            Err(Error::InvalidAddress)
        );
        assert_eq!(
            block_on(bus.read(0x42, &mut [0u8; 1])),
            Err(Error::InvalidAddress)
        );
    }

    #[test]
    fn gpio_state_is_observable() {
        let mut pin = MockGpio::new();
        assert!(!pin.get());
        pin.set(true);
        assert!(pin.get());
        assert_eq!(pin.writes(), 1);
        pin.set(true);
        assert_eq!(pin.writes(), 1);
    }

    #[test]
    fn spi_loopback_copies_tx_to_rx() {
        let mut spi = MockSpi::loopback();
        let tx = [0x01, 0x02, 0x03];
        let mut rx = [0u8; 3];
        block_on(spi.transfer(&tx, &mut rx)).unwrap();
        assert_eq!(rx, tx);
    }

    #[test]
    fn spi_rejects_length_mismatch() {
        let mut spi = MockSpi::loopback();
        let mut rx = [0u8; 2];
        assert_eq!(
            block_on(spi.transfer(&[0u8; 3], &mut rx)),
            Err(Error::InvalidArgument)
        );
    }

    #[test]
    fn serial_accumulates_writes() {
        let mut port = MockSerial::new();
        block_on(port.write(b"hello ")).unwrap();
        block_on(port.write(b"world")).unwrap();
        assert_eq!(port.written(), b"hello world");
    }

    #[test]
    fn flash_requires_erase_before_write() {
        let mut flash = MockFlash::new(256, 64);

        // Erased flash accepts a first program.
        block_on(flash.write(0, b"x")).unwrap();
        // Programming the same byte again without an erase must fail.
        assert_eq!(block_on(flash.write(0, b"y")), Err(Error::NotErased));

        block_on(flash.erase(0, 64)).unwrap();
        block_on(flash.write(0, b"ok")).unwrap();

        let mut buf = [0u8; 2];
        block_on(flash.read(0, &mut buf)).unwrap();
        assert_eq!(&buf, b"ok");
    }

    #[test]
    fn flash_rejects_unaligned_erase_and_out_of_range() {
        let mut flash = MockFlash::new(256, 64);
        assert_eq!(block_on(flash.erase(1, 64)), Err(Error::InvalidArgument));
        assert_eq!(block_on(flash.erase(0, 512)), Err(Error::OutOfRange));
        assert_eq!(
            block_on(flash.read(255, &mut [0u8; 2])),
            Err(Error::OutOfRange)
        );
    }

    #[test]
    fn delay_accumulates() {
        let mut delay = MockDelay::new();
        delay.delay_us(10);
        delay.delay_ms(2);
        assert_eq!(delay.elapsed_us(), 10 + 2_000);
    }
}
