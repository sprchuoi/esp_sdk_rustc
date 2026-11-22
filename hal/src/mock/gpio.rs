//! Mock gpio peripheral.

use crate::GpioPin;

/// A digital pin whose level and write count are observable.
pub struct MockGpio {
    level: bool,
    writes: u32,
}

impl Default for MockGpio {
    fn default() -> Self {
        Self::new()
    }
}

impl MockGpio {
    /// Create a low pin.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            level: false,
            writes: 0,
        }
    }

    /// Number of level *changes* written so far.
    #[must_use]
    pub const fn writes(&self) -> u32 {
        self.writes
    }
}

impl GpioPin for MockGpio {
    fn set(&mut self, high: bool) {
        if high != self.level {
            self.level = high;
            self.writes = self.writes.saturating_add(1);
        }
    }

    fn get(&self) -> bool {
        self.level
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gpio_state_is_observable() {
        let mut pin = MockGpio::new();
        assert!(!pin.get());
        pin.set(true);
        assert!(pin.get());
        assert_eq!(pin.writes(), 1);
        pin.set(true);
        assert_eq!(pin.writes(), 1);
    }
}
