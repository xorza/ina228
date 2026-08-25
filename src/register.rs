//! Every INA228 register address the driver touches.

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
