use embedded_hal::i2c::ErrorKind;
use embedded_hal_mock::eh1::i2c::{Mock, Transaction};
use ina228::{
    AdcConfig, AdcRange, AlertConfig, AveragingCount, ConfigurationError, ConversionTime,
    DEFAULT_ADDRESS, Error as DriverError, Ina228, InitializationError, OperatingMode,
};

const ADDR: u8 = DEFAULT_ADDRESS;
const DEFAULT_ADC_CONFIG: u16 = 0xFB68;
const SHUTDOWN_ADC_CONFIG: u16 = 0x0B68;
const SHUTDOWN_ALT_ADC_CONFIG: u16 = 0x8B68;
const CONTINUOUS_BUS_ADC_CONFIG: u16 = 0x9B68;
const CONTINUOUS_SHUNT_ADC_CONFIG: u16 = 0xAB68;
const ADC_MODE_MASK: u16 = 0xF000;

/// Compute SHUNT_CAL the same way the driver does (f32 current_lsb, then f64 multiply).
fn expected_shunt_cal(max_current: f32, shunt_ohm: f32, adc_range_40mv: bool) -> u16 {
    let current_lsb = max_current / 524_288.0_f32;
    let mut shunt_cal = 13107.2e6 * current_lsb as f64 * shunt_ohm as f64;
    if adc_range_40mv {
        shunt_cal *= 4.0;
    }
    shunt_cal.round() as u16
}

fn write_txn(reg: u8, value: u16) -> Transaction {
    let bytes = value.to_be_bytes();
    Transaction::write(ADDR, vec![reg, bytes[0], bytes[1]])
}

fn read_txn(reg: u8, bytes: &[u8]) -> Transaction {
    Transaction::write_read(ADDR, vec![reg], bytes.to_vec())
}

fn mock_with_config(config: u16, transactions: &[Transaction]) -> Mock {
    let mut expected = Vec::with_capacity(transactions.len() + 1);
    expected.push(read_txn(0x00, &config.to_be_bytes()));
    expected.extend_from_slice(transactions);
    Mock::new(&expected)
}

fn mock(transactions: &[Transaction]) -> Mock {
    mock_with_config(0, transactions)
}

fn active_calibration_txns(shunt_cal: u16, config: u16) -> [Transaction; 7] {
    [
        read_txn(0x01, &DEFAULT_ADC_CONFIG.to_be_bytes()),
        write_txn(0x01, SHUTDOWN_ADC_CONFIG),
        write_txn(0x02, shunt_cal),
        read_txn(0x00, &config.to_be_bytes()),
        write_txn(0x00, config | (1 << 14)),
        write_txn(0x11, u16::MAX),
        write_txn(0x01, DEFAULT_ADC_CONFIG),
    ]
}

fn continuous_snapshot_txns(
    adc_config: u16,
    diagnostic: u16,
    energy: u64,
    charge: u64,
) -> [Transaction; 6] {
    [
        read_txn(0x01, &adc_config.to_be_bytes()),
        write_txn(0x01, adc_config & !ADC_MODE_MASK),
        read_txn(0x0B, &diagnostic.to_be_bytes()),
        read_txn(0x09, &u40_bytes(energy)),
        read_txn(0x0A, &u40_bytes(charge)),
        write_txn(0x01, adc_config),
    ]
}

fn assert_panics_with(expected_message: &str, operation: impl FnOnce()) {
    let payload = std::panic::catch_unwind(std::panic::AssertUnwindSafe(operation))
        .expect_err("expected operation to panic");
    let message = payload
        .downcast_ref::<&str>()
        .copied()
        .or_else(|| payload.downcast_ref::<String>().map(String::as_str))
        .expect("panic payload must be a string");
    assert!(
        message.contains(expected_message),
        "expected panic containing {expected_message:?}, got {message:?}"
    );
}

fn assert_configuration_error(
    result: Result<(), DriverError<ErrorKind>>,
    expected: ConfigurationError,
) {
    assert_eq!(result, Err(DriverError::InvalidConfiguration(expected)));
}

fn u24_bytes(val: u32) -> [u8; 3] {
    [(val >> 16) as u8, (val >> 8) as u8, val as u8]
}

fn u40_bytes(val: u64) -> [u8; 5] {
    [
        (val >> 32) as u8,
        (val >> 24) as u8,
        (val >> 16) as u8,
        (val >> 8) as u8,
        val as u8,
    ]
}

#[test]
fn new_default_address() {
    let i2c = mock(&[]);
    let ina = Ina228::new(i2c, DEFAULT_ADDRESS).unwrap();
    ina.release().done();
}

#[test]
fn new_max_address() {
    let i2c = Mock::new(&[Transaction::write_read(0x4F, vec![0x00], vec![0x00, 0x00])]);
    let ina = Ina228::new(i2c, 0x4F).unwrap();
    ina.release().done();
}

#[test]
fn new_returns_i2c_with_config_read_error() {
    let i2c = Mock::new(&[
        Transaction::write_read(ADDR, vec![0x00], vec![0x00, 0x00]).with_error(ErrorKind::Bus)
    ]);
    match Ina228::new(i2c, ADDR) {
        Ok(_) => panic!("expected CONFIG read to fail"),
        Err(InitializationError::InvalidAddress { address, .. }) => {
            panic!("expected I2C error, got invalid address {address:#04X}")
        }
        Err(InitializationError::I2c { mut i2c, error }) => {
            assert_eq!(error, ErrorKind::Bus);
            i2c.done();
        }
    }
}

#[test]
fn new_returns_i2c_with_invalid_address() {
    for address in [0x00, 0x3F, 0x50, u8::MAX] {
        let i2c = Mock::new(&[]);
        match Ina228::new(i2c, address) {
            Ok(_) => panic!("expected address {address:#04X} to be rejected"),
            Err(InitializationError::InvalidAddress {
                mut i2c,
                address: returned_address,
            }) => {
                assert_eq!(returned_address, address);
                i2c.done();
            }
            Err(InitializationError::I2c { .. }) => {
                panic!("invalid address {address:#04X} attempted an I2C transaction")
            }
        }
    }
}

#[test]
fn reset() {
    let shunt_cal = expected_shunt_cal(10.0, 0.01, false);
    let mut transactions = Vec::from(active_calibration_txns(shunt_cal, 0));
    transactions.extend([
        write_txn(0x00, 1 << 15),
        read_txn(0x05, &[0x00, 0x00, 0x00]),
    ]);
    let i2c = mock(&transactions);
    let mut ina = Ina228::new(i2c, ADDR).unwrap();
    ina.calibrate(10.0, 0.01).unwrap();
    ina.reset().unwrap();
    // bus_voltage should still work after reset (doesn't need calibration)
    assert_eq!(ina.bus_voltage().unwrap(), 0.0);
    assert_panics_with("call calibrate() before reading current", || {
        let _ = ina.current();
    });
    ina.release().done();
}

#[test]
fn reset_write_failure_invalidates_scale_state_until_recovery() {
    let shunt_cal = expected_shunt_cal(4.0, 0.01, true);
    let reset_failure = Transaction::write(ADDR, vec![0x00, 0x80, 0x00]).with_error(ErrorKind::Bus);
    let mut transactions = Vec::from(active_calibration_txns(shunt_cal, 1 << 4));
    transactions.extend([
        reset_failure,
        write_txn(0x00, 1 << 15),
        read_txn(0x0B, &(1_u16 << 1).to_be_bytes()),
        read_txn(0x01, &DEFAULT_ADC_CONFIG.to_be_bytes()),
        read_txn(0x04, &u24_bytes(3200 << 4)),
    ]);
    let i2c = mock_with_config(1 << 4, &transactions);
    let mut ina = Ina228::new(i2c, ADDR).unwrap();
    ina.calibrate(4.0, 0.01).unwrap();

    assert_eq!(ina.reset(), Err(DriverError::I2c(ErrorKind::Bus)));
    assert_panics_with("call calibrate() before reading current", || {
        let _ = ina.current();
    });
    assert_eq!(ina.shunt_voltage(), Err(DriverError::AdcRangeUnknown));
    assert_eq!(
        ina.set_shunt_overvoltage_limit(0.01),
        Err(DriverError::AdcRangeUnknown)
    );

    ina.reset().unwrap();
    assert_eq!(ina.shunt_voltage(), Err(DriverError::ShuntVoltageStale));
    assert!(ina.take_diagnostic_flags().unwrap().conversion_ready);
    let shunt_voltage = ina.shunt_voltage().unwrap();
    assert!(
        (shunt_voltage - 0.001).abs() < 1e-6,
        "expected 0.001V in the reset 163mV range, got {shunt_voltage}"
    );
    ina.release().done();

    let shunt_cal = expected_shunt_cal(4.0, 0.01, true);
    let reset_failure = Transaction::write(ADDR, vec![0x00, 0x80, 0x00]).with_error(ErrorKind::Bus);
    let i2c = mock_with_config(
        1 << 4,
        &[
            reset_failure,
            read_txn(0x00, &(1_u16 << 4).to_be_bytes()),
            read_txn(0x01, &DEFAULT_ADC_CONFIG.to_be_bytes()),
            write_txn(0x01, SHUTDOWN_ADC_CONFIG),
            write_txn(0x02, shunt_cal),
            read_txn(0x00, &(1_u16 << 4).to_be_bytes()),
            write_txn(0x00, (1 << 14) | (1 << 4)),
            write_txn(0x11, u16::MAX),
            write_txn(0x01, DEFAULT_ADC_CONFIG),
            read_txn(0x07, &u24_bytes(262144 << 4)),
        ],
    );
    let mut ina = Ina228::new(i2c, ADDR).unwrap();

    assert_eq!(ina.reset(), Err(DriverError::I2c(ErrorKind::Bus)));
    ina.calibrate(4.0, 0.01).unwrap();
    assert_eq!(ina.current().unwrap(), 2.0);
    ina.release().done();
}

