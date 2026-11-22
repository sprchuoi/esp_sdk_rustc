//! Synchronisation primitives that need no allocator.
//!
//! This module starts with [`Signal`], the ISR-to-task wake primitive. It is
//! built on a single atomic and is therefore sound on both cores of the S3 and
//! on RISC-V parts.
//!
//! Blocking channels and mutexes need a waker and a critical section; they
//! arrive with the executor integration (Phase 1) and the arch layer, because
//! their soundness depends on interrupt masking, not on the kernel core.

use core::sync::atomic::{AtomicBool, Ordering};

/// A one-bit event flag.
///
/// `signal` is safe to call from an interrupt handler; `try_take` consumes the
/// bit. Losing a duplicate signal is intentional: a signal means "something
/// happened", and the task re-reads the source state.
#[derive(Debug, Default)]
pub struct Signal {
    flag: AtomicBool,
}

impl Signal {
    /// Create an unsignalled flag.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            flag: AtomicBool::new(false),
        }
    }

    /// Raise the flag. Idempotent.
    pub fn signal(&self) {
        self.flag.store(true, Ordering::Release);
    }

    /// Atomically consume the flag, returning whether it was set.
    pub fn try_take(&self) -> bool {
        self.flag.swap(false, Ordering::AcqRel)
    }

    /// Whether the flag is currently set, without consuming it.
    #[must_use]
    pub fn is_signalled(&self) -> bool {
        self.flag.load(Ordering::Acquire)
    }

    /// Clear the flag.
    pub fn reset(&self) {
        self.flag.store(false, Ordering::Release);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn starts_clear() {
        let signal = Signal::new();
        assert!(!signal.is_signalled());
        assert!(!signal.try_take());
    }

    #[test]
    fn take_consumes_the_flag() {
        let signal = Signal::new();
        signal.signal();
        assert!(signal.is_signalled());
        assert!(signal.try_take());
        assert!(!signal.try_take());
        assert!(!signal.is_signalled());
    }

    #[test]
    fn reset_clears_a_pending_signal() {
        let signal = Signal::new();
        signal.signal();
        signal.reset();
        assert!(!signal.try_take());
    }

    #[test]
    fn signal_is_visible_across_threads() {
        let signal = Signal::new();
        std::thread::scope(|scope| {
            scope.spawn(|| {
                signal.signal();
            });
        });
        assert!(signal.try_take());
    }

    #[test]
    fn repeated_signals_collapse_to_one() {
        let signal = Signal::new();
        for _ in 0..10 {
            signal.signal();
        }
        assert!(signal.try_take());
        assert!(!signal.try_take());
    }
}
