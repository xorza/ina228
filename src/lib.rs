#![no_std]

mod registers;

use embedded_hal::i2c::I2c;
use registers::Register;

pub use registers::{AdcRange, AveragingCount, ConversionTime, OperatingMode};

/// Default I2C address (A0=GND, A1=GND).
pub const DEFAULT_ADDRESS: u8 = 0x40;
/// Expected value from the manufacturer ID register (Texas Instruments).
pub const MANUFACTURER_ID: u16 = 0x5449;
/// Device ID (upper 12 bits of register 0x3F; lower 4 bits are die revision).
pub const DEVICE_ID: u16 = 0x228;

/// INA228 high-precision digital power monitor driver.
///
/// Supports bus/shunt voltage, current, power, energy, and charge measurements
/// over I2C. Valid addresses are `0x40..=0x4F` (set via A0/A1 pins).
#[derive(Debug)]
pub struct Ina228<I2C> {
    i2c: I2C,
    address: u8,
    current_lsb: f32,
    shunt_resistance_ohm: f32,
    adc_range: AdcRange,
}

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

/// Status flags from the DIAG_ALRT register.
#[derive(Debug, Clone, Copy, Default)]
pub struct DiagnosticFlags {
    pub memory_status: bool,
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

impl<I2C: I2c> Ina228<I2C> {
    /// Creates a new INA228 driver. Panics if `address` is not in `0x40..=0x4F`.
    pub fn new(i2c: I2C, address: u8) -> Self {
        assert!(
            (0x40..=0x4F).contains(&address),
            "INA228 address must be in 0x40..=0x4F"
        );
        Self {
            i2c,
            address,
            current_lsb: 0.0,
            shunt_resistance_ohm: 0.0,
            adc_range: AdcRange::Range163mV,
        }
    }

    /// Performs a soft reset, restoring all registers to defaults.
    pub fn reset(&mut self) -> Result<(), I2C::Error> {
        // Bit 15 = RST
        self.write_u16(Register::Config, 1 << 15)?;
        self.current_lsb = 0.0;
        self.shunt_resistance_ohm = 0.0;
        self.adc_range = AdcRange::Range163mV;
        Ok(())
    }

    /// Configures operating mode, per-channel conversion times, and averaging.
    /// Writes the ADC_CONFIG register.
    pub fn configure(
        &mut self,
        mode: OperatingMode,
        vbus_ct: ConversionTime,
        vshunt_ct: ConversionTime,
        temp_ct: ConversionTime,
        avg: AveragingCount,
    ) -> Result<(), I2C::Error> {
        let value = ((mode as u16) << 12)
            | ((vbus_ct as u16) << 9)
            | ((vshunt_ct as u16) << 6)
            | ((temp_ct as u16) << 3)
            | (avg as u16);
        self.write_u16(Register::AdcConfig, value)
    }

    /// Sets the shunt ADC full-scale range. Re-writes SHUNT_CAL if already calibrated.
    pub fn set_adc_range(&mut self, range: AdcRange) -> Result<(), I2C::Error> {
        let config = self.read_u16(Register::Config)?;
        let value = match range {
            AdcRange::Range163mV => config & !(1 << 4),
            AdcRange::Range40mV => config | (1 << 4),
        };
        self.write_u16(Register::Config, value)?;

        self.adc_range = range;

        // Re-write SHUNT_CAL if already calibrated, since the range multiplier changed.
        if self.current_lsb != 0.0 {
            self.write_shunt_cal(self.current_lsb, self.shunt_resistance_ohm)?;
        }
        Ok(())
    }

    /// Calibrate for current/power measurement.
    /// `max_current_a`: maximum expected current in Amps.
    /// `shunt_resistance_ohm`: shunt resistor value in Ohms.
    pub fn calibrate(
        &mut self,
        max_current_a: f32,
        shunt_resistance_ohm: f32,
    ) -> Result<(), I2C::Error> {
        assert!(max_current_a > 0.0, "max_current must be positive");
        assert!(
            shunt_resistance_ohm > 0.0,
            "shunt_resistance must be positive"
        );

        let current_lsb = max_current_a / 524_288.0; // 2^19
        self.write_shunt_cal(current_lsb, shunt_resistance_ohm)?;
        self.current_lsb = current_lsb;
        self.shunt_resistance_ohm = shunt_resistance_ohm;
        Ok(())
    }

    fn write_shunt_cal(
        &mut self,
        current_lsb: f32,
        shunt_resistance_ohm: f32,
    ) -> Result<(), I2C::Error> {
        let mut shunt_cal = 13107.2e6 * current_lsb as f64 * shunt_resistance_ohm as f64;
        if self.adc_range == AdcRange::Range40mV {
            shunt_cal *= 4.0;
        }

        assert!(
            shunt_cal <= 32767.0,
            "SHUNT_CAL overflow: reduce max_current or shunt_resistance"
        );
        let shunt_cal = shunt_cal as u16 & 0x7FFF; // 15-bit
        self.write_u16(Register::ShuntCal, shunt_cal)
    }

