//! The DIAG_ALRT register (0x0B), in both directions.
//!
//! [`AlertConfig`] writes its upper control bits; [`DiagnosticFlags`] reads the status
//! bits back. Both live here with the bit positions they share, so neither direction can
//! drift from the other.

/// Alert pin configuration written to the upper bits of DIAG_ALRT.
///
/// All fields default to `false`. Use struct-update syntax to set only what you need:
///
/// ```ignore
/// ina.configure_alerts(AlertConfig { latch: true, active_high: true, ..Default::default() })?;
/// ```
#[derive(Debug, Clone, Copy, Default)]
pub struct AlertConfig {
    /// Latch alerts until DIAG_ALRT is read (vs. transparent mode).
    pub latch: bool,
    /// ALERT pin polarity: `true` = active high, `false` = active low.
    pub active_high: bool,
    /// Assert ALERT on conversion-ready.
    pub conversion_ready: bool,
    /// Compare alerts against the averaged ADC value rather than each conversion.
    pub slow_alert: bool,
}

impl AlertConfig {
    const LATCH: u16 = 1 << 15;
    const CONVERSION_READY_ENABLE: u16 = 1 << 14;
    const SLOW_ALERT: u16 = 1 << 13;
    const ACTIVE_HIGH: u16 = 1 << 12;

    /// The word to write to DIAG_ALRT.
    pub(crate) fn bits(self) -> u16 {
        let mut value = 0;
        if self.latch {
            value |= Self::LATCH;
        }
        if self.conversion_ready {
            value |= Self::CONVERSION_READY_ENABLE;
        }
        if self.slow_alert {
            value |= Self::SLOW_ALERT;
        }
        if self.active_high {
            value |= Self::ACTIVE_HIGH;
        }
        value
    }
}

/// Snapshot of status flags from the DIAG_ALRT register.
#[derive(Debug, Clone, Copy)]
pub struct DiagnosticFlags {
    /// `true` when the device trim memory checksum is valid.
    pub memory_ok: bool,
    pub conversion_ready: bool,
    pub energy_overflow: bool,
    pub math_overflow: bool,
    pub temp_over_limit: bool,
    pub shunt_over_limit: bool,
    pub shunt_under_limit: bool,
    pub bus_over_limit: bool,
    pub bus_under_limit: bool,
    pub power_over_limit: bool,
    pub charge_overflow: bool,
}

impl DiagnosticFlags {
    const ENERGY_OVERFLOW: u16 = 1 << 11;
    const CHARGE_OVERFLOW: u16 = 1 << 10;
    const MATH_OVERFLOW: u16 = 1 << 9;
    const TEMP_OVER_LIMIT: u16 = 1 << 7;
    const SHUNT_OVER_LIMIT: u16 = 1 << 6;
    const SHUNT_UNDER_LIMIT: u16 = 1 << 5;
    const BUS_OVER_LIMIT: u16 = 1 << 4;
    const BUS_UNDER_LIMIT: u16 = 1 << 3;
    const POWER_OVER_LIMIT: u16 = 1 << 2;
    const CONVERSION_READY: u16 = 1 << 1;
    const MEMORY_OK: u16 = 1;

    /// Decodes a word read back from DIAG_ALRT.
    pub(crate) fn from_bits(word: u16) -> Self {
        Self {
            memory_ok: word & Self::MEMORY_OK != 0,
            conversion_ready: word & Self::CONVERSION_READY != 0,
            energy_overflow: word & Self::ENERGY_OVERFLOW != 0,
            math_overflow: word & Self::MATH_OVERFLOW != 0,
            temp_over_limit: word & Self::TEMP_OVER_LIMIT != 0,
            shunt_over_limit: word & Self::SHUNT_OVER_LIMIT != 0,
            shunt_under_limit: word & Self::SHUNT_UNDER_LIMIT != 0,
            bus_over_limit: word & Self::BUS_OVER_LIMIT != 0,
            bus_under_limit: word & Self::BUS_UNDER_LIMIT != 0,
            power_over_limit: word & Self::POWER_OVER_LIMIT != 0,
            charge_overflow: word & Self::CHARGE_OVERFLOW != 0,
        }
    }
}
