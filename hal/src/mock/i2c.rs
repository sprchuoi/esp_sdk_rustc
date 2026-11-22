//! Mock i2c peripheral.

use crate::{Error, I2cBus, Result};

const MAX_DEVICES: usize = 2;
const REGISTER_SPACE: usize = 4096;

struct MockDevice {
    addr: u8,
    /// Address bytes sent before data: 1 for a register device, 2 for an EEPROM.
    address_bytes: u8,
    pointer: u16,
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

    /// Attach a register device at `addr` (one address byte).
    #[must_use]
    pub fn with_device(self, addr: u8) -> Self {
        self.with_addressed_device(addr, 1)
    }

    /// Attach a 2-byte-addressed EEPROM at `addr`.
    #[must_use]
    pub fn with_eeprom(self, addr: u8) -> Self {
        self.with_addressed_device(addr, 2)
    }

    /// Attach a device whose transfers start with `address_bytes` pointer bytes.
    ///
    /// # Panics
    /// Panics if the mock device table is full or `address_bytes` is not 1 or 2;
    /// both are test setup errors, not runtime conditions.
    #[must_use]
    pub fn with_addressed_device(mut self, addr: u8, address_bytes: u8) -> Self {
        assert!(
            address_bytes == 1 || address_bytes == 2,
            "address width must be 1 or 2"
        );
        let slot = self
            .devices
            .iter_mut()
            .find(|slot| slot.is_none())
            .expect("mock I2C device table full");
        *slot = Some(MockDevice {
            addr,
            address_bytes,
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
        let width = usize::from(device.address_bytes);
        if bytes.len() < width {
            return Err(Error::InvalidArgument);
        }
        let (pointer_bytes, data) = bytes.split_at(width);
        let mut pointer = 0u16;
        for &byte in pointer_bytes {
            pointer = (pointer << 8) | u16::from(byte);
        }
        device.pointer = pointer;
        for byte in data {
            let index = usize::from(device.pointer) % REGISTER_SPACE;
            device.memory[index] = *byte;
            device.pointer = device.pointer.wrapping_add(1) % REGISTER_SPACE as u16;
        }
        Ok(())
    }

    async fn read(&mut self, addr: u8, buf: &mut [u8]) -> Result<()> {
        let device = self.device_mut(addr)?;
        for slot in buf {
            let index = usize::from(device.pointer) % REGISTER_SPACE;
            *slot = device.memory[index];
            device.pointer = device.pointer.wrapping_add(1) % REGISTER_SPACE as u16;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mock::block_on;

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
}
