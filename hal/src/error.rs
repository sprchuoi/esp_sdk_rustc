//! The HAL error type shared by every peripheral.

/// Result type used across the HAL.
pub type Result<T> = core::result::Result<T, Error>;

/// Errors a peripheral implementation may report.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Error {
    /// The bus reported an error (NACK, framing, CRC, ...).
    Bus,
    /// The operation did not complete before its deadline.
    Timeout,
    /// The addressed device does not exist on this bus.
    InvalidAddress,
    /// An argument was out of range for the peripheral.
    InvalidArgument,
    /// The peripheral cannot service the request yet (busy, not initialised).
    NotReady,
    /// A flash write hit a non-erased region.
    NotErased,
    /// The requested range is outside the device.
    OutOfRange,
    /// The peripheral exists in the SoC but this build does not support it.
    Unsupported,
}
