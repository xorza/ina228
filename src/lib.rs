#![no_std]

mod adc_config;
mod calibration;
mod config;
mod error;
mod registers;
mod scale;

use adc_config::AdcConfigWord;
use calibration::Calibration;
use config::Config;
use embedded_hal::i2c::I2c;
use registers::{Register, diagnostic_alert};

pub use adc_config::AdcConfig;
pub use error::{CaptureError, ConfigurationError, Error, InitializationError};
pub use registers::{AdcRange, AveragingCount, ConversionTime, OperatingMode};

/// Default I2C address (A0=GND, A1=GND).
pub const DEFAULT_ADDRESS: u8 = 0x40;
/// Expected value from the manufacturer ID register (Texas Instruments).
pub const MANUFACTURER_ID: u16 = 0x5449;
/// Device ID (upper 12 bits of register 0x3F; lower 4 bits are die revision).
pub const DEVICE_ID: u16 = 0x228;

/// Widens a `bits`-wide two's-complement value to a full `i64`.
fn sign_extend(value: u64, bits: u32) -> i64 {
    let shift = u64::BITS - bits;
    ((value << shift) as i64) >> shift
}

fn encode_signed(value: f32, lsb: f64) -> Result<u16, ConfigurationError> {
    let raw = value as f64 / lsb;
    if !raw.is_finite() || raw <= i16::MIN as f64 - 0.5 || raw >= i16::MAX as f64 + 0.5 {
        return Err(ConfigurationError::Unrepresentable);
    }
    let rounded = if raw >= 0.0 { raw + 0.5 } else { raw - 0.5 };
    Ok(rounded as i16 as u16)
}

fn encode_unsigned(value: f32, lsb: f64, max_raw: u16) -> Result<u16, ConfigurationError> {
    let raw = value as f64 / lsb;
    if !raw.is_finite() || raw < 0.0 || raw >= max_raw as f64 + 0.5 {
        return Err(ConfigurationError::Unrepresentable);
    }
    Ok((raw + 0.5) as u16)
}

/// INA228 high-precision digital power monitor driver.
///
/// Measures bus and shunt voltage, current, power, energy, and charge over I2C. Valid
/// addresses are `0x40..=0x4F`, set via the A0/A1 pins.
///
/// Three rules hold for every method below and are not repeated on each one:
///
/// - **Freshness.** Nothing here waits for a conversion. Output registers keep the last
///   completed result, so after a reset or any change of configuration, calibration, or
///   range, wait for a new conversion on each channel you intend to read.
/// - **Suspend and restore.** Methods that change scaling suspend conversions and put the
///   previous ADC configuration back, even when the work between them fails. Restoring a
///   running mode starts a fresh conversion and clears the conversion-ready flag; a mode
///   that was already shut down stays shut down, so call [`configure`](Self::configure)
///   before waiting for data.
/// - **Fail-stop.** An I2C write error cannot prove whether the device accepted the value.
///   After one, recover with [`reset`](Self::reset) or a new driver before any further
///   scaled operation. Failed reads are retryable.
#[derive(Debug)]
pub struct Ina228<I2C> {
    i2c: I2C,
    address: u8,
    calibration: Option<Calibration>,
    /// Live CONFIG, cached so the read-modify-write methods can skip the read. Assumes
    /// this driver is the only writer on the bus.
    config: Config,
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
    /// Creates a driver and reads CONFIG, so it starts from the device's live settings.
    ///
    /// # Errors
    ///
    /// Returns an [`InitializationError`] containing the I2C bus if `address` is outside
    /// `0x40..=0x4F` or CONFIG cannot be read.
    pub fn new(i2c: I2C, address: u8) -> Result<Self, InitializationError<I2C>> {
        if !(0x40..=0x4F).contains(&address) {
            return Err(InitializationError::InvalidAddress { i2c, address });
        }
        let mut driver = Self {
            i2c,
            address,
            calibration: None,
            config: Config::RESET_VALUE,
        };
        match driver.read_u16(Register::Config) {
            Ok(value) => {
                driver.config = Config::from_device(value);
                Ok(driver)
            }
            Err(error) => Err(InitializationError::I2c {
                i2c: driver.release(),
                error,
            }),
        }
    }

