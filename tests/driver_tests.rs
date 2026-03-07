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

fn read_u16_txn(reg: u8, value: u16) -> Transaction {
    let bytes = value.to_be_bytes();
    Transaction::write_read(ADDR, vec![reg], bytes.to_vec())
}

fn read_u24_txn(reg: u8, b0: u8, b1: u8, b2: u8) -> Transaction {
    Transaction::write_read(ADDR, vec![reg], vec![b0, b1, b2])
}

fn read_u40_txn(reg: u8, bytes: [u8; 5]) -> Transaction {
    Transaction::write_read(ADDR, vec![reg], bytes.to_vec())
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
        read_u24_txn(0x05, 0x00, 0x00, 0x00),
    ]);
    let mut ina = Ina228::new(i2c, ADDR);
    ina.calibrate(10.0, 0.01);
    ina.reset();
    // bus_voltage should still work after reset (doesn't need calibration)
    assert_eq!(ina.bus_voltage(), 0.0);
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
    );
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
    );
    ina.release().done();
}

#[test]
fn set_adc_range_40mv() {
    let i2c = Mock::new(&[
        read_u16_txn(0x00, 0x0000), // read CONFIG
        write_txn(0x00, 1 << 4),    // write CONFIG with ADCRANGE=1
    ]);
    let mut ina = Ina228::new(i2c, ADDR);
    ina.set_adc_range(AdcRange::Range40mV);
    ina.release().done();
}

#[test]
fn set_adc_range_163mv_clears_bit() {
    let i2c = Mock::new(&[
        read_u16_txn(0x00, 0x0010), // CONFIG with ADCRANGE already set
        write_txn(0x00, 0x0000),    // clears bit 4
    ]);
    let mut ina = Ina228::new(i2c, ADDR);
    ina.set_adc_range(AdcRange::Range163mV);
    ina.release().done();
}

#[test]
fn calibrate_10a_10mohm() {
    let shunt_cal = expected_shunt_cal(10.0, 0.01, false);
    let i2c = Mock::new(&[write_txn(0x02, shunt_cal)]);
    let mut ina = Ina228::new(i2c, ADDR);
    ina.calibrate(10.0, 0.01);
    ina.release().done();
}

#[test]
fn calibrate_with_40mv_range() {
    let shunt_cal = expected_shunt_cal(5.0, 0.01, true);

    let i2c = Mock::new(&[
        // set_adc_range reads then writes CONFIG
        read_u16_txn(0x00, 0x0000),
        write_txn(0x00, 1 << 4),
        // calibrate writes SHUNT_CAL
        write_txn(0x02, shunt_cal),
    ]);
    let mut ina = Ina228::new(i2c, ADDR);
    ina.set_adc_range(AdcRange::Range40mV);
    ina.calibrate(5.0, 0.01);
    ina.release().done();
}

#[test]
#[should_panic(expected = "max_current must be positive")]
fn calibrate_zero_current_panics() {
    let i2c = Mock::new(&[]);
    let mut ina = Ina228::new(i2c, ADDR);
    ina.calibrate(0.0, 0.01);
}

#[test]
#[should_panic(expected = "shunt_resistance must be positive")]
fn calibrate_zero_shunt_panics() {
    let i2c = Mock::new(&[]);
    let mut ina = Ina228::new(i2c, ADDR);
    ina.calibrate(10.0, 0.0);
}

#[test]
fn bus_voltage_known_value() {
    // 12.0V / 195.3125e-6 = 61440 (raw 20-bit)
    // In 24-bit register: 61440 << 4 = 983040 = 0x0F_0000
    let raw_24 = 61440_u32 << 4;
    let b0 = (raw_24 >> 16) as u8;
    let b1 = (raw_24 >> 8) as u8;
    let b2 = raw_24 as u8;

    let i2c = Mock::new(&[read_u24_txn(0x05, b0, b1, b2)]);
    let mut ina = Ina228::new(i2c, ADDR);
    let v = ina.bus_voltage();
    assert!((v - 12.0).abs() < 0.001, "expected ~12.0V, got {v}");
    ina.release().done();
}

