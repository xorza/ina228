//! The CONFIG register (0x00) as a typed word.
//!
//! Wrapping the raw `u16` puts every CONFIG bit definition in one place and makes the
//! driver's cached copy structurally unable to hold a command bit: a [`Config`] is built
//! only by masking a device read or by a `with_*` builder, and none of those set RST or
//! RSTACC. The two self-clearing commands are plain `u16` words instead, so no caller can
//! store one by mistake.

use crate::adc_range::AdcRange;

/// A CONFIG value the device holds between operations.
///
/// Never carries RST or RSTACC, which act once and then clear themselves.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Config(u16);

impl Config {
    const RESET: u16 = 1 << 15;
    const RESET_ACCUMULATORS: u16 = 1 << 14;
    const TEMPERATURE_COMPENSATION: u16 = 1 << 5;
    const ADC_RANGE: u16 = 1 << 4;
    const SELF_CLEARING: u16 = Self::RESET | Self::RESET_ACCUMULATORS;

    /// CONFIG after a reset or power-up.
    pub(crate) const RESET_VALUE: Self = Self(0);

    /// The single write that triggers a soft reset — a command, not a configuration.
    pub(crate) const RESET_COMMAND: u16 = Self::RESET;

    /// Interprets a word read back from CONFIG.
    pub(crate) fn from_device(word: u16) -> Self {
        Self(word & !Self::SELF_CLEARING)
    }

    /// The word to write to CONFIG.
    pub(crate) fn bits(self) -> u16 {
        self.0
    }

    /// This configuration plus the self-clearing RSTACC bit, for one write.
    pub(crate) fn accumulator_reset_command(self) -> u16 {
        self.0 | Self::RESET_ACCUMULATORS
    }

    /// Shunt ADC full-scale range this configuration selects.
    pub(crate) fn adc_range(self) -> AdcRange {
        if self.0 & Self::ADC_RANGE == 0 {
            AdcRange::Range163mV
        } else {
            AdcRange::Range40mV
        }
    }

    pub(crate) fn with_adc_range(self, range: AdcRange) -> Self {
        match range {
            AdcRange::Range163mV => Self(self.0 & !Self::ADC_RANGE),
            AdcRange::Range40mV => Self(self.0 | Self::ADC_RANGE),
        }
    }

    pub(crate) fn with_temperature_compensation(self, enabled: bool) -> Self {
        if enabled {
            Self(self.0 | Self::TEMPERATURE_COMPENSATION)
        } else {
            Self(self.0 & !Self::TEMPERATURE_COMPENSATION)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Caching a word that still carried a command bit would re-trigger the command on
    /// the next unrelated CONFIG write.
    #[test]
    fn from_device_drops_the_self_clearing_command_bits() {
        assert_eq!(Config::RESET_COMMAND, 0x8000);
        assert_eq!(Config::from_device(0xFFFF).bits(), 0x3FFF);
        assert_eq!(Config::from_device(1 << 15), Config::RESET_VALUE);
        assert_eq!(Config::from_device(1 << 14), Config::RESET_VALUE);
    }

    /// CONVDLY occupies bits 13:6 and the driver models no part of it, so every word the
    /// builders and the accumulator command produce has to carry a delay programmed
    /// elsewhere through untouched.
    #[test]
    fn builders_and_commands_preserve_the_bits_they_do_not_own() {
        let base = Config::from_device(0x0BC0);
        assert_eq!(base.with_adc_range(AdcRange::Range40mV).bits(), 0x0BD0);
        assert_eq!(base.with_adc_range(AdcRange::Range163mV), base);
        assert_eq!(base.with_temperature_compensation(true).bits(), 0x0BE0);
        assert_eq!(
            base.with_temperature_compensation(true)
                .with_temperature_compensation(false),
            base
        );
        assert_eq!(base.accumulator_reset_command(), 0x4BC0);
    }
}
