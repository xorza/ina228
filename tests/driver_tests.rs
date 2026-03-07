use embedded_hal::i2c::ErrorKind;
use embedded_hal_mock::eh1::i2c::{Mock, Transaction};
use ina228::{AdcRange, AveragingCount, ConversionTime, DEFAULT_ADDRESS, Ina228, OperatingMode};

const ADDR: u8 = DEFAULT_ADDRESS;

/// Compute SHUNT_CAL the same way the driver does (f32 current_lsb, then f64 multiply).
fn expected_shunt_cal(max_current: f32, shunt_ohm: f32, adc_range_40mv: bool) -> u16 {
    let current_lsb = max_current / 524_288.0_f32;
    let mut shunt_cal = 13107.2e6 * current_lsb as f64 * shunt_ohm as f64;
    if adc_range_40mv {
        shunt_cal *= 4.0;
    }
    shunt_cal as u16 & 0x7FFF
}

fn write_txn(reg: u8, value: u16) -> Transaction {
    let bytes = value.to_be_bytes();
    Transaction::write(ADDR, vec![reg, bytes[0], bytes[1]])
}

fn read_txn(reg: u8, bytes: &[u8]) -> Transaction {
    Transaction::write_read(ADDR, vec![reg], bytes.to_vec())
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
    let i2c = Mock::new(&[]);
    let ina = Ina228::new(i2c, DEFAULT_ADDRESS);
    ina.release().done();
}

#[test]
fn new_max_address() {
    let i2c = Mock::new(&[]);
    let ina = Ina228::new(i2c, 0x4F);
    ina.release().done();
}

#[test]
#[should_panic(expected = "INA228 address must be in 0x40..=0x4F")]
fn new_invalid_address_low() {
    let i2c = Mock::new(&[]);
    Ina228::new(i2c, 0x39);
}

#[test]
#[should_panic(expected = "INA228 address must be in 0x40..=0x4F")]
fn new_invalid_address_high() {
    let i2c = Mock::new(&[]);
    Ina228::new(i2c, 0x50);
}

#[test]
fn reset() {
    let shunt_cal = expected_shunt_cal(10.0, 0.01, false);
    let i2c = Mock::new(&[
        // calibrate writes SHUNT_CAL
        write_txn(0x02, shunt_cal),
        // reset writes CONFIG
        write_txn(0x00, 1 << 15),
        // bus_voltage still works (no calibration needed)
        read_txn(0x05, &[0x00, 0x00, 0x00]),
    ]);
    let mut ina = Ina228::new(i2c, ADDR);
    ina.calibrate(10.0, 0.01).unwrap();
    ina.reset().unwrap();
    // bus_voltage should still work after reset (doesn't need calibration)
    assert_eq!(ina.bus_voltage().unwrap(), 0.0);
    ina.release().done();
}

#[test]
fn configure_continuous_all() {
    // MODE=0xF(12), VBUSCT=5(9), VSHUNTCT=5(6), TEMPCT=5(3), AVG=3(0)
    let expected = (0xF << 12) | (5 << 9) | (5 << 6) | (5 << 3) | 3;
    let i2c = Mock::new(&[write_txn(0x01, expected)]);
    let mut ina = Ina228::new(i2c, ADDR);
    ina.configure(
        OperatingMode::ContinuousAll,
        ConversionTime::Us1052,
        ConversionTime::Us1052,
        ConversionTime::Us1052,
        AveragingCount::N64,
    )
    .unwrap();
    ina.release().done();
}

#[test]
fn configure_triggered_shunt_fast() {
    // MODE=0x2(12), VBUSCT=0(9), VSHUNTCT=0(6), TEMPCT=0(3), AVG=0(0)
    let expected = 0x2 << 12;
    let i2c = Mock::new(&[write_txn(0x01, expected)]);
    let mut ina = Ina228::new(i2c, ADDR);
    ina.configure(
        OperatingMode::TriggeredShunt,
        ConversionTime::Us50,
        ConversionTime::Us50,
        ConversionTime::Us50,
        AveragingCount::N1,
    )
    .unwrap();
    ina.release().done();
}

