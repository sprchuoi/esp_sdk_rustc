//! AT24Cxx I2C EEPROM driver.
//!
//! Wire protocol: a transfer starts with a one- or two-byte big-endian address
//! pointer, followed by data. The driver splits writes at page boundaries and
//! waits [`WRITE_CYCLE_MS`] after each page, because the part NACKs while its
//! internal write cycle runs.
//!
//! Address width follows capacity (≤ 256 bytes uses one pointer byte), which is
//! how the AT24 family actually addresses its array.

use crate::DriverError;
use crate::eeprom::Eeprom;
use crate::hal::{Delay, I2cBus};

/// Typical `tWR`, the time the part is busy after a page write.
pub const WRITE_CYCLE_MS: u32 = 5;

/// Largest page size supported by the AT24 family.
const MAX_PAGE: usize = 128;

/// Largest AT24 part (AT24C1024 = 1 Mbit).
const MAX_CAPACITY: u32 = 1_048_576;

/// Configuration for one AT24 part.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct At24Config {
    addr: u8,
    capacity: u32,
    page_size: u16,
}

impl At24Config {
    /// Validate and build a configuration.
    ///
    /// # Errors
    /// Returns [`DriverError::InvalidConfig`] when the address is outside
    /// `0x50..=0x57`, the capacity is not a power of two in `32..=1048576`, or
    /// the page size is not a power of two in `8..=128` and at most the capacity.
    pub const fn new(addr: u8, capacity: u32, page_size: u16) -> Result<Self, DriverError> {
        if addr < 0x50 || addr > 0x57 {
            return Err(DriverError::InvalidConfig(
                "AT24 address must be in 0x50..=0x57",
            ));
        }
        if capacity < 32 || capacity > MAX_CAPACITY || !capacity.is_power_of_two() {
            return Err(DriverError::InvalidConfig(
                "AT24 capacity must be a power of two in 32..=1048576",
            ));
        }
        if page_size < 8
            || page_size > 128
            || !(page_size as u32).is_power_of_two()
            || page_size as u32 > capacity
        {
            return Err(DriverError::InvalidConfig(
                "AT24 page size must be a power of two in 8..=128",
            ));
        }
        Ok(Self {
            addr,
            capacity,
            page_size,
        })
    }

    /// I2C address.
    #[must_use]
    pub const fn addr(&self) -> u8 {
        self.addr
    }

    /// Capacity in bytes.
    #[must_use]
    pub const fn capacity(&self) -> u32 {
        self.capacity
    }

    /// Page size in bytes.
    #[must_use]
    pub const fn page_size(&self) -> u16 {
        self.page_size
    }

    /// Number of address-pointer bytes implied by the capacity.
    #[must_use]
    pub const fn address_bytes(&self) -> u8 {
        if self.capacity <= 256 { 1 } else { 2 }
    }
}

/// An AT24Cxx device on an I2C bus.
pub struct At24<B: I2cBus, D: Delay> {
    bus: B,
    delay: D,
    config: At24Config,
}

impl<B: I2cBus, D: Delay> At24<B, D> {
    /// Build a driver from a validated configuration.
    #[must_use]
    pub const fn new(bus: B, delay: D, config: At24Config) -> Self {
        Self { bus, delay, config }
    }

    /// The device configuration.
    #[must_use]
    pub const fn config(&self) -> &At24Config {
        &self.config
    }

    fn check_range(&self, offset: u32, len: usize) -> Result<(), DriverError> {
        let end = u64::from(offset) + len as u64;
        if end > u64::from(self.config.capacity) {
            return Err(DriverError::OutOfRange);
        }
        Ok(())
    }

    fn pointer(&self, offset: u32, frame: &mut [u8; MAX_PAGE + 2]) -> usize {
        let width = usize::from(self.config.address_bytes());
        if width == 1 {
            frame[0] = offset as u8;
        } else {
            frame[0] = (offset >> 8) as u8;
            frame[1] = offset as u8;
        }
        width
    }
}

impl<B: I2cBus, D: Delay> Eeprom for At24<B, D> {
    fn capacity(&self) -> u32 {
        self.config.capacity
    }

    fn page_size(&self) -> usize {
        usize::from(self.config.page_size)
    }

    async fn read(&mut self, offset: u32, buf: &mut [u8]) -> Result<(), DriverError> {
        self.check_range(offset, buf.len())?;
        if buf.is_empty() {
            return Ok(());
        }
        let mut frame = [0u8; MAX_PAGE + 2];
        let width = self.pointer(offset, &mut frame);
        self.bus.write(self.config.addr, &frame[..width]).await?;
        self.bus.read(self.config.addr, buf).await?;
        Ok(())
    }