    /// Enables shunt temperature compensation with the given coefficient (ppm/°C).
    pub fn set_temp_compensation(&mut self, tempco_ppm: u16) -> Result<(), I2C::Error> {
        let config = self.read_u16(Register::Config)?;
        self.write_u16(Register::Config, config | (1 << 5))?;
        self.write_u16(Register::ShuntTempco, tempco_ppm & 0x3FFF)
    }

    /// Disables shunt temperature compensation.
    pub fn disable_temp_compensation(&mut self) -> Result<(), I2C::Error> {
        let config = self.read_u16(Register::Config)?;
        self.write_u16(Register::Config, config & !(1 << 5))
    }

    /// Returns bus voltage in Volts.
    pub fn bus_voltage(&mut self) -> Result<f32, I2C::Error> {
        let raw = self.read_u24(Register::Vbus)? >> 4;
        Ok(raw as f32 * 195.3125e-6)
    }

    /// Returns shunt voltage in Volts. LSB depends on the configured ADC range.
    pub fn shunt_voltage(&mut self) -> Result<f32, I2C::Error> {
        let raw = self.read_i20(Register::Vshunt)?;
        let lsb = match self.adc_range {
            AdcRange::Range163mV => 312.5e-9,
            AdcRange::Range40mV => 78.125e-9,
        };
        Ok(raw as f32 * lsb)
    }

    /// Returns current in Amps. Requires prior [`calibrate`](Self::calibrate) call.
    pub fn current(&mut self) -> Result<f32, I2C::Error> {
        debug_assert!(
            self.current_lsb != 0.0,
            "call calibrate() before reading current"
        );
        let raw = self.read_i20(Register::Current)?;
        Ok(raw as f32 * self.current_lsb)
    }

    /// Returns power in Watts. Requires prior [`calibrate`](Self::calibrate) call.
    pub fn power(&mut self) -> Result<f32, I2C::Error> {
        debug_assert!(
            self.current_lsb != 0.0,
            "call calibrate() before reading power"
        );
        let raw = self.read_u24(Register::Power)?;
        Ok(raw as f32 * 3.2 * self.current_lsb)
    }

    /// Returns accumulated energy in Joules. Requires prior [`calibrate`](Self::calibrate) call.
    pub fn energy(&mut self) -> Result<f64, I2C::Error> {
        debug_assert!(
            self.current_lsb != 0.0,
            "call calibrate() before reading energy"
        );
        let raw = self.read_u40(Register::Energy)?;
        Ok(raw as f64 * 16.0 * 3.2 * self.current_lsb as f64)
    }

    /// Returns accumulated charge in Coulombs. Requires prior [`calibrate`](Self::calibrate) call.
    pub fn charge(&mut self) -> Result<f64, I2C::Error> {
        debug_assert!(
            self.current_lsb != 0.0,
            "call calibrate() before reading charge"
        );
        let raw = self.read_i40(Register::Charge)?;
        Ok(raw as f64 * self.current_lsb as f64)
    }

    /// Returns die temperature in degrees Celsius.
    pub fn die_temperature(&mut self) -> Result<f32, I2C::Error> {
        let raw = self.read_u16(Register::DieTemp)? as i16;
        Ok(raw as f32 * 7.8125e-3)
    }

    /// Resets the energy and charge accumulator registers to zero.
    pub fn reset_accumulators(&mut self) -> Result<(), I2C::Error> {
        let config = self.read_u16(Register::Config)?;
        self.write_u16(Register::Config, config | (1 << 14))
    }

    /// Returns `true` if a new conversion result is available.
    pub fn conversion_ready(&mut self) -> Result<bool, I2C::Error> {
        let diag = self.read_u16(Register::DiagAlrt)?;
        Ok(diag & (1 << 1) != 0)
    }

    /// Reads all diagnostic and alert flags from the DIAG_ALRT register.
    pub fn diagnostic_flags(&mut self) -> Result<DiagnosticFlags, I2C::Error> {
        let d = self.read_u16(Register::DiagAlrt)?;
        Ok(DiagnosticFlags {
            memory_status: d & (1 << 15) != 0,
            conversion_ready: d & (1 << 1) != 0,
            energy_overflow: d & (1 << 9) != 0,
            math_overflow: d & (1 << 8) != 0,
            temp_over_limit: d & (1 << 7) != 0,
            shunt_over_limit: d & (1 << 6) != 0,
            shunt_under_limit: d & (1 << 5) != 0,
            bus_over_limit: d & (1 << 4) != 0,
            bus_under_limit: d & (1 << 3) != 0,
            power_over_limit: d & (1 << 2) != 0,
            charge_overflow: d & (1 << 0) != 0,
        })
    }

    /// Configures alert pin behavior (DIAG_ALRT upper bits). Preserves the lower
    /// 10 status-flag bits.
    pub fn configure_alerts(&mut self, cfg: AlertConfig) -> Result<(), I2C::Error> {
        let diag = self.read_u16(Register::DiagAlrt)?;
        let mut value = diag & 0x03FF;
        if cfg.conversion_ready {
            value |= 1 << 14;
        }
        if cfg.slow_alert {
            value |= 1 << 13;
        }
        if cfg.active_high {
            value |= 1 << 12;
        }
        if cfg.latch {
            value |= 1 << 11;
        }
        self.write_u16(Register::DiagAlrt, value)
    }