#[test]
fn set_adc_range_40mv() {
    let i2c = Mock::new(&[
        read_txn(0x00, &0x0000_u16.to_be_bytes()), // read CONFIG
        write_txn(0x00, 1 << 4),                   // write CONFIG with ADCRANGE=1
    ]);
    let mut ina = Ina228::new(i2c, ADDR);
    ina.set_adc_range(AdcRange::Range40mV).unwrap();
    ina.release().done();
}

#[test]
fn set_adc_range_163mv_clears_bit() {
    let i2c = Mock::new(&[
        read_txn(0x00, &0x0010_u16.to_be_bytes()), // CONFIG with ADCRANGE already set
        write_txn(0x00, 0x0000),                   // clears bit 4
    ]);
    let mut ina = Ina228::new(i2c, ADDR);
    ina.set_adc_range(AdcRange::Range163mV).unwrap();
    ina.release().done();
}

#[test]
fn calibrate_10a_10mohm() {
    let shunt_cal = expected_shunt_cal(10.0, 0.01, false);
    let i2c = Mock::new(&[write_txn(0x02, shunt_cal)]);
    let mut ina = Ina228::new(i2c, ADDR);
    ina.calibrate(10.0, 0.01).unwrap();
    ina.release().done();
}

#[test]
fn calibrate_with_40mv_range() {
    let shunt_cal = expected_shunt_cal(5.0, 0.01, true);

    let i2c = Mock::new(&[
        // set_adc_range reads then writes CONFIG
        read_txn(0x00, &0x0000_u16.to_be_bytes()),
        write_txn(0x00, 1 << 4),
        // calibrate writes SHUNT_CAL
        write_txn(0x02, shunt_cal),
    ]);
    let mut ina = Ina228::new(i2c, ADDR);
    ina.set_adc_range(AdcRange::Range40mV).unwrap();
    ina.calibrate(5.0, 0.01).unwrap();
    ina.release().done();
}

#[test]
#[should_panic(expected = "max_current must be positive")]
fn calibrate_zero_current_panics() {
    let i2c = Mock::new(&[]);
    let mut ina = Ina228::new(i2c, ADDR);
    ina.calibrate(0.0, 0.01).unwrap();
}

#[test]
#[should_panic(expected = "shunt_resistance must be positive")]
fn calibrate_zero_shunt_panics() {
    let i2c = Mock::new(&[]);
    let mut ina = Ina228::new(i2c, ADDR);
    ina.calibrate(10.0, 0.0).unwrap();
}

#[test]
fn bus_voltage_known_value() {
    // 12.0V / 195.3125e-6 = 61440 (raw 20-bit)
    // In 24-bit register: 61440 << 4 = 983040 = 0x0F_0000
    let i2c = Mock::new(&[read_txn(0x05, &u24_bytes(61440 << 4))]);
    let mut ina = Ina228::new(i2c, ADDR);
    let v = ina.bus_voltage().unwrap();
    assert!((v - 12.0).abs() < 0.001, "expected ~12.0V, got {v}");
    ina.release().done();
}

#[test]
fn bus_voltage_zero() {
    let i2c = Mock::new(&[read_txn(0x05, &[0, 0, 0])]);
    let mut ina = Ina228::new(i2c, ADDR);
    assert_eq!(ina.bus_voltage().unwrap(), 0.0);
    ina.release().done();
}

#[test]
fn shunt_voltage_positive() {
    // 0.001V / 312.5e-9 = 3200 (raw 20-bit)
    // In 24-bit register: 3200 << 4 = 51200 = 0x00C800
    let i2c = Mock::new(&[read_txn(0x04, &u24_bytes(3200 << 4))]);
    let mut ina = Ina228::new(i2c, ADDR);
    let v = ina.shunt_voltage().unwrap();
    assert!((v - 0.001).abs() < 1e-6, "expected ~0.001V, got {v}");
    ina.release().done();
}

#[test]
fn shunt_voltage_negative() {
    // -3200 in 20-bit two's complement, shifted left 4
    let raw_24 = (((-3200_i32) as u32) & 0xF_FFFF) << 4;
    let i2c = Mock::new(&[read_txn(0x04, &u24_bytes(raw_24))]);
    let mut ina = Ina228::new(i2c, ADDR);
    let v = ina.shunt_voltage().unwrap();
    assert!((v - (-0.001)).abs() < 1e-6, "expected ~-0.001V, got {v}");
    ina.release().done();
}