#[test]
fn configure_encodes_every_adc_enum_variant() {
    let mode_cases = [
        (OperatingMode::Shutdown, 0x0),
        (OperatingMode::TriggeredBus, 0x1),
        (OperatingMode::TriggeredShunt, 0x2),
        (OperatingMode::TriggeredBusShunt, 0x3),
        (OperatingMode::TriggeredTemp, 0x4),
        (OperatingMode::TriggeredTempBus, 0x5),
        (OperatingMode::TriggeredTempShunt, 0x6),
        (OperatingMode::TriggeredAll, 0x7),
        (OperatingMode::ContinuousBus, 0x9),
        (OperatingMode::ContinuousShunt, 0xA),
        (OperatingMode::ContinuousBusShunt, 0xB),
        (OperatingMode::ContinuousTemp, 0xC),
        (OperatingMode::ContinuousTempBus, 0xD),
        (OperatingMode::ContinuousTempShunt, 0xE),
        (OperatingMode::ContinuousAll, 0xF),
    ];
    let conversion_time_cases = [
        (ConversionTime::Us50, 0),
        (ConversionTime::Us84, 1),
        (ConversionTime::Us150, 2),
        (ConversionTime::Us280, 3),
        (ConversionTime::Us540, 4),
        (ConversionTime::Us1052, 5),
        (ConversionTime::Us2074, 6),
        (ConversionTime::Us4120, 7),
    ];
    let averaging_cases = [
        (AveragingCount::N1, 0),
        (AveragingCount::N4, 1),
        (AveragingCount::N16, 2),
        (AveragingCount::N64, 3),
        (AveragingCount::N128, 4),
        (AveragingCount::N256, 5),
        (AveragingCount::N512, 6),
        (AveragingCount::N1024, 7),
    ];

    let mut transactions = Vec::new();
    for &(_, encoded) in &mode_cases {
        transactions.push(write_txn(
            0x01,
            (DEFAULT_ADC_CONFIG & !0xF000) | (encoded << 12),
        ));
    }
    for &(_, encoded) in &conversion_time_cases {
        transactions.push(write_txn(
            0x01,
            (DEFAULT_ADC_CONFIG & !(0x7 << 9)) | (encoded << 9),
        ));
        transactions.push(write_txn(
            0x01,
            (DEFAULT_ADC_CONFIG & !(0x7 << 6)) | (encoded << 6),
        ));
        transactions.push(write_txn(
            0x01,
            (DEFAULT_ADC_CONFIG & !(0x7 << 3)) | (encoded << 3),
        ));
    }
    for &(_, encoded) in &averaging_cases {
        transactions.push(write_txn(0x01, (DEFAULT_ADC_CONFIG & !0x7) | encoded));
    }

    let i2c = mock(&transactions);
    let mut ina = Ina228::new(i2c, ADDR).unwrap();
    for &(mode, _) in &mode_cases {
        ina.configure(AdcConfig {
            mode,
            ..AdcConfig::default()
        })
        .unwrap();
    }
    for &(conversion_time, _) in &conversion_time_cases {
        ina.configure(AdcConfig {
            bus_conversion_time: conversion_time,
            ..AdcConfig::default()
        })
        .unwrap();
        ina.configure(AdcConfig {
            shunt_conversion_time: conversion_time,
            ..AdcConfig::default()
        })
        .unwrap();
        ina.configure(AdcConfig {
            temperature_conversion_time: conversion_time,
            ..AdcConfig::default()
        })
        .unwrap();
    }
    for &(averaging, _) in &averaging_cases {
        ina.configure(AdcConfig {
            averaging,
            ..AdcConfig::default()
        })
        .unwrap();
    }
    ina.release().done();
}

#[test]
fn set_adc_range_40mv() {
    let i2c = mock(&[
        read_txn(0x00, &0x0000_u16.to_be_bytes()),
        read_txn(0x01, &DEFAULT_ADC_CONFIG.to_be_bytes()),
        write_txn(0x01, SHUTDOWN_ADC_CONFIG),
        write_txn(0x0C, i16::MAX as u16),
        write_txn(0x0D, i16::MIN as u16),
        write_txn(0x00, 1 << 4),
        write_txn(0x01, DEFAULT_ADC_CONFIG),
    ]);
    let mut ina = Ina228::new(i2c, ADDR).unwrap();
    ina.set_adc_range(AdcRange::Range40mV).unwrap();
    ina.release().done();
}

#[test]
fn set_adc_range_163mv_clears_bit() {
    let i2c = mock_with_config(
        1 << 4,
        &[
            read_txn(0x00, &0x0010_u16.to_be_bytes()),
            read_txn(0x01, &DEFAULT_ADC_CONFIG.to_be_bytes()),
            write_txn(0x01, SHUTDOWN_ADC_CONFIG),
            write_txn(0x0C, i16::MAX as u16),
            write_txn(0x0D, i16::MIN as u16),
            write_txn(0x00, 0x0000),
            write_txn(0x01, DEFAULT_ADC_CONFIG),
        ],
    );
    let mut ina = Ina228::new(i2c, ADDR).unwrap();
    ina.set_adc_range(AdcRange::Range163mV).unwrap();
    ina.set_adc_range(AdcRange::Range163mV).unwrap();
    ina.release().done();
}

#[test]
fn set_adc_range_preserves_shutdown_mode() {
    for adc_config in [SHUTDOWN_ADC_CONFIG, SHUTDOWN_ALT_ADC_CONFIG] {
        let i2c = mock(&[
            read_txn(0x00, &0x0000_u16.to_be_bytes()),
            read_txn(0x01, &adc_config.to_be_bytes()),
            write_txn(0x0C, i16::MAX as u16),
            write_txn(0x0D, i16::MIN as u16),
            write_txn(0x00, 1 << 4),
        ]);
        let mut ina = Ina228::new(i2c, ADDR).unwrap();
        ina.set_adc_range(AdcRange::Range40mV).unwrap();
        ina.release().done();
    }
}