#[test]
fn bus_voltage_zero() {
    let i2c = Mock::new(&[read_u24_txn(0x05, 0, 0, 0)]);
    let mut ina = Ina228::new(i2c, ADDR);
    assert_eq!(ina.bus_voltage(), 0.0);
    ina.release().done();
}

#[test]
fn shunt_voltage_positive() {
    // 0.001V / 312.5e-9 = 3200 (raw 20-bit)
    // In 24-bit register: 3200 << 4 = 51200 = 0x00C800
    let raw_24 = 3200_u32 << 4;
    let b0 = (raw_24 >> 16) as u8;
    let b1 = (raw_24 >> 8) as u8;
    let b2 = raw_24 as u8;

    let i2c = Mock::new(&[read_u24_txn(0x04, b0, b1, b2)]);
    let mut ina = Ina228::new(i2c, ADDR);
    let v = ina.shunt_voltage();
    assert!((v - 0.001).abs() < 1e-6, "expected ~0.001V, got {v}");
    ina.release().done();
}

#[test]
fn shunt_voltage_negative() {
    // -3200 in 20-bit two's complement: 0xFFFFF - 3200 + 1 = 0xFF380
    // Shift left 4: 0xFF3800
    let neg_raw_20 = ((-3200_i32) as u32) & 0xF_FFFF;
    let raw_24 = neg_raw_20 << 4;
    let b0 = (raw_24 >> 16) as u8;
    let b1 = (raw_24 >> 8) as u8;
    let b2 = raw_24 as u8;

    let i2c = Mock::new(&[read_u24_txn(0x04, b0, b1, b2)]);
    let mut ina = Ina228::new(i2c, ADDR);
    let v = ina.shunt_voltage();
    assert!((v - (-0.001)).abs() < 1e-6, "expected ~-0.001V, got {v}");
    ina.release().done();
}

#[test]
fn current_positive() {
    // With 10A max: current_lsb = 10/524288
    // 5A / current_lsb = 262144 raw
    let shunt_cal = expected_shunt_cal(10.0, 0.01, false);

    let raw_20 = 262144_u32;
    let raw_24 = raw_20 << 4;
    let b0 = (raw_24 >> 16) as u8;
    let b1 = (raw_24 >> 8) as u8;
    let b2 = raw_24 as u8;

    let i2c = Mock::new(&[write_txn(0x02, shunt_cal), read_u24_txn(0x07, b0, b1, b2)]);
    let mut ina = Ina228::new(i2c, ADDR);
    ina.calibrate(10.0, 0.01);
    let current = ina.current();
    assert!(
        (current - 5.0).abs() < 0.001,
        "expected ~5.0A, got {current}"
    );
    ina.release().done();
}

#[test]
fn current_negative() {
    let shunt_cal = expected_shunt_cal(10.0, 0.01, false);

    // -262144 in 20-bit two's complement
    let neg_raw_20 = ((-262144_i32) as u32) & 0xF_FFFF;
    let raw_24 = neg_raw_20 << 4;
    let b0 = (raw_24 >> 16) as u8;
    let b1 = (raw_24 >> 8) as u8;
    let b2 = raw_24 as u8;

    let i2c = Mock::new(&[write_txn(0x02, shunt_cal), read_u24_txn(0x07, b0, b1, b2)]);
    let mut ina = Ina228::new(i2c, ADDR);
    ina.calibrate(10.0, 0.01);
    let current = ina.current();
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
    let b0 = (raw_24 >> 16) as u8;
    let b1 = (raw_24 >> 8) as u8;
    let b2 = raw_24 as u8;

    let i2c = Mock::new(&[write_txn(0x02, shunt_cal), read_u24_txn(0x08, b0, b1, b2)]);
    let mut ina = Ina228::new(i2c, ADDR);
    ina.calibrate(10.0, 0.01);
    let p = ina.power();
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
    let bytes: [u8; 5] = [
        (raw_40 >> 32) as u8,
        (raw_40 >> 24) as u8,
        (raw_40 >> 16) as u8,
        (raw_40 >> 8) as u8,
        raw_40 as u8,
    ];

    let i2c = Mock::new(&[write_txn(0x02, shunt_cal), read_u40_txn(0x09, bytes)]);
    let mut ina = Ina228::new(i2c, ADDR);
    ina.calibrate(10.0, 0.01);
    let e = ina.energy();
    assert!((e - energy_j).abs() < 1.0, "expected ~{energy_j}J, got {e}");
    ina.release().done();
}

