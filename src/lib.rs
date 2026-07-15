#![no_std]

mod registers;

use embedded_hal::i2c::I2c;
use registers::{Register, adc_config, config, diagnostic_alert};

pub use registers::{AdcRange, AveragingCount, ConversionTime, OperatingMode};

/// Default I2C address (A0=GND, A1=GND).
pub const DEFAULT_ADDRESS: u8 = 0x40;
/// Expected value from the manufacturer ID register (Texas Instruments).
pub const MANUFACTURER_ID: u16 = 0x5449;
/// Device ID (upper 12 bits of register 0x3F; lower 4 bits are die revision).
pub const DEVICE_ID: u16 = 0x228;

/// Invalid physical configuration supplied to the driver.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigurationError {
    /// Maximum expected current must be finite and positive.
    MaxCurrent,
    /// Shunt resistance must be finite and positive.
    ShuntResistance,
    /// Calibration cannot be represented for the selected ADC range.
    Calibration,
    /// Shunt temperature coefficient exceeds the 14-bit register.
    TemperatureCoefficient,
    /// Shunt-voltage threshold cannot be represented by its signed register.
    ShuntVoltageLimit,
    /// Bus-voltage threshold cannot be represented by its 15-bit register.
    BusVoltageLimit,
    /// Temperature threshold cannot be represented by its signed register.
    TemperatureLimit,
    /// Power threshold cannot be represented by its unsigned register.
    PowerLimit,
    /// Energy and charge accumulators are invalid outside continuous conversion modes.
    AccumulatorMode,
}

/// INA228 operation error.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error<E> {
    /// I2C bus operation failed.
    I2c(E),
    /// A physical configuration value is invalid or unrepresentable.
    InvalidConfiguration(ConfigurationError),
}

impl<E> From<E> for Error<E> {
    fn from(error: E) -> Self {
        Self::I2c(error)
    }
}

/// Failure returned while constructing an [`Ina228`].
#[derive(Debug)]
pub enum InitializationError<I2C: I2c> {
    /// The supplied address is outside the INA228 address range.
    InvalidAddress {
        /// I2C bus returned to the caller for recovery or retry.
        i2c: I2C,
        /// Invalid address supplied by the caller.
        address: u8,
    },
    /// Reading CONFIG from the device failed.
    I2c {
        /// I2C bus returned to the caller for recovery or retry.
        i2c: I2C,
        /// Error reported by the I2C bus.
        error: I2C::Error,
    },
}

#[derive(Debug, Clone, Copy)]
struct Calibration {
    current_lsb: f32,
    shunt_resistance_ohm: f32,
}

#[derive(Debug, Clone, Copy)]
struct SuspendedConversions {
    adc_config: u16,
    resume: bool,
}

impl Calibration {
    const CURRENT_ADC_COUNTS: f32 = 524_288.0;
    const POWER_LSB_MULTIPLIER: f64 = 3.2;

    fn shunt_cal(self, adc_range: AdcRange) -> Result<u16, ConfigurationError> {
        let max_shunt_voltage = self.current_lsb as f64
            * Self::CURRENT_ADC_COUNTS as f64
            * self.shunt_resistance_ohm as f64;
        if !max_shunt_voltage.is_finite() || max_shunt_voltage > adc_range.full_scale_voltage() {
            return Err(ConfigurationError::Calibration);
        }

        let shunt_cal = 13107.2e6
            * self.current_lsb as f64
            * self.shunt_resistance_ohm as f64
            * adc_range.shunt_cal_multiplier();

        let shunt_cal = shunt_cal + 0.5;
        if !shunt_cal.is_finite() || !(1.0..32768.0).contains(&shunt_cal) {
            return Err(ConfigurationError::Calibration);
        }
        Ok(shunt_cal as u16)
    }

    fn power_lsb(self) -> f32 {
        Self::POWER_LSB_MULTIPLIER as f32 * self.current_lsb
    }

    fn energy_lsb(self) -> f64 {
        16.0 * Self::POWER_LSB_MULTIPLIER * self.current_lsb as f64
    }
}

