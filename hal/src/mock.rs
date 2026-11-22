//! Host-side mock peripherals.
//!
//! These implementations are always ready, so they can be driven from a
//! single-threaded test with [`block_on`]. They are compiled for unit tests and
//! for the `mock` feature (other crates' tests, host simulations).

use crate::{Delay, Error, Flash, GpioPin, I2cBus, Result, Serial, SpiBus};
use core::future::Future;
use core::task::{Context, Poll, Waker};

/// Poll a future to completion using a no-op waker.
///
/// Only for futures that never return `Pending` (all mocks here). Panics if the
/// future does not complete within a bounded number of polls.
pub fn block_on<F: Future>(future: F) -> F::Output {
    let waker = Waker::noop();
    let mut cx = Context::from_waker(waker);
    let mut future = core::pin::pin!(future);
    for _ in 0..64 {
        if let Poll::Ready(output) = future.as_mut().poll(&mut cx) {
            return output;
        }
    }
    panic!("mock future stayed pending");
}

const MAX_DEVICES: usize = 4;
const REGISTER_SPACE: usize = 256;

struct MockDevice {
    addr: u8,
    pointer: u8,
    memory: [u8; REGISTER_SPACE],
}

/// An I2C bus with register-pointer devices.
///
/// Writing `[reg, b0, b1, ...]` sets the register pointer to `reg` and stores
/// the following bytes from there. Reading returns bytes from the current
/// pointer, auto-incrementing as a real EEPROM/register device does.
pub struct MockI2c {
    devices: [Option<MockDevice>; MAX_DEVICES],
}

impl Default for MockI2c {
    fn default() -> Self {
        Self::new()
    }
}

impl MockI2c {
    /// Create a bus with no devices attached.
    #[must_use]
    pub fn new() -> Self {
        Self {
            devices: core::array::from_fn(|_| None),
        }
    }

    /// Attach a device at `addr`, returning the bus for chaining.
    ///
    /// # Panics
    /// Panics if the four mock device slots are already used, which is a test
    /// setup error, not a runtime condition.
    #[must_use]
    pub fn with_device(mut self, addr: u8) -> Self {
        let slot = self
            .devices
            .iter_mut()
            .find(|slot| slot.is_none())
            .expect("mock I2C device table full");
        *slot = Some(MockDevice {
            addr,
            pointer: 0,
            memory: [0; REGISTER_SPACE],
        });
        self
    }

    /// Contents of a device's register space.
    #[must_use]
    pub fn memory(&self, addr: u8) -> Option<&[u8]> {
        self.devices
            .iter()
            .flatten()
            .find(|device| device.addr == addr)
            .map(|device| &device.memory[..REGISTER_SPACE])
    }

    fn device_mut(&mut self, addr: u8) -> Result<&mut MockDevice> {
        self.devices
            .iter_mut()
            .flatten()
            .find(|device| device.addr == addr)
            .ok_or(Error::InvalidAddress)
    }
}

impl I2cBus for MockI2c {
    async fn write(&mut self, addr: u8, bytes: &[u8]) -> Result<()> {
        let device = self.device_mut(addr)?;
        let Some((&register, data)) = bytes.split_first() else {
            return Ok(());
        };
        device.pointer = register;
        for byte in data {
            let index = usize::from(device.pointer);
            device.memory[index] = *byte;
            device.pointer = device.pointer.wrapping_add(1);
        }
        Ok(())
    }

    async fn read(&mut self, addr: u8, buf: &mut [u8]) -> Result<()> {
        let device = self.device_mut(addr)?;
        for slot in buf {
            let index = usize::from(device.pointer);
            *slot = device.memory[index];
            device.pointer = device.pointer.wrapping_add(1);
        }
        Ok(())
    }
}

/// A digital pin whose level and write count are observable.
pub struct MockGpio {
    level: bool,
    writes: u32,
}

impl Default for MockGpio {
    fn default() -> Self {
        Self::new()
    }
}

impl MockGpio {
    /// Create a low pin.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            level: false,
            writes: 0,
        }
    }

    /// Number of level *changes* written so far.
    #[must_use]
    pub const fn writes(&self) -> u32 {
        self.writes
    }
}

impl GpioPin for MockGpio {
    fn set(&mut self, high: bool) {
        if high != self.level {
            self.level = high;
            self.writes = self.writes.saturating_add(1);
        }
    }

    fn get(&self) -> bool {
        self.level
    }
}

/// An SPI controller that either loops `tx` back into `rx` or sinks it.
pub struct MockSpi {
    loopback: bool,
    last_tx: [u8; 32],
    last_tx_len: usize,
}

impl MockSpi {
    /// A controller that copies `tx` into `rx`.
    #[must_use]
    pub const fn loopback() -> Self {
        Self {
            loopback: true,
            last_tx: [0; 32],
            last_tx_len: 0,
        }
    }

    /// A controller that returns zeroes.
    #[must_use]
    pub const fn sink() -> Self {
        Self {
            loopback: false,
            last_tx: [0; 32],
            last_tx_len: 0,
        }
    }

    /// Bytes seen by the most recent transfer (truncated to 32).
    #[must_use]
    pub fn last_tx(&self) -> &[u8] {
        &self.last_tx[..self.last_tx_len]
    }
}