#[test]
fn range_change_requires_completed_shunt_conversion() {
    let i2c = mock(&[
        read_txn(0x00, &0x0000_u16.to_be_bytes()),
        read_txn(0x01, &DEFAULT_ADC_CONFIG.to_be_bytes()),
        write_txn(0x01, SHUTDOWN_ADC_CONFIG),
        write_txn(0x0C, i16::MAX as u16),
        write_txn(0x0D, i16::MIN as u16),
        write_txn(0x00, 1 << 4),
        write_txn(0x01, DEFAULT_ADC_CONFIG),
        read_txn(0x0B, &0_u16.to_be_bytes()),
        read_txn(0x0B, &(1_u16 << 1).to_be_bytes()),
        read_txn(0x01, &DEFAULT_ADC_CONFIG.to_be_bytes()),
        read_txn(0x04, &u24_bytes(12800 << 4)),
    ]);
    let mut ina = Ina228::new(i2c, ADDR).unwrap();
    ina.set_adc_range(AdcRange::Range40mV).unwrap();

    assert_eq!(ina.shunt_voltage(), Err(DriverError::ShuntVoltageStale));
    assert!(!ina.take_diagnostic_flags().unwrap().conversion_ready);
    assert_eq!(ina.shunt_voltage(), Err(DriverError::ShuntVoltageStale));
    assert!(ina.take_diagnostic_flags().unwrap().conversion_ready);
    let shunt_voltage = ina.shunt_voltage().unwrap();
    assert!(
        (shunt_voltage - 0.001).abs() < 1e-6,
        "expected 0.001V from the fresh 40mV conversion, got {shunt_voltage}"
    );
    ina.release().done();

    let i2c = mock(&[
        read_txn(0x00, &0x0000_u16.to_be_bytes()),
        read_txn(0x01, &SHUTDOWN_ADC_CONFIG.to_be_bytes()),
        write_txn(0x0C, i16::MAX as u16),
        write_txn(0x0D, i16::MIN as u16),
        write_txn(0x00, 1 << 4),
        write_txn(0x01, CONTINUOUS_BUS_ADC_CONFIG),
        read_txn(0x0B, &(1_u16 << 1).to_be_bytes()),
        read_txn(0x01, &CONTINUOUS_BUS_ADC_CONFIG.to_be_bytes()),
        write_txn(0x01, CONTINUOUS_SHUNT_ADC_CONFIG),
        read_txn(0x0B, &(1_u16 << 1).to_be_bytes()),
        read_txn(0x01, &CONTINUOUS_SHUNT_ADC_CONFIG.to_be_bytes()),
        read_txn(0x04, &u24_bytes(12800 << 4)),
    ]);
    let mut ina = Ina228::new(i2c, ADDR).unwrap();
    ina.set_adc_range(AdcRange::Range40mV).unwrap();

    ina.configure(AdcConfig {
        mode: OperatingMode::ContinuousBus,
        ..Default::default()
    })
    .unwrap();
    assert!(ina.take_diagnostic_flags().unwrap().conversion_ready);
    assert_eq!(ina.shunt_voltage(), Err(DriverError::ShuntVoltageStale));
    ina.configure(AdcConfig {
        mode: OperatingMode::ContinuousShunt,
        ..Default::default()
    })
    .unwrap();
    assert!(ina.take_diagnostic_flags().unwrap().conversion_ready);
    let shunt_voltage = ina.shunt_voltage().unwrap();
    assert!(
        (shunt_voltage - 0.001).abs() < 1e-6,
        "expected 0.001V after the first shunt conversion, got {shunt_voltage}"
    );
    ina.release().done();

    let shunt_cal = expected_shunt_cal(4.0, 0.01, true);
    let mut transactions = vec![
        read_txn(0x00, &0x0000_u16.to_be_bytes()),
        read_txn(0x01, &DEFAULT_ADC_CONFIG.to_be_bytes()),
        write_txn(0x01, SHUTDOWN_ADC_CONFIG),
        write_txn(0x0C, i16::MAX as u16),
        write_txn(0x0D, i16::MIN as u16),
        write_txn(0x00, 1 << 4),
        write_txn(0x01, DEFAULT_ADC_CONFIG),
    ];
    transactions.extend(active_calibration_txns(shunt_cal, 1 << 4));
    transactions.extend(continuous_snapshot_txns(DEFAULT_ADC_CONFIG, 1 << 1, 0, 0));
    transactions.push(read_txn(0x04, &u24_bytes(12800 << 4)));
    let i2c = mock(&transactions);
    let mut ina = Ina228::new(i2c, ADDR).unwrap();
    ina.set_adc_range(AdcRange::Range40mV).unwrap();
    ina.calibrate(4.0, 0.01).unwrap();

    assert_eq!(ina.shunt_voltage(), Err(DriverError::ShuntVoltageStale));
    assert!(
        ina.take_accumulator_snapshot()
            .unwrap()
            .diagnostic_flags
            .conversion_ready
    );
    let shunt_voltage = ina.shunt_voltage().unwrap();
    assert!(
        (shunt_voltage - 0.001).abs() < 1e-6,
        "expected snapshot acknowledgement to publish fresh VSHUNT, got {shunt_voltage}"
    );
    ina.release().done();

    let failed_mode_read =
        read_txn(0x01, &DEFAULT_ADC_CONFIG.to_be_bytes()).with_error(ErrorKind::Bus);
    let i2c = mock(&[
        read_txn(0x00, &0x0000_u16.to_be_bytes()),
        read_txn(0x01, &DEFAULT_ADC_CONFIG.to_be_bytes()),
        write_txn(0x01, SHUTDOWN_ADC_CONFIG),
        write_txn(0x0C, i16::MAX as u16),
        write_txn(0x0D, i16::MIN as u16),
        write_txn(0x00, 1 << 4),
        write_txn(0x01, DEFAULT_ADC_CONFIG),
        read_txn(0x0B, &(1_u16 << 1).to_be_bytes()),
        failed_mode_read,
    ]);
    let mut ina = Ina228::new(i2c, ADDR).unwrap();
    ina.set_adc_range(AdcRange::Range40mV).unwrap();

    assert!(matches!(
        ina.take_diagnostic_flags(),
        Err(DriverError::I2c(ErrorKind::Bus))
    ));
    assert_eq!(ina.shunt_voltage(), Err(DriverError::ShuntVoltageStale));
    ina.release().done();
}

#[test]
fn set_adc_range_pre_config_failures_preserve_range() {
    let failed_shutdown =
        Transaction::write(ADDR, vec![0x01, 0x0B, 0x68]).with_error(ErrorKind::Bus);
    let i2c = mock(&[
        read_txn(0x00, &0x0000_u16.to_be_bytes()),
        read_txn(0x01, &DEFAULT_ADC_CONFIG.to_be_bytes()),
        failed_shutdown,
        read_txn(0x04, &u24_bytes(3200 << 4)),
    ]);
    let mut ina = Ina228::new(i2c, ADDR).unwrap();
    assert_eq!(
        ina.set_adc_range(AdcRange::Range40mV),
        Err(DriverError::I2c(ErrorKind::Bus))
    );
    let shunt_voltage = ina.shunt_voltage().unwrap();
    assert!(
        (shunt_voltage - 0.001).abs() < 1e-6,
        "expected 0.001V in the preserved 163mV range, got {shunt_voltage}"
    );
    ina.release().done();

    let failed_sovl = Transaction::write(ADDR, vec![0x0C, 0x7F, 0xFF]).with_error(ErrorKind::Bus);
    let i2c = mock(&[
        read_txn(0x00, &0x0000_u16.to_be_bytes()),
        read_txn(0x01, &DEFAULT_ADC_CONFIG.to_be_bytes()),
        write_txn(0x01, SHUTDOWN_ADC_CONFIG),
        failed_sovl,
        read_txn(0x04, &u24_bytes(3200 << 4)),
    ]);
    let mut ina = Ina228::new(i2c, ADDR).unwrap();
    assert_eq!(
        ina.set_adc_range(AdcRange::Range40mV),
        Err(DriverError::I2c(ErrorKind::Bus))
    );
    let shunt_voltage = ina.shunt_voltage().unwrap();
    assert!(
        (shunt_voltage - 0.001).abs() < 1e-6,
        "expected 0.001V in the preserved 163mV range, got {shunt_voltage}"
    );
    ina.release().done();

    let failed_suvl = Transaction::write(ADDR, vec![0x0D, 0x80, 0x00]).with_error(ErrorKind::Bus);
    let i2c = mock(&[
        read_txn(0x00, &0x0000_u16.to_be_bytes()),
        read_txn(0x01, &DEFAULT_ADC_CONFIG.to_be_bytes()),
        write_txn(0x01, SHUTDOWN_ADC_CONFIG),
        write_txn(0x0C, i16::MAX as u16),
        failed_suvl,
        read_txn(0x04, &u24_bytes(3200 << 4)),
    ]);
    let mut ina = Ina228::new(i2c, ADDR).unwrap();
    assert_eq!(
        ina.set_adc_range(AdcRange::Range40mV),
        Err(DriverError::I2c(ErrorKind::Bus))
    );
    let shunt_voltage = ina.shunt_voltage().unwrap();
    assert!(
        (shunt_voltage - 0.001).abs() < 1e-6,
        "expected 0.001V in the preserved 163mV range, got {shunt_voltage}"
    );
    ina.release().done();
}

#[test]
fn calibrate_encodes_normal_and_minimum_shunt_cal() {
    let normal_shunt_cal = expected_shunt_cal(10.0, 0.01, false);
    let minimum_shunt_cal = expected_shunt_cal(1.0, 0.00004, false);
    assert_eq!(normal_shunt_cal, 2500);
    assert_eq!(minimum_shunt_cal, 1);
    assert_ne!(normal_shunt_cal, minimum_shunt_cal);

    let mut transactions = Vec::from(active_calibration_txns(normal_shunt_cal, 0));
    transactions.extend(active_calibration_txns(minimum_shunt_cal, 0));
    let i2c = mock(&transactions);
    let mut ina = Ina228::new(i2c, ADDR).unwrap();
    ina.calibrate(10.0, 0.01).unwrap();
    ina.calibrate(1.0, 0.00004).unwrap();
    ina.release().done();
}

#[test]
fn calibrate_with_40mv_range() {
    let shunt_cal = expected_shunt_cal(4.0, 0.01, true);

    let i2c = mock(&[
        read_txn(0x00, &0x0000_u16.to_be_bytes()),
        read_txn(0x01, &DEFAULT_ADC_CONFIG.to_be_bytes()),
        write_txn(0x01, SHUTDOWN_ADC_CONFIG),
        write_txn(0x0C, i16::MAX as u16),
        write_txn(0x0D, i16::MIN as u16),
        write_txn(0x00, 1 << 4),
        write_txn(0x01, DEFAULT_ADC_CONFIG),
        read_txn(0x01, &DEFAULT_ADC_CONFIG.to_be_bytes()),
        write_txn(0x01, SHUTDOWN_ADC_CONFIG),
        write_txn(0x02, shunt_cal),
        read_txn(0x00, &(1_u16 << 4).to_be_bytes()),
        write_txn(0x00, (1 << 14) | (1 << 4)),
        write_txn(0x11, u16::MAX),
        write_txn(0x01, DEFAULT_ADC_CONFIG),
    ]);
    let mut ina = Ina228::new(i2c, ADDR).unwrap();
    ina.set_adc_range(AdcRange::Range40mV).unwrap();
    ina.calibrate(4.0, 0.01).unwrap();
    ina.release().done();
}

