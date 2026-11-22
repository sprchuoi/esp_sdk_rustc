//! EEPROM device class.
//!
//! An EEPROM is byte-addressable, page-writable and slow to write: the part is
//! busy for a few milliseconds after each page. The trait exposes those facts so
//! a caller cannot accidentally treat it as flash (or spin-wait through the
//! write cycle).

use crate::DriverError;

pub mod at24;

pub use at24::{At24, At24Config, WRITE_CYCLE_MS};

/// A byte-addressable EEPROM.
pub trait Eeprom {
    /// Total capacity in bytes.
    fn capacity(&self) -> u32;

    /// Write granularity in bytes (the page size).
    fn page_size(&self) -> usize;

    /// Read `buf.len()` bytes starting at `offset`.
    async fn read(&mut self, offset: u32, buf: &mut [u8]) -> Result<(), DriverError>;

    /// Write `data` starting at `offset`, splitting at page boundaries and
    /// waiting out the write cycle between pages.
    async fn write(&mut self, offset: u32, data: &[u8]) -> Result<(), DriverError>;
}