#[test]
fn current_positive() {
    // With 10A max: current_lsb = 10/524288
    // 5A / current_lsb = 262144 raw
    let shunt_cal = expected_shunt_cal(10.0, 0.01, false);
    let i2c = Mock::new(&[
        write_txn(0x02, shunt_cal),
        read_txn(0x07, &u24_bytes(262144 << 4)),
    ]);
    let mut ina = Ina228::new(i2c, ADDR);
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
    let i2c = Mock::new(&[
        write_txn(0x02, shunt_cal),
        read_txn(0x07, &u24_bytes(raw_24)),
    ]);
    let mut ina = Ina228::new(i2c, ADDR);
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
    let i2c = Mock::new(&[
        write_txn(0x02, shunt_cal),
        read_txn(0x08, &u24_bytes(raw_24)),
    ]);
    let mut ina = Ina228::new(i2c, ADDR);
    ina.calibrate(10.0, 0.01).unwrap();
    let p = ina.power().unwrap();
    assert!((p - power_w).abs() < 0.1, "expected ~{power_w}W, got {p}");
    ina.release().done();
}

#[test]
fn energy_read() {
    let shunt_cal = expected_shunt_cal(10.0, 0.01, false);
    let current_lsb = (10.0_f32 / 524_288.0) as f64;

    // Energy raw = energy_j / (16.0 * 3.2 * current_lsb)
    let energy_j = 1000.0_f64;
    let raw_40 = (energy_j / (16.0 * 3.2 * current_lsb)) as u64;
    let i2c = Mock::new(&[
        write_txn(0x02, shunt_cal),
        read_txn(0x09, &u40_bytes(raw_40)),
    ]);
    let mut ina = Ina228::new(i2c, ADDR);
    ina.calibrate(10.0, 0.01).unwrap();
    let e = ina.energy().unwrap();
    assert!((e - energy_j).abs() < 1.0, "expected ~{energy_j}J, got {e}");
    ina.release().done();
}

#[test]
fn charge_positive() {
    let shunt_cal = expected_shunt_cal(10.0, 0.01, false);
    let current_lsb = (10.0_f32 / 524_288.0) as f64;

    let charge_c = 100.0_f64;
    let raw_40 = (charge_c / current_lsb) as u64;
    let i2c = Mock::new(&[
        write_txn(0x02, shunt_cal),
        read_txn(0x0A, &u40_bytes(raw_40)),
    ]);
    let mut ina = Ina228::new(i2c, ADDR);
    ina.calibrate(10.0, 0.01).unwrap();
    let c = ina.charge().unwrap();
    assert!(
        (c - charge_c).abs() < 0.01,
        "expected ~{charge_c}C, got {c}"
    );
    ina.release().done();
}

#[test]
fn charge_negative() {
    let shunt_cal = expected_shunt_cal(10.0, 0.01, false);
    let current_lsb = (10.0_f32 / 524_288.0) as f64;

    let charge_c = -100.0_f64;
    // Negative 40-bit: compute raw as signed then mask to 40 bits
    let raw_40 = ((charge_c / current_lsb) as i64 as u64) & 0xFF_FFFF_FFFF;
    let i2c = Mock::new(&[
        write_txn(0x02, shunt_cal),
        read_txn(0x0A, &u40_bytes(raw_40)),
    ]);
    let mut ina = Ina228::new(i2c, ADDR);
    ina.calibrate(10.0, 0.01).unwrap();
    let c = ina.charge().unwrap();
    assert!(
        (c - charge_c).abs() < 0.01,
        "expected ~{charge_c}C, got {c}"
    );
    ina.release().done();
}

#[test]
fn die_temperature_positive() {
    // 25C / 7.8125e-3 = 3200
    let i2c = Mock::new(&[read_txn(0x06, &3200_u16.to_be_bytes())]);
    let mut ina = Ina228::new(i2c, ADDR);
    let t = ina.die_temperature().unwrap();
    assert!((t - 25.0).abs() < 0.01, "expected ~25.0C, got {t}");
    ina.release().done();
}