    /// Performs a soft reset, restoring all registers to defaults.
    ///
    /// Equivalent to power-up, with no delay of its own: wait at least 300 µs for
    /// oscillator and ADC stability before anything else.
    pub fn reset(&mut self) -> Result<(), Error<I2C::Error>> {
        self.write_u16(Register::Config, Config::RESET_COMMAND)?;
        self.calibration = None;
        self.config = Config::RESET_VALUE;
        Ok(())
    }

    /// Writes ADC_CONFIG: operating mode, per-channel conversion times, and averaging.
    pub fn configure(&mut self, config: AdcConfig) -> Result<(), Error<I2C::Error>> {
        Ok(self.write_u16(Register::AdcConfig, AdcConfigWord::of(config).bits())?)
    }

    /// Sets the shunt ADC full-scale range, re-writing SHUNT_CAL if already calibrated.
    ///
    /// Clears the shunt over- and under-voltage alert thresholds, whose register scale
    /// depends on the range; set them again afterward.
    pub fn set_adc_range(&mut self, range: AdcRange) -> Result<(), Error<I2C::Error>> {
        if self.adc_range() == range {
            return Ok(());
        }

        let calibration = self.calibration;
        let shunt_cal = calibration
            .map(|calibration| calibration.shunt_cal(range))
            .transpose()
            .map_err(Error::InvalidConfiguration)?;
        let config_value = self.config.with_adc_range(range);
        let adc_config = self.read_adc_config()?;
        self.with_conversions_suspended(adc_config, |driver| {
            driver.write_u16(Register::Sovl, i16::MAX as u16)?;
            driver.write_u16(Register::Suvl, i16::MIN as u16)?;
            driver.write_config(config_value)?;
            if let Some(shunt_cal) = shunt_cal {
                driver.write_u16(Register::ShuntCal, shunt_cal)?;
            }
            Ok(())
        })
    }

    /// Calibrates for current, power, energy, and charge measurement.
    ///
    /// `max_current_a` is the maximum expected current in Amps, `shunt_resistance_ohm` the
    /// shunt value in Ohms. Their product must be strictly below the selected ADC range's
    /// positive full-scale voltage.
    ///
    /// Resets ENERGY, CHARGE, and MATHOF so no sample accumulated at the old CURRENT_LSB
    /// is read back at the new one, and resets PWR_LIMIT to its least-restrictive value
    /// because its watt scale moves with CURRENT_LSB too — call
    /// [`set_power_limit`](Self::set_power_limit) again afterward.
    pub fn calibrate(
        &mut self,
        max_current_a: f32,
        shunt_resistance_ohm: f32,
    ) -> Result<(), Error<I2C::Error>> {
        let calibration = Calibration::new(max_current_a, shunt_resistance_ohm)
            .map_err(Error::InvalidConfiguration)?;
        let adc_range = self.adc_range();
        let shunt_cal = calibration
            .shunt_cal(adc_range)
            .map_err(Error::InvalidConfiguration)?;
        let adc_config = self.read_adc_config()?;
        self.with_conversions_suspended(adc_config, |driver| {
            driver.write_u16(Register::ShuntCal, shunt_cal)?;
            driver.reset_accumulators()?;
            driver.write_u16(Register::PwrLimit, u16::MAX)?;
            driver.calibration = Some(calibration);
            Ok(())
        })
    }

    /// Enables shunt temperature compensation with a coefficient from 0 to 16383 ppm/°C.
    ///
    /// SHUNT_TEMPCO is written before TEMPCOMP is enabled, so a partial failure cannot
    /// activate a stale coefficient.
    pub fn set_temp_compensation(&mut self, tempco_ppm: u16) -> Result<(), Error<I2C::Error>> {
        if tempco_ppm > scale::UNSIGNED_14_BIT_MAX {
            return Err(Error::InvalidConfiguration(
                ConfigurationError::Unrepresentable,
            ));
        }
        let config_value = self.config.with_temperature_compensation(true);
        let adc_config = self.read_adc_config()?;
        self.with_conversions_suspended(adc_config, |driver| {
            driver.write_u16(Register::ShuntTempco, tempco_ppm)?;
            driver.write_config(config_value)
        })
    }