#[test]
fn charge_positive() {
    let shunt_cal = expected_shunt_cal(10.0, 0.01, false);
    let current_lsb = (10.0_f32 / 524_288.0) as f64;

    let charge_c = 100.0_f64;
    let raw_40 = (charge_c / current_lsb) as u64;
    let bytes: [u8; 5] = [
        (raw_40 >> 32) as u8,
        (raw_40 >> 24) as u8,
        (raw_40 >> 16) as u8,
        (raw_40 >> 8) as u8,
        raw_40 as u8,
    ];

    let i2c = Mock::new(&[write_txn(0x02, shunt_cal), read_u40_txn(0x0A, bytes)]);
    let mut ina = Ina228::new(i2c, ADDR);
    ina.calibrate(10.0, 0.01);
    let c = ina.charge();
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
    let bytes: [u8; 5] = [
        (raw_40 >> 32) as u8,
        (raw_40 >> 24) as u8,
        (raw_40 >> 16) as u8,
        (raw_40 >> 8) as u8,
        raw_40 as u8,
    ];

    let i2c = Mock::new(&[write_txn(0x02, shunt_cal), read_u40_txn(0x0A, bytes)]);
    let mut ina = Ina228::new(i2c, ADDR);
    ina.calibrate(10.0, 0.01);
    let c = ina.charge();
    assert!(
        (c - charge_c).abs() < 0.01,
        "expected ~{charge_c}C, got {c}"
    );
    ina.release().done();
}

#[test]
fn die_temperature_positive() {
    // 25°C / 7.8125e-3 = 3200
    let raw = 3200_u16;
    let i2c = Mock::new(&[read_u16_txn(0x06, raw)]);
    let mut ina = Ina228::new(i2c, ADDR);
    let t = ina.die_temperature();
    assert!((t - 25.0).abs() < 0.01, "expected ~25.0°C, got {t}");
    ina.release().done();
}

#[test]
fn die_temperature_negative() {
    // -10°C / 7.8125e-3 = -1280 → as u16 = 64256 (0xFB00)
    let raw = (-1280_i16) as u16;
    let i2c = Mock::new(&[read_u16_txn(0x06, raw)]);
    let mut ina = Ina228::new(i2c, ADDR);
    let t = ina.die_temperature();
    assert!((t - (-10.0)).abs() < 0.01, "expected ~-10.0°C, got {t}");
    ina.release().done();
}

#[test]
fn set_temp_compensation() {
    let i2c = Mock::new(&[
        read_u16_txn(0x00, 0x0000),   // read CONFIG
        write_txn(0x00, 1 << 5),      // set TEMPCOMP bit
        write_txn(0x03, 15 & 0x3FFF), // write SHUNT_TEMPCO
    ]);
    let mut ina = Ina228::new(i2c, ADDR);
    ina.set_temp_compensation(15);
    ina.release().done();
}

#[test]
fn reset_accumulators() {
    let i2c = Mock::new(&[
        read_u16_txn(0x00, 0x0000), // read CONFIG
        write_txn(0x00, 1 << 14),   // set RSTACC bit
    ]);
    let mut ina = Ina228::new(i2c, ADDR);
    ina.reset_accumulators();
    ina.release().done();
}