#[test]
fn die_temperature_negative() {
    // -10C / 7.8125e-3 = -1280 -> as u16 = 64256 (0xFB00)
    let i2c = Mock::new(&[read_txn(0x06, &((-1280_i16) as u16).to_be_bytes())]);
    let mut ina = Ina228::new(i2c, ADDR);
    let t = ina.die_temperature().unwrap();
    assert!((t - (-10.0)).abs() < 0.01, "expected ~-10.0C, got {t}");
    ina.release().done();
}

#[test]
fn set_temp_compensation() {
    let i2c = Mock::new(&[
        read_txn(0x00, &0x0000_u16.to_be_bytes()), // read CONFIG
        write_txn(0x00, 1 << 5),                   // set TEMPCOMP bit
        write_txn(0x03, 15 & 0x3FFF),              // write SHUNT_TEMPCO
    ]);
    let mut ina = Ina228::new(i2c, ADDR);
    ina.set_temp_compensation(15).unwrap();
    ina.release().done();
}

#[test]
fn reset_accumulators() {
    let i2c = Mock::new(&[
        read_txn(0x00, &0x0000_u16.to_be_bytes()), // read CONFIG
        write_txn(0x00, 1 << 14),                  // set RSTACC bit
    ]);
    let mut ina = Ina228::new(i2c, ADDR);
    ina.reset_accumulators().unwrap();
    ina.release().done();
}

#[test]
fn conversion_ready_true() {
    let i2c = Mock::new(&[read_txn(0x0B, &(1_u16 << 1).to_be_bytes())]);
    let mut ina = Ina228::new(i2c, ADDR);
    assert!(ina.conversion_ready().unwrap());
    ina.release().done();
}

#[test]
fn conversion_ready_false() {
    let i2c = Mock::new(&[read_txn(0x0B, &0x0000_u16.to_be_bytes())]);
    let mut ina = Ina228::new(i2c, ADDR);
    assert!(!ina.conversion_ready().unwrap());
    ina.release().done();
}

#[test]
fn manufacturer_id() {
    let i2c = Mock::new(&[read_txn(0x3E, &0x5449_u16.to_be_bytes())]);
    let mut ina = Ina228::new(i2c, ADDR);
    assert_eq!(ina.manufacturer_id().unwrap(), 0x5449);
    ina.release().done();
}

#[test]
fn device_id() {
    // Register returns 0x2281 (device=0x228, revision=1)
    let i2c = Mock::new(&[read_txn(0x3F, &0x2281_u16.to_be_bytes())]);
    let mut ina = Ina228::new(i2c, ADDR);
    assert_eq!(ina.device_id().unwrap(), 0x228);
    ina.release().done();
}

#[test]
fn read_instant_returns_measurements() {
    let shunt_cal = expected_shunt_cal(10.0, 0.01, false);

    let i2c = Mock::new(&[
        // calibrate
        write_txn(0x02, shunt_cal),
        // bus_voltage: 12V -> raw_20=61440, raw_24=61440<<4
        read_txn(0x05, &[0x0F, 0x00, 0x00]),
        // shunt_voltage: 0V
        read_txn(0x04, &[0x00, 0x00, 0x00]),
        // current: 0A
        read_txn(0x07, &[0x00, 0x00, 0x00]),
        // power: 0W
        read_txn(0x08, &[0x00, 0x00, 0x00]),
        // die_temp: 25C -> 3200
        read_txn(0x06, &3200_u16.to_be_bytes()),
    ]);
    let mut ina = Ina228::new(i2c, ADDR);
    ina.calibrate(10.0, 0.01).unwrap();
    let m = ina.read_instant().unwrap();
    assert!((m.bus_voltage_v - 12.0).abs() < 0.001);
    assert_eq!(m.shunt_voltage_v, 0.0);
    assert_eq!(m.current_a, 0.0);
    assert_eq!(m.power_w, 0.0);
    assert!((m.die_temp_c - 25.0).abs() < 0.01);
    ina.release().done();
}

#[test]
fn disable_temp_compensation() {
    let i2c = Mock::new(&[
        read_txn(0x00, &0x0020_u16.to_be_bytes()), // CONFIG with TEMPCOMP set
        write_txn(0x00, 0x0000),                   // clears TEMPCOMP bit
    ]);
    let mut ina = Ina228::new(i2c, ADDR);
    ina.disable_temp_compensation().unwrap();
    ina.release().done();
}

