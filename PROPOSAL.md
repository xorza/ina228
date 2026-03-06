# INA228 Rust Driver — Implementation Proposal

## Overview

The INA228 is a TI 85V, 20-bit high-precision digital power/energy/charge monitor with I2C interface. No Rust crate exists for it. This proposal covers building an `embedded-hal`-based driver to read all measurement data from the INA228 on ESP32.

## INA228 Key Specs

| Feature | Value |
|---------|-------|
| Bus voltage range | 0–85V |
| ADC resolution | 20-bit |
| Shunt voltage LSB | 312.5 nV (±163.84 mV range) or 78.125 nV (±40.96 mV range) |
| Bus voltage LSB | 195.3125 µV |
| Measurements | Voltage, current, power, energy, charge, die temperature |
| I2C addresses | 0x40–0x4F (16 options via A0/A1 pins) |
| Register sizes | Mixed: 16-bit, 24-bit, and 40-bit |

## Register Map

| Addr | Name | R/W | Size | Description |
|------|------|-----|------|-------------|
| 0x00 | CONFIG | RW | 16-bit | Reset, ADC range, conversion delay, temp compensation |
| 0x01 | ADC_CONFIG | RW | 16-bit | Operating mode, conversion times, averaging |
| 0x02 | SHUNT_CAL | RW | 16-bit | Shunt calibration value |
| 0x03 | SHUNT_TEMPCO | RW | 16-bit | Shunt temp coefficient (ppm/°C) |
| 0x04 | VSHUNT | R | 24-bit | Shunt voltage (20-bit, 4 reserved LSBs) |
| 0x05 | VBUS | R | 24-bit | Bus voltage (20-bit, 4 reserved LSBs) |
| 0x06 | DIETEMP | R | 16-bit | Die temperature |
| 0x07 | CURRENT | R | 24-bit | Current (20-bit signed, 4 reserved LSBs) |
| 0x08 | POWER | R | 24-bit | Power (24-bit unsigned) |
| 0x09 | ENERGY | R | 40-bit | Accumulated energy |
| 0x0A | CHARGE | R | 40-bit | Accumulated charge (signed) |
| 0x0B | DIAG_ALRT | RW | 16-bit | Diagnostic flags and alert config |
| 0x0C | SOVL | RW | 16-bit | Shunt overvoltage threshold |
| 0x0D | SUVL | RW | 16-bit | Shunt undervoltage threshold |
| 0x0E | BOVL | RW | 16-bit | Bus overvoltage threshold |
| 0x0F | BUVL | RW | 16-bit | Bus undervoltage threshold |
| 0x10 | TEMP_LIMIT | RW | 16-bit | Temperature over-limit threshold |
| 0x11 | PWR_LIMIT | RW | 16-bit | Power over-limit threshold |
| 0x3E | MANUFACTURER_ID | R | 16-bit | 0x5449 ("TI") |
| 0x3F | DEVICE_ID | R | 16-bit | 0x2280 |

## Differences from INA226 (which has existing Rust crates)

- 20-bit ADC vs 16-bit → needs 24-bit and 40-bit register reads
- 85V bus range vs 36V
- Energy and charge accumulation registers (40-bit)
- Built-in die temperature sensor
- Shunt temperature compensation
- Configuration split across two registers (CONFIG + ADC_CONFIG)
- Calibration formula is inverted: `SHUNT_CAL = 13107.2e6 × CURRENT_LSB × R_SHUNT`

## Proposed Architecture

### Crate Structure

```
src/
├── main.rs          # ESP32 application using the driver
├── ina228/
│   ├── mod.rs       # Re-exports
│   ├── driver.rs    # INA228 struct, I2C read/write, measurement methods
│   ├── registers.rs # Register addresses, bit field constants, enums
│   └── config.rs    # Configuration builder types
```

### Core Types