#[test]
fn calibrate_rejects_invalid_inputs_before_i2c() {
    let i2c = mock(&[]);
    let mut ina = Ina228::new(i2c, ADDR).unwrap();

    for max_current in [0.0, -1.0, f32::NAN, f32::INFINITY] {
        assert_configuration_error(
            ina.calibrate(max_current, 0.01),
            ConfigurationError::MaxCurrent,
        );
    }
    for shunt_resistance in [0.0, -0.01, f32::NAN, f32::INFINITY] {
        assert_configuration_error(
            ina.calibrate(10.0, shunt_resistance),
            ConfigurationError::ShuntResistance,
        );
    }
    for (max_current, shunt_resistance) in [(100.0, 0.1), (1.0e-6, 1.0e-6)] {
        assert_configuration_error(
            ina.calibrate(max_current, shunt_resistance),
            ConfigurationError::Calibration,
        );
    }

    ina.release().done();

    for (config, full_scale_current) in [(0, 0.16384), (1 << 4, 0.04096)] {
        let i2c = mock_with_config(config, &[]);
        let mut ina = Ina228::new(i2c, ADDR).unwrap();
        assert_configuration_error(
            ina.calibrate(full_scale_current, 1.0),
            ConfigurationError::Calibration,
        );
        ina.release().done();
    }
}

#[test]
fn calibration_required_operations_panic_before_i2c() {
    let i2c = mock(&[]);
    let mut ina = Ina228::new(i2c, ADDR).unwrap();

    assert_panics_with("call calibrate() before reading current", || {
        let _ = ina.current();
    });
    assert_panics_with("call calibrate() before reading power", || {
        let _ = ina.power();
    });
    assert_panics_with("call calibrate() before reading accumulators", || {
        let _ = ina.take_accumulator_snapshot();
    });
    assert_panics_with("call calibrate() before setting power limit", || {
        let _ = ina.set_power_limit(1.0);
    });

    ina.release().done();
}

#[test]
fn bus_voltage_known_value() {
    // 12.0V / 195.3125e-6 = 61440 (raw 20-bit)
    // In 24-bit register: 61440 << 4 = 983040 = 0x0F_0000
    let i2c = mock(&[read_txn(0x05, &u24_bytes(61440 << 4))]);
    let mut ina = Ina228::new(i2c, ADDR).unwrap();
    let v = ina.bus_voltage().unwrap();
    assert!((v - 12.0).abs() < 0.001, "expected ~12.0V, got {v}");
    ina.release().done();
}

#[test]
fn bus_voltage_zero() {
    let i2c = mock(&[read_txn(0x05, &[0, 0, 0])]);
    let mut ina = Ina228::new(i2c, ADDR).unwrap();
    assert_eq!(ina.bus_voltage().unwrap(), 0.0);
    ina.release().done();
}

#[test]
fn shunt_voltage_positive() {
    // 0.001V / 312.5e-9 = 3200 (raw 20-bit)
    // In 24-bit register: 3200 << 4 = 51200 = 0x00C800
    let i2c = mock(&[read_txn(0x04, &u24_bytes(3200 << 4))]);
    let mut ina = Ina228::new(i2c, ADDR).unwrap();
    let v = ina.shunt_voltage().unwrap();
    assert!((v - 0.001).abs() < 1e-6, "expected ~0.001V, got {v}");
    ina.release().done();
}

#[test]
fn shunt_voltage_negative() {
    // -3200 in 20-bit two's complement, shifted left 4
    let raw_24 = (((-3200_i32) as u32) & 0xF_FFFF) << 4;
    let i2c = mock(&[read_txn(0x04, &u24_bytes(raw_24))]);
    let mut ina = Ina228::new(i2c, ADDR).unwrap();
    let v = ina.shunt_voltage().unwrap();
    assert!((v - (-0.001)).abs() < 1e-6, "expected ~-0.001V, got {v}");
    ina.release().done();
}

#[test]
fn current_positive() {
    // With 10A max: current_lsb = 10/524288
    // 5A / current_lsb = 262144 raw
    let shunt_cal = expected_shunt_cal(10.0, 0.01, false);
    let mut transactions = Vec::from(active_calibration_txns(shunt_cal, 0));
    transactions.push(read_txn(0x07, &u24_bytes(262144 << 4)));
    let i2c = mock(&transactions);
    let mut ina = Ina228::new(i2c, ADDR).unwrap();
    ina.calibrate(10.0, 0.01).unwrap();
    let current = ina.current().unwrap();
    assert!(
        (current - 5.0).abs() < 0.001,
        "expected ~5.0A, got {current}"
    );
    ina.release().done();
}

#[test]
fn current_negative() {
    let shunt_cal = expected_shunt_cal(10.0, 0.01, false);
    // -262144 in 20-bit two's complement, shifted left 4
    let raw_24 = (((-262144_i32) as u32) & 0xF_FFFF) << 4;
    let mut transactions = Vec::from(active_calibration_txns(shunt_cal, 0));
    transactions.push(read_txn(0x07, &u24_bytes(raw_24)));
    let i2c = mock(&transactions);
    let mut ina = Ina228::new(i2c, ADDR).unwrap();
    ina.calibrate(10.0, 0.01).unwrap();
    let current = ina.current().unwrap();
    assert!(
        (current - (-5.0)).abs() < 0.001,
        "expected ~-5.0A, got {current}"
    );
    ina.release().done();
}

#[test]
fn power_read() {
    let shunt_cal = expected_shunt_cal(10.0, 0.01, false);
    let current_lsb = 10.0_f32 / 524_288.0;

    // Power raw = power_w / (3.2 * current_lsb)
    let power_w = 60.0_f32;
    let raw_24 = (power_w / (3.2 * current_lsb)) as u32;
    let mut transactions = Vec::from(active_calibration_txns(shunt_cal, 0));
    transactions.push(read_txn(0x08, &u24_bytes(raw_24)));
    let i2c = mock(&transactions);
    let mut ina = Ina228::new(i2c, ADDR).unwrap();
    ina.calibrate(10.0, 0.01).unwrap();
    let p = ina.power().unwrap();
    assert!((p - power_w).abs() < 0.1, "expected ~{power_w}W, got {p}");
    ina.release().done();
}

#[test]
fn accumulator_snapshot_reports_values_and_overflows() {
    let shunt_cal = expected_shunt_cal(10.0, 0.01, false);
    let diagnostic_bits: u16 = (1 << 11) | (1 << 10) | (1 << 1) | 1;
    let mut transactions = Vec::from(active_calibration_txns(shunt_cal, 0));
    transactions.extend(continuous_snapshot_txns(
        DEFAULT_ADC_CONFIG,
        diagnostic_bits,
        10_240,
        524_288,
    ));
    let i2c = mock(&transactions);
    let mut ina = Ina228::new(i2c, ADDR).unwrap();
    ina.calibrate(10.0, 0.01).unwrap();
    let snapshot = ina.take_accumulator_snapshot().unwrap();
    assert_eq!(snapshot.energy_joules, 10.0);
    assert_eq!(snapshot.charge_coulombs, 10.0);
    assert!(snapshot.diagnostic_flags.memory_ok);
    assert!(snapshot.diagnostic_flags.conversion_ready);
    assert!(snapshot.diagnostic_flags.energy_overflow);
    assert!(snapshot.diagnostic_flags.charge_overflow);
    ina.release().done();

    let negative_charge_raw = ((-524_288_i64) as u64) & 0xFF_FFFF_FFFF;
    let mut transactions = Vec::from(active_calibration_txns(shunt_cal, 0));
    transactions.extend(continuous_snapshot_txns(
        DEFAULT_ADC_CONFIG,
        1,
        0,
        negative_charge_raw,
    ));
    let i2c = mock(&transactions);
    let mut ina = Ina228::new(i2c, ADDR).unwrap();
    ina.calibrate(10.0, 0.01).unwrap();
    let snapshot = ina.take_accumulator_snapshot().unwrap();
    assert_eq!(snapshot.energy_joules, 0.0);
    assert_eq!(snapshot.charge_coulombs, -10.0);
    assert!(!snapshot.diagnostic_flags.energy_overflow);
    assert!(!snapshot.diagnostic_flags.charge_overflow);
    ina.release().done();
}

