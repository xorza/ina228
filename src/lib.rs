//! Platform-agnostic `no_std` driver for the TI INA228 power, energy, and charge monitor.
//!
//! [`Ina228`] wraps an `embedded-hal` 1.0 I2C bus and reads bus voltage, shunt voltage,
//! die temperature, and — once [`Ina228::calibrate`] has run — current, power, energy, and
//! charge in physical units. The rules that hold across the whole API (measurement
//! freshness, how conversions are suspended and restored, what an I2C write error means,
//! and when calibration is required) are documented once on [`Ina228`].

#![no_std]

mod adc_config;
mod adc_range;
mod calibration;
mod config;
mod diag_alrt;
mod error;
mod ina228;
mod register;
mod scale;

pub use adc_config::{AdcConfig, AveragingCount, ConversionTime, OperatingMode};
pub use adc_range::AdcRange;
pub use diag_alrt::{AlertConfig, DiagnosticFlags};
pub use error::{CaptureError, ConfigurationError, Error, InitializationError};
pub use ina228::{AccumulatorSnapshot, DEFAULT_ADDRESS, DEVICE_ID, Ina228, MANUFACTURER_ID};