fn encode_signed(
    value: f32,
    lsb: f32,
    error: ConfigurationError,
) -> Result<u16, ConfigurationError> {
    if !value.is_finite() {
        return Err(error);
    }
    let raw = value / lsb;
    if !raw.is_finite() || raw <= i16::MIN as f32 - 0.5 || raw >= i16::MAX as f32 + 0.5 {
        return Err(error);
    }
    let rounded = if raw >= 0.0 { raw + 0.5 } else { raw - 0.5 };
    Ok(rounded as i16 as u16)
}

fn encode_unsigned(
    value: f32,
    lsb: f32,
    max_raw: u16,
    error: ConfigurationError,
) -> Result<u16, ConfigurationError> {
    if !value.is_finite() {
        return Err(error);
    }
    let raw = value / lsb;
    if !raw.is_finite() || raw < 0.0 || raw >= max_raw as f32 + 0.5 {
        return Err(error);
    }
    Ok((raw + 0.5) as u16)
}

/// INA228 high-precision digital power monitor driver.
///
/// Supports bus/shunt voltage, current, power, energy, and charge measurements
/// over I2C. Valid addresses are `0x40..=0x4F` (set via A0/A1 pins).
#[derive(Debug)]
pub struct Ina228<I2C> {
    i2c: I2C,
    address: u8,
    calibration: Option<Calibration>,
    adc_range: AdcRange,
}

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

/// Coherent energy, charge, and diagnostic state captured by
/// [`Ina228::take_accumulator_snapshot`].
#[derive(Debug, Clone, Copy)]
pub struct AccumulatorSnapshot {
    pub energy_joules: f64,
    pub charge_coulombs: f64,
    /// Flags captured before reading ENERGY and CHARGE clears their overflow indicators.
    pub diagnostic_flags: DiagnosticFlags,
}

impl<I2C: I2c> Ina228<I2C> {
    /// Creates a driver and reads CONFIG to synchronize the ADC range.
    ///
    /// # Errors
    ///
    /// Returns an [`InitializationError`] containing the I2C bus if `address` is outside
    /// `0x40..=0x4F` or CONFIG cannot be read.
    pub fn new(i2c: I2C, address: u8) -> Result<Self, InitializationError<I2C>> {
        if !(0x40..=0x4F).contains(&address) {
            return Err(InitializationError::InvalidAddress { i2c, address });
        }
        let mut i2c = i2c;
        let config_value = match Self::read_u16_from(&mut i2c, address, Register::Config) {
            Ok(value) => value,
            Err(error) => return Err(InitializationError::I2c { i2c, error }),
        };
        let adc_range = AdcRange::from_config(config_value);
        Ok(Self {
            i2c,
            address,
            calibration: None,
            adc_range,
        })
    }

    /// Performs a soft reset, restoring all registers to defaults.
    pub fn reset(&mut self) -> Result<(), Error<I2C::Error>> {
        self.write_u16(Register::Config, config::RESET)?;
        self.calibration = None;
        self.adc_range = AdcRange::Range163mV;
        Ok(())
    }

    /// Configures operating mode, per-channel conversion times, and averaging.
    /// Writes the ADC_CONFIG register.
    pub fn configure(&mut self, config: AdcConfig) -> Result<(), Error<I2C::Error>> {
        let value = ((config.mode as u16) << 12)
            | ((config.bus_conversion_time as u16) << 9)
            | ((config.shunt_conversion_time as u16) << 6)
            | ((config.temperature_conversion_time as u16) << 3)
            | (config.averaging as u16);
        self.write_u16(Register::AdcConfig, value)
    }

    /// Sets the shunt ADC full-scale range. Re-writes SHUNT_CAL if already calibrated.
    ///
    /// Changing range disables the shunt over- and under-voltage alerts because their
    /// register scale depends on the selected range. Conversions are suspended while
    /// CONFIG and SHUNT_CAL are updated, then the previous ADC configuration is restored.
    /// The caller must wait for a new conversion before reading measurements produced
    /// under the new range; this method does not wait for conversion completion.
    ///
    /// An I2C failure after conversions are suspended leaves the ADC in shutdown mode.
    /// If CONFIG succeeds but the SHUNT_CAL write fails, the new range remains active
    /// and calibration-dependent operations require another [`calibrate`](Self::calibrate)
    /// call. Use [`configure`](Self::configure) to resume conversions after an error.
    pub fn set_adc_range(&mut self, range: AdcRange) -> Result<(), Error<I2C::Error>> {
        if range == self.adc_range {
            return Ok(());
        }

        let calibration = self.calibration;
        let shunt_cal = calibration
            .map(|calibration| calibration.shunt_cal(range))
            .transpose()
            .map_err(Error::InvalidConfiguration)?;
        let config_value = self.read_u16(Register::Config)?;
        let suspended = self.suspend_conversions()?;

        self.write_u16(Register::Sovl, i16::MAX as u16)?;
        self.write_u16(Register::Suvl, i16::MIN as u16)?;

        let value = range.apply_to_config(config_value);
        self.write_u16(Register::Config, value)?;

        self.adc_range = range;

        if let Some(shunt_cal) = shunt_cal {
            self.calibration = None;
            self.write_u16(Register::ShuntCal, shunt_cal)?;
            self.calibration = calibration;
        }

        self.restore_conversions(suspended)
    }