#[test]
fn diagnostic_flags_all_clear() {
    let i2c = Mock::new(&[read_txn(0x0B, &0x0000_u16.to_be_bytes())]);
    let mut ina = Ina228::new(i2c, ADDR);
    let flags = ina.diagnostic_flags().unwrap();
    assert!(!flags.conversion_ready);
    assert!(!flags.temp_over_limit);
    assert!(!flags.shunt_over_limit);
    assert!(!flags.bus_over_limit);
    assert!(!flags.power_over_limit);
    ina.release().done();
}

#[test]
fn diagnostic_flags_alerts_set() {
    // Set TMPOL(7), BUSOL(4), CNVRF(1)
    let diag: u16 = (1 << 7) | (1 << 4) | (1 << 1);
    let i2c = Mock::new(&[read_txn(0x0B, &diag.to_be_bytes())]);
    let mut ina = Ina228::new(i2c, ADDR);
    let flags = ina.diagnostic_flags().unwrap();
    assert!(flags.temp_over_limit);
    assert!(flags.bus_over_limit);
    assert!(flags.conversion_ready);
    assert!(!flags.shunt_over_limit);
    assert!(!flags.power_over_limit);
    ina.release().done();
}

#[test]
fn configure_alerts_latch_active_high() {
    let i2c = Mock::new(&[
        read_txn(0x0B, &0x0000_u16.to_be_bytes()),
        // CNVR(14) | APOL(12) | ALATCH(11) = 0x5800
        write_txn(0x0B, (1 << 14) | (1 << 12) | (1 << 11)),
    ]);
    let mut ina = Ina228::new(i2c, ADDR);
    ina.configure_alerts(true, true, true, false).unwrap();
    ina.release().done();
}

#[test]
fn set_shunt_overvoltage_limit() {
    // 0.05V / 5uV = 10000
    let expected_raw = (0.05_f32 / 5.0e-6) as i16 as u16;
    let i2c = Mock::new(&[write_txn(0x0C, expected_raw)]);
    let mut ina = Ina228::new(i2c, ADDR);
    ina.set_shunt_overvoltage_limit(0.05).unwrap();
    ina.release().done();
}

#[test]
fn set_shunt_undervoltage_limit() {
    // -0.05V / 5uV = -10000
    let expected_raw = (-0.05_f32 / 5.0e-6) as i16 as u16;
    let i2c = Mock::new(&[write_txn(0x0D, expected_raw)]);
    let mut ina = Ina228::new(i2c, ADDR);
    ina.set_shunt_undervoltage_limit(-0.05).unwrap();
    ina.release().done();
}

#[test]
fn set_bus_overvoltage_limit() {
    // 48V / 3.125mV = 15360
    let expected_raw = (48.0_f32 / 3.125e-3) as u16;
    let i2c = Mock::new(&[write_txn(0x0E, expected_raw)]);
    let mut ina = Ina228::new(i2c, ADDR);
    ina.set_bus_overvoltage_limit(48.0).unwrap();
    ina.release().done();
}

#[test]
fn set_bus_undervoltage_limit() {
    // 3.0V / 3.125mV = 960
    let expected_raw = (3.0_f32 / 3.125e-3) as u16;
    let i2c = Mock::new(&[write_txn(0x0F, expected_raw)]);
    let mut ina = Ina228::new(i2c, ADDR);
    ina.set_bus_undervoltage_limit(3.0).unwrap();
    ina.release().done();
}

#[test]
fn set_temperature_limit() {
    // 80C / 7.8125e-3 = 10240
    let expected_raw = (80.0_f32 / 7.8125e-3) as i16 as u16;
    let i2c = Mock::new(&[write_txn(0x10, expected_raw)]);
    let mut ina = Ina228::new(i2c, ADDR);
    ina.set_temperature_limit(80.0).unwrap();
    ina.release().done();
}

#[test]
fn set_power_limit() {
    let shunt_cal = expected_shunt_cal(10.0, 0.01, false);
    let current_lsb = 10.0_f32 / 524_288.0;
    let power_lsb = 3.2 * current_lsb;
    // 100W / (256 * power_lsb)
    let expected_raw = (100.0_f32 / (256.0 * power_lsb)) as u16;

    let i2c = Mock::new(&[write_txn(0x02, shunt_cal), write_txn(0x11, expected_raw)]);
    let mut ina = Ina228::new(i2c, ADDR);
    ina.calibrate(10.0, 0.01).unwrap();
    ina.set_power_limit(100.0).unwrap();
    ina.release().done();
}