    /// Disables shunt temperature compensation.
    pub fn disable_temp_compensation(&mut self) -> Result<(), Error<I2C::Error>> {
        let config_value = self.config.with_temperature_compensation(false);
        let adc_config = self.read_adc_config()?;
        self.with_conversions_suspended(adc_config, |driver| driver.write_config(config_value))
    }

    /// Returns bus voltage in Volts.
    pub fn bus_voltage(&mut self) -> Result<f32, Error<I2C::Error>> {
        let raw = self.read_u24(Register::Vbus)? >> 4;
        Ok(raw as f32 * scale::BUS_VOLTAGE_LSB)
    }

    /// Returns shunt voltage in Volts. The LSB depends on the configured ADC range.
    pub fn shunt_voltage(&mut self) -> Result<f32, Error<I2C::Error>> {
        let raw = self.read_i20(Register::Vshunt)?;
        Ok(raw as f32 * self.adc_range().shunt_voltage_lsb())
    }

    /// Returns current in Amps. Requires prior [`calibrate`](Self::calibrate) call.
    pub fn current(&mut self) -> Result<f32, Error<I2C::Error>> {
        let calibration = self
            .calibration
            .expect("call calibrate() before reading current");
        let raw = self.read_i20(Register::Current)?;
        Ok((raw as f64 * calibration.current_lsb()) as f32)
    }

    /// Returns power in Watts. Requires prior [`calibrate`](Self::calibrate) call.
    pub fn power(&mut self) -> Result<f32, Error<I2C::Error>> {
        let calibration = self
            .calibration
            .expect("call calibrate() before reading power");
        let raw = self.read_u24(Register::Power)?;
        Ok((raw as f64 * calibration.power_lsb()) as f32)
    }

    /// Takes a coherent energy, charge, and diagnostic snapshot.
    ///
    /// Valid only in continuous conversion modes. Conversions are suspended so DIAG_ALRT,
    /// ENERGY, and CHARGE cannot change between the three reads, leaving a brief gap where
    /// nothing accumulates.
    ///
    /// Reading leaves the accumulated values alone — only
    /// [`reset_accumulators`](Self::reset_accumulators) clears those — but it does consume
    /// flag state: DIAG_ALRT acknowledges conversion-ready and any latched alert, and
    /// ENERGY and CHARGE clear their overflow indicators. A capture that fails part-way
    /// loses whichever of those its completed reads already took; one that completes but
    /// cannot resume conversions comes back through [`CaptureError::NotResumed`] with the
    /// snapshot intact.
    pub fn take_accumulator_snapshot(
        &mut self,
    ) -> Result<AccumulatorSnapshot, CaptureError<I2C::Error>> {
        let calibration = self
            .calibration
            .expect("call calibrate() before reading accumulators");
        let adc_config = self.read_adc_config().map_err(Error::I2c)?;
        if !adc_config.accumulates() {
            return Err(Error::InvalidConfiguration(ConfigurationError::AccumulatorMode).into());
        }
        // The snapshot has to outlive a failed restore: its reads already consumed the
        // device's flag state, so dropping it would lose those readings for good.
        let mut captured = None;
        let outcome = self.with_conversions_suspended(adc_config, |driver| {
            let diagnostic_flags = driver.take_diagnostic_flags()?;
            let energy_raw = driver.read_u40(Register::Energy)?;
            let charge_raw = driver.read_i40(Register::Charge)?;
            let snapshot = AccumulatorSnapshot {
                energy_joules: energy_raw as f64 * calibration.energy_lsb(),
                charge_coulombs: charge_raw as f64 * calibration.current_lsb(),
                diagnostic_flags,
            };
            captured = Some(snapshot);
            Ok(snapshot)
        });
        // `captured` is set only once the reads have all succeeded, so a snapshot beside
        // an error can only mean the capture completed and the restore did not.
        match (outcome, captured) {
            (Ok(snapshot), _) => Ok(snapshot),
            (Err(error), Some(snapshot)) => Err(CaptureError::NotResumed { snapshot, error }),
            (Err(error), None) => Err(CaptureError::Failed(error)),
        }
    }

