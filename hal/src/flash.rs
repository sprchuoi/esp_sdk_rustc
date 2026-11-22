//! Byte-addressable flash.

use crate::Result;

/// Byte-addressable NOR flash.
///
/// Writes require a prior erase of the affected region; implementations must
/// return [`crate::Error::NotErased`] rather than silently ANDing bits.
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
