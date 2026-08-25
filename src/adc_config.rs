//! The ADC_CONFIG register (0x01): the caller's view and the device's.
//!
//! [`AdcConfig`] is what a caller builds; [`AdcConfigWord`] is the 16-bit register behind
//! it. Keeping both here puts every ADC_CONFIG field position in one file, and lets the
//! questions the driver asks about a read-back word — is the ADC stopped, do the
//! accumulators run — be named once instead of re-derived from raw bits at each site.
//! [`OperatingMode`], [`ConversionTime`], and [`AveragingCount`] are the fields
//! [`AdcConfig`] is built from, so they live here too.

/// ADC operating mode, conversion times, and averaging configuration.
///
/// The default matches the ADC_CONFIG reset value documented by the INA228 datasheet.
#[derive(Debug, Clone, Copy)]
pub struct AdcConfig {
    /// Channels to measure and whether conversions are triggered or continuous.
    pub mode: OperatingMode,
    /// Bus-voltage conversion time.
    pub bus_conversion_time: ConversionTime,
    /// Shunt-voltage conversion time.
    pub shunt_conversion_time: ConversionTime,
    /// Die-temperature conversion time.
    pub temperature_conversion_time: ConversionTime,
    /// Number of ADC samples averaged into each result.
    pub averaging: AveragingCount,
}

impl Default for AdcConfig {
    fn default() -> Self {
        Self {
            mode: OperatingMode::ContinuousAll,
            bus_conversion_time: ConversionTime::Us1052,
            shunt_conversion_time: ConversionTime::Us1052,
            temperature_conversion_time: ConversionTime::Us1052,
            averaging: AveragingCount::N1,
        }
    }
}

/// An ADC_CONFIG word as the device holds it.
///
/// Carried around whole rather than decoded, so a word read back from the device is
/// written back bit-for-bit.
#[derive(Debug, Clone, Copy)]
pub(crate) struct AdcConfigWord(u16);

impl AdcConfigWord {
    const MODE_SHIFT: u32 = 12;
    const BUS_CONVERSION_SHIFT: u32 = 9;
    const SHUNT_CONVERSION_SHIFT: u32 = 6;
    const TEMPERATURE_CONVERSION_SHIFT: u32 = 3;

    const MODE_MASK: u16 = 0xF << Self::MODE_SHIFT;
    const ALTERNATE_SHUTDOWN: u16 = 0x8 << Self::MODE_SHIFT;
    const FIRST_CONTINUOUS: u16 = 0x9 << Self::MODE_SHIFT;

    pub(crate) fn of(config: AdcConfig) -> Self {
        Self(
            (config.mode as u16) << Self::MODE_SHIFT
                | (config.bus_conversion_time as u16) << Self::BUS_CONVERSION_SHIFT
                | (config.shunt_conversion_time as u16) << Self::SHUNT_CONVERSION_SHIFT
                | (config.temperature_conversion_time as u16) << Self::TEMPERATURE_CONVERSION_SHIFT
                | config.averaging as u16,
        )
    }

    pub(crate) fn from_device(word: u16) -> Self {
        Self(word)
    }

    /// The word to write to ADC_CONFIG.
    pub(crate) fn bits(self) -> u16 {
        self.0
    }

    /// The same configuration with the ADC stopped.
    pub(crate) fn shut_down(self) -> Self {
        Self(self.0 & !Self::MODE_MASK)
    }

    /// `true` when the ADC has already stopped converting.
    ///
    /// The datasheet gives shutdown two encodings, 0h and 8h; a device in either needs no
    /// stopping and no restoring.
    pub(crate) fn is_shutdown(self) -> bool {
        let mode = self.0 & Self::MODE_MASK;
        mode == 0 || mode == Self::ALTERNATE_SHUTDOWN
    }

    /// `true` when ENERGY and CHARGE accumulate.
    ///
    /// TI defines the accumulators only for the continuous modes, which are 9h and above.
    pub(crate) fn accumulates(self) -> bool {
        self.0 & Self::MODE_MASK >= Self::FIRST_CONTINUOUS
    }
}

/// ADC conversion time per sample.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum ConversionTime {
    /// 50 µs.
    Us50 = 0,
    /// 84 µs.
    Us84 = 1,
    /// 150 µs.
    Us150 = 2,
    /// 280 µs.
    Us280 = 3,
    /// 540 µs.
    Us540 = 4,
    /// 1052 µs.
    Us1052 = 5,
    /// 2074 µs.
    Us2074 = 6,
    /// 4120 µs.
    Us4120 = 7,
}

/// Number of ADC samples to average per conversion result.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum AveragingCount {
    /// No averaging: one sample per result.
    N1 = 0,
    /// 4 samples.
    N4 = 1,
    /// 16 samples.
    N16 = 2,
    /// 64 samples.
    N64 = 3,
    /// 128 samples.
    N128 = 4,
    /// 256 samples.
    N256 = 5,
    /// 512 samples.
    N512 = 6,
    /// 1024 samples.
    N1024 = 7,
}

/// ADC operating mode: selects which channels to measure and whether to
/// run continuously or in single-shot (triggered) mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum OperatingMode {
    /// Powered down; no conversions and no current draw from the ADC.
    Shutdown = 0x0,
    /// One shot: bus voltage.
    TriggeredBus = 0x1,
    /// One shot: shunt voltage.
    TriggeredShunt = 0x2,
    /// One shot: bus and shunt voltage.
    TriggeredBusShunt = 0x3,
    /// One shot: die temperature.
    TriggeredTemp = 0x4,
    /// One shot: die temperature and bus voltage.
    TriggeredTempBus = 0x5,
    /// One shot: die temperature and shunt voltage.
    TriggeredTempShunt = 0x6,
    /// One shot: bus voltage, shunt voltage, and die temperature.
    TriggeredAll = 0x7,
    /// Continuous: bus voltage.
    ContinuousBus = 0x9,
    /// Continuous: shunt voltage.
    ContinuousShunt = 0xA,
    /// Continuous: bus and shunt voltage.
    ContinuousBusShunt = 0xB,
    /// Continuous: die temperature.
    ContinuousTemp = 0xC,
    /// Continuous: die temperature and bus voltage.
    ContinuousTempBus = 0xD,
    /// Continuous: die temperature and shunt voltage.
    ContinuousTempShunt = 0xE,
    /// Continuous: bus voltage, shunt voltage, and die temperature.
    ContinuousAll = 0xF,
}