    /// Returns die temperature in degrees Celsius.
    pub fn die_temperature(&mut self) -> Result<f32, Error<I2C::Error>> {
        let raw = self.read_u16(Register::DieTemp)? as i16;
        Ok(raw as f32 * scale::DIE_TEMPERATURE_LSB)
    }

    /// Resets the energy and charge accumulator registers and clears MATHOF.
    pub fn reset_accumulators(&mut self) -> Result<(), Error<I2C::Error>> {
        Ok(self.write_u16(Register::Config, self.config.accumulator_reset_command())?)
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
        Ok(self.write_u16(Register::DiagAlrt, value)?)
    }

    /// Set shunt over-voltage limit in Volts.
    pub fn set_shunt_overvoltage_limit(&mut self, voltage_v: f32) -> Result<(), Error<I2C::Error>> {
        let lsb = self.adc_range().shunt_limit_lsb() as f64;
        self.write_signed_limit(Register::Sovl, voltage_v, lsb)
    }

    /// Set shunt under-voltage limit in Volts.
    pub fn set_shunt_undervoltage_limit(
        &mut self,
        voltage_v: f32,
    ) -> Result<(), Error<I2C::Error>> {
        let lsb = self.adc_range().shunt_limit_lsb() as f64;
        self.write_signed_limit(Register::Suvl, voltage_v, lsb)
    }

    /// Set bus over-voltage limit in Volts.
    pub fn set_bus_overvoltage_limit(&mut self, voltage_v: f32) -> Result<(), Error<I2C::Error>> {
        self.write_unsigned_limit(
            Register::Bovl,
            voltage_v,
            scale::BUS_LIMIT_LSB as f64,
            scale::UNSIGNED_15_BIT_MAX,
        )
    }

    /// Set bus under-voltage limit in Volts.
    pub fn set_bus_undervoltage_limit(&mut self, voltage_v: f32) -> Result<(), Error<I2C::Error>> {
        self.write_unsigned_limit(
            Register::Buvl,
            voltage_v,
            scale::BUS_LIMIT_LSB as f64,
            scale::UNSIGNED_15_BIT_MAX,
        )
    }

    /// Set temperature over-limit in degrees Celsius.
    pub fn set_temperature_limit(&mut self, temp_c: f32) -> Result<(), Error<I2C::Error>> {
        self.write_signed_limit(
            Register::TempLimit,
            temp_c,
            scale::DIE_TEMPERATURE_LSB as f64,
        )
    }

    /// Set power over-limit in Watts.
    pub fn set_power_limit(&mut self, power_w: f32) -> Result<(), Error<I2C::Error>> {
        let calibration = self
            .calibration
            .expect("call calibrate() before setting power limit");
        self.write_unsigned_limit(
            Register::PwrLimit,
            power_w,
            scale::POWER_LIMIT_TRUNCATION * calibration.power_lsb(),
            u16::MAX,
        )
    }

