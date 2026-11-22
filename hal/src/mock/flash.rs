//! Mock flash peripheral.

use crate::{Error, Flash, Result};

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mock::block_on;

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
}
