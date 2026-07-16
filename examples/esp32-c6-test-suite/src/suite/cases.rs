use std::fmt::Debug;
use std::thread;
use std::time::{Duration, Instant};

use embedded_hal::{digital::InputPin, i2c::I2c};
use ina228::{
    AdcConfig, AdcRange, AlertConfig, AveragingCount, ConfigurationError, ConversionTime,
    DEVICE_ID, DiagnosticFlags, Error, Ina228, MANUFACTURER_ID, OperatingMode,
};

use crate::suite::{ResultContext, TestResult, require};

const MAX_CURRENT_A: f32 = 10.0;
const SHUNT_RESISTANCE_OHM: f32 = 0.002;
const SHUNT_TEMPCO_PPM: u16 = 50;
const MIN_FIXTURE_BUS_VOLTAGE_V: f32 = 0.1;
const MIN_FIXTURE_POWER_W: f32 = 0.05;
const MAX_BUS_VOLTAGE_V: f32 = 85.0;
const MIN_DIE_TEMPERATURE_C: f32 = -40.0;
const MAX_DIE_TEMPERATURE_C: f32 = 125.0;
const RANGE_40_MAX_VOLTAGE_V: f32 = 0.04096;
const RANGE_163_MAX_VOLTAGE_V: f32 = 0.16384;
const ALERT_SHUNT_LIMIT_V: f32 = 0.03;
const SAFE_POWER_LIMIT_W: f32 = MAX_CURRENT_A * 100.0;
const CONVERSION_TIMEOUT: Duration = Duration::from_secs(3);
const RESET_STABILIZATION: Duration = Duration::from_millis(1);
const ALERT_LATCH_OBSERVATION_DELAY: Duration = Duration::from_millis(100);
const MODE_CONVERSION_TIME_US: u64 = 4_120;
const MODE_AVERAGING_SAMPLES: u64 = 4;
const CONVERSION_TIME_AVERAGING_SAMPLES: u64 = 256;
const AVERAGING_CONVERSION_TIME_US: u64 = 2_074;
const TIMING_UPPER_SLACK_US: u64 = 3_000;
const TIMING_TIMEOUT_SLACK_US: u64 = 2_000;

