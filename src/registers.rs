#[derive(Debug, Clone, Copy)]
#[repr(u8)]
#[allow(dead_code)]
pub enum Register {
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdcRange {
    /// ±163.84 mV, LSB = 312.5 nV
    Range163mV = 0,
    /// ±40.96 mV, LSB = 78.125 nV
    Range40mV = 1,
}

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