    /// Reads the manufacturer ID register (expected: `0x5449` for TI).
    pub fn manufacturer_id(&mut self) -> Result<u16, Error<I2C::Error>> {
        Ok(self.read_u16(Register::ManufacturerId)?)
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

    fn write_signed_limit(
        &mut self,
        reg: Register,
        value: f32,
        lsb: f64,
    ) -> Result<(), Error<I2C::Error>> {
        let raw = encode_signed(value, lsb).map_err(Error::InvalidConfiguration)?;
        Ok(self.write_u16(reg, raw)?)
    }

    fn write_unsigned_limit(
        &mut self,
        reg: Register,
        value: f32,
        lsb: f64,
        max_raw: u16,
    ) -> Result<(), Error<I2C::Error>> {
        let raw = encode_unsigned(value, lsb, max_raw).map_err(Error::InvalidConfiguration)?;
        Ok(self.write_u16(reg, raw)?)
    }

    /// Shunt ADC range currently programmed into CONFIG.
    fn adc_range(&self) -> AdcRange {
        self.config.adc_range()
    }

    fn read_adc_config(&mut self) -> Result<AdcConfigWord, I2C::Error> {
        Ok(AdcConfigWord::from_device(
            self.read_u16(Register::AdcConfig)?,
        ))
    }

    /// Writes CONFIG and records it as the new cached value.
    ///
    /// Taking a [`Config`] is what keeps the cache honest: the self-clearing commands
    /// used by [`reset`](Self::reset) and
    /// [`reset_accumulators`](Self::reset_accumulators) are bare words, so they cannot
    /// reach this path.
    fn write_config(&mut self, value: Config) -> Result<(), Error<I2C::Error>> {
        self.write_u16(Register::Config, value.bits())?;
        self.config = value;
        Ok(())
    }

    /// Runs `body` with conversions suspended, then puts `adc_config` back.
    ///
    /// The restore is attempted whether or not `body` succeeds, so no failure inside the
    /// suspended window can leave the ADC shut down. A `body` error wins over a restore
    /// error: it happened first and says what actually went wrong. Both shutdown
    /// encodings are already stopped, so they run `body` with no writes at all.
    fn with_conversions_suspended<T>(
        &mut self,
        adc_config: AdcConfigWord,
        body: impl FnOnce(&mut Self) -> Result<T, Error<I2C::Error>>,
    ) -> Result<T, Error<I2C::Error>> {
        if adc_config.is_shutdown() {
            return body(self);
        }
        self.write_u16(Register::AdcConfig, adc_config.shut_down().bits())?;
        let outcome = body(self);
        let restored = self
            .write_u16(Register::AdcConfig, adc_config.bits())
            .map_err(Error::I2c);
        outcome.and_then(|value| restored.map(|()| value))
    }

    /// Reads `N` bytes from `reg`. Like every bus helper here it hands back the raw
    /// `I2C::Error`, which `?` widens to [`Error`] at the public boundary.
    fn read_bytes<const N: usize>(&mut self, reg: Register) -> Result<[u8; N], I2C::Error> {
        let mut bytes = [0u8; N];
        self.i2c
            .write_read(self.address, &[reg as u8], &mut bytes)?;
        Ok(bytes)
    }

    fn write_u16(&mut self, reg: Register, value: u16) -> Result<(), I2C::Error> {
        let [high, low] = value.to_be_bytes();
        self.i2c.write(self.address, &[reg as u8, high, low])
    }

    fn read_u16(&mut self, reg: Register) -> Result<u16, I2C::Error> {
        Ok(u16::from_be_bytes(self.read_bytes(reg)?))
    }

    fn read_u24(&mut self, reg: Register) -> Result<u32, I2C::Error> {
        let [a, b, c] = self.read_bytes(reg)?;
        Ok(u32::from_be_bytes([0, a, b, c]))
    }

    fn read_u40(&mut self, reg: Register) -> Result<u64, I2C::Error> {
        let [a, b, c, d, e] = self.read_bytes(reg)?;
        Ok(u64::from_be_bytes([0, 0, 0, a, b, c, d, e]))
    }

    /// VSHUNT and CURRENT are 20-bit signed values in the upper bits of a 24-bit register.
    fn read_i20(&mut self, reg: Register) -> Result<i32, I2C::Error> {
        Ok(sign_extend(u64::from(self.read_u24(reg)? >> 4), 20) as i32)
    }

    fn read_i40(&mut self, reg: Register) -> Result<i64, I2C::Error> {
        Ok(sign_extend(self.read_u40(reg)?, 40))
    }
}
