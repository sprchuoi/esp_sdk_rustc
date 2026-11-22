//! Mock delay peripheral.

use crate::Delay;

/// A delay that records accumulated time instead of waiting.
pub struct MockDelay {
    elapsed_us: u32,
}

impl Default for MockDelay {
    fn default() -> Self {
        Self::new()
    }
}

impl MockDelay {
    /// Create a delay at zero.
    #[must_use]
    pub const fn new() -> Self {
        Self { elapsed_us: 0 }
    }

    /// Total microseconds requested so far.
    #[must_use]
    pub const fn elapsed_us(&self) -> u32 {
        self.elapsed_us
    }
}

impl Delay for MockDelay {
    fn delay_us(&mut self, us: u32) {
        self.elapsed_us = self.elapsed_us.saturating_add(us);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn delay_accumulates() {
        let mut delay = MockDelay::new();
        delay.delay_us(10);
        delay.delay_ms(2);
        assert_eq!(delay.elapsed_us(), 10 + 2_000);
    }
}
