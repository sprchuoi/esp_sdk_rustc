//! Random number generator.

/// Hardware random number generator.
pub trait Rng {
    /// Fill `buf` with random bytes.
    fn fill(&mut self, buf: &mut [u8]);
}