#[derive(Debug, Clone, Copy)]
pub(crate) struct ModeCase {
    pub(crate) name: &'static str,
    mode: OperatingMode,
    active_channels: u64,
    continuous: bool,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct ConversionTimeCase {
    pub(crate) name: &'static str,
    conversion_time: ConversionTime,
    conversion_time_us: u64,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct AveragingCase {
    pub(crate) name: &'static str,
    averaging: AveragingCount,
    samples: u64,
}

#[derive(Debug, Clone, Copy)]
struct Measurements {
    bus_voltage_v: f32,
    shunt_voltage_v: f32,
    current_a: f32,
    power_w: f32,
    die_temperature_c: f32,
}

#[derive(Debug, Clone, Copy)]
struct ConversionObservation {
    flags: DiagnosticFlags,
    elapsed: Duration,
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum InvalidConfigurationCase {
    ZeroMaximumCurrent,
    ZeroShuntResistance,
    CalibrationBeyondRange,
    TemperatureCoefficient,
    InfiniteShuntLimit,
    NegativeBusLimit,
    NanTemperatureLimit,
    NegativePowerLimit,
    SnapshotInTriggeredMode,
}

impl InvalidConfigurationCase {
    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::ZeroMaximumCurrent => "zero maximum current",
            Self::ZeroShuntResistance => "zero shunt resistance",
            Self::CalibrationBeyondRange => "calibration beyond ADC range",
            Self::TemperatureCoefficient => "15-bit temperature coefficient",
            Self::InfiniteShuntLimit => "infinite shunt limit",
            Self::NegativeBusLimit => "negative bus limit",
            Self::NanTemperatureLimit => "NaN temperature limit",
            Self::NegativePowerLimit => "negative power limit",
            Self::SnapshotInTriggeredMode => "accumulator snapshot in triggered mode",
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum AlertThresholdCase {
    ShuntOvervoltage,
    ShuntUndervoltage,
    BusOvervoltage,
    BusUndervoltage,
    Temperature,
    Power,
}

impl AlertThresholdCase {
    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::ShuntOvervoltage => "shunt overvoltage",
            Self::ShuntUndervoltage => "shunt undervoltage",
            Self::BusOvervoltage => "bus overvoltage",
            Self::BusUndervoltage => "bus undervoltage",
            Self::Temperature => "temperature over-limit",
            Self::Power => "power over-limit",
        }
    }
}

pub(crate) const INVALID_CONFIGURATION_CASES: [InvalidConfigurationCase; 9] = [
    InvalidConfigurationCase::ZeroMaximumCurrent,
    InvalidConfigurationCase::ZeroShuntResistance,
    InvalidConfigurationCase::CalibrationBeyondRange,
    InvalidConfigurationCase::TemperatureCoefficient,
    InvalidConfigurationCase::InfiniteShuntLimit,
    InvalidConfigurationCase::NegativeBusLimit,
    InvalidConfigurationCase::NanTemperatureLimit,
    InvalidConfigurationCase::NegativePowerLimit,
    InvalidConfigurationCase::SnapshotInTriggeredMode,
];

pub(crate) const ALERT_THRESHOLD_CASES: [AlertThresholdCase; 6] = [
    AlertThresholdCase::ShuntOvervoltage,
    AlertThresholdCase::ShuntUndervoltage,
    AlertThresholdCase::BusOvervoltage,
    AlertThresholdCase::BusUndervoltage,
    AlertThresholdCase::Temperature,
    AlertThresholdCase::Power,
];

pub(crate) const ACTIVE_MODES: [ModeCase; 14] = [
    ModeCase {
        name: "TriggeredBus",
        mode: OperatingMode::TriggeredBus,
        active_channels: 1,
        continuous: false,
    },
    ModeCase {
        name: "TriggeredShunt",
        mode: OperatingMode::TriggeredShunt,
        active_channels: 1,
        continuous: false,
    },
    ModeCase {
        name: "TriggeredBusShunt",
        mode: OperatingMode::TriggeredBusShunt,
        active_channels: 2,
        continuous: false,
    },
    ModeCase {
        name: "TriggeredTemp",
        mode: OperatingMode::TriggeredTemp,
        active_channels: 1,
        continuous: false,
    },
    ModeCase {
        name: "TriggeredTempBus",
        mode: OperatingMode::TriggeredTempBus,
        active_channels: 2,
        continuous: false,
    },
    ModeCase {
        name: "TriggeredTempShunt",
        mode: OperatingMode::TriggeredTempShunt,
        active_channels: 2,
        continuous: false,
    },
    ModeCase {
        name: "TriggeredAll",
        mode: OperatingMode::TriggeredAll,
        active_channels: 3,
        continuous: false,
    },
    ModeCase {
        name: "ContinuousBus",
        mode: OperatingMode::ContinuousBus,
        active_channels: 1,
        continuous: true,
    },
    ModeCase {
        name: "ContinuousShunt",
        mode: OperatingMode::ContinuousShunt,
        active_channels: 1,
        continuous: true,
    },
    ModeCase {
        name: "ContinuousBusShunt",
        mode: OperatingMode::ContinuousBusShunt,
        active_channels: 2,
        continuous: true,
    },
    ModeCase {
        name: "ContinuousTemp",
        mode: OperatingMode::ContinuousTemp,
        active_channels: 1,
        continuous: true,
    },
    ModeCase {
        name: "ContinuousTempBus",
        mode: OperatingMode::ContinuousTempBus,
        active_channels: 2,
        continuous: true,
    },
    ModeCase {
        name: "ContinuousTempShunt",
        mode: OperatingMode::ContinuousTempShunt,
        active_channels: 2,
        continuous: true,
    },
    ModeCase {
        name: "ContinuousAll",
        mode: OperatingMode::ContinuousAll,
        active_channels: 3,
        continuous: true,
    },
];

pub(crate) const CONVERSION_TIMES: [ConversionTimeCase; 8] = [
    ConversionTimeCase {
        name: "50 us",
        conversion_time: ConversionTime::Us50,
        conversion_time_us: 50,
    },
    ConversionTimeCase {
        name: "84 us",
        conversion_time: ConversionTime::Us84,
        conversion_time_us: 84,
    },
    ConversionTimeCase {
        name: "150 us",
        conversion_time: ConversionTime::Us150,
        conversion_time_us: 150,
    },
    ConversionTimeCase {
        name: "280 us",
        conversion_time: ConversionTime::Us280,
        conversion_time_us: 280,
    },
    ConversionTimeCase {
        name: "540 us",
        conversion_time: ConversionTime::Us540,
        conversion_time_us: 540,
    },
    ConversionTimeCase {
        name: "1052 us",
        conversion_time: ConversionTime::Us1052,
        conversion_time_us: 1_052,
    },
    ConversionTimeCase {
        name: "2074 us",
        conversion_time: ConversionTime::Us2074,
        conversion_time_us: 2_074,
    },
    ConversionTimeCase {
        name: "4120 us",
        conversion_time: ConversionTime::Us4120,
        conversion_time_us: 4_120,
    },
];

pub(crate) const AVERAGING_COUNTS: [AveragingCase; 8] = [
    AveragingCase {
        name: "1 sample",
        averaging: AveragingCount::N1,
        samples: 1,
    },
    AveragingCase {
        name: "4 samples",
        averaging: AveragingCount::N4,
        samples: 4,
    },
    AveragingCase {
        name: "16 samples",
        averaging: AveragingCount::N16,
        samples: 16,
    },
    AveragingCase {
        name: "64 samples",
        averaging: AveragingCount::N64,
        samples: 64,
    },
    AveragingCase {
        name: "128 samples",
        averaging: AveragingCount::N128,
        samples: 128,
    },
    AveragingCase {
        name: "256 samples",
        averaging: AveragingCount::N256,
        samples: 256,
    },
    AveragingCase {
        name: "512 samples",
        averaging: AveragingCount::N512,
        samples: 512,
    },
    AveragingCase {
        name: "1024 samples",
        averaging: AveragingCount::N1024,
        samples: 1_024,
    },
];

pub(crate) fn identity<I2C>(ina: &mut Ina228<I2C>) -> TestResult
where
    I2C: I2c,
    I2C::Error: Debug,
{
    let manufacturer = ina.manufacturer_id().context("read manufacturer ID")?;
    let device = ina.device_id().context("read device ID")?;
    let revision = ina.die_revision().context("read die revision")?;

    require(
        manufacturer == MANUFACTURER_ID,
        format!("manufacturer ID 0x{manufacturer:04X}, expected 0x{MANUFACTURER_ID:04X}"),
    )?;
    require(
        device == DEVICE_ID,
        format!("device ID 0x{device:03X}, expected 0x{DEVICE_ID:03X}"),
    )?;
    require(
        revision <= 0x0F,
        format!("die revision {revision} is not 4-bit"),
    )?;
    println!(
        "INA228 identity: manufacturer=0x{manufacturer:04X}, device=0x{device:03X}, revision={revision}"
    );
    Ok(())
}

pub(crate) fn reset<I2C>(ina: &mut Ina228<I2C>) -> TestResult
where
    I2C: I2c,
    I2C::Error: Debug,
{
    reset_device(ina)?;
    match ina.shunt_voltage() {
        Err(Error::ShuntVoltageStale) => {}
        Err(error) => return Err(format!("unexpected post-reset shunt error: {error:?}")),
        Ok(value) => return Err(format!("post-reset shunt voltage was not stale: {value} V")),
    }

    let flags = wait_for_conversion(ina)?;
    validate_clean_diagnostics(flags)?;
    let measurements = read_uncalibrated_measurements(ina)?;
    validate_common_measurements(measurements, RANGE_163_MAX_VOLTAGE_V)
}

pub(crate) fn adc_shutdown<I2C>(ina: &mut Ina228<I2C>) -> TestResult
where
    I2C: I2c,
    I2C::Error: Debug,
{
    reset_device(ina)?;
    ina.configure(fast_config(OperatingMode::Shutdown, AveragingCount::N1))
        .context("enter shutdown mode")?;
    ina.take_diagnostic_flags()
        .context("acknowledge conversion-ready before shutdown check")?;
    thread::sleep(Duration::from_millis(20));
    let shutdown_flags = ina
        .take_diagnostic_flags()
        .context("read diagnostics in shutdown mode")?;
    require(
        !shutdown_flags.conversion_ready,
        "shutdown mode produced a conversion",
    )
}

pub(crate) fn adc_mode<I2C>(ina: &mut Ina228<I2C>, case: ModeCase) -> TestResult
where
    I2C: I2c,
    I2C::Error: Debug,
{
    reset_device(ina)?;
    ina.configure(AdcConfig {
        mode: case.mode,
        bus_conversion_time: ConversionTime::Us4120,
        shunt_conversion_time: ConversionTime::Us4120,
        temperature_conversion_time: ConversionTime::Us4120,
        averaging: AveragingCount::N4,
    })
    .context(case.name)?;

    let expected = Duration::from_micros(
        MODE_CONVERSION_TIME_US * MODE_AVERAGING_SAMPLES * case.active_channels,
    );
    let first = wait_for_conversion_timed(ina, timing_wait_timeout(expected))?;
    validate_conversion_duration(case.name, first.elapsed, expected)?;

    if case.continuous {
        let second = wait_for_conversion_timed(ina, timing_wait_timeout(expected))?;
        let maximum = timing_upper_bound(expected);
        require(
            second.elapsed <= maximum,
            format!(
                "{}: continuous conversion recurred in {} us, expected no later than {} us",
                case.name,
                second.elapsed.as_micros(),
                maximum.as_micros()
            ),
        )?;
    } else {
        require_no_conversion(ina, timing_upper_bound(expected), case.name)?;
    }

    Ok(())
}

pub(crate) fn adc_conversion_time<I2C>(
    ina: &mut Ina228<I2C>,
    case: ConversionTimeCase,
) -> TestResult
where
    I2C: I2c,
    I2C::Error: Debug,
{
    reset_device(ina)?;
    ina.configure(AdcConfig {
        mode: OperatingMode::TriggeredBus,
        bus_conversion_time: case.conversion_time,
        shunt_conversion_time: ConversionTime::Us50,
        temperature_conversion_time: ConversionTime::Us50,
        averaging: AveragingCount::N256,
    })
    .context(case.name)?;

    let expected =
        Duration::from_micros(case.conversion_time_us * CONVERSION_TIME_AVERAGING_SAMPLES);
    let observation = wait_for_conversion_timed(ina, timing_wait_timeout(expected))?;
    validate_conversion_duration(case.name, observation.elapsed, expected)
}

pub(crate) fn adc_averaging<I2C>(ina: &mut Ina228<I2C>, case: AveragingCase) -> TestResult
where
    I2C: I2c,
    I2C::Error: Debug,
{
    reset_device(ina)?;
    ina.configure(AdcConfig {
        mode: OperatingMode::TriggeredBus,
        bus_conversion_time: ConversionTime::Us2074,
        shunt_conversion_time: ConversionTime::Us50,
        temperature_conversion_time: ConversionTime::Us50,
        averaging: case.averaging,
    })
    .context(case.name)?;

    let expected = Duration::from_micros(AVERAGING_CONVERSION_TIME_US * case.samples);
    let observation = wait_for_conversion_timed(ina, timing_wait_timeout(expected))?;
    validate_conversion_duration(case.name, observation.elapsed, expected)
}

pub(crate) fn invalid_configuration<I2C>(
    ina: &mut Ina228<I2C>,
    case: InvalidConfigurationCase,
) -> TestResult
where
    I2C: I2c,
    I2C::Error: Debug,
{
    reset_device(ina)?;
    match case {
        InvalidConfigurationCase::ZeroMaximumCurrent => expect_configuration_error(
            ina.calibrate(0.0, SHUNT_RESISTANCE_OHM),
            ConfigurationError::MaxCurrent,
            case.name(),
        ),
        InvalidConfigurationCase::ZeroShuntResistance => expect_configuration_error(
            ina.calibrate(MAX_CURRENT_A, 0.0),
            ConfigurationError::ShuntResistance,
            case.name(),
        ),
        InvalidConfigurationCase::CalibrationBeyondRange => {
            ina.set_adc_range(AdcRange::Range40mV)
                .context("select 40 mV range")?;
            expect_configuration_error(
                ina.calibrate(100.0, SHUNT_RESISTANCE_OHM),
                ConfigurationError::Calibration,
                case.name(),
            )
        }
        InvalidConfigurationCase::TemperatureCoefficient => expect_configuration_error(
            ina.set_temp_compensation(0x4000),
            ConfigurationError::TemperatureCoefficient,
            case.name(),
        ),
        InvalidConfigurationCase::InfiniteShuntLimit => expect_configuration_error(
            ina.set_shunt_overvoltage_limit(f32::INFINITY),
            ConfigurationError::ShuntVoltageLimit,
            case.name(),
        ),
        InvalidConfigurationCase::NegativeBusLimit => expect_configuration_error(
            ina.set_bus_overvoltage_limit(-1.0),
            ConfigurationError::BusVoltageLimit,
            case.name(),
        ),
        InvalidConfigurationCase::NanTemperatureLimit => expect_configuration_error(
            ina.set_temperature_limit(f32::NAN),
            ConfigurationError::TemperatureLimit,
            case.name(),
        ),
        InvalidConfigurationCase::NegativePowerLimit => {
            ina.set_adc_range(AdcRange::Range40mV)
                .context("select 40 mV range")?;
            ina.calibrate(MAX_CURRENT_A, SHUNT_RESISTANCE_OHM)
                .context("establish valid calibration")?;
            expect_configuration_error(
                ina.set_power_limit(-1.0),
                ConfigurationError::PowerLimit,
                case.name(),
            )
        }
        InvalidConfigurationCase::SnapshotInTriggeredMode => {
            ina.set_adc_range(AdcRange::Range40mV)
                .context("select 40 mV range")?;
            ina.calibrate(MAX_CURRENT_A, SHUNT_RESISTANCE_OHM)
                .context("establish valid calibration")?;
            ina.configure(fast_config(OperatingMode::TriggeredAll, AveragingCount::N1))
                .context("enter triggered mode")?;
            expect_configuration_error(
                ina.take_accumulator_snapshot(),
                ConfigurationError::AccumulatorMode,
                case.name(),
            )
        }
    }
}

pub(crate) fn ranges_and_calibration<I2C>(ina: &mut Ina228<I2C>) -> TestResult
where
    I2C: I2c,
    I2C::Error: Debug,
{
    prepare_measurements(ina, AdcRange::Range163mV)?;
    let current_163 = ina.current().context("read current in 163 mV range")?;
    require(current_163.is_finite(), "163 mV current is not finite")?;

    ina.set_adc_range(AdcRange::Range40mV)
        .context("change to 40 mV range")?;
    expect_stale_shunt_voltage(ina, "40 mV range change")?;
    wait_for_conversion(ina)?;
    let shunt_40 = ina.shunt_voltage().context("read 40 mV shunt voltage")?;
    let current_40 = ina
        .current()
        .context("read current after 40 mV range change")?;
    require(
        shunt_40.abs() <= RANGE_40_MAX_VOLTAGE_V,
        format!("40 mV range returned {shunt_40} V"),
    )?;
    require(current_40.is_finite(), "40 mV current is not finite")?;

    ina.set_adc_range(AdcRange::Range163mV)
        .context("change back to 163 mV range")?;
    expect_stale_shunt_voltage(ina, "163 mV range change")?;
    wait_for_conversion(ina)?;
    let shunt_163 = ina
        .shunt_voltage()
        .context("read restored 163 mV shunt voltage")?;
    require(
        shunt_163.abs() <= RANGE_163_MAX_VOLTAGE_V,
        format!("163 mV range returned {shunt_163} V"),
    )?;

    ina.set_adc_range(AdcRange::Range40mV)
        .context("restore 40 mV range")?;
    wait_for_conversion(ina)?;
    Ok(())
}

pub(crate) fn measurements<I2C>(ina: &mut Ina228<I2C>) -> TestResult
where
    I2C: I2c,
    I2C::Error: Debug,
{
    prepare_measurements(ina, AdcRange::Range40mV)?;

    ina.set_temp_compensation(SHUNT_TEMPCO_PPM)
        .context("enable shunt temperature compensation")?;
    wait_for_conversion(ina)?;
    require(
        ina.current()
            .context("read temperature-compensated current")?
            .is_finite(),
        "temperature-compensated current is not finite",
    )?;
    ina.disable_temp_compensation()
        .context("disable shunt temperature compensation")?;
    wait_for_conversion(ina)?;

    let measurements = read_measurements(ina)?;
    validate_common_measurements(measurements, RANGE_40_MAX_VOLTAGE_V)?;
    require(
        measurements.current_a.abs() <= MAX_CURRENT_A * 1.01,
        format!(
            "current {} A exceeds the configured {} A range",
            measurements.current_a, MAX_CURRENT_A
        ),
    )?;
    require(
        measurements.power_w >= MIN_FIXTURE_POWER_W,
        format!(
            "power {} W is below the {MIN_FIXTURE_POWER_W} W fixture minimum; connect a positive-direction load",
            measurements.power_w
        ),
    )?;

    let expected_current = measurements.shunt_voltage_v / SHUNT_RESISTANCE_OHM;
    let current_tolerance = (expected_current.abs() * 0.02).max(0.002);
    require(
        (measurements.current_a - expected_current).abs() <= current_tolerance,
        format!(
            "current/shunt mismatch: measured={} A, Vshunt/R={} A, tolerance={} A",
            measurements.current_a, expected_current, current_tolerance
        ),
    )?;

    let expected_power = measurements.bus_voltage_v * measurements.current_a;
    let power_tolerance = (expected_power.abs() * 0.05).max(0.05);
    require(
        (measurements.power_w - expected_power).abs() <= power_tolerance,
        format!(
            "power mismatch: measured={} W, Vbus*current={} W, tolerance={} W",
            measurements.power_w, expected_power, power_tolerance
        ),
    )?;

    println!(
        "measurements: bus={:.6} V, shunt={:.9} V, current={:.6} A, power={:.6} W, temperature={:.3} C",
        measurements.bus_voltage_v,
        measurements.shunt_voltage_v,
        measurements.current_a,
        measurements.power_w,
        measurements.die_temperature_c
    );
    Ok(())
}

pub(crate) fn accumulators<I2C>(ina: &mut Ina228<I2C>) -> TestResult
where
    I2C: I2c,
    I2C::Error: Debug,
{
    prepare_measurements(ina, AdcRange::Range40mV)?;
    ina.reset_accumulators()
        .context("reset accumulators before growth check")?;
    thread::sleep(Duration::from_millis(500));

    let first = ina
        .take_accumulator_snapshot()
        .context("take first accumulator snapshot")?;
    validate_snapshot(
        first.energy_joules,
        first.charge_coulombs,
        first.diagnostic_flags,
    )?;
    let current_a = ina
        .current()
        .context("read current for charge cross-check")?;
    let power_w = ina.power().context("read power for energy cross-check")?;
    let started = Instant::now();
    thread::sleep(Duration::from_millis(300));
    let second = ina
        .take_accumulator_snapshot()
        .context("take second accumulator snapshot")?;
    let elapsed_s = started.elapsed().as_secs_f64();
    validate_snapshot(
        second.energy_joules,
        second.charge_coulombs,
        second.diagnostic_flags,
    )?;

    let energy_delta = second.energy_joules - first.energy_joules;
    let expected_energy_delta = power_w as f64 * elapsed_s;
    let energy_lsb = 51.2 * (MAX_CURRENT_A / 524_288.0) as f64;
    let energy_tolerance = expected_energy_delta.abs() * 0.35 + 4.0 * energy_lsb;
    require(
        energy_delta >= 0.0,
        format!("energy decreased by {} J", -energy_delta),
    )?;
    require(
        (energy_delta - expected_energy_delta).abs() <= energy_tolerance,
        format!(
            "energy delta={} J, expected power*time={}*{}={} J, tolerance={} J",
            energy_delta, power_w, elapsed_s, expected_energy_delta, energy_tolerance
        ),
    )?;

    let charge_delta = second.charge_coulombs - first.charge_coulombs;
    let expected_charge_delta = current_a as f64 * elapsed_s;
    let charge_lsb = (MAX_CURRENT_A / 524_288.0) as f64;
    let charge_tolerance = expected_charge_delta.abs() * 0.35 + 4.0 * charge_lsb;
    require(
        charge_delta >= 0.0,
        format!(
            "charge decreased by {} C with a positive-direction fixture",
            -charge_delta
        ),
    )?;
    require(
        (charge_delta - expected_charge_delta).abs() <= charge_tolerance,
        format!(
            "charge delta={} C, expected current*time={}*{}={} C, tolerance={} C",
            charge_delta, current_a, elapsed_s, expected_charge_delta, charge_tolerance
        ),
    )?;

    ina.reset_accumulators()
        .context("reset accumulated values")?;
    let reset = ina
        .take_accumulator_snapshot()
        .context("read reset accumulated values")?;
    require(
        reset.energy_joules < second.energy_joules,
        format!(
            "energy reset did not reduce the value: before={} J, after={} J",
            second.energy_joules, reset.energy_joules
        ),
    )?;
    require(
        reset.charge_coulombs < second.charge_coulombs,
        format!(
            "charge reset did not reduce the value: before={} C, after={} C",
            second.charge_coulombs, reset.charge_coulombs
        ),
    )?;

    thread::sleep(Duration::from_millis(100));
    let resumed = ina
        .take_accumulator_snapshot()
        .context("read accumulators after resumed conversion")?;
    require(
        resumed.energy_joules > reset.energy_joules,
        "energy did not resume accumulating after snapshot",
    )?;
    require(
        resumed.charge_coulombs > reset.charge_coulombs,
        "charge did not resume accumulating after snapshot",
    )?;
    Ok(())
}

pub(crate) fn alert_active_low<I2C, ALERT>(ina: &mut Ina228<I2C>, alert: &mut ALERT) -> TestResult
where
    I2C: I2c,
    I2C::Error: Debug,
    ALERT: InputPin,
    ALERT::Error: Debug,
{
    alert_transparent_polarity(ina, alert, false)
}

pub(crate) fn alert_active_high<I2C, ALERT>(ina: &mut Ina228<I2C>, alert: &mut ALERT) -> TestResult
where
    I2C: I2c,
    I2C::Error: Debug,
    ALERT: InputPin,
    ALERT::Error: Debug,
{
    alert_transparent_polarity(ina, alert, true)
}

pub(crate) fn alert_latch<I2C, ALERT>(ina: &mut Ina228<I2C>, alert: &mut ALERT) -> TestResult
where
    I2C: I2c,
    I2C::Error: Debug,
    ALERT: InputPin,
    ALERT::Error: Debug,
{
    prepare_alert_fixture(ina)?;
    ina.configure_alerts(AlertConfig {
        latch: true,
        ..AlertConfig::default()
    })
    .context("enable latched ALERT mode")?;
    wait_for_alert_level(alert, true, "latched ALERT idle state")?;

    ina.set_bus_overvoltage_limit(0.0)
        .context("trigger latched bus overvoltage alert")?;
    wait_for_alert_level(alert, false, "latched ALERT assertion")?;
    ina.set_bus_overvoltage_limit(MAX_BUS_VOLTAGE_V)
        .context("clear latched bus overvoltage condition")?;
    thread::sleep(ALERT_LATCH_OBSERVATION_DELAY);
    require_alert_level(alert, false, "latched ALERT persistence")?;

    let flags = ina
        .take_diagnostic_flags()
        .context("acknowledge latched ALERT")?;
    require(
        flags.bus_over_limit,
        "latched bus overvoltage flag did not persist until acknowledgement",
    )?;
    wait_for_alert_level(alert, true, "latched ALERT acknowledgement")?;
    ina.configure_alerts(AlertConfig::default())
        .context("restore transparent ALERT mode")
}

pub(crate) fn alert_conversion_ready<I2C, ALERT>(
    ina: &mut Ina228<I2C>,
    alert: &mut ALERT,
) -> TestResult
where
    I2C: I2c,
    I2C::Error: Debug,
    ALERT: InputPin,
    ALERT::Error: Debug,
{
    prepare_alert_fixture(ina)?;
    ina.configure(fast_config(OperatingMode::Shutdown, AveragingCount::N1))
        .context("enter shutdown before conversion-ready ALERT test")?;
    ina.configure_alerts(AlertConfig {
        conversion_ready: true,
        ..AlertConfig::default()
    })
    .context("enable conversion-ready ALERT")?;
    wait_for_alert_level(alert, true, "conversion-ready ALERT idle state")?;

    ina.configure(fast_config(OperatingMode::TriggeredAll, AveragingCount::N1))
        .context("start triggered conversion for ALERT")?;
    wait_for_alert_level(alert, false, "conversion-ready ALERT assertion")?;
    let flags = ina
        .take_diagnostic_flags()
        .context("acknowledge conversion-ready ALERT")?;
    require(
        flags.conversion_ready,
        "ALERT asserted without the conversion-ready flag",
    )?;
    wait_for_alert_level(alert, true, "conversion-ready ALERT acknowledgement")?;
    ina.configure_alerts(AlertConfig::default())
        .context("disable conversion-ready ALERT")
}

pub(crate) fn alert_threshold<I2C>(ina: &mut Ina228<I2C>, case: AlertThresholdCase) -> TestResult
where
    I2C: I2c,
    I2C::Error: Debug,
{
    prepare_alert_fixture(ina)?;
    let flags = match case {
        AlertThresholdCase::ShuntOvervoltage => {
            ina.set_shunt_overvoltage_limit(-ALERT_SHUNT_LIMIT_V)
                .context("set triggering shunt overvoltage limit")?;
            let flags = fresh_alert_flags(ina)?;
            ina.set_shunt_overvoltage_limit(ALERT_SHUNT_LIMIT_V)
                .context("restore shunt overvoltage limit")?;
            flags
        }
        AlertThresholdCase::ShuntUndervoltage => {
            ina.set_shunt_undervoltage_limit(ALERT_SHUNT_LIMIT_V)
                .context("set triggering shunt undervoltage limit")?;
            let flags = fresh_alert_flags(ina)?;
            ina.set_shunt_undervoltage_limit(-ALERT_SHUNT_LIMIT_V)
                .context("restore shunt undervoltage limit")?;
            flags
        }
        AlertThresholdCase::BusOvervoltage => {
            ina.set_bus_overvoltage_limit(0.0)
                .context("set triggering bus overvoltage limit")?;
            let flags = fresh_alert_flags(ina)?;
            ina.set_bus_overvoltage_limit(MAX_BUS_VOLTAGE_V)
                .context("restore bus overvoltage limit")?;
            flags
        }
        AlertThresholdCase::BusUndervoltage => {
            ina.set_bus_undervoltage_limit(MAX_BUS_VOLTAGE_V)
                .context("set triggering bus undervoltage limit")?;
            let flags = fresh_alert_flags(ina)?;
            ina.set_bus_undervoltage_limit(0.0)
                .context("restore bus undervoltage limit")?;
            flags
        }
        AlertThresholdCase::Temperature => {
            ina.set_temperature_limit(MIN_DIE_TEMPERATURE_C)
                .context("set triggering temperature limit")?;
            let flags = fresh_alert_flags(ina)?;
            ina.set_temperature_limit(MAX_DIE_TEMPERATURE_C)
                .context("restore temperature limit")?;
            flags
        }
        AlertThresholdCase::Power => {
            ina.set_power_limit(0.0)
                .context("set triggering power limit")?;
            let flags = fresh_alert_flags(ina)?;
            ina.set_power_limit(SAFE_POWER_LIMIT_W)
                .context("restore power limit")?;
            flags
        }
    };

    let asserted = match case {
        AlertThresholdCase::ShuntOvervoltage => flags.shunt_over_limit,
        AlertThresholdCase::ShuntUndervoltage => flags.shunt_under_limit,
        AlertThresholdCase::BusOvervoltage => flags.bus_over_limit,
        AlertThresholdCase::BusUndervoltage => flags.bus_under_limit,
        AlertThresholdCase::Temperature => flags.temp_over_limit,
        AlertThresholdCase::Power => flags.power_over_limit,
    };
    require(asserted, format!("{} flag did not assert", case.name()))
}

fn alert_transparent_polarity<I2C, ALERT>(
    ina: &mut Ina228<I2C>,
    alert: &mut ALERT,
    active_high: bool,
) -> TestResult
where
    I2C: I2c,
    I2C::Error: Debug,
    ALERT: InputPin,
    ALERT::Error: Debug,
{
    prepare_alert_fixture(ina)?;
    ina.configure_alerts(AlertConfig {
        active_high,
        ..AlertConfig::default()
    })
    .context("configure transparent ALERT polarity")?;

    wait_for_alert_level(alert, !active_high, "transparent ALERT idle state")?;
    ina.set_bus_overvoltage_limit(0.0)
        .context("trigger transparent bus overvoltage alert")?;
    wait_for_alert_level(alert, active_high, "transparent ALERT assertion")?;
    ina.set_bus_overvoltage_limit(MAX_BUS_VOLTAGE_V)
        .context("clear transparent bus overvoltage condition")?;
    wait_for_alert_level(alert, !active_high, "transparent ALERT clearing")?;

    let flags = ina
        .take_diagnostic_flags()
        .context("read cleared transparent ALERT flags")?;
    require(
        !flags.bus_over_limit,
        "transparent bus overvoltage flag remained set after the condition cleared",
    )?;
    ina.configure_alerts(AlertConfig::default())
        .context("restore default ALERT polarity")?;
    wait_for_alert_level(alert, true, "restored ALERT idle state")
}

fn reset_device<I2C>(ina: &mut Ina228<I2C>) -> TestResult
where
    I2C: I2c,
    I2C::Error: Debug,
{
    ina.reset().context("reset INA228")?;
    thread::sleep(RESET_STABILIZATION);
    Ok(())
}

fn prepare_measurements<I2C>(ina: &mut Ina228<I2C>, range: AdcRange) -> TestResult
where
    I2C: I2c,
    I2C::Error: Debug,
{
    reset_device(ina)?;
    ina.configure(measurement_config())
        .context("configure continuous measurements")?;
    ina.set_adc_range(range).context("set ADC range")?;
    ina.calibrate(MAX_CURRENT_A, SHUNT_RESISTANCE_OHM)
        .context("calibrate current and power")?;
    let flags = wait_for_conversion(ina)?;
    validate_clean_diagnostics(flags)
}

fn prepare_alert_fixture<I2C>(ina: &mut Ina228<I2C>) -> TestResult
where
    I2C: I2c,
    I2C::Error: Debug,
{
    prepare_measurements(ina, AdcRange::Range40mV)?;
    let baseline = read_measurements(ina)?;
    require(
        baseline.bus_voltage_v >= MIN_FIXTURE_BUS_VOLTAGE_V,
        format!(
            "bus voltage {} V is below the {MIN_FIXTURE_BUS_VOLTAGE_V} V fixture minimum",
            baseline.bus_voltage_v
        ),
    )?;
    require(
        baseline.power_w >= MIN_FIXTURE_POWER_W,
        format!(
            "power {} W is below the {MIN_FIXTURE_POWER_W} W fixture minimum",
            baseline.power_w
        ),
    )?;
    set_safe_limits(ina)?;
    ina.configure_alerts(AlertConfig::default())
        .context("clear and restore default ALERT configuration")
}

fn wait_for_alert_level<ALERT>(alert: &mut ALERT, expected_high: bool, case: &str) -> TestResult
where
    ALERT: InputPin,
    ALERT::Error: Debug,
{
    let started = Instant::now();
    while started.elapsed() < CONVERSION_TIMEOUT {
        if alert.is_high().context("read ALERT GPIO")? == expected_high {
            return Ok(());
        }
        thread::sleep(Duration::from_millis(1));
    }

    let observed_high = alert.is_high().context("read ALERT GPIO after timeout")?;
    let expected = if expected_high { "high" } else { "low" };
    let observed = if observed_high { "high" } else { "low" };
    Err(format!(
        "{case}: ALERT remained {observed}, expected {expected} within {} ms",
        CONVERSION_TIMEOUT.as_millis()
    ))
}

fn require_alert_level<ALERT>(alert: &mut ALERT, expected_high: bool, case: &str) -> TestResult
where
    ALERT: InputPin,
    ALERT::Error: Debug,
{
    let observed_high = alert.is_high().context("read ALERT GPIO")?;
    let expected = if expected_high { "high" } else { "low" };
    let observed = if observed_high { "high" } else { "low" };
    require(
        observed_high == expected_high,
        format!("{case}: ALERT was {observed}, expected {expected}"),
    )
}

fn wait_for_conversion<I2C>(ina: &mut Ina228<I2C>) -> TestResult<DiagnosticFlags>
where
    I2C: I2c,
    I2C::Error: Debug,
{
    Ok(wait_for_conversion_timed(ina, CONVERSION_TIMEOUT)?.flags)
}

fn wait_for_conversion_timed<I2C>(
    ina: &mut Ina228<I2C>,
    timeout: Duration,
) -> TestResult<ConversionObservation>
where
    I2C: I2c,
    I2C::Error: Debug,
{
    let started = Instant::now();
    while started.elapsed() < timeout {
        let flags = ina
            .take_diagnostic_flags()
            .context("poll conversion-ready")?;
        if flags.conversion_ready {
            return Ok(ConversionObservation {
                flags,
                elapsed: started.elapsed(),
            });
        }
        thread::sleep(Duration::from_millis(1));
    }
    Err(format!(
        "conversion-ready timed out after {} ms",
        timeout.as_millis()
    ))
}

fn require_no_conversion<I2C>(ina: &mut Ina228<I2C>, duration: Duration, case: &str) -> TestResult
where
    I2C: I2c,
    I2C::Error: Debug,
{
    let started = Instant::now();
    while started.elapsed() < duration {
        let flags = ina
            .take_diagnostic_flags()
            .context("poll for unexpected conversion-ready")?;
        require(
            !flags.conversion_ready,
            format!("{case}: triggered mode produced another conversion"),
        )?;
        thread::sleep(Duration::from_millis(1));
    }
    Ok(())
}

fn validate_conversion_duration(case: &str, observed: Duration, expected: Duration) -> TestResult {
    let expected_us = expected.as_micros();
    let observed_us = observed.as_micros();
    let minimum_us = expected_us * 9 / 10;
    let maximum_us = expected_us * 11 / 10 + u128::from(TIMING_UPPER_SLACK_US);

    println!(
        "ADC timing {case}: observed={observed_us} us, expected={expected_us} us, window={minimum_us}..={maximum_us} us"
    );
    require(
        (minimum_us..=maximum_us).contains(&observed_us),
        format!(
            "{case}: conversion completed in {observed_us} us, expected {minimum_us}..={maximum_us} us"
        ),
    )
}

fn timing_upper_bound(expected: Duration) -> Duration {
    Duration::from_micros(
        u64::try_from(expected.as_micros() * 11 / 10).expect("ADC timing upper bound must fit u64")
            + TIMING_UPPER_SLACK_US,
    )
}

fn timing_wait_timeout(expected: Duration) -> Duration {
    timing_upper_bound(expected) + Duration::from_micros(TIMING_TIMEOUT_SLACK_US)
}

fn validate_clean_diagnostics(flags: DiagnosticFlags) -> TestResult {
    require(flags.memory_ok, "device memory checksum is invalid")?;
    require(!flags.energy_overflow, "unexpected energy overflow")?;
    require(!flags.math_overflow, "unexpected math overflow")?;
    require(!flags.temp_over_limit, "unexpected temperature alert")?;
    require(
        !flags.shunt_over_limit,
        "unexpected shunt overvoltage alert",
    )?;
    require(
        !flags.shunt_under_limit,
        "unexpected shunt undervoltage alert",
    )?;
    require(!flags.bus_over_limit, "unexpected bus overvoltage alert")?;
    require(!flags.bus_under_limit, "unexpected bus undervoltage alert")?;
    require(!flags.power_over_limit, "unexpected power alert")?;
    require(!flags.charge_overflow, "unexpected charge overflow")
}

fn read_uncalibrated_measurements<I2C>(ina: &mut Ina228<I2C>) -> TestResult<Measurements>
where
    I2C: I2c,
    I2C::Error: Debug,
{
    Ok(Measurements {
        bus_voltage_v: ina.bus_voltage().context("read bus voltage")?,
        shunt_voltage_v: ina.shunt_voltage().context("read shunt voltage")?,
        current_a: 0.0,
        power_w: 0.0,
        die_temperature_c: ina.die_temperature().context("read die temperature")?,
    })
}

fn read_measurements<I2C>(ina: &mut Ina228<I2C>) -> TestResult<Measurements>
where
    I2C: I2c,
    I2C::Error: Debug,
{
    Ok(Measurements {
        bus_voltage_v: ina.bus_voltage().context("read bus voltage")?,
        shunt_voltage_v: ina.shunt_voltage().context("read shunt voltage")?,
        current_a: ina.current().context("read current")?,
        power_w: ina.power().context("read power")?,
        die_temperature_c: ina.die_temperature().context("read die temperature")?,
    })
}

fn validate_common_measurements(measurements: Measurements, shunt_full_scale_v: f32) -> TestResult {
    require(
        measurements.bus_voltage_v.is_finite()
            && (0.0..=MAX_BUS_VOLTAGE_V).contains(&measurements.bus_voltage_v),
        format!("invalid bus voltage {} V", measurements.bus_voltage_v),
    )?;
    require(
        measurements.shunt_voltage_v.is_finite()
            && measurements.shunt_voltage_v.abs() <= shunt_full_scale_v,
        format!("invalid shunt voltage {} V", measurements.shunt_voltage_v),
    )?;
    require(
        measurements.die_temperature_c.is_finite()
            && (MIN_DIE_TEMPERATURE_C..=MAX_DIE_TEMPERATURE_C)
                .contains(&measurements.die_temperature_c),
        format!(
            "invalid die temperature {} C",
            measurements.die_temperature_c
        ),
    )?;
    require(measurements.current_a.is_finite(), "current is not finite")?;
    require(
        measurements.power_w.is_finite() && measurements.power_w >= 0.0,
        format!("invalid power {} W", measurements.power_w),
    )
}

fn validate_snapshot(
    energy_joules: f64,
    charge_coulombs: f64,
    flags: DiagnosticFlags,
) -> TestResult {
    require(
        energy_joules.is_finite() && energy_joules >= 0.0,
        format!("invalid accumulated energy {energy_joules} J"),
    )?;
    require(
        charge_coulombs.is_finite(),
        format!("invalid accumulated charge {charge_coulombs} C"),
    )?;
    require(flags.memory_ok, "memory checksum failed during snapshot")?;
    require(!flags.energy_overflow, "energy accumulator overflowed")?;
    require(!flags.charge_overflow, "charge accumulator overflowed")?;
    require(!flags.math_overflow, "device math overflowed")
}

fn expect_configuration_error<T, E: Debug>(
    result: Result<T, Error<E>>,
    expected: ConfigurationError,
    case: &str,
) -> TestResult {
    match result {
        Err(Error::InvalidConfiguration(actual)) if actual == expected => Ok(()),
        Err(error) => Err(format!("{case}: returned {error:?}, expected {expected:?}")),
        Ok(_) => Err(format!("{case}: accepted invalid input")),
    }
}

fn expect_stale_shunt_voltage<I2C>(ina: &mut Ina228<I2C>, case: &str) -> TestResult
where
    I2C: I2c,
    I2C::Error: Debug,
{
    match ina.shunt_voltage() {
        Err(Error::ShuntVoltageStale) => Ok(()),
        Err(error) => Err(format!("{case}: returned {error:?}")),
        Ok(voltage) => Err(format!("{case}: returned stale value {voltage} V")),
    }
}

fn set_safe_limits<I2C>(ina: &mut Ina228<I2C>) -> TestResult
where
    I2C: I2c,
    I2C::Error: Debug,
{
    ina.set_shunt_overvoltage_limit(ALERT_SHUNT_LIMIT_V)
        .context("set safe shunt overvoltage limit")?;
    ina.set_shunt_undervoltage_limit(-ALERT_SHUNT_LIMIT_V)
        .context("set safe shunt undervoltage limit")?;
    ina.set_bus_overvoltage_limit(MAX_BUS_VOLTAGE_V)
        .context("set safe bus overvoltage limit")?;
    ina.set_bus_undervoltage_limit(0.0)
        .context("set safe bus undervoltage limit")?;
    ina.set_temperature_limit(MAX_DIE_TEMPERATURE_C)
        .context("set safe temperature limit")?;
    ina.set_power_limit(SAFE_POWER_LIMIT_W)
        .context("set safe power limit")
}

fn fresh_alert_flags<I2C>(ina: &mut Ina228<I2C>) -> TestResult<DiagnosticFlags>
where
    I2C: I2c,
    I2C::Error: Debug,
{
    ina.configure(measurement_config())
        .context("restart conversion for alert check")?;
    let flags = wait_for_conversion(ina)?;
    require(flags.memory_ok, "memory checksum failed during alert check")?;
    Ok(flags)
}

fn fast_config(mode: OperatingMode, averaging: AveragingCount) -> AdcConfig {
    AdcConfig {
        mode,
        bus_conversion_time: ConversionTime::Us50,
        shunt_conversion_time: ConversionTime::Us50,
        temperature_conversion_time: ConversionTime::Us50,
        averaging,
    }
}

fn measurement_config() -> AdcConfig {
    AdcConfig {
        averaging: AveragingCount::N16,
        ..AdcConfig::default()
    }
}
