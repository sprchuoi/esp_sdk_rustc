//! Digital input/output pins.

/// A digital output/input pin.
pub trait GpioPin {
    /// Drive the pin high or low.
    fn set(&mut self, high: bool);

    /// Read the pin level.
    fn get(&self) -> bool;
}
