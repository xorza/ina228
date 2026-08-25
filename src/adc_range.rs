//! The shunt ADC full-scale range, and the physical scales that follow from it.

use crate::scale;

/// Shunt ADC full-scale range selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum AdcRange {
    /// ±163.84 mV, LSB = 312.5 nV
    Range163mV = 0,
    /// ±40.96 mV, LSB = 78.125 nV
    Range40mV = 1,
}

impl AdcRange {
    /// This range's VSHUNT LSB as a fraction of the ±163.84 mV range's.
    ///
    /// The only per-range scale factor in the crate. Every shunt-domain scale below is
    /// this ratio applied to a base in [`crate::scale`], and the ratio is a power of two,
    /// so the two ranges stay exactly consistent.
    fn shunt_lsb_ratio(self) -> f32 {
        match self {
            Self::Range163mV => 1.0,
            Self::Range40mV => 0.25,
        }
    }

    /// VSHUNT LSB in Volts.
    pub(crate) fn shunt_voltage_lsb(self) -> f32 {
        scale::SHUNT_VOLTAGE_LSB * self.shunt_lsb_ratio()
    }

    /// SOVL/SUVL LSB in Volts.
    pub(crate) fn shunt_limit_lsb(self) -> f32 {
        self.shunt_voltage_lsb() * scale::VOLTAGE_LIMIT_TRUNCATION
    }

    /// Positive full-scale shunt voltage in Volts, as the endpoint of the `f32` grid.
    ///
    /// Calibration compares `max_current_a × shunt_resistance_ohm` against this, and both
    /// are caller-supplied `f32`. The endpoint is therefore `2^19` VSHUNT LSBs taken from
    /// the `f32` grid, not the exact decimal 163.84 mV: `0.16384_f32` sits 4.1 nV below that
    /// decimal, so an exact endpoint would admit a caller asking for exactly full scale.
    pub(crate) fn full_scale_voltage(self) -> f64 {
        self.shunt_voltage_lsb() as f64 * scale::SIGNED_20_BIT_FULL_SCALE
    }

    /// SHUNT_CAL counts per Amp-per-LSB per Ohm for this range.
    ///
    /// The datasheet gives `13107.2e6 × CURRENT_LSB × R_SHUNT` with a 4× factor on the
    /// ±40.96 mV range. That factor is the inverse of the VSHUNT LSB ratio, so it follows
    /// from [`Self::shunt_lsb_ratio`] instead of being restated.
    pub(crate) fn shunt_cal_scale(self) -> f64 {
        scale::SHUNT_CAL_SCALE / self.shunt_lsb_ratio() as f64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn range_scales_match_the_datasheet_decimals() {
        assert_eq!(AdcRange::Range163mV.shunt_voltage_lsb(), 312.5e-9);
        assert_eq!(AdcRange::Range40mV.shunt_voltage_lsb(), 78.125e-9);
        assert_eq!(AdcRange::Range163mV.shunt_limit_lsb(), 5.0e-6);
        assert_eq!(AdcRange::Range40mV.shunt_limit_lsb(), 1.25e-6);
        assert_eq!(AdcRange::Range163mV.shunt_cal_scale(), 13107.2e6);
        assert_eq!(AdcRange::Range40mV.shunt_cal_scale(), 52428.8e6);
    }

    /// The calibration endpoint is the largest shunt voltage an `f32` caller can name, so
    /// asking for exactly full scale is rejected rather than landing just inside it.
    #[test]
    fn full_scale_voltage_is_pinned_to_the_f32_grid() {
        assert_eq!(
            AdcRange::Range163mV.full_scale_voltage(),
            0.16384_f32 as f64
        );
        assert_eq!(AdcRange::Range40mV.full_scale_voltage(), 0.04096_f32 as f64);
        assert!(AdcRange::Range163mV.full_scale_voltage() < 0.16384_f64);
        assert!(AdcRange::Range40mV.full_scale_voltage() < 0.04096_f64);
    }
}