#[test]
fn accumulator_snapshot_requires_continuous_mode() {
    let shunt_cal = expected_shunt_cal(10.0, 0.01, false);

    for mode in 0_u16..=15 {
        let adc_config = (mode << 12) | SHUTDOWN_ADC_CONFIG;
        let mut transactions = Vec::from(active_calibration_txns(shunt_cal, 0));
        if mode >= 9 {
            transactions.extend(continuous_snapshot_txns(adc_config, 1, 0, 0));
        } else {
            transactions.push(read_txn(0x01, &adc_config.to_be_bytes()));
        }
        let i2c = mock(&transactions);
        let mut ina = Ina228::new(i2c, ADDR).unwrap();
        ina.calibrate(10.0, 0.01).unwrap();

        let result = ina.take_accumulator_snapshot();
        if mode >= 9 {
            let snapshot = result.unwrap();
            assert_eq!(snapshot.energy_joules, 0.0);
            assert_eq!(snapshot.charge_coulombs, 0.0);
            assert!(snapshot.diagnostic_flags.memory_ok);
        } else {
            assert!(matches!(
                result,
                Err(DriverError::InvalidConfiguration(
                    ConfigurationError::AccumulatorMode
                ))
            ));
        }
        ina.release().done();
    }
}

#[test]
fn accumulator_snapshot_failures_leave_conversions_suspended() {
    let shunt_cal = expected_shunt_cal(10.0, 0.01, false);
    let mut transactions = Vec::from(active_calibration_txns(shunt_cal, 0));
    transactions.extend([
        read_txn(0x01, &DEFAULT_ADC_CONFIG.to_be_bytes()),
        write_txn(0x01, SHUTDOWN_ADC_CONFIG),
        read_txn(0x0B, &[0, 0]).with_error(ErrorKind::Bus),
        write_txn(0x01, DEFAULT_ADC_CONFIG),
    ]);
    let i2c = mock(&transactions);
    let mut ina = Ina228::new(i2c, ADDR).unwrap();
    ina.calibrate(10.0, 0.01).unwrap();

    assert!(matches!(
        ina.take_accumulator_snapshot(),
        Err(DriverError::I2c(ErrorKind::Bus))
    ));
    ina.configure(AdcConfig::default()).unwrap();
    ina.release().done();

    let failed_restore =
        Transaction::write(ADDR, vec![0x01, 0xFB, 0x68]).with_error(ErrorKind::Bus);
    let mut transactions = Vec::from(active_calibration_txns(shunt_cal, 0));
    transactions.extend([
        read_txn(0x01, &DEFAULT_ADC_CONFIG.to_be_bytes()),
        write_txn(0x01, SHUTDOWN_ADC_CONFIG),
        read_txn(0x0B, &1_u16.to_be_bytes()),
        read_txn(0x09, &u40_bytes(0)),
        read_txn(0x0A, &u40_bytes(0)),
        failed_restore,
        write_txn(0x01, DEFAULT_ADC_CONFIG),
    ]);
    let i2c = mock(&transactions);
    let mut ina = Ina228::new(i2c, ADDR).unwrap();
    ina.calibrate(10.0, 0.01).unwrap();

    assert!(matches!(
        ina.take_accumulator_snapshot(),
        Err(DriverError::I2c(ErrorKind::Bus))
    ));
    ina.configure(AdcConfig::default()).unwrap();
    ina.release().done();
}

#[test]
fn die_temperature_positive() {
    // 25C / 7.8125e-3 = 3200
    let i2c = mock(&[read_txn(0x06, &3200_u16.to_be_bytes())]);
    let mut ina = Ina228::new(i2c, ADDR).unwrap();
    let t = ina.die_temperature().unwrap();
    assert!((t - 25.0).abs() < 0.01, "expected ~25.0C, got {t}");
    ina.release().done();
}

#[test]
fn die_temperature_negative() {
    // -10C / 7.8125e-3 = -1280 -> as u16 = 64256 (0xFB00)
    let i2c = mock(&[read_txn(0x06, &((-1280_i16) as u16).to_be_bytes())]);
    let mut ina = Ina228::new(i2c, ADDR).unwrap();
    let t = ina.die_temperature().unwrap();
    assert!((t - (-10.0)).abs() < 0.01, "expected ~-10.0C, got {t}");
    ina.release().done();
}

#[test]
fn set_temp_compensation() {
    let i2c = mock(&[
        read_txn(0x00, &0x0000_u16.to_be_bytes()),
        read_txn(0x01, &DEFAULT_ADC_CONFIG.to_be_bytes()),
        write_txn(0x01, SHUTDOWN_ADC_CONFIG),
        write_txn(0x03, 15),
        write_txn(0x00, 1 << 5),
        write_txn(0x01, DEFAULT_ADC_CONFIG),
        read_txn(0x00, &(1_u16 << 5).to_be_bytes()),
        read_txn(0x01, &DEFAULT_ADC_CONFIG.to_be_bytes()),
        write_txn(0x01, SHUTDOWN_ADC_CONFIG),
        write_txn(0x03, 0x3FFF),
        write_txn(0x00, 1 << 5),
        write_txn(0x01, DEFAULT_ADC_CONFIG),
    ]);
    let mut ina = Ina228::new(i2c, ADDR).unwrap();
    ina.set_temp_compensation(15).unwrap();
    ina.set_temp_compensation(0x3FFF).unwrap();
    ina.release().done();
}

#[test]
fn set_temp_compensation_write_failures_are_safe() {
    let failed_shutdown =
        Transaction::write(ADDR, vec![0x01, 0x0B, 0x68]).with_error(ErrorKind::Bus);
    let i2c = mock(&[
        read_txn(0x00, &0x0000_u16.to_be_bytes()),
        read_txn(0x01, &DEFAULT_ADC_CONFIG.to_be_bytes()),
        failed_shutdown,
    ]);
    let mut ina = Ina228::new(i2c, ADDR).unwrap();
    assert_eq!(
        ina.set_temp_compensation(15),
        Err(DriverError::I2c(ErrorKind::Bus))
    );
    ina.release().done();

    let failed_coefficient =
        Transaction::write(ADDR, vec![0x03, 0x00, 0x0F]).with_error(ErrorKind::Bus);
    let i2c = mock(&[
        read_txn(0x00, &0x0000_u16.to_be_bytes()),
        read_txn(0x01, &DEFAULT_ADC_CONFIG.to_be_bytes()),
        write_txn(0x01, SHUTDOWN_ADC_CONFIG),
        failed_coefficient,
    ]);
    let mut ina = Ina228::new(i2c, ADDR).unwrap();
    assert_eq!(
        ina.set_temp_compensation(15),
        Err(DriverError::I2c(ErrorKind::Bus))
    );
    ina.release().done();

    let failed_enable = Transaction::write(ADDR, vec![0x00, 0x00, 0x20]).with_error(ErrorKind::Bus);
    let i2c = mock(&[
        read_txn(0x00, &0x0000_u16.to_be_bytes()),
        read_txn(0x01, &DEFAULT_ADC_CONFIG.to_be_bytes()),
        write_txn(0x01, SHUTDOWN_ADC_CONFIG),
        write_txn(0x03, 15),
        failed_enable,
    ]);
    let mut ina = Ina228::new(i2c, ADDR).unwrap();
    assert_eq!(
        ina.set_temp_compensation(15),
        Err(DriverError::I2c(ErrorKind::Bus))
    );
    ina.release().done();

    let failed_restore =
        Transaction::write(ADDR, vec![0x01, 0xFB, 0x68]).with_error(ErrorKind::Bus);
    let i2c = mock(&[
        read_txn(0x00, &0x0000_u16.to_be_bytes()),
        read_txn(0x01, &DEFAULT_ADC_CONFIG.to_be_bytes()),
        write_txn(0x01, SHUTDOWN_ADC_CONFIG),
        write_txn(0x03, 15),
        write_txn(0x00, 1 << 5),
        failed_restore,
        write_txn(0x01, DEFAULT_ADC_CONFIG),
    ]);
    let mut ina = Ina228::new(i2c, ADDR).unwrap();
    assert_eq!(
        ina.set_temp_compensation(15),
        Err(DriverError::I2c(ErrorKind::Bus))
    );
    ina.configure(AdcConfig::default()).unwrap();
    ina.release().done();
}

#[test]
fn reset_accumulators() {
    let i2c = mock(&[
        read_txn(0x00, &0x0000_u16.to_be_bytes()), // read CONFIG
        write_txn(0x00, 1 << 14),                  // set RSTACC bit
    ]);
    let mut ina = Ina228::new(i2c, ADDR).unwrap();
    ina.reset_accumulators().unwrap();
    ina.release().done();
}

#[test]
fn manufacturer_id() {
    let i2c = mock(&[read_txn(0x3E, &0x5449_u16.to_be_bytes())]);
    let mut ina = Ina228::new(i2c, ADDR).unwrap();
    assert_eq!(ina.manufacturer_id().unwrap(), 0x5449);
    ina.release().done();
}

#[test]
fn device_id() {
    // Register returns 0x2281 (device=0x228, revision=1)
    let i2c = mock(&[read_txn(0x3F, &0x2281_u16.to_be_bytes())]);
    let mut ina = Ina228::new(i2c, ADDR).unwrap();
    assert_eq!(ina.device_id().unwrap(), 0x228);
    ina.release().done();
}