    /// Set shunt over-voltage limit in Volts.
    pub fn set_shunt_overvoltage_limit(&mut self, voltage_v: f32) -> Result<(), I2C::Error> {
        let raw = (voltage_v / self.shunt_limit_lsb()) as i16;
        self.write_u16(Register::Sovl, raw as u16)
    }

    /// Set shunt under-voltage limit in Volts.
    pub fn set_shunt_undervoltage_limit(&mut self, voltage_v: f32) -> Result<(), I2C::Error> {
        let raw = (voltage_v / self.shunt_limit_lsb()) as i16;
        self.write_u16(Register::Suvl, raw as u16)
    }

    /// Set bus over-voltage limit in Volts.
    pub fn set_bus_overvoltage_limit(&mut self, voltage_v: f32) -> Result<(), I2C::Error> {
        let raw = (voltage_v / 3.125e-3) as u16;
        self.write_u16(Register::Bovl, raw)
    }

    /// Set bus under-voltage limit in Volts.
    pub fn set_bus_undervoltage_limit(&mut self, voltage_v: f32) -> Result<(), I2C::Error> {
        let raw = (voltage_v / 3.125e-3) as u16;
        self.write_u16(Register::Buvl, raw)
    }

    /// Set temperature over-limit in degrees Celsius.
    pub fn set_temperature_limit(&mut self, temp_c: f32) -> Result<(), I2C::Error> {
        let raw = (temp_c / 7.8125e-3) as i16;
        self.write_u16(Register::TempLimit, raw as u16)
    }

    /// Set power over-limit in Watts.
    pub fn set_power_limit(&mut self, power_w: f32) -> Result<(), I2C::Error> {
        debug_assert!(
            self.current_lsb != 0.0,
            "call calibrate() before setting power limit"
        );
        let power_lsb = 3.2 * self.current_lsb;
        let raw = (power_w / (256.0 * power_lsb)) as u16;
        self.write_u16(Register::PwrLimit, raw)
    }

    /// Reads the manufacturer ID register (expected: `0x5449` for TI).
    pub fn manufacturer_id(&mut self) -> Result<u16, I2C::Error> {
        self.read_u16(Register::ManufacturerId)
    }

    /// Returns the device ID (upper 12 bits, without die revision).
    pub fn device_id(&mut self) -> Result<u16, I2C::Error> {
        Ok(self.read_u16(Register::DeviceId)? >> 4)
    }

    /// Returns the die revision (lower 4 bits of device ID register).
    pub fn die_revision(&mut self) -> Result<u8, I2C::Error> {
        Ok((self.read_u16(Register::DeviceId)? & 0xF) as u8)
    }

    /// Consumes the driver and returns the underlying I2C bus.
    pub fn release(self) -> I2C {
        self.i2c
    }

    fn shunt_limit_lsb(&self) -> f32 {
        match self.adc_range {
            AdcRange::Range163mV => 5.0e-6,
            AdcRange::Range40mV => 1.25e-6,
        }
    }

    // --- I2C helpers ---

    fn read_u16(&mut self, reg: Register) -> Result<u16, I2C::Error> {
        let mut buf = [0u8; 2];
        self.i2c.write_read(self.address, &[reg as u8], &mut buf)?;
        Ok(u16::from_be_bytes(buf))
    }

    fn write_u16(&mut self, reg: Register, value: u16) -> Result<(), I2C::Error> {
        let bytes = value.to_be_bytes();
        self.i2c
            .write(self.address, &[reg as u8, bytes[0], bytes[1]])
    }

    fn read_u24(&mut self, reg: Register) -> Result<u32, I2C::Error> {
        let mut buf = [0u8; 3];
        self.i2c.write_read(self.address, &[reg as u8], &mut buf)?;
        Ok(((buf[0] as u32) << 16) | ((buf[1] as u32) << 8) | (buf[2] as u32))
    }

    fn read_i20(&mut self, reg: Register) -> Result<i32, I2C::Error> {
        let raw = self.read_u24(reg)? >> 4;
        Ok(((raw as i32) << 12) >> 12)
    }

    fn read_u40(&mut self, reg: Register) -> Result<u64, I2C::Error> {
        let mut buf = [0u8; 5];
        self.i2c.write_read(self.address, &[reg as u8], &mut buf)?;
        Ok(((buf[0] as u64) << 32)
            | ((buf[1] as u64) << 24)
            | ((buf[2] as u64) << 16)
            | ((buf[3] as u64) << 8)
            | (buf[4] as u64))
    }

    fn read_i40(&mut self, reg: Register) -> Result<i64, I2C::Error> {
        let raw = self.read_u40(reg)?;
        Ok(((raw as i64) << 24) >> 24)
    }
}