#[test]
fn conversion_ready_true() {
    let i2c = Mock::new(&[read_u16_txn(0x0B, 1 << 1)]);
    let mut ina = Ina228::new(i2c, ADDR);
    assert!(ina.conversion_ready());
    ina.release().done();
}

#[test]
fn conversion_ready_false() {
    let i2c = Mock::new(&[read_u16_txn(0x0B, 0x0000)]);
    let mut ina = Ina228::new(i2c, ADDR);
    assert!(!ina.conversion_ready());
    ina.release().done();
}

#[test]
fn manufacturer_id() {
    let i2c = Mock::new(&[read_u16_txn(0x3E, 0x5449)]);
    let mut ina = Ina228::new(i2c, ADDR);
    assert_eq!(ina.manufacturer_id(), 0x5449);
    ina.release().done();
}

#[test]
fn device_id() {
    // Register returns 0x2281 (device=0x228, revision=1)
    let i2c = Mock::new(&[read_u16_txn(0x3F, 0x2281)]);
    let mut ina = Ina228::new(i2c, ADDR);
    assert_eq!(ina.device_id(), 0x228);
    ina.release().done();
}

#[test]
fn read_instant_returns_measurements() {
    let shunt_cal = expected_shunt_cal(10.0, 0.01, false);

    let i2c = Mock::new(&[
        // calibrate
        write_txn(0x02, shunt_cal),
        // bus_voltage: 12V → raw_20=61440, raw_24=61440<<4
        read_u24_txn(0x05, 0x0F, 0x00, 0x00),
        // shunt_voltage: 0V
        read_u24_txn(0x04, 0x00, 0x00, 0x00),
        // current: 0A
        read_u24_txn(0x07, 0x00, 0x00, 0x00),
        // power: 0W
        read_u24_txn(0x08, 0x00, 0x00, 0x00),
        // die_temp: 25°C → 3200
        read_u16_txn(0x06, 3200),
    ]);
    let mut ina = Ina228::new(i2c, ADDR);
    ina.calibrate(10.0, 0.01);
    let m = ina.read_instant();
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
        read_u16_txn(0x00, 0x0020), // CONFIG with TEMPCOMP set
        write_txn(0x00, 0x0000),    // clears TEMPCOMP bit
    ]);
    let mut ina = Ina228::new(i2c, ADDR);
    ina.disable_temp_compensation();
    ina.release().done();
}

#[test]
fn diagnostic_flags_all_clear() {
    let i2c = Mock::new(&[read_u16_txn(0x0B, 0x0000)]);
    let mut ina = Ina228::new(i2c, ADDR);
    let flags = ina.diagnostic_flags();
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
    let diag = (1 << 7) | (1 << 4) | (1 << 1);
    let i2c = Mock::new(&[read_u16_txn(0x0B, diag)]);
    let mut ina = Ina228::new(i2c, ADDR);
    let flags = ina.diagnostic_flags();
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
        read_u16_txn(0x0B, 0x0000),
        // CNVR(14) | APOL(12) | ALATCH(11) = 0x5800
        write_txn(0x0B, (1 << 14) | (1 << 12) | (1 << 11)),
    ]);
    let mut ina = Ina228::new(i2c, ADDR);
    ina.configure_alerts(true, true, true, false);
    ina.release().done();
}

#[test]
fn set_shunt_overvoltage_limit() {
    // 0.05V / 5µV = 10000
    let expected_raw = (0.05_f32 / 5.0e-6) as i16 as u16;
    let i2c = Mock::new(&[write_txn(0x0C, expected_raw)]);
    let mut ina = Ina228::new(i2c, ADDR);
    ina.set_shunt_overvoltage_limit(0.05);
    ina.release().done();
}

#[test]
fn set_shunt_undervoltage_limit() {
    // -0.05V / 5µV = -10000
    let expected_raw = (-0.05_f32 / 5.0e-6) as i16 as u16;
    let i2c = Mock::new(&[write_txn(0x0D, expected_raw)]);
    let mut ina = Ina228::new(i2c, ADDR);
    ina.set_shunt_undervoltage_limit(-0.05);
    ina.release().done();
}

