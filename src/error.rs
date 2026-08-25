//! Failures the driver reports to its caller.

use embedded_hal::i2c::I2c;

/// Invalid physical configuration supplied to the driver.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigurationError {
    /// Maximum expected current must be finite and positive.
    MaxCurrent,
    /// Shunt resistance must be finite and positive.
    ShuntResistance,
    /// Calibration cannot be represented for the selected ADC range.
    Calibration,
    /// A value cannot be represented by the register it targets.
    ///
    /// Every method that reports this takes one physical argument, so the call itself
    /// says which value was rejected.
    Unrepresentable,
    /// Energy and charge accumulators are invalid outside continuous conversion modes.
    AccumulatorMode,
}

/// INA228 operation error.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error<E> {
    /// I2C bus operation failed.
    I2c(E),
    /// A physical configuration value is invalid or unrepresentable.
    InvalidConfiguration(ConfigurationError),
}

impl<E> From<E> for Error<E> {
    fn from(error: E) -> Self {
        Self::I2c(error)
    }
}

/// Failure returned while constructing an [`Ina228`](crate::Ina228).
#[derive(Debug)]
pub enum InitializationError<I2C: I2c> {
    /// The supplied address is outside the INA228 address range.
    InvalidAddress {
        /// I2C bus returned to the caller for recovery or retry.
        i2c: I2C,
        /// Invalid address supplied by the caller.
        address: u8,
    },
    /// Reading CONFIG from the device failed.
    I2c {
        /// I2C bus returned to the caller for recovery or retry.
        i2c: I2C,
        /// Error reported by the I2C bus.
        error: I2C::Error,
    },
}