#[test]
fn disable_temp_compensation() {
    let i2c = mock(&[
        read_txn(0x00, &0x0020_u16.to_be_bytes()),
        read_txn(0x01, &DEFAULT_ADC_CONFIG.to_be_bytes()),
        write_txn(0x01, SHUTDOWN_ADC_CONFIG),
        write_txn(0x00, 0x0000),
        write_txn(0x01, DEFAULT_ADC_CONFIG),
    ]);
    let mut ina = Ina228::new(i2c, ADDR).unwrap();
    ina.disable_temp_compensation().unwrap();
    ina.release().done();
}

#[test]
fn temperature_compensation_preserves_shutdown_modes() {
    for adc_config in [SHUTDOWN_ADC_CONFIG, SHUTDOWN_ALT_ADC_CONFIG] {
        let i2c = mock(&[
            read_txn(0x00, &0x0000_u16.to_be_bytes()),
            read_txn(0x01, &adc_config.to_be_bytes()),
            write_txn(0x03, 15),
            write_txn(0x00, 1 << 5),
            read_txn(0x00, &(1_u16 << 5).to_be_bytes()),
            read_txn(0x01, &adc_config.to_be_bytes()),
            write_txn(0x00, 0),
        ]);
        let mut ina = Ina228::new(i2c, ADDR).unwrap();
        ina.set_temp_compensation(15).unwrap();
        ina.disable_temp_compensation().unwrap();
        ina.release().done();
    }
}

#[test]
fn diagnostic_flags_only_memory_ok() {
    let i2c = mock(&[read_txn(0x0B, &0x0001_u16.to_be_bytes())]);
    let mut ina = Ina228::new(i2c, ADDR).unwrap();
    let flags = ina.take_diagnostic_flags().unwrap();
    assert!(flags.memory_ok);
    assert!(!flags.conversion_ready);
    assert!(!flags.energy_overflow);
    assert!(!flags.math_overflow);
    assert!(!flags.temp_over_limit);
    assert!(!flags.shunt_over_limit);
    assert!(!flags.shunt_under_limit);
    assert!(!flags.bus_over_limit);
    assert!(!flags.bus_under_limit);
    assert!(!flags.power_over_limit);
    assert!(!flags.charge_overflow);
    ina.release().done();
}

#[test]
fn diagnostic_flags_alerts_set() {
    // Set reserved bit 8 alongside TMPOL(7), BUSOL(4), and CNVRF(1).
    let diag: u16 = (1 << 8) | (1 << 7) | (1 << 4) | (1 << 1);
    let i2c = mock(&[read_txn(0x0B, &diag.to_be_bytes())]);
    let mut ina = Ina228::new(i2c, ADDR).unwrap();
    let flags = ina.take_diagnostic_flags().unwrap();
    assert!(!flags.memory_ok);
    assert!(flags.temp_over_limit);
    assert!(flags.bus_over_limit);
    assert!(flags.conversion_ready);
    assert!(!flags.energy_overflow);
    assert!(!flags.math_overflow);
    assert!(!flags.shunt_over_limit);
    assert!(!flags.shunt_under_limit);
    assert!(!flags.bus_under_limit);
    assert!(!flags.power_over_limit);
    assert!(!flags.charge_overflow);
    ina.release().done();
}

#[test]
fn configure_alerts_encodes_each_control_bit() {
    let cases = [
        (AlertConfig::default(), 0),
        (
            AlertConfig {
                latch: true,
                ..Default::default()
            },
            1 << 15,
        ),
        (
            AlertConfig {
                conversion_ready: true,
                ..Default::default()
            },
            1 << 14,
        ),
        (
            AlertConfig {
                slow_alert: true,
                ..Default::default()
            },
            1 << 13,
        ),
        (
            AlertConfig {
                active_high: true,
                ..Default::default()
            },
            1 << 12,
        ),
    ];
    let transactions: Vec<_> = cases
        .iter()
        .map(|(_, value)| write_txn(0x0B, *value))
        .collect();
    let i2c = mock(&transactions);
    let mut ina = Ina228::new(i2c, ADDR).unwrap();
    for (config, _) in cases {
        ina.configure_alerts(config).unwrap();
    }
    ina.release().done();
}

#[test]
fn set_shunt_overvoltage_limit() {
    // 10000.75 LSB rounds to 10001.
    let voltage = 10_000.75_f32 * 5.0e-6;
    let expected_raw = 10_001;
    let i2c = mock(&[write_txn(0x0C, expected_raw)]);
    let mut ina = Ina228::new(i2c, ADDR).unwrap();
    ina.set_shunt_overvoltage_limit(voltage).unwrap();
    ina.release().done();
}

#[test]
fn set_shunt_undervoltage_limit() {
    // -10000.75 LSB rounds to -10001.
    let voltage = -10_000.75_f32 * 5.0e-6;
    let expected_raw = (-10_001_i16) as u16;
    let i2c = mock(&[write_txn(0x0D, expected_raw)]);
    let mut ina = Ina228::new(i2c, ADDR).unwrap();
    ina.set_shunt_undervoltage_limit(voltage).unwrap();
    ina.release().done();
}

#[test]
fn set_bus_overvoltage_limit() {
    // 15360.75 LSB rounds to 15361; 32767 is the 15-bit register maximum.
    let rounded_voltage = 15_360.75_f32 * 3.125e-3;
    let maximum_voltage = 32_767.0_f32 * 3.125e-3;
    let i2c = mock(&[write_txn(0x0E, 15_361), write_txn(0x0E, 0x7FFF)]);
    let mut ina = Ina228::new(i2c, ADDR).unwrap();
    ina.set_bus_overvoltage_limit(rounded_voltage).unwrap();
    ina.set_bus_overvoltage_limit(maximum_voltage).unwrap();
    ina.release().done();
}

#[test]
fn set_bus_undervoltage_limit() {
    // 3.0V / 3.125mV = 960
    let expected_raw = (3.0_f32 / 3.125e-3).round() as u16;
    let i2c = mock(&[write_txn(0x0F, expected_raw)]);
    let mut ina = Ina228::new(i2c, ADDR).unwrap();
    ina.set_bus_undervoltage_limit(3.0).unwrap();
    ina.release().done();
}

#[test]
fn set_temperature_limit() {
    // 80C / 7.8125e-3 = 10240
    let expected_raw = (80.0_f32 / 7.8125e-3).round() as i16 as u16;
    let i2c = mock(&[write_txn(0x10, expected_raw)]);
    let mut ina = Ina228::new(i2c, ADDR).unwrap();
    ina.set_temperature_limit(80.0).unwrap();
    ina.release().done();
}

#[test]
fn set_power_limit() {
    let shunt_cal = expected_shunt_cal(10.0, 0.01, false);
    let current_lsb = 10.0_f32 / 524_288.0;
    let power_lsb = 3.2 * current_lsb;
    // 100W / (256 * power_lsb)
    let expected_raw = (100.0_f32 / (256.0 * power_lsb)).round() as u16;
    let maximum_power = u16::MAX as f32 * 256.0 * power_lsb;

    let mut transactions = Vec::from(active_calibration_txns(shunt_cal, 0));
    transactions.extend([write_txn(0x11, expected_raw), write_txn(0x11, u16::MAX)]);
    let i2c = mock(&transactions);
    let mut ina = Ina228::new(i2c, ADDR).unwrap();
    ina.calibrate(10.0, 0.01).unwrap();
    ina.set_power_limit(100.0).unwrap();
    ina.set_power_limit(maximum_power).unwrap();
    ina.release().done();
}

#[test]
fn calibrate_resets_existing_power_limit() {
    let initial_shunt_cal = expected_shunt_cal(10.0, 0.01, false);
    let replacement_shunt_cal = expected_shunt_cal(5.0, 0.01, false);
    let initial_current_lsb = 10.0_f32 / 524_288.0;
    let initial_limit = (100.0_f32 / (256.0 * 3.2 * initial_current_lsb)).round() as u16;
    let mut transactions = Vec::from(active_calibration_txns(initial_shunt_cal, 0));
    transactions.push(write_txn(0x11, initial_limit));
    transactions.extend(active_calibration_txns(replacement_shunt_cal, 0));
    let i2c = mock(&transactions);
    let mut ina = Ina228::new(i2c, ADDR).unwrap();

    ina.calibrate(10.0, 0.01).unwrap();
    ina.set_power_limit(100.0).unwrap();
    ina.calibrate(5.0, 0.01).unwrap();
    ina.release().done();
}

#[test]
fn physical_limit_setters_reject_invalid_values_before_i2c() {
    let i2c = mock(&[]);
    let mut ina = Ina228::new(i2c, ADDR).unwrap();

    for voltage in [f32::NAN, -0.2, 0.2] {
        assert_configuration_error(
            ina.set_shunt_overvoltage_limit(voltage),
            ConfigurationError::ShuntVoltageLimit,
        );
    }
    for voltage in [f32::INFINITY, -0.001, 102.4] {
        assert_configuration_error(
            ina.set_bus_undervoltage_limit(voltage),
            ConfigurationError::BusVoltageLimit,
        );
    }
    for temperature in [f32::NEG_INFINITY, -256.01, 256.0] {
        assert_configuration_error(
            ina.set_temperature_limit(temperature),
            ConfigurationError::TemperatureLimit,
        );
    }

    ina.release().done();
}

