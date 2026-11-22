//! SPI bus.

use crate::Result;

/// Asynchronous SPI controller. Chip select is owned by the device driver.
pub trait SpiBus {
    /// Full-duplex transfer: `tx` is clocked out, `rx` is filled.
    ///
    /// `tx` and `rx` must have the same length; implementations return
    /// [`crate::Error::InvalidArgument`] otherwise.
    async fn transfer(&mut self, tx: &[u8], rx: &mut [u8]) -> Result<()>;
}