```rust
#[derive(Debug)]
pub struct Ina228<I2C> {
    i2c: I2C,
    address: u8,
    current_lsb: f32,  // Set during calibration
    adc_range: AdcRange,
}

#[derive(Debug, Clone, Copy)]
pub enum AdcRange {
    Range163mV,  // ±163.84 mV, LSB = 312.5 nV
    Range40mV,   // ±40.96 mV, LSB = 78.125 nV
}

#[derive(Debug, Clone, Copy)]
pub enum ConversionTime {
    Us50, Us84, Us150, Us280, Us540, Us1052, Us2074, Us4120,
}

#[derive(Debug, Clone, Copy)]
pub enum AveragingCount {
    N1, N4, N16, N64, N128, N256, N512, N1024,
}

#[derive(Debug, Clone, Copy)]
pub enum OperatingMode {
    Shutdown,
    // Triggered modes
    TriggeredBus,
    TriggeredShunt,
    TriggeredBusShunt,
    TriggeredTemp,
    TriggeredTempBus,
    TriggeredTempShunt,
    TriggeredAll,
    // Continuous modes
    ContinuousBus,
    ContinuousShunt,
    ContinuousBusShunt,
    ContinuousTemp,
    ContinuousTempBus,
    ContinuousTempShunt,
    ContinuousAll,  // default
}

/// Physical measurement results
#[derive(Debug)]
pub struct Measurements {
    pub bus_voltage_v: f32,
    pub shunt_voltage_v: f32,
    pub current_a: f32,
    pub power_w: f32,
    pub die_temp_c: f32,
}
```

### Public API

```rust
impl<I2C: I2c> Ina228<I2C> {
    /// Create driver with I2C address (default 0x40)
    pub fn new(i2c: I2C, address: u8) -> Self;

    /// Reset all registers to defaults
    pub fn reset(&mut self);

    /// Configure ADC: mode, conversion times, averaging
    pub fn configure(
        &mut self,
        mode: OperatingMode,
        vbus_ct: ConversionTime,
        vshunt_ct: ConversionTime,
        temp_ct: ConversionTime,
        avg: AveragingCount,
    );

    /// Set ADC range (±163.84mV or ±40.96mV)
    pub fn set_adc_range(&mut self, range: AdcRange);

    /// Calibrate for current measurement
    /// max_current_a: maximum expected current in Amps
    /// shunt_resistance_ohm: shunt resistor value in Ohms
    pub fn calibrate(&mut self, max_current_a: f32, shunt_resistance_ohm: f32);

    /// Enable shunt temperature compensation
    pub fn set_temp_compensation(&mut self, tempco_ppm: u16);

    // --- Measurement reads ---

    /// Read bus voltage in Volts
    pub fn bus_voltage(&mut self) -> f32;

    /// Read shunt voltage in Volts
    pub fn shunt_voltage(&mut self) -> f32;

    /// Read current in Amps (requires calibration)
    pub fn current(&mut self) -> f32;

    /// Read power in Watts (requires calibration)
    pub fn power(&mut self) -> f32;

    /// Read accumulated energy in Joules (requires calibration)
    pub fn energy(&mut self) -> f64;

    /// Read accumulated charge in Coulombs (requires calibration)
    pub fn charge(&mut self) -> f64;

    /// Read die temperature in °C
    pub fn die_temperature(&mut self) -> f32;

    /// Read all basic measurements at once
    pub fn read_all(&mut self) -> Measurements;

    /// Reset energy and charge accumulators
    pub fn reset_accumulators(&mut self);

    /// Check if conversion is ready
    pub fn conversion_ready(&mut self) -> bool;

    /// Read manufacturer ID (should be 0x5449)
    pub fn manufacturer_id(&mut self) -> u16;

    /// Read device ID (should be 0x2280)
    pub fn device_id(&mut self) -> u16;
}
```

### I2C Low-Level Read Helpers

Three internal read functions needed due to mixed register sizes:

- `read_u16(reg)` — CONFIG, ADC_CONFIG, DIETEMP, thresholds, IDs
- `read_u24(reg) -> u32` — VSHUNT, VBUS, CURRENT, POWER (right-shift by 4 for 20-bit values)
- `read_u40(reg) -> u64` — ENERGY, CHARGE

All reads: write register pointer byte, then read N bytes big-endian.