#[test]
fn set_power_limit_rejects_invalid_values_before_i2c() {
    let shunt_cal = expected_shunt_cal(10.0, 0.01, false);
    let transactions = active_calibration_txns(shunt_cal, 0);
    let i2c = mock(&transactions);
    let mut ina = Ina228::new(i2c, ADDR).unwrap();
    ina.calibrate(10.0, 0.01).unwrap();

    for power in [f32::NAN, -1.0, 1024.0] {
        assert_configuration_error(ina.set_power_limit(power), ConfigurationError::PowerLimit);
    }

    ina.release().done();
}

#[test]
fn die_revision() {
    // Register returns 0x2285 (device=0x228, revision=5)
    let i2c = mock(&[read_txn(0x3F, &0x2285_u16.to_be_bytes())]);
    let mut ina = Ina228::new(i2c, ADDR).unwrap();
    assert_eq!(ina.die_revision().unwrap(), 5);
    ina.release().done();
}

#[test]
fn shunt_voltage_uses_range_read_during_initialization() {
    // 0.001V / 78.125e-9 = 12800 (raw 20-bit)
    let i2c = mock_with_config(1 << 4, &[read_txn(0x04, &u24_bytes(12800 << 4))]);
    let mut ina = Ina228::new(i2c, ADDR).unwrap();
    let v = ina.shunt_voltage().unwrap();
    assert!((v - 0.001).abs() < 1e-6, "expected ~0.001V, got {v}");
    ina.release().done();
}

#[test]
fn set_adc_range_after_calibrate_recalibrates() {
    let shunt_cal_163mv = expected_shunt_cal(4.0, 0.01, false);
    let shunt_cal_40mv = expected_shunt_cal(4.0, 0.01, true);

    let mut transactions = Vec::from(active_calibration_txns(shunt_cal_163mv, 0));
    transactions.extend([
        read_txn(0x00, &0x0000_u16.to_be_bytes()),
        read_txn(0x01, &DEFAULT_ADC_CONFIG.to_be_bytes()),
        write_txn(0x01, SHUTDOWN_ADC_CONFIG),
        write_txn(0x0C, i16::MAX as u16),
        write_txn(0x0D, i16::MIN as u16),
        write_txn(0x00, 1 << 4),
        write_txn(0x02, shunt_cal_40mv),
        write_txn(0x01, DEFAULT_ADC_CONFIG),
    ]);
    let i2c = mock(&transactions);
    let mut ina = Ina228::new(i2c, ADDR).unwrap();
    ina.calibrate(4.0, 0.01).unwrap();
    ina.set_adc_range(AdcRange::Range40mV).unwrap();
    ina.release().done();
}

#[test]
fn set_adc_range_restore_failure_keeps_new_range_and_calibration() {
    let shunt_cal_163mv = expected_shunt_cal(4.0, 0.01, false);
    let shunt_cal_40mv = expected_shunt_cal(4.0, 0.01, true);
    let mut transactions = Vec::from(active_calibration_txns(shunt_cal_163mv, 0));
    transactions.extend([
        read_txn(0x00, &0x0000_u16.to_be_bytes()),
        read_txn(0x01, &DEFAULT_ADC_CONFIG.to_be_bytes()),
        write_txn(0x01, SHUTDOWN_ADC_CONFIG),
        write_txn(0x0C, i16::MAX as u16),
        write_txn(0x0D, i16::MIN as u16),
        write_txn(0x00, 1 << 4),
        write_txn(0x02, shunt_cal_40mv),
        Transaction::write(ADDR, vec![0x01, 0xFB, 0x68]).with_error(ErrorKind::Bus),
        read_txn(0x07, &u24_bytes(262144 << 4)),
        write_txn(0x0C, 8000),
        write_txn(0x01, DEFAULT_ADC_CONFIG),
    ]);
    let i2c = mock(&transactions);
    let mut ina = Ina228::new(i2c, ADDR).unwrap();
    ina.calibrate(4.0, 0.01).unwrap();

    assert_eq!(
        ina.set_adc_range(AdcRange::Range40mV),
        Err(DriverError::I2c(ErrorKind::Bus))
    );
    assert_eq!(ina.current().unwrap(), 2.0);
    ina.set_shunt_overvoltage_limit(0.01).unwrap();
    ina.configure(AdcConfig::default()).unwrap();
    ina.release().done();
}

#[test]
fn set_adc_range_config_error_invalidates_scale_state_until_readback() {
    let shunt_cal = expected_shunt_cal(4.0, 0.01, false);
    let mut transactions = Vec::from(active_calibration_txns(shunt_cal, 0));
    transactions.extend([
        read_txn(0x00, &0x0000_u16.to_be_bytes()),
        read_txn(0x01, &DEFAULT_ADC_CONFIG.to_be_bytes()),
        write_txn(0x01, SHUTDOWN_ADC_CONFIG),
        write_txn(0x0C, i16::MAX as u16),
        write_txn(0x0D, i16::MIN as u16),
        Transaction::write(ADDR, vec![0x00, 0x00, 0x10]).with_error(ErrorKind::Bus),
        read_txn(0x00, &(1_u16 << 4).to_be_bytes()),
        write_txn(0x01, DEFAULT_ADC_CONFIG),
        read_txn(0x0B, &(1_u16 << 1).to_be_bytes()),
        read_txn(0x01, &DEFAULT_ADC_CONFIG.to_be_bytes()),
        read_txn(0x04, &u24_bytes(12800 << 4)),
    ]);
    let i2c = mock(&transactions);
    let mut ina = Ina228::new(i2c, ADDR).unwrap();
    ina.calibrate(4.0, 0.01).unwrap();

    assert!(ina.set_adc_range(AdcRange::Range40mV).is_err());
    assert_panics_with("call calibrate() before reading current", || {
        let _ = ina.current();
    });
    assert_eq!(ina.shunt_voltage(), Err(DriverError::AdcRangeUnknown));
    assert_eq!(
        ina.set_shunt_overvoltage_limit(0.01),
        Err(DriverError::AdcRangeUnknown)
    );

    ina.set_adc_range(AdcRange::Range40mV).unwrap();
    ina.configure(AdcConfig::default()).unwrap();
    assert_eq!(ina.shunt_voltage(), Err(DriverError::ShuntVoltageStale));
    assert!(ina.take_diagnostic_flags().unwrap().conversion_ready);
    let shunt_voltage = ina.shunt_voltage().unwrap();
    assert!(
        (shunt_voltage - 0.001).abs() < 1e-6,
        "expected 0.001V in the synchronized 40mV range, got {shunt_voltage}"
    );
    ina.release().done();
}

#[test]
fn set_adc_range_shunt_cal_error_invalidates_calibration() {
    let shunt_cal_163mv = expected_shunt_cal(4.0, 0.01, false);
    let shunt_cal_40mv = expected_shunt_cal(4.0, 0.01, true);
    let shunt_cal_40mv_bytes = shunt_cal_40mv.to_be_bytes();
    let mut transactions = Vec::from(active_calibration_txns(shunt_cal_163mv, 0));
    transactions.extend([
        read_txn(0x00, &0x0000_u16.to_be_bytes()),
        read_txn(0x01, &DEFAULT_ADC_CONFIG.to_be_bytes()),
        write_txn(0x01, SHUTDOWN_ADC_CONFIG),
        write_txn(0x0C, i16::MAX as u16),
        write_txn(0x0D, i16::MIN as u16),
        write_txn(0x00, 1 << 4),
        Transaction::write(
            ADDR,
            vec![0x02, shunt_cal_40mv_bytes[0], shunt_cal_40mv_bytes[1]],
        )
        .with_error(ErrorKind::Bus),
    ]);
    let i2c = mock(&transactions);
    let mut ina = Ina228::new(i2c, ADDR).unwrap();
    ina.calibrate(4.0, 0.01).unwrap();

    assert!(ina.set_adc_range(AdcRange::Range40mV).is_err());
    assert_eq!(ina.shunt_voltage(), Err(DriverError::ShuntVoltageStale));
    assert_panics_with("call calibrate() before reading current", || {
        let _ = ina.current();
    });
    ina.release().done();
}

#[test]
fn set_adc_range_rejects_incompatible_calibration_without_i2c() {
    let shunt_cal = expected_shunt_cal(10.0, 0.01, false);
    let mut transactions = Vec::from(active_calibration_txns(shunt_cal, 0));
    transactions.extend([
        read_txn(0x07, &u24_bytes(262144 << 4)),
        read_txn(0x04, &u24_bytes(3200 << 4)),
    ]);
    let i2c = mock(&transactions);
    let mut ina = Ina228::new(i2c, ADDR).unwrap();
    ina.calibrate(10.0, 0.01).unwrap();

    assert_configuration_error(
        ina.set_adc_range(AdcRange::Range40mV),
        ConfigurationError::Calibration,
    );
    assert_eq!(ina.current().unwrap(), 5.0);
    let shunt_voltage = ina.shunt_voltage().unwrap();
    assert!(
        (shunt_voltage - 0.001).abs() < 1e-6,
        "expected 0.001V in the preserved 163mV range, got {shunt_voltage}"
    );
    ina.release().done();
}

