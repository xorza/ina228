#[derive(Debug, Clone, Copy)]
#[repr(u8)]
pub(crate) enum Register {
    Config = 0x00,
    AdcConfig = 0x01,
    ShuntCal = 0x02,
    ShuntTempco = 0x03,
    Vshunt = 0x04,
    Vbus = 0x05,
    DieTemp = 0x06,
    Current = 0x07,
    Power = 0x08,
    Energy = 0x09,
    Charge = 0x0A,
    DiagAlrt = 0x0B,
    Sovl = 0x0C,
    Suvl = 0x0D,
    Bovl = 0x0E,
    Buvl = 0x0F,
    TempLimit = 0x10,
    PwrLimit = 0x11,
    ManufacturerId = 0x3E,
    DeviceId = 0x3F,
}

pub(crate) mod config {
    pub(crate) const RESET: u16 = 1 << 15;
    pub(crate) const RESET_ACCUMULATORS: u16 = 1 << 14;
    pub(crate) const TEMPERATURE_COMPENSATION: u16 = 1 << 5;
    pub(crate) const ADC_RANGE: u16 = 1 << 4;
}

pub(crate) mod adc_config {
    pub(crate) const MODE_MASK: u16 = 0xF << 12;
    pub(crate) const ALTERNATE_SHUTDOWN_MODE: u16 = 8 << 12;
    pub(crate) const FIRST_CONTINUOUS_MODE: u16 = 9 << 12;

    pub(crate) fn converts_shunt(value: u16) -> bool {
        ((value & MODE_MASK) >> 12) & 0b0010 != 0
    }
}

pub(crate) mod diagnostic_alert {
    pub(crate) const LATCH: u16 = 1 << 15;
    pub(crate) const CONVERSION_READY_ENABLE: u16 = 1 << 14;
    pub(crate) const SLOW_ALERT: u16 = 1 << 13;
    pub(crate) const ACTIVE_HIGH: u16 = 1 << 12;
    pub(crate) const ENERGY_OVERFLOW: u16 = 1 << 11;
    pub(crate) const CHARGE_OVERFLOW: u16 = 1 << 10;
    pub(crate) const MATH_OVERFLOW: u16 = 1 << 9;
    pub(crate) const TEMP_OVER_LIMIT: u16 = 1 << 7;
    pub(crate) const SHUNT_OVER_LIMIT: u16 = 1 << 6;
    pub(crate) const SHUNT_UNDER_LIMIT: u16 = 1 << 5;
    pub(crate) const BUS_OVER_LIMIT: u16 = 1 << 4;
    pub(crate) const BUS_UNDER_LIMIT: u16 = 1 << 3;
    pub(crate) const POWER_OVER_LIMIT: u16 = 1 << 2;
    pub(crate) const CONVERSION_READY: u16 = 1 << 1;
    pub(crate) const MEMORY_OK: u16 = 1;
}

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
    pub(crate) fn from_config(value: u16) -> Self {
        if value & config::ADC_RANGE == 0 {
            Self::Range163mV
        } else {
            Self::Range40mV
        }
    }

    pub(crate) fn apply_to_config(self, value: u16) -> u16 {
        match self {
            Self::Range163mV => value & !config::ADC_RANGE,
            Self::Range40mV => value | config::ADC_RANGE,
        }
    }

    pub(crate) fn full_scale_voltage(self) -> f64 {
        match self {
            Self::Range163mV => 0.16384_f32 as f64,
            Self::Range40mV => 0.04096_f32 as f64,
        }
    }

    pub(crate) fn shunt_voltage_lsb(self) -> f32 {
        match self {
            Self::Range163mV => 312.5e-9,
            Self::Range40mV => 78.125e-9,
        }
    }

    pub(crate) fn shunt_limit_lsb(self) -> f32 {
        match self {
            Self::Range163mV => 5.0e-6,
            Self::Range40mV => 1.25e-6,
        }
    }

    pub(crate) fn shunt_cal_multiplier(self) -> f64 {
        match self {
            Self::Range163mV => 1.0,
            Self::Range40mV => 4.0,
        }
    }
}

/// ADC conversion time per sample.
#[derive(Debug, Clone, Copy)]
#[repr(u16)]
pub enum ConversionTime {
    Us50 = 0,
    Us84 = 1,
    Us150 = 2,
    Us280 = 3,
    Us540 = 4,
    Us1052 = 5,
    Us2074 = 6,
    Us4120 = 7,
}

/// Number of ADC samples to average per conversion result.
#[derive(Debug, Clone, Copy)]
#[repr(u16)]
pub enum AveragingCount {
    N1 = 0,
    N4 = 1,
    N16 = 2,
    N64 = 3,
    N128 = 4,
    N256 = 5,
    N512 = 6,
    N1024 = 7,
}

/// ADC operating mode: selects which channels to measure and whether to
/// run continuously or in single-shot (triggered) mode.
#[derive(Debug, Clone, Copy)]
#[repr(u16)]
pub enum OperatingMode {
    Shutdown = 0x0,
    TriggeredBus = 0x1,
    TriggeredShunt = 0x2,
    TriggeredBusShunt = 0x3,
    TriggeredTemp = 0x4,
    TriggeredTempBus = 0x5,
    TriggeredTempShunt = 0x6,
    TriggeredAll = 0x7,
    ContinuousBus = 0x9,
    ContinuousShunt = 0xA,
    ContinuousBusShunt = 0xB,
    ContinuousTemp = 0xC,
    ContinuousTempBus = 0xD,
    ContinuousTempShunt = 0xE,
    ContinuousAll = 0xF,
}
