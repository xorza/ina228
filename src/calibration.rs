//! Scale factors derived from the caller's shunt and expected-current figures.

use crate::adc_range::AdcRange;
use crate::error::ConfigurationError;
use crate::scale;

/// Current scale derived from a calibration, with both fields finite and positive.
///
/// [`Calibration::new`] is the only way to build one, so the arithmetic below never has
/// to re-check for a non-finite operand.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Calibration {
    current_lsb: f64,
    shunt_resistance_ohm: f64,
}

impl Calibration {
    pub(crate) fn new(
        max_current_a: f32,
        shunt_resistance_ohm: f32,
    ) -> Result<Self, ConfigurationError> {
        if !max_current_a.is_finite() || max_current_a <= 0.0 {
            return Err(ConfigurationError::MaxCurrent);
        }
        if !shunt_resistance_ohm.is_finite() || shunt_resistance_ohm <= 0.0 {
            return Err(ConfigurationError::ShuntResistance);
        }
        Ok(Self {
            current_lsb: max_current_a as f64 / scale::SIGNED_20_BIT_FULL_SCALE,
            shunt_resistance_ohm: shunt_resistance_ohm as f64,
        })
    }

    pub(crate) fn shunt_cal(self, adc_range: AdcRange) -> Result<u16, ConfigurationError> {
        let max_shunt_voltage =
            self.current_lsb * scale::SIGNED_20_BIT_FULL_SCALE * self.shunt_resistance_ohm;
        if max_shunt_voltage >= adc_range.full_scale_voltage() {
            return Err(ConfigurationError::Calibration);
        }

        let exact = adc_range.shunt_cal_scale() * self.current_lsb * self.shunt_resistance_ohm;
        // Both bounds sit half a count out because they constrain the rounded code, which
        // SHUNT_CAL requires to land in 1..=UNSIGNED_15_BIT_MAX.
        const MIN_CODE: f64 = 1.0;
        let max_code = scale::UNSIGNED_15_BIT_MAX as f64;
        if exact < MIN_CODE - 0.5 || exact >= max_code + 0.5 {
            return Err(ConfigurationError::Calibration);
        }
        Ok((exact + 0.5) as u16)
    }

    /// Amps per CURRENT register count.
    pub(crate) fn current_lsb(self) -> f64 {
        self.current_lsb
    }

    pub(crate) fn power_lsb(self) -> f64 {
        scale::POWER_LSB_MULTIPLIER * self.current_lsb
    }

    pub(crate) fn energy_lsb(self) -> f64 {
        scale::ENERGY_LSB_MULTIPLIER * self.power_lsb()
    }
}