    /// Calibrate for current, power, energy, and charge measurement.
    ///
    /// Conversions are suspended while SHUNT_CAL and the accumulators are updated,
    /// then the previous ADC configuration is restored. Restoring a non-shutdown
    /// mode starts a fresh conversion and clears the previous conversion-ready flag;
    /// this method does not wait for conversion completion. If the previous mode was
    /// shutdown, call [`configure`](Self::configure) before waiting for fresh data.
    ///
    /// If an I2C failure occurs after conversions are suspended, the ADC remains in
    /// shutdown mode. If SHUNT_CAL succeeds but the accumulator reset fails,
    /// calibration-dependent operations require another `calibrate()` call. Use
    /// [`configure`](Self::configure) to resume conversions after an error.
    /// `max_current_a`: maximum expected current in Amps.
    /// `shunt_resistance_ohm`: shunt resistor value in Ohms.
    pub fn calibrate(
        &mut self,
        max_current_a: f32,
        shunt_resistance_ohm: f32,
    ) -> Result<(), Error<I2C::Error>> {
        if !max_current_a.is_finite() || max_current_a <= 0.0 {
            return Err(Error::InvalidConfiguration(ConfigurationError::MaxCurrent));
        }
        if !shunt_resistance_ohm.is_finite() || shunt_resistance_ohm <= 0.0 {
            return Err(Error::InvalidConfiguration(
                ConfigurationError::ShuntResistance,
            ));
        }

        let calibration = Calibration {
            current_lsb: max_current_a / Calibration::CURRENT_ADC_COUNTS,
            shunt_resistance_ohm,
        };
        let shunt_cal = calibration
            .shunt_cal(self.adc_range)
            .map_err(Error::InvalidConfiguration)?;
        let suspended = self.suspend_conversions()?;
        self.write_u16(Register::ShuntCal, shunt_cal)?;
        self.calibration = None;
        self.reset_accumulators()?;
        self.calibration = Some(calibration);
        self.restore_conversions(suspended)
    }

    /// Enables shunt temperature compensation with a coefficient from 0 to 16383 ppm/°C.
    ///
    /// Conversions are suspended while the coefficient and CONFIG are updated, then
    /// the previous ADC configuration is restored. Restoring a non-shutdown mode
    /// starts a fresh conversion and clears the previous conversion-ready flag; this
    /// method does not wait for conversion completion. If the previous mode was
    /// shutdown, call [`configure`](Self::configure) before waiting for fresh data.
    /// An I2C failure after suspension leaves the ADC in shutdown mode.
    pub fn set_temp_compensation(&mut self, tempco_ppm: u16) -> Result<(), Error<I2C::Error>> {
        if tempco_ppm > 0x3FFF {
            return Err(Error::InvalidConfiguration(
                ConfigurationError::TemperatureCoefficient,
            ));
        }
        let config_value = self.read_u16(Register::Config)?;
        let suspended = self.suspend_conversions()?;
        self.write_u16(Register::ShuntTempco, tempco_ppm)?;
        self.write_u16(
            Register::Config,
            config_value | config::TEMPERATURE_COMPENSATION,
        )?;
        self.restore_conversions(suspended)
    }