impl SpiBus for MockSpi {
    async fn transfer(&mut self, tx: &[u8], rx: &mut [u8]) -> Result<()> {
        if tx.len() != rx.len() {
            return Err(Error::InvalidArgument);
        }
        if tx.len() > self.last_tx.len() {
            return Err(Error::InvalidArgument);
        }
        self.last_tx[..tx.len()].copy_from_slice(tx);
        self.last_tx_len = tx.len();
        if self.loopback {
            rx.copy_from_slice(tx);
        } else {
            rx.fill(0);
        }
        Ok(())
    }
}

const SERIAL_BUFFER: usize = 512;

/// A serial port that records written bytes and serves scripted input.
pub struct MockSerial {
    output: [u8; SERIAL_BUFFER],
    output_len: usize,
    input: [u8; SERIAL_BUFFER],
    input_len: usize,
    input_pos: usize,
}

impl Default for MockSerial {
    fn default() -> Self {
        Self::new()
    }
}

impl MockSerial {
    /// Create an empty port.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            output: [0; SERIAL_BUFFER],
            output_len: 0,
            input: [0; SERIAL_BUFFER],
            input_len: 0,
            input_pos: 0,
        }
    }

    /// Bytes written so far.
    #[must_use]
    pub fn written(&self) -> &[u8] {
        &self.output[..self.output_len]
    }

    /// Queue bytes to be returned by `read`.
    pub fn push_input(&mut self, bytes: &[u8]) {
        let room = SERIAL_BUFFER - self.input_len;
        let count = bytes.len().min(room);
        self.input[self.input_len..self.input_len + count].copy_from_slice(&bytes[..count]);
        self.input_len += count;
    }
}

impl Serial for MockSerial {
    async fn write(&mut self, bytes: &[u8]) -> Result<()> {
        if self.output_len + bytes.len() > SERIAL_BUFFER {
            return Err(Error::OutOfRange);
        }
        self.output[self.output_len..self.output_len + bytes.len()].copy_from_slice(bytes);
        self.output_len += bytes.len();
        Ok(())
    }

    async fn read(&mut self, buf: &mut [u8]) -> Result<usize> {
        let available = self.input_len - self.input_pos;
        if available == 0 {
            return Err(Error::NotReady);
        }
        let count = buf.len().min(available);
        buf[..count].copy_from_slice(&self.input[self.input_pos..self.input_pos + count]);
        self.input_pos += count;
        Ok(count)
    }
}

const FLASH_MAX: usize = 512;

/// A NOR flash mock with erase-before-write semantics.
pub struct MockFlash {
    data: [u8; FLASH_MAX],
    capacity: usize,
    erase_size: u32,
}

impl MockFlash {
    /// Create a flash of `capacity` bytes (at most 512) with `erase_size`.
    ///
    /// # Panics
    /// Panics if `capacity` exceeds the mock's backing store or `erase_size` is
    /// zero — both are test setup errors.
    #[must_use]
    pub fn new(capacity: usize, erase_size: usize) -> Self {
        assert!(capacity <= FLASH_MAX, "mock flash capacity too large");
        assert!(erase_size > 0, "erase size must be non-zero");
        Self {
            data: [0xFF; FLASH_MAX],
            capacity,
            erase_size: erase_size as u32,
        }
    }

    fn check(offset: u64, len: u64, capacity: usize) -> Result<usize> {
        let start = usize::try_from(offset).map_err(|_| Error::OutOfRange)?;
        let end = usize::try_from(offset + len).map_err(|_| Error::OutOfRange)?;
        if end > capacity {
            return Err(Error::OutOfRange);
        }
        Ok(start)
    }
}

impl Flash for MockFlash {
    fn capacity(&self) -> u64 {
        self.capacity as u64
    }

    fn erase_size(&self) -> u32 {
        self.erase_size
    }

    async fn read(&mut self, offset: u64, buf: &mut [u8]) -> Result<()> {
        let start = Self::check(offset, buf.len() as u64, self.capacity)?;
        buf.copy_from_slice(&self.data[start..start + buf.len()]);
        Ok(())
    }

    async fn erase(&mut self, offset: u64, len: u64) -> Result<()> {
        let erase = u64::from(self.erase_size);
        if !offset.is_multiple_of(erase) || !len.is_multiple_of(erase) {
            return Err(Error::InvalidArgument);
        }
        let start = Self::check(offset, len, self.capacity)?;
        let end = start + len as usize;
        self.data[start..end].fill(0xFF);
        Ok(())
    }

    async fn write(&mut self, offset: u64, data: &[u8]) -> Result<()> {
        let start = Self::check(offset, data.len() as u64, self.capacity)?;
        if self.data[start..start + data.len()]
            .iter()
            .any(|byte| *byte != 0xFF)
        {
            return Err(Error::NotErased);
        }
        self.data[start..start + data.len()].copy_from_slice(data);
        Ok(())
    }
}

/// A delay that records accumulated time instead of waiting.
pub struct MockDelay {
    elapsed_us: u32,
}

impl Default for MockDelay {
    fn default() -> Self {
        Self::new()
    }
}

impl MockDelay {
    /// Create a delay at zero.
    #[must_use]
    pub const fn new() -> Self {
        Self { elapsed_us: 0 }
    }

    /// Total microseconds requested so far.
    #[must_use]
    pub const fn elapsed_us(&self) -> u32 {
        self.elapsed_us
    }
}

impl Delay for MockDelay {
    fn delay_us(&mut self, us: u32) {
        self.elapsed_us = self.elapsed_us.saturating_add(us);
    }
}