#[test]
fn die_revision() {
    // Register returns 0x2285 (device=0x228, revision=5)
    let i2c = Mock::new(&[read_txn(0x3F, &0x2285_u16.to_be_bytes())]);
    let mut ina = Ina228::new(i2c, ADDR);
    assert_eq!(ina.die_revision().unwrap(), 5);
    ina.release().done();
}

#[test]
fn shunt_voltage_40mv_range() {
    // 0.001V / 78.125e-9 = 12800 (raw 20-bit)
    let i2c = Mock::new(&[
        // set_adc_range reads then writes CONFIG
        read_txn(0x00, &0x0000_u16.to_be_bytes()),
        write_txn(0x00, 1 << 4),
        // shunt voltage read
        read_txn(0x04, &u24_bytes(12800 << 4)),
    ]);
    let mut ina = Ina228::new(i2c, ADDR);
    ina.set_adc_range(AdcRange::Range40mV).unwrap();
    let v = ina.shunt_voltage().unwrap();
    assert!((v - 0.001).abs() < 1e-6, "expected ~0.001V, got {v}");
    ina.release().done();
}

#[test]
#[should_panic(expected = "SHUNT_CAL overflow")]
fn calibrate_shunt_cal_overflow_panics() {
    let i2c = Mock::new(&[]);
    let mut ina = Ina228::new(i2c, ADDR);
    // 100A max with 0.1 ohm shunt -> SHUNT_CAL ~ 250000, way over 32767
    ina.calibrate(100.0, 0.1).unwrap();
}

#[test]
fn set_adc_range_after_calibrate_recalibrates() {
    let shunt_cal_163mv = expected_shunt_cal(5.0, 0.01, false);
    let shunt_cal_40mv = expected_shunt_cal(5.0, 0.01, true);

    let i2c = Mock::new(&[
        // calibrate writes SHUNT_CAL (163mV range)
        write_txn(0x02, shunt_cal_163mv),
        // set_adc_range reads CONFIG, writes CONFIG, then recalibrates
        read_txn(0x00, &0x0000_u16.to_be_bytes()),
        write_txn(0x00, 1 << 4),
        write_txn(0x02, shunt_cal_40mv),
    ]);
    let mut ina = Ina228::new(i2c, ADDR);
    ina.calibrate(5.0, 0.01).unwrap();
    ina.set_adc_range(AdcRange::Range40mV).unwrap();
    ina.release().done();
}

#[test]
fn i2c_error_propagates() {
    let i2c = Mock::new(&[
        Transaction::write_read(ADDR, vec![0x05], vec![0, 0, 0]).with_error(ErrorKind::Bus)
    ]);
    let mut ina = Ina228::new(i2c, ADDR);
    let result = ina.bus_voltage();
    assert!(result.is_err());
    ina.release().done();
}

#[test]
fn calibrate_rollback_on_i2c_error() {
    let shunt_cal_ok = expected_shunt_cal(10.0, 0.01, false);
    let shunt_cal_fail = expected_shunt_cal(1.0, 0.1, false);
    let fail_bytes = shunt_cal_fail.to_be_bytes();
    let i2c = Mock::new(&[
        // First calibrate succeeds
        write_txn(0x02, shunt_cal_ok),
        // Second calibrate fails on I2C write
        Transaction::write(ADDR, vec![0x02, fail_bytes[0], fail_bytes[1]])
            .with_error(ErrorKind::Bus),
        // After failed calibrate, current() should still use the original calibration
        read_txn(0x07, &u24_bytes(262144 << 4)),
    ]);
    let mut ina = Ina228::new(i2c, ADDR);
    ina.calibrate(10.0, 0.01).unwrap();

    // This calibrate should fail and rollback
    assert!(ina.calibrate(1.0, 0.1).is_err());

    // current_lsb should still be from the first calibrate (10A/524288)
    let current = ina.current().unwrap();
    assert!(
        (current - 5.0).abs() < 0.001,
        "expected ~5.0A (original calibration), got {current}"
    );
    ina.release().done();
}
