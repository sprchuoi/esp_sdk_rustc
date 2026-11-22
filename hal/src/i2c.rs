//! I2C bus.

use crate::Result;

/// Asynchronous I2C controller.
pub trait I2cBus {
    /// Write `bytes` to the device at `addr`.
    async fn write(&mut self, addr: u8, bytes: &[u8]) -> Result<()>;

    /// Read into `buf` from the device at `addr`.
    async fn read(&mut self, addr: u8, buf: &mut [u8]) -> Result<()>;
}
