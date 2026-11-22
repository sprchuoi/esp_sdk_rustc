//! Mock spi peripheral.

use crate::{Error, Result, SpiBus};

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mock::block_on;

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
}
