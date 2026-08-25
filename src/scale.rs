//! Physical scale factors for the INA228 registers.
//!
//! Each measured quantity contributes exactly one LSB here, taken from the datasheet
//! register map; every related factor is derived from it. A threshold register compares
//! against a truncation of its measurement register, so its scale is that measurement LSB
//! times a power of two, and the ±40.96 mV shunt range is the ±163.84 mV range scaled by
//! a power of two. Every such factor is a power of two, so the derived constants are
//! bit-identical to the datasheet's decimal values and cannot drift apart.
//!
//! Register scale factors are `f32`: they turn a raw count into the `f32` a caller reads,
//! and they define the grid a caller's own `f32` arguments land on. Calibration-domain
//! quantities are `f64`, where SHUNT_CAL and the 40-bit accumulators need the headroom;
//! widening happens where the two domains meet.

/// VSHUNT LSB in Volts on the ±163.84 mV range.
pub(crate) const SHUNT_VOLTAGE_LSB: f32 = 312.5e-9;

/// VBUS LSB in Volts.
pub(crate) const BUS_VOLTAGE_LSB: f32 = 195.3125e-6;

/// DIETEMP LSB in degrees Celsius. TEMP_LIMIT compares on the same scale, untruncated.
pub(crate) const DIE_TEMPERATURE_LSB: f32 = 7.8125e-3;

/// One SOVL/SUVL or BOVL/BUVL count in measurement LSBs.
pub(crate) const VOLTAGE_LIMIT_TRUNCATION: f32 = 16.0;

/// BOVL/BUVL LSB in Volts.
pub(crate) const BUS_LIMIT_LSB: f32 = BUS_VOLTAGE_LSB * VOLTAGE_LIMIT_TRUNCATION;

/// Largest code in a 15-bit register field.
///
/// BOVL/BUVL and SHUNT_CAL each reserve bit 15 of an otherwise unsigned 16-bit register.
pub(crate) const UNSIGNED_15_BIT_MAX: u16 = 0x7FFF;

/// Largest code in a 14-bit register field: SHUNT_TEMPCO's coefficient occupies bits 13:0.
pub(crate) const UNSIGNED_14_BIT_MAX: u16 = 0x3FFF;

/// Positive full scale of a 20-bit signed register, in counts.
///
/// VSHUNT and CURRENT are both 20-bit signed, which makes this both VSHUNT's positive
/// endpoint and the divisor that turns a maximum expected current into CURRENT_LSB.
pub(crate) const SIGNED_20_BIT_FULL_SCALE: f64 = (1u32 << 19) as f64;

/// POWER LSB as a multiple of CURRENT_LSB.
pub(crate) const POWER_LSB_MULTIPLIER: f64 = 3.2;

/// ENERGY LSB as a multiple of the POWER LSB.
pub(crate) const ENERGY_LSB_MULTIPLIER: f64 = 16.0;

/// One PWR_LIMIT count in POWER LSBs: PWR_LIMIT compares against bits 23:8 of POWER.
pub(crate) const POWER_LIMIT_TRUNCATION: f64 = 256.0;

/// SHUNT_CAL counts per Amp-per-LSB per Ohm, on the ±163.84 mV range.
///
/// Equal to `2^12 / 312.5 nV`, which the datasheet writes as `13107.2 × 10^6`. Stated as
/// the exact integer it is rather than divided out of [`SHUNT_VOLTAGE_LSB`]: that
/// division rounds the LSB and then the quotient, where this literal is exact in `f64`.
pub(crate) const SHUNT_CAL_SCALE: f64 = 13_107_200_000.0;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derived_constants_match_the_datasheet_decimals() {
        assert_eq!(BUS_LIMIT_LSB, 3.125e-3);
        assert_eq!(SIGNED_20_BIT_FULL_SCALE, 524_288.0);
        assert_eq!(SHUNT_CAL_SCALE, 13107.2e6);
        assert_eq!(SHUNT_CAL_SCALE as u64 as f64, SHUNT_CAL_SCALE);
    }
}
