//! Host-side mock peripherals.
//!
//! Each mock lives in its own module, matching the trait it implements. They are
//! always ready, so they can be driven from a single-threaded test with
//! [`block_on`].

use core::future::Future;
use core::task::{Context, Poll, Waker};

mod delay;
mod flash;
mod gpio;
mod i2c;
mod serial;
mod spi;

pub use delay::MockDelay;
pub use flash::MockFlash;
pub use gpio::MockGpio;
pub use i2c::MockI2c;
pub use serial::MockSerial;
pub use spi::MockSpi;

/// Poll a future to completion using a no-op waker.
///
/// Only for futures that never return `Pending` (all mocks here). Panics if the
/// future does not complete within a bounded number of polls.
pub fn block_on<F: Future>(future: F) -> F::Output {
    let waker = Waker::noop();
    let mut cx = Context::from_waker(waker);
    let mut future = core::pin::pin!(future);
    for _ in 0..64 {
        if let Poll::Ready(output) = future.as_mut().poll(&mut cx) {
            return output;
        }
    }
    panic!("mock future stayed pending");
}
