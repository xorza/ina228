mod registers;

use embedded_hal::i2c::I2c;
use registers::Register;

pub use registers::{AdcRange, AveragingCount, ConversionTime, OperatingMode};

pub const DEFAULT_ADDRESS: u8 = 0x40;
pub const MANUFACTURER_ID: u16 = 0x5449;
/// Device ID (upper 12 bits of register 0x3F; lower 4 bits are die revision).
pub const DEVICE_ID: u16 = 0x228;

#[derive(Debug)]
pub struct Ina228<I2C> {
    i2c: I2C,
    address: u8,
    current_lsb: f32,
    adc_range: AdcRange,
}

#[derive(Debug, Clone)]
pub struct Measurements {
    pub bus_voltage_v: f32,
    pub shunt_voltage_v: f32,
    pub current_a: f32,
    pub power_w: f32,
    pub die_temp_c: f32,
}

impl<I2C: I2c> Ina228<I2C> {
    pub fn new(i2c: I2C, address: u8) -> Self {
        assert!(
            (0x40..=0x4F).contains(&address),
            "INA228 address must be in 0x40..=0x4F"
        );
        Self {
            i2c,
            address,
            current_lsb: 0.0,
            adc_range: AdcRange::Range163mV,
        }
    }

    pub fn reset(&mut self) {
        // Bit 15 = RST
        self.write_u16(Register::Config, 1 << 15);
    }

    pub fn configure(
        &mut self,
        mode: OperatingMode,
        vbus_ct: ConversionTime,
        vshunt_ct: ConversionTime,
        temp_ct: ConversionTime,
        avg: AveragingCount,
    ) {
        let value = ((mode as u16) << 12)
            | ((vbus_ct as u16) << 9)
            | ((vshunt_ct as u16) << 6)
            | ((temp_ct as u16) << 3)
            | (avg as u16);
        self.write_u16(Register::AdcConfig, value);
    }

    pub fn set_adc_range(&mut self, range: AdcRange) {
        let config = self.read_u16(Register::Config);
        let value = match range {
            AdcRange::Range163mV => config & !(1 << 4),
            AdcRange::Range40mV => config | (1 << 4),
        };
        self.write_u16(Register::Config, value);
        self.adc_range = range;
    }

    /// Calibrate for current/power measurement.
    /// `max_current_a`: maximum expected current in Amps.
    /// `shunt_resistance_ohm`: shunt resistor value in Ohms.
    pub fn calibrate(&mut self, max_current_a: f32, shunt_resistance_ohm: f32) {
        assert!(max_current_a > 0.0, "max_current must be positive");
        assert!(
            shunt_resistance_ohm > 0.0,
            "shunt_resistance must be positive"
        );

        self.current_lsb = max_current_a / 524_288.0; // 2^19

        let mut shunt_cal = 13107.2e6 * self.current_lsb as f64 * shunt_resistance_ohm as f64;
        if self.adc_range == AdcRange::Range40mV {
            shunt_cal *= 4.0;
        }

        let shunt_cal = shunt_cal as u16 & 0x7FFF; // 15-bit
        self.write_u16(Register::ShuntCal, shunt_cal);
    }

    pub fn set_temp_compensation(&mut self, tempco_ppm: u16) {
        let config = self.read_u16(Register::Config);
        self.write_u16(Register::Config, config | (1 << 5));
        self.write_u16(Register::ShuntTempco, tempco_ppm & 0x3FFF);
    }

    pub fn bus_voltage(&mut self) -> f32 {
        let raw = self.read_u24(Register::Vbus) >> 4;
        raw as f32 * 195.3125e-6
    }

    pub fn shunt_voltage(&mut self) -> f32 {
        let raw = self.read_i20(Register::Vshunt);
        let lsb = match self.adc_range {
            AdcRange::Range163mV => 312.5e-9,
            AdcRange::Range40mV => 78.125e-9,
        };
        raw as f32 * lsb
    }

