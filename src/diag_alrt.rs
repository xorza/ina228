//! The DIAG_ALRT register (0x0B), in both directions.
//!
//! The register is split in two: [`AlertConfig`] owns the control bits 15:12 that the
//! driver writes, [`DiagnosticFlags`] the status bits 11:0 it reads back. Both live here
//! so the one partition is stated once.

/// Alert pin configuration written to the upper bits of DIAG_ALRT.
///
/// All fields default to `false`. Use struct-update syntax to set only what you need:
///
/// ```
/// use ina228::AlertConfig;
///
/// let alerts = AlertConfig { latch: true, active_high: true, ..Default::default() };
/// assert!(alerts.latch && alerts.active_high && !alerts.slow_alert);
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

/// Status flags read back from DIAG_ALRT.
///
/// Wraps the register word rather than expanding it, so a capture costs two bytes and a
/// caller pays only for the flags it actually asks about.
#[derive(Debug, Clone, Copy)]
pub struct DiagnosticFlags(u16);

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

    pub(crate) fn from_device(word: u16) -> Self {
        Self(word)
    }

    /// The raw DIAG_ALRT word, including any bit this type does not name.
    pub fn bits(self) -> u16 {
        self.0
    }

    /// `true` when the device trim memory checksum is valid.
    pub fn memory_ok(self) -> bool {
        self.0 & Self::MEMORY_OK != 0
    }

    /// A conversion finished since this register was last read.
    pub fn conversion_ready(self) -> bool {
        self.0 & Self::CONVERSION_READY != 0
    }

    /// The 40-bit ENERGY accumulator wrapped. Cleared by reading ENERGY.
    pub fn energy_overflow(self) -> bool {
        self.0 & Self::ENERGY_OVERFLOW != 0
    }

    /// The 40-bit CHARGE accumulator wrapped. Cleared by reading CHARGE.
    pub fn charge_overflow(self) -> bool {
        self.0 & Self::CHARGE_OVERFLOW != 0
    }

    /// An arithmetic overflow spoiled a current, power, energy, or charge result.
    pub fn math_overflow(self) -> bool {
        self.0 & Self::MATH_OVERFLOW != 0
    }

    /// Die temperature rose above TEMP_LIMIT.
    pub fn temp_over_limit(self) -> bool {
        self.0 & Self::TEMP_OVER_LIMIT != 0
    }

    /// Shunt voltage rose above SOVL.
    pub fn shunt_over_limit(self) -> bool {
        self.0 & Self::SHUNT_OVER_LIMIT != 0
    }

    /// Shunt voltage fell below SUVL.
    pub fn shunt_under_limit(self) -> bool {
        self.0 & Self::SHUNT_UNDER_LIMIT != 0
    }

    /// Bus voltage rose above BOVL.
    pub fn bus_over_limit(self) -> bool {
        self.0 & Self::BUS_OVER_LIMIT != 0
    }

    /// Bus voltage fell below BUVL.
    pub fn bus_under_limit(self) -> bool {
        self.0 & Self::BUS_UNDER_LIMIT != 0
    }

    /// Power rose above PWR_LIMIT.
    pub fn power_over_limit(self) -> bool {
        self.0 & Self::POWER_OVER_LIMIT != 0
    }
}
