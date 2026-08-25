//! Failures the driver reports to its caller.

use embedded_hal::i2c::I2c;

use crate::ina228::AccumulatorSnapshot;

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

/// Failure from [`Ina228::take_accumulator_snapshot`](crate::Ina228::take_accumulator_snapshot).
#[derive(Debug, Clone, Copy)]
pub enum CaptureError<E> {
    /// The capture did not complete.
    Failed(Error<E>),
    /// The capture completed, but conversions could not be resumed, so the ADC may still
    /// be shut down; reconfigure before relying on fresh measurements.
    ///
    /// The snapshot is carried here rather than dropped because the reads that produced it
    /// already consumed the device's flag state — DIAG_ALRT's alerts and the ENERGY and
    /// CHARGE overflow indicators — and no retry can recover those.
    NotResumed {
        /// The capture that completed before the restore was attempted.
        snapshot: AccumulatorSnapshot,
        /// Why conversions could not be resumed.
        error: Error<E>,
    },
}

impl<E> From<Error<E>> for CaptureError<E> {
    fn from(error: Error<E>) -> Self {
        Self::Failed(error)
    }
}
