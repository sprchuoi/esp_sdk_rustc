//! Mock serial peripheral.

use crate::{Error, Result, Serial};

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mock::block_on;

    #[test]
    fn serial_accumulates_writes() {
        let mut port = MockSerial::new();
        block_on(port.write(b"hello ")).unwrap();
        block_on(port.write(b"world")).unwrap();
        assert_eq!(port.written(), b"hello world");
    }
}