    /// Disables shunt temperature compensation while conversions are suspended.
    ///
    /// The previous ADC configuration is restored afterward. Restoring a non-shutdown
    /// mode starts a fresh conversion and clears the previous conversion-ready flag;
    /// this method does not wait for conversion completion. If the previous mode was
    /// shutdown, call [`configure`](Self::configure) before waiting for fresh data.
    /// An I2C failure after suspension leaves the ADC in shutdown mode.
    pub fn disable_temp_compensation(&mut self) -> Result<(), Error<I2C::Error>> {
        let config_value = self.read_u16(Register::Config)?;
        let suspended = self.suspend_conversions()?;
        self.write_u16(
            Register::Config,
            config_value & !config::TEMPERATURE_COMPENSATION,
        )?;
        self.restore_conversions(suspended)
    }

    /// Returns bus voltage in Volts.
    pub fn bus_voltage(&mut self) -> Result<f32, Error<I2C::Error>> {
        let raw = self.read_u24(Register::Vbus)? >> 4;
        Ok(raw as f32 * 195.3125e-6)
    }

    /// Returns shunt voltage in Volts. LSB depends on the configured ADC range.
    pub fn shunt_voltage(&mut self) -> Result<f32, Error<I2C::Error>> {
        let raw = self.read_i20(Register::Vshunt)?;
        Ok(raw as f32 * self.adc_range.shunt_voltage_lsb())
    }

    /// Returns current in Amps. Requires prior [`calibrate`](Self::calibrate) call.
    pub fn current(&mut self) -> Result<f32, Error<I2C::Error>> {
        let calibration = self
            .calibration
            .expect("call calibrate() before reading current");
        let raw = self.read_i20(Register::Current)?;
        Ok(raw as f32 * calibration.current_lsb)
    }

    /// Returns power in Watts. Requires prior [`calibrate`](Self::calibrate) call.
    pub fn power(&mut self) -> Result<f32, Error<I2C::Error>> {
        let calibration = self
            .calibration
            .expect("call calibrate() before reading power");
        let raw = self.read_u24(Register::Power)?;
        Ok(raw as f32 * calibration.power_lsb())
    }

    /// Takes a coherent energy, charge, and diagnostic snapshot.
    ///
    /// Accumulators are valid only in continuous conversion modes. Conversions are
    /// suspended during the capture so DIAG_ALRT, ENERGY, and CHARGE cannot change
    /// between transactions, then the previous ADC configuration is restored. The
    /// suspension creates a brief gap during which energy and charge are not accumulated.
    ///
    /// Reading DIAG_ALRT acknowledges conversion-ready and any latched threshold alerts.
    /// Reading ENERGY and CHARGE clears their respective overflow indicators. If an I2C
    /// operation fails after suspension, the ADC remains in shutdown mode and earlier
    /// acknowledgement or clear-on-read effects may already have occurred. Use
    /// [`configure`](Self::configure) to resume conversions after an error.
    pub fn take_accumulator_snapshot(&mut self) -> Result<AccumulatorSnapshot, Error<I2C::Error>> {
        let calibration = self
            .calibration
            .expect("call calibrate() before reading accumulators");
        let adc_config = self.read_u16(Register::AdcConfig)?;
        let mode = adc_config & adc_config::MODE_MASK;
        if mode < adc_config::FIRST_CONTINUOUS_MODE {
            return Err(Error::InvalidConfiguration(
                ConfigurationError::AccumulatorMode,
            ));
        }
        let suspended = self.suspend_captured_conversions(adc_config)?;
        let diagnostic_flags = self.take_diagnostic_flags()?;
        let energy_raw = self.read_u40(Register::Energy)?;
        let charge_raw = self.read_i40(Register::Charge)?;
        let snapshot = AccumulatorSnapshot {
            energy_joules: energy_raw as f64 * calibration.energy_lsb(),
            charge_coulombs: charge_raw as f64 * calibration.current_lsb as f64,
            diagnostic_flags,
        };
        self.restore_conversions(suspended)?;
        Ok(snapshot)
    }

    /// Returns die temperature in degrees Celsius.
    pub fn die_temperature(&mut self) -> Result<f32, Error<I2C::Error>> {
        let raw = self.read_u16(Register::DieTemp)? as i16;
        Ok(raw as f32 * 7.8125e-3)
    }

    /// Resets the energy and charge accumulator registers to zero.
    pub fn reset_accumulators(&mut self) -> Result<(), Error<I2C::Error>> {
        let config_value = self.read_u16(Register::Config)?;
        self.write_u16(Register::Config, config_value | config::RESET_ACCUMULATORS)
    }

