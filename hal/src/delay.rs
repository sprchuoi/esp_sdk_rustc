//! Blocking delays.

/// Blocking, short delays. Implementations must not sleep the executor.
pub trait Delay {
    /// Spin or busy-wait for `us` microseconds.
    fn delay_us(&mut self, us: u32);

    /// Delay for `ms` milliseconds.
    fn delay_ms(&mut self, ms: u32) {
        self.delay_us(ms.saturating_mul(1_000));
    }
}
