# INA228 Rust Driver — AI Notes

## Project Structure

```
src/
├── lib.rs          # Driver struct, public API, I2C helpers
├── registers.rs    # Register addresses, config enums (AdcRange, ConversionTime, etc.)
tests/
└── driver_tests.rs # 42 integration tests using embedded-hal-mock
examples/
└── esp32/          # Standalone ESP32 project (esp-idf-hal + esp-idf-svc)
    ├── Cargo.toml
    ├── .cargo/config.toml  # target xtensa-esp32-espidf, ESP_IDF v5.4.3
    ├── rust-toolchain.toml # channel = "esp"
    ├── build.rs
    ├── sdkconfig.defaults
    └── src/main.rs         # Reads INA228 on GPIO21/22, 2mΩ shunt (R002)
```

## Architecture

- `#![no_std]` platform-agnostic driver using `embedded_hal::i2c::I2c` trait
- Single `Ina228<I2C>` generic struct holding I2C bus, address, calibration state (current_lsb, shunt_resistance_ohm, adc_range)
- Mixed register sizes: `read_u16`, `read_u24`, `read_u40` internal helpers
- 20-bit signed values use arithmetic shift sign extension (`read_i20`)
- 40-bit signed values use arithmetic shift sign extension (`read_i40`)

## Key Constants

- Default address: `0x40` (valid range `0x40..=0x4F`)
- Manufacturer ID: `0x5449` ("TI")
- Device ID: `0x228` (upper 12 bits of register 0x3F; lower 4 bits are die revision)

## Public API

### Measurements
- `bus_voltage()` → f32 (Volts)
- `shunt_voltage()` → f32 (Volts, sign depends on current direction)
- `current()` → f32 (Amps, requires calibrate())
- `power()` → f32 (Watts, requires calibrate())
- `energy()` → f64 (Joules, accumulator, requires calibrate())
- `charge()` → f64 (Coulombs, signed accumulator, requires calibrate())
- `die_temperature()` → f32 (°C)
- `read_instant()` → Measurements struct (bus, shunt, current, power, temp)

### Configuration
- `reset()` — soft reset all registers
- `configure(mode, vbus_ct, vshunt_ct, temp_ct, avg)` — ADC config
- `set_adc_range(range)` — ±163.84mV or ±40.96mV shunt range; auto-recalibrates SHUNT_CAL if already calibrated
- `calibrate(max_current_a, shunt_resistance_ohm)` — required before current/power/energy/charge
- `set_temp_compensation(tempco_ppm)` — enable shunt temp compensation
- `disable_temp_compensation()` — disable shunt temp compensation
- `reset_accumulators()` — clear energy/charge registers

### Alerts & Diagnostics
- `diagnostic_flags()` → DiagnosticFlags struct (all alert/overflow flags)
- `configure_alerts(latch, active_high, conversion_ready_alert, slow_alert)`
- `set_shunt_overvoltage_limit(voltage_v)`
- `set_shunt_undervoltage_limit(voltage_v)`
- `set_bus_overvoltage_limit(voltage_v)`
- `set_bus_undervoltage_limit(voltage_v)`
- `set_temperature_limit(temp_c)`
- `set_power_limit(power_w)` — requires calibrate()

### Identity
- `manufacturer_id()` → u16
- `device_id()` → u16 (upper 12 bits)
- `die_revision()` → u8 (lower 4 bits)
- `conversion_ready()` → bool
- `release()` → I2C (consume driver, return bus)

## Calibration

- `CURRENT_LSB = max_current / 2^19`
- `SHUNT_CAL = 13107.2e6 × CURRENT_LSB × R_SHUNT` (×4 for 40mV range)
- `assert!` panics if SHUNT_CAL exceeds 15-bit max (32767) — reduce max_current or shunt_resistance
- `debug_assert!` fires if current/power/energy/charge read before calibrate()
- `set_adc_range()` auto-recalibrates via stored shunt_resistance_ohm

## Testing

- 46 integration tests in `tests/driver_tests.rs` using `embedded-hal-mock` (eh1 feature)
- Uses I2C mock with expected transactions to verify all register reads/writes
- Covers: construction, reset, configure, calibrate, all measurements, sign extension,
  temp compensation, alerts, diagnostic flags, thresholds, IDs, read_instant

## Dependencies

- `embedded-hal = "1.0"` (runtime)
- `embedded-hal-mock = { version = "0.11", features = ["eh1"] }` (dev/test)

## ESP32 Example

- Target: ESP32-WROOM-32 with INA228 board (A0=GND, A1=GND → address 0x40)
- I2C: GPIO21 (SDA), GPIO22 (SCL), 400kHz
- Shunt: R002 (2mΩ), max current 10A
- ESP-IDF: v5.4.3 (v5.5.x incompatible with esp-idf-hal 0.45)
- Deps: esp-idf-svc 0.51, esp-idf-hal 0.45, embuild 0.33