    /// Takes all diagnostic and alert flags from the DIAG_ALRT register.
    ///
    /// This acknowledges conversion-ready and, in latched mode, threshold alert flags.
    pub fn take_diagnostic_flags(&mut self) -> Result<DiagnosticFlags, Error<I2C::Error>> {
        let d = self.read_u16(Register::DiagAlrt)?;
        Ok(DiagnosticFlags {
            memory_ok: d & diagnostic_alert::MEMORY_OK != 0,
            conversion_ready: d & diagnostic_alert::CONVERSION_READY != 0,
            energy_overflow: d & diagnostic_alert::ENERGY_OVERFLOW != 0,
            math_overflow: d & diagnostic_alert::MATH_OVERFLOW != 0,
            temp_over_limit: d & diagnostic_alert::TEMP_OVER_LIMIT != 0,
            shunt_over_limit: d & diagnostic_alert::SHUNT_OVER_LIMIT != 0,
            shunt_under_limit: d & diagnostic_alert::SHUNT_UNDER_LIMIT != 0,
            bus_over_limit: d & diagnostic_alert::BUS_OVER_LIMIT != 0,
            bus_under_limit: d & diagnostic_alert::BUS_UNDER_LIMIT != 0,
            power_over_limit: d & diagnostic_alert::POWER_OVER_LIMIT != 0,
            charge_overflow: d & diagnostic_alert::CHARGE_OVERFLOW != 0,
        })
    }

    /// Configures alert pin behavior. Writing DIAG_ALRT acknowledges latched alerts.
    pub fn configure_alerts(&mut self, cfg: AlertConfig) -> Result<(), Error<I2C::Error>> {
        let mut value = 0;
        if cfg.latch {
            value |= diagnostic_alert::LATCH;
        }
        if cfg.conversion_ready {
            value |= diagnostic_alert::CONVERSION_READY_ENABLE;
        }
        if cfg.slow_alert {
            value |= diagnostic_alert::SLOW_ALERT;
        }
        if cfg.active_high {
            value |= diagnostic_alert::ACTIVE_HIGH;
        }
        self.write_u16(Register::DiagAlrt, value)
    }

    /// Set shunt over-voltage limit in Volts.
    pub fn set_shunt_overvoltage_limit(&mut self, voltage_v: f32) -> Result<(), Error<I2C::Error>> {
        let raw = encode_signed(
            voltage_v,
            self.adc_range.shunt_limit_lsb(),
            ConfigurationError::ShuntVoltageLimit,
        )
        .map_err(Error::InvalidConfiguration)?;
        self.write_u16(Register::Sovl, raw)
    }

    /// Set shunt under-voltage limit in Volts.
    pub fn set_shunt_undervoltage_limit(
        &mut self,
        voltage_v: f32,
    ) -> Result<(), Error<I2C::Error>> {
        let raw = encode_signed(
            voltage_v,
            self.adc_range.shunt_limit_lsb(),
            ConfigurationError::ShuntVoltageLimit,
        )
        .map_err(Error::InvalidConfiguration)?;
        self.write_u16(Register::Suvl, raw)
    }

    /// Set bus over-voltage limit in Volts.
    pub fn set_bus_overvoltage_limit(&mut self, voltage_v: f32) -> Result<(), Error<I2C::Error>> {
        let raw = encode_unsigned(
            voltage_v,
            3.125e-3,
            0x7FFF,
            ConfigurationError::BusVoltageLimit,
        )
        .map_err(Error::InvalidConfiguration)?;
        self.write_u16(Register::Bovl, raw)
    }

    /// Set bus under-voltage limit in Volts.
    pub fn set_bus_undervoltage_limit(&mut self, voltage_v: f32) -> Result<(), Error<I2C::Error>> {
        let raw = encode_unsigned(
            voltage_v,
            3.125e-3,
            0x7FFF,
            ConfigurationError::BusVoltageLimit,
        )
        .map_err(Error::InvalidConfiguration)?;
        self.write_u16(Register::Buvl, raw)
    }

    /// Set temperature over-limit in degrees Celsius.
    pub fn set_temperature_limit(&mut self, temp_c: f32) -> Result<(), Error<I2C::Error>> {
        let raw = encode_signed(temp_c, 7.8125e-3, ConfigurationError::TemperatureLimit)
            .map_err(Error::InvalidConfiguration)?;
        self.write_u16(Register::TempLimit, raw)
    }