#[test]
fn set_bus_overvoltage_limit() {
    // 48V / 3.125mV = 15360
    let expected_raw = (48.0_f32 / 3.125e-3) as u16;
    let i2c = Mock::new(&[write_txn(0x0E, expected_raw)]);
    let mut ina = Ina228::new(i2c, ADDR);
    ina.set_bus_overvoltage_limit(48.0);
    ina.release().done();
}

#[test]
fn set_bus_undervoltage_limit() {
    // 3.0V / 3.125mV = 960
    let expected_raw = (3.0_f32 / 3.125e-3) as u16;
    let i2c = Mock::new(&[write_txn(0x0F, expected_raw)]);
    let mut ina = Ina228::new(i2c, ADDR);
    ina.set_bus_undervoltage_limit(3.0);
    ina.release().done();
}

#[test]
fn set_temperature_limit() {
    // 80°C / 7.8125e-3 = 10240
    let expected_raw = (80.0_f32 / 7.8125e-3) as i16 as u16;
    let i2c = Mock::new(&[write_txn(0x10, expected_raw)]);
    let mut ina = Ina228::new(i2c, ADDR);
    ina.set_temperature_limit(80.0);
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
    ina.calibrate(10.0, 0.01);
    ina.set_power_limit(100.0);
    ina.release().done();
}

#[test]
fn die_revision() {
    // Register returns 0x2285 (device=0x228, revision=5)
    let i2c = Mock::new(&[read_u16_txn(0x3F, 0x2285)]);
    let mut ina = Ina228::new(i2c, ADDR);
    assert_eq!(ina.die_revision(), 5);
    ina.release().done();
}

#[test]
fn shunt_voltage_40mv_range() {
    // 0.001V / 78.125e-9 = 12800 (raw 20-bit)
    let raw_24 = 12800_u32 << 4;
    let b0 = (raw_24 >> 16) as u8;
    let b1 = (raw_24 >> 8) as u8;
    let b2 = raw_24 as u8;

    let i2c = Mock::new(&[
        // set_adc_range reads then writes CONFIG
        read_u16_txn(0x00, 0x0000),
        write_txn(0x00, 1 << 4),
        // shunt voltage read
        read_u24_txn(0x04, b0, b1, b2),
    ]);
    let mut ina = Ina228::new(i2c, ADDR);
    ina.set_adc_range(AdcRange::Range40mV);
    let v = ina.shunt_voltage();
    assert!((v - 0.001).abs() < 1e-6, "expected ~0.001V, got {v}");
    ina.release().done();
}

#[test]
#[should_panic(expected = "SHUNT_CAL overflow")]
fn calibrate_shunt_cal_overflow_panics() {
    let i2c = Mock::new(&[]);
    let mut ina = Ina228::new(i2c, ADDR);
    // 100A max with 0.1 ohm shunt → SHUNT_CAL ≈ 250000, way over 32767
    ina.calibrate(100.0, 0.1);
}

#[test]
fn set_adc_range_after_calibrate_recalibrates() {
    let shunt_cal_163mv = expected_shunt_cal(5.0, 0.01, false);
    let shunt_cal_40mv = expected_shunt_cal(5.0, 0.01, true);

    let i2c = Mock::new(&[
        // calibrate writes SHUNT_CAL (163mV range)
        write_txn(0x02, shunt_cal_163mv),
        // set_adc_range reads CONFIG, writes CONFIG, then recalibrates
        read_u16_txn(0x00, 0x0000),
        write_txn(0x00, 1 << 4),
        write_txn(0x02, shunt_cal_40mv),
    ]);
    let mut ina = Ina228::new(i2c, ADDR);
    ina.calibrate(5.0, 0.01);
    ina.set_adc_range(AdcRange::Range40mV);
    ina.release().done();
}