    async fn write(&mut self, offset: u32, data: &[u8]) -> Result<(), DriverError> {
        self.check_range(offset, data.len())?;
        if data.is_empty() {
            return Ok(());
        }

        let page = usize::from(self.config.page_size);
        let width = usize::from(self.config.address_bytes());
        let mut written = 0usize;
        let mut frame = [0u8; MAX_PAGE + 2];

        while written < data.len() {
            let absolute = offset as usize + written;
            let in_page = absolute % page;
            let chunk = (page - in_page).min(data.len() - written);

            let pointer_width = self.pointer(absolute as u32, &mut frame);
            frame[pointer_width..pointer_width + chunk]
                .copy_from_slice(&data[written..written + chunk]);

            self.bus
                .write(self.config.addr, &frame[..width + chunk])
                .await?;
            self.delay.delay_ms(WRITE_CYCLE_MS);
            written += chunk;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hal::mock::{MockDelay, MockI2c, block_on};

    fn device() -> At24<MockI2c, MockDelay> {
        let bus = MockI2c::new().with_eeprom(0x50);
        let config = At24Config::new(0x50, 4096, 32).unwrap();
        At24::new(bus, MockDelay::new(), config)
    }

    #[test]
    fn config_rejects_bad_parameters() {
        assert!(At24Config::new(0x00, 4096, 32).is_err());
        assert!(At24Config::new(0x58, 4096, 32).is_err());
        assert!(At24Config::new(0x50, 1000, 32).is_err());
        assert!(At24Config::new(0x50, 16, 8).is_err());
        assert!(At24Config::new(0x50, 4096, 33).is_err());
        assert!(At24Config::new(0x50, 4096, 256).is_err());
        assert!(At24Config::new(0x50, 4096, 32).is_ok());
    }

    #[test]
    fn address_width_follows_capacity() {
        assert_eq!(At24Config::new(0x50, 256, 8).unwrap().address_bytes(), 1);
        assert_eq!(At24Config::new(0x50, 512, 16).unwrap().address_bytes(), 2);
        assert_eq!(
            At24Config::new(0x50, 1_048_576, 128)
                .unwrap()
                .address_bytes(),
            2
        );
    }

    #[test]
    fn roundtrip_across_a_page_boundary() {
        let mut dev = device();
        let data: [u8; 70] = core::array::from_fn(|i| i as u8);

        block_on(dev.write(30, &data)).unwrap();

        let mut read_back = [0u8; 70];
        block_on(dev.read(30, &mut read_back)).unwrap();
        assert_eq!(read_back, data);

        // 70 bytes from offset 30, page 32 -> 2 + 32 + 32 + 4 = 4 pages.
        assert_eq!(dev.delay.elapsed_us(), 4 * WRITE_CYCLE_MS * 1_000);
    }

    #[test]
    fn page_aligned_write_uses_one_cycle() {
        let mut dev = device();
        block_on(dev.write(0, &[0xAB; 32])).unwrap();
        assert_eq!(dev.delay.elapsed_us(), WRITE_CYCLE_MS * 1_000);
    }

    #[test]
    fn two_byte_pointer_addresses_the_right_byte() {
        let mut dev = device();
        block_on(dev.write(0x123, &[0x11, 0x22])).unwrap();

        let mut read_back = [0u8; 2];
        block_on(dev.read(0x123, &mut read_back)).unwrap();
        assert_eq!(read_back, [0x11, 0x22]);
    }

    #[test]
    fn writes_outside_capacity_are_rejected() {
        let mut dev = device();
        assert_eq!(
            block_on(dev.write(4095, &[0; 2])),
            Err(DriverError::OutOfRange)
        );
        assert_eq!(
            block_on(dev.read(4096, &mut [0u8; 1])),
            Err(DriverError::OutOfRange)
        );
    }

    #[test]
    fn empty_transfers_are_noops() {
        let mut dev = device();
        block_on(dev.write(0, &[])).unwrap();
        block_on(dev.read(0, &mut [])).unwrap();
        assert_eq!(dev.delay.elapsed_us(), 0);
    }

    #[test]
    fn capacity_and_page_size_are_exposed() {
        let dev = device();
        assert_eq!(Eeprom::capacity(&dev), 4096);
        assert_eq!(Eeprom::page_size(&dev), 32);
    }
}