    pub fn current(&mut self) -> f32 {
        let raw = self.read_i20(Register::Current);
        raw as f32 * self.current_lsb
    }

    pub fn power(&mut self) -> f32 {
        let raw = self.read_u24(Register::Power);
        raw as f32 * 3.2 * self.current_lsb
    }

    pub fn energy(&mut self) -> f64 {
        let raw = self.read_u40(Register::Energy);
        raw as f64 * 16.0 * 3.2 * self.current_lsb as f64
    }

    pub fn charge(&mut self) -> f64 {
        let raw = self.read_i40(Register::Charge);
        raw as f64 * self.current_lsb as f64
    }

    pub fn die_temperature(&mut self) -> f32 {
        let raw = self.read_u16(Register::DieTemp) as i16;
        raw as f32 * 7.8125e-3
    }

    pub fn read_all(&mut self) -> Measurements {
        Measurements {
            bus_voltage_v: self.bus_voltage(),
            shunt_voltage_v: self.shunt_voltage(),
            current_a: self.current(),
            power_w: self.power(),
            die_temp_c: self.die_temperature(),
        }
    }

    pub fn reset_accumulators(&mut self) {
        let config = self.read_u16(Register::Config);
        self.write_u16(Register::Config, config | (1 << 14));
    }

    pub fn conversion_ready(&mut self) -> bool {
        let diag = self.read_u16(Register::DiagAlrt);
        diag & (1 << 1) != 0
    }

    pub fn manufacturer_id(&mut self) -> u16 {
        self.read_u16(Register::ManufacturerId)
    }

    /// Returns the device ID (upper 12 bits, without die revision).
    pub fn device_id(&mut self) -> u16 {
        self.read_u16(Register::DeviceId) >> 4
    }

    /// Returns the die revision (lower 4 bits of device ID register).
    pub fn die_revision(&mut self) -> u8 {
        (self.read_u16(Register::DeviceId) & 0xF) as u8
    }

    pub fn release(self) -> I2C {
        self.i2c
    }

    // --- I2C helpers ---

    fn read_u16(&mut self, reg: Register) -> u16 {
        let mut buf = [0u8; 2];
        self.i2c
            .write_read(self.address, &[reg as u8], &mut buf)
            .unwrap();
        u16::from_be_bytes(buf)
    }

    fn write_u16(&mut self, reg: Register, value: u16) {
        let bytes = value.to_be_bytes();
        self.i2c
            .write(self.address, &[reg as u8, bytes[0], bytes[1]])
            .unwrap();
    }

    fn read_u24(&mut self, reg: Register) -> u32 {
        let mut buf = [0u8; 3];
        self.i2c
            .write_read(self.address, &[reg as u8], &mut buf)
            .unwrap();
        ((buf[0] as u32) << 16) | ((buf[1] as u32) << 8) | (buf[2] as u32)
    }

    fn read_i20(&mut self, reg: Register) -> i32 {
        let raw = self.read_u24(reg) >> 4;
        // Sign-extend from bit 19
        if raw & (1 << 19) != 0 {
            raw as i32 | !0xF_FFFF
        } else {
            raw as i32
        }
    }

    fn read_u40(&mut self, reg: Register) -> u64 {
        let mut buf = [0u8; 5];
        self.i2c
            .write_read(self.address, &[reg as u8], &mut buf)
            .unwrap();
        ((buf[0] as u64) << 32)
            | ((buf[1] as u64) << 24)
            | ((buf[2] as u64) << 16)
            | ((buf[3] as u64) << 8)
            | (buf[4] as u64)
    }

    fn read_i40(&mut self, reg: Register) -> i64 {
        let raw = self.read_u40(reg);
        // Sign-extend from bit 39
        if raw & (1 << 39) != 0 {
            raw as i64 | !0xFF_FFFF_FFFF
        } else {
            raw as i64
        }
    }
}
