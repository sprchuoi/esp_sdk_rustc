//! Serial port.

use crate::Result;

/// Asynchronous byte-oriented serial port (console, log sink).
pub trait Serial {
    /// Write all bytes.
    async fn write(&mut self, bytes: &[u8]) -> Result<()>;

    /// Read at least one byte, returning how many were read.
    async fn read(&mut self, buf: &mut [u8]) -> Result<usize>;
}