### Calibration Logic

```
CURRENT_LSB = max_current / 2^19

For ADCRANGE=0: SHUNT_CAL = 13107.2e6 × CURRENT_LSB × R_SHUNT
For ADCRANGE=1: SHUNT_CAL = 4 × 13107.2e6 × CURRENT_LSB × R_SHUNT
```

### Value Conversion Formulas

| Register | Formula |
|----------|---------|
| Bus voltage (V) | `raw_20bit × 195.3125e-6` |
| Shunt voltage (V) | `raw_20bit × 312.5e-9` (range 0) or `× 78.125e-9` (range 1) |
| Current (A) | `raw_20bit_signed × CURRENT_LSB` |
| Power (W) | `raw_24bit × 3.2 × CURRENT_LSB` |
| Energy (J) | `raw_40bit × 16 × 3.2 × CURRENT_LSB` |
| Charge (C) | `raw_40bit_signed × CURRENT_LSB` |
| Temperature (°C) | `raw_16bit × 7.8125e-3` |

Note: 20-bit signed values require sign extension from bit 19.

## ESP32 Integration

Dependencies for `esp-idf-hal` based project:

```toml
[dependencies]
esp-idf-hal = "0.44"
esp-idf-svc = "0.49"
embedded-hal = "1.0"
log = "0.4"
```

The driver itself only depends on `embedded-hal` traits (`embedded_hal::i2c::I2c`), making it portable to any platform. The ESP32 main.rs will use `esp-idf-hal` to get the I2C peripheral and pass it to the driver.

### Example Usage (main.rs)

```rust
use esp_idf_hal::i2c::{I2cConfig, I2cDriver};
use esp_idf_hal::peripherals::Peripherals;

fn main() {
    let peripherals = Peripherals::take().unwrap();
    let i2c = I2cDriver::new(
        peripherals.i2c0,
        peripherals.pins.gpio21, // SDA
        peripherals.pins.gpio22, // SCL
        &I2cConfig::new().baudrate(400.kHz().into()),
    ).unwrap();

    let mut ina = Ina228::new(i2c, 0x40);

    // Verify communication
    assert_eq!(ina.manufacturer_id(), 0x5449);
    assert_eq!(ina.device_id(), 0x2280);

    // Configure: continuous all, 1052µs conversion, 64x averaging
    ina.configure(
        OperatingMode::ContinuousAll,
        ConversionTime::Us1052,
        ConversionTime::Us1052,
        ConversionTime::Us1052,
        AveragingCount::N64,
    );

    // Calibrate for 10A max with 10mΩ shunt
    ina.calibrate(10.0, 0.01);

    loop {
        let m = ina.read_all();
        log::info!(
            "V={:.3}V I={:.3}A P={:.3}W T={:.1}°C",
            m.bus_voltage_v, m.current_a, m.power_w, m.die_temp_c
        );
        // delay...
    }
}
```

## Implementation Steps

1. **Register definitions** — Constants for all register addresses, bit masks, and enum-to-value mappings
2. **I2C helpers** — `read_u16`, `read_u24`, `read_u40`, `write_u16` with big-endian handling
3. **Configuration** — `new()`, `reset()`, `configure()`, `set_adc_range()`
4. **Calibration** — `calibrate()` computing CURRENT_LSB and SHUNT_CAL
5. **Measurement reads** — All individual read methods with proper conversion
6. **Diagnostics** — `conversion_ready()`, ID reads, alert flag reads
7. **ESP32 main.rs** — Wire up I2C peripheral, init driver, continuous read loop
8. **Testing** — Verify against hardware, check values against known loads

## References

- [TI INA228 Datasheet](https://www.ti.com/lit/ds/symlink/ina228.pdf)
- [RobTillaart/INA228 Arduino library](https://github.com/RobTillaart/INA228)
- [ina226 Rust crate](https://crates.io/crates/ina226) (similar device, reference implementation)
- [esp-idf-hal I2C docs](https://docs.esp-rs.org/esp-idf-hal/esp_idf_hal/i2c/)