    /// Set power over-limit in Watts.
    pub fn set_power_limit(&mut self, power_w: f32) -> Result<(), Error<I2C::Error>> {
        let calibration = self
            .calibration
            .expect("call calibrate() before setting power limit");
        let raw = encode_unsigned(
            power_w,
            256.0 * calibration.power_lsb(),
            u16::MAX,
            ConfigurationError::PowerLimit,
        )
        .map_err(Error::InvalidConfiguration)?;
        self.write_u16(Register::PwrLimit, raw)
    }

    /// Reads the manufacturer ID register (expected: `0x5449` for TI).
    pub fn manufacturer_id(&mut self) -> Result<u16, Error<I2C::Error>> {
        self.read_u16(Register::ManufacturerId)
    }

    /// Returns the device ID (upper 12 bits, without die revision).
    pub fn device_id(&mut self) -> Result<u16, Error<I2C::Error>> {
        Ok(self.read_u16(Register::DeviceId)? >> 4)
    }

    /// Returns the die revision (lower 4 bits of device ID register).
    pub fn die_revision(&mut self) -> Result<u8, Error<I2C::Error>> {
        Ok((self.read_u16(Register::DeviceId)? & 0xF) as u8)
    }

    /// Consumes the driver and returns the underlying I2C bus.
    pub fn release(self) -> I2C {
        self.i2c
    }

    fn suspend_conversions(&mut self) -> Result<SuspendedConversions, Error<I2C::Error>> {
        let adc_config = self.read_u16(Register::AdcConfig)?;
        self.suspend_captured_conversions(adc_config)
    }

    fn suspend_captured_conversions(
        &mut self,
        adc_config: u16,
    ) -> Result<SuspendedConversions, Error<I2C::Error>> {
        let mode = adc_config & adc_config::MODE_MASK;
        let resume = mode != 0 && mode != adc_config::ALTERNATE_SHUTDOWN_MODE;
        if resume {
            self.write_u16(Register::AdcConfig, adc_config & !adc_config::MODE_MASK)?;
        }
        Ok(SuspendedConversions { adc_config, resume })
    }

    fn restore_conversions(
        &mut self,
        suspended: SuspendedConversions,
    ) -> Result<(), Error<I2C::Error>> {
        if suspended.resume {
            self.write_u16(Register::AdcConfig, suspended.adc_config)?;
        }
        Ok(())
    }

    fn read_u16_from(i2c: &mut I2C, address: u8, reg: Register) -> Result<u16, I2C::Error> {
        let mut buf = [0u8; 2];
        i2c.write_read(address, &[reg as u8], &mut buf)?;
        Ok(u16::from_be_bytes(buf))
    }

    fn read_u16(&mut self, reg: Register) -> Result<u16, Error<I2C::Error>> {
        Self::read_u16_from(&mut self.i2c, self.address, reg).map_err(Error::I2c)
    }

    fn write_u16(&mut self, reg: Register, value: u16) -> Result<(), Error<I2C::Error>> {
        let bytes = value.to_be_bytes();
        self.i2c
            .write(self.address, &[reg as u8, bytes[0], bytes[1]])
            .map_err(Error::I2c)
    }

    fn read_u24(&mut self, reg: Register) -> Result<u32, Error<I2C::Error>> {
        let mut bytes = [0u8; 4];
        self.i2c
            .write_read(self.address, &[reg as u8], &mut bytes[1..])?;
        Ok(u32::from_be_bytes(bytes))
    }

    fn read_i20(&mut self, reg: Register) -> Result<i32, Error<I2C::Error>> {
        let raw = self.read_u24(reg)? >> 4;
        Ok(((raw as i32) << 12) >> 12)
    }

    fn read_u40(&mut self, reg: Register) -> Result<u64, Error<I2C::Error>> {
        let mut bytes = [0u8; 8];
        self.i2c
            .write_read(self.address, &[reg as u8], &mut bytes[3..])?;
        Ok(u64::from_be_bytes(bytes))
    }

    fn read_i40(&mut self, reg: Register) -> Result<i64, Error<I2C::Error>> {
        let raw = self.read_u40(reg)?;
        Ok(((raw as i64) << 24) >> 24)
    }
}