#[test]
fn i2c_error_propagates() {
    let i2c =
        mock(
            &[Transaction::write_read(ADDR, vec![0x05], vec![0, 0, 0]).with_error(ErrorKind::Bus)],
        );
    let mut ina = Ina228::new(i2c, ADDR).unwrap();
    let result = ina.bus_voltage();
    assert_eq!(result, Err(DriverError::I2c(ErrorKind::Bus)));
    ina.release().done();
}

#[test]
fn calibrate_preserves_shutdown_modes() {
    let shunt_cal = expected_shunt_cal(10.0, 0.01, false);

    for adc_config in [SHUTDOWN_ADC_CONFIG, SHUTDOWN_ALT_ADC_CONFIG] {
        let i2c = mock(&[
            read_txn(0x01, &adc_config.to_be_bytes()),
            write_txn(0x02, shunt_cal),
            read_txn(0x00, &0x0000_u16.to_be_bytes()),
            write_txn(0x00, 1 << 14),
            write_txn(0x11, u16::MAX),
        ]);
        let mut ina = Ina228::new(i2c, ADDR).unwrap();
        ina.calibrate(10.0, 0.01).unwrap();
        ina.release().done();
    }
}

#[test]
fn calibrate_failures_leave_safe_state() {
    let shunt_cal_ok = expected_shunt_cal(10.0, 0.01, false);
    let shunt_cal_fail = expected_shunt_cal(2.0, 0.04, false);
    let fail_bytes = shunt_cal_fail.to_be_bytes();
    let mut transactions = Vec::from(active_calibration_txns(shunt_cal_ok, 0));
    transactions.extend([
        read_txn(0x01, &DEFAULT_ADC_CONFIG.to_be_bytes()),
        write_txn(0x01, SHUTDOWN_ADC_CONFIG),
        Transaction::write(ADDR, vec![0x02, fail_bytes[0], fail_bytes[1]])
            .with_error(ErrorKind::Bus),
    ]);
    let i2c = mock(&transactions);
    let mut ina = Ina228::new(i2c, ADDR).unwrap();
    ina.calibrate(10.0, 0.01).unwrap();

    assert_eq!(
        ina.calibrate(2.0, 0.04),
        Err(DriverError::I2c(ErrorKind::Bus))
    );
    assert_panics_with("call calibrate() before reading current", || {
        let _ = ina.current();
    });
    ina.release().done();

    let mut transactions = Vec::from(active_calibration_txns(shunt_cal_ok, 0));
    transactions.extend([
        read_txn(0x01, &DEFAULT_ADC_CONFIG.to_be_bytes()),
        write_txn(0x01, SHUTDOWN_ADC_CONFIG),
        write_txn(0x02, shunt_cal_fail),
        read_txn(0x00, &0x0000_u16.to_be_bytes()).with_error(ErrorKind::Bus),
    ]);
    let i2c = mock(&transactions);
    let mut ina = Ina228::new(i2c, ADDR).unwrap();
    ina.calibrate(10.0, 0.01).unwrap();

    assert_eq!(
        ina.calibrate(2.0, 0.04),
        Err(DriverError::I2c(ErrorKind::Bus))
    );
    assert_panics_with("call calibrate() before reading current", || {
        let _ = ina.current();
    });
    ina.release().done();

    let mut transactions = Vec::from(active_calibration_txns(shunt_cal_ok, 0));
    transactions.extend([
        read_txn(0x01, &DEFAULT_ADC_CONFIG.to_be_bytes()),
        write_txn(0x01, SHUTDOWN_ADC_CONFIG),
        write_txn(0x02, shunt_cal_fail),
        read_txn(0x00, &0x0000_u16.to_be_bytes()),
        Transaction::write(ADDR, vec![0x00, 0x40, 0x00]).with_error(ErrorKind::Bus),
    ]);
    transactions.push(write_txn(0x01, DEFAULT_ADC_CONFIG));
    transactions.extend(active_calibration_txns(shunt_cal_fail, 0));
    transactions.push(read_txn(0x07, &u24_bytes(262144 << 4)));
    let i2c = mock(&transactions);
    let mut ina = Ina228::new(i2c, ADDR).unwrap();
    ina.calibrate(10.0, 0.01).unwrap();

    assert_eq!(
        ina.calibrate(2.0, 0.04),
        Err(DriverError::I2c(ErrorKind::Bus))
    );
    assert_panics_with("call calibrate() before reading current", || {
        let _ = ina.current();
    });
    ina.configure(AdcConfig::default()).unwrap();
    ina.calibrate(2.0, 0.04).unwrap();
    let current = ina.current().unwrap();
    assert!(
        (current - 1.0).abs() < 0.001,
        "expected ~1.0A (replacement calibration), got {current}"
    );
    ina.release().done();

    let failed_power_limit =
        Transaction::write(ADDR, vec![0x11, 0xFF, 0xFF]).with_error(ErrorKind::Bus);
    let mut transactions = Vec::from(active_calibration_txns(shunt_cal_ok, 0));
    transactions.extend([
        read_txn(0x01, &DEFAULT_ADC_CONFIG.to_be_bytes()),
        write_txn(0x01, SHUTDOWN_ADC_CONFIG),
        write_txn(0x02, shunt_cal_fail),
        read_txn(0x00, &0x0000_u16.to_be_bytes()),
        write_txn(0x00, 1 << 14),
        failed_power_limit,
        write_txn(0x01, DEFAULT_ADC_CONFIG),
    ]);
    let i2c = mock(&transactions);
    let mut ina = Ina228::new(i2c, ADDR).unwrap();
    ina.calibrate(10.0, 0.01).unwrap();

    assert_eq!(
        ina.calibrate(2.0, 0.04),
        Err(DriverError::I2c(ErrorKind::Bus))
    );
    assert_panics_with("call calibrate() before reading current", || {
        let _ = ina.current();
    });
    ina.configure(AdcConfig::default()).unwrap();
    ina.release().done();

    let failed_restore =
        Transaction::write(ADDR, vec![0x01, 0xFB, 0x68]).with_error(ErrorKind::Bus);
    let i2c = mock(&[
        read_txn(0x01, &DEFAULT_ADC_CONFIG.to_be_bytes()),
        write_txn(0x01, SHUTDOWN_ADC_CONFIG),
        write_txn(0x02, shunt_cal_fail),
        read_txn(0x00, &0x0000_u16.to_be_bytes()),
        write_txn(0x00, 1 << 14),
        write_txn(0x11, u16::MAX),
        failed_restore,
        read_txn(0x07, &u24_bytes(262144 << 4)),
        write_txn(0x01, DEFAULT_ADC_CONFIG),
    ]);
    let mut ina = Ina228::new(i2c, ADDR).unwrap();
    assert_eq!(
        ina.calibrate(2.0, 0.04),
        Err(DriverError::I2c(ErrorKind::Bus))
    );
    let current = ina.current().unwrap();
    assert!(
        (current - 1.0).abs() < 0.001,
        "expected ~1.0A (replacement calibration), got {current}"
    );
    ina.configure(AdcConfig::default()).unwrap();
    ina.release().done();
}

#[test]
fn diagnostic_flags_overflow_and_under_limits() {
    // Set ENERGYOF(11), CHARGEOF(10), MATHOF(9), SHUNTUL(5), BUSUL(3), MEMSTAT(0).
    let diag: u16 = (1 << 11) | (1 << 10) | (1 << 9) | (1 << 5) | (1 << 3) | 1;
    let i2c = mock(&[read_txn(0x0B, &diag.to_be_bytes())]);
    let mut ina = Ina228::new(i2c, ADDR).unwrap();
    let flags = ina.take_diagnostic_flags().unwrap();
    assert!(flags.memory_ok);
    assert!(flags.energy_overflow);
    assert!(flags.math_overflow);
    assert!(flags.shunt_under_limit);
    assert!(flags.bus_under_limit);
    assert!(flags.charge_overflow);
    assert!(!flags.conversion_ready);
    assert!(!flags.temp_over_limit);
    assert!(!flags.shunt_over_limit);
    assert!(!flags.bus_over_limit);
    assert!(!flags.power_over_limit);
    ina.release().done();
}

#[test]
fn set_temp_compensation_rejects_above_14_bits_before_i2c() {
    let i2c = mock(&[]);
    let mut ina = Ina228::new(i2c, ADDR).unwrap();
    assert_configuration_error(
        ina.set_temp_compensation(0x4000),
        ConfigurationError::TemperatureCoefficient,
    );
    assert_configuration_error(
        ina.set_temp_compensation(u16::MAX),
        ConfigurationError::TemperatureCoefficient,
    );
    ina.release().done();
}
