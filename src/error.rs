//! Failures the driver reports to its caller.

use core::fmt;

use embedded_hal::i2c::{Error as _, I2c};

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

impl fmt::Display for ConfigurationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::MaxCurrent => "maximum expected current must be finite and positive",
            Self::ShuntResistance => "shunt resistance must be finite and positive",
            Self::Calibration => "calibration is not representable for the selected ADC range",
            Self::Unrepresentable => "value is not representable by the register it targets",
            Self::AccumulatorMode => "energy and charge require a continuous conversion mode",
        };
        f.write_str(message)
    }
}

impl core::error::Error for ConfigurationError {}

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

/// Bus failures print their [`ErrorKind`](embedded_hal::i2c::ErrorKind), which every
/// `embedded-hal` error can produce; `Debug` carries whatever detail the bus adds on top.
impl<E: embedded_hal::i2c::Error> fmt::Display for Error<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::I2c(error) => write!(f, "I2C bus error: {}", error.kind()),
            Self::InvalidConfiguration(error) => error.fmt(f),
        }
    }
}

impl<E: embedded_hal::i2c::Error> core::error::Error for Error<E> {
    /// A bus error is not a source: `embedded_hal::i2c::Error` guarantees only `Debug`.
    fn source(&self) -> Option<&(dyn core::error::Error + 'static)> {
        match self {
            Self::I2c(_) => None,
            Self::InvalidConfiguration(error) => Some(error),
        }
    }
}

/// Failure returned while constructing an [`Ina228`](crate::Ina228).
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

/// Written by hand rather than derived: the derive would demand `I2C: Debug` because both
/// variants carry the bus, and a bus is rarely `Debug`. Omitting it keeps this printable
/// for every caller — `embedded_hal::i2c::Error` already guarantees `I2C::Error: Debug`.
impl<I2C: I2c> fmt::Debug for InitializationError<I2C> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidAddress { address, .. } => f
                .debug_struct("InvalidAddress")
                .field("address", address)
                .finish_non_exhaustive(),
            Self::I2c { error, .. } => f
                .debug_struct("I2c")
                .field("error", error)
                .finish_non_exhaustive(),
        }
    }
}

impl<I2C: I2c> fmt::Display for InitializationError<I2C> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidAddress { address, .. } => {
                write!(f, "I2C address {address:#04X} is outside 0x40..=0x4F")
            }
            Self::I2c { error, .. } => {
                write!(f, "could not read CONFIG: {}", error.kind())
            }
        }
    }
}

impl<I2C: I2c> core::error::Error for InitializationError<I2C> {}

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

impl<E: embedded_hal::i2c::Error> fmt::Display for CaptureError<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Failed(error) => write!(f, "accumulator capture failed: {error}"),
            Self::NotResumed { error, .. } => {
                write!(
                    f,
                    "accumulator capture could not resume conversions: {error}"
                )
            }
        }
    }
}

/// `'static` is needed only to hand the inner [`Error`] out as a `source`; every real
/// bus error type satisfies it.
impl<E: embedded_hal::i2c::Error + 'static> core::error::Error for CaptureError<E> {
    fn source(&self) -> Option<&(dyn core::error::Error + 'static)> {
        match self {
            Self::Failed(error) | Self::NotResumed { error, .. } => Some(error),
        }
    }
}
