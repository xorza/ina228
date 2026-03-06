# INA228 Rust Driver — AI Notes

## Project Structure

```
src/
├── lib.rs          # Driver struct, public API, I2C helpers
├── registers.rs    # Register addresses, config enums (AdcRange, ConversionTime, etc.)
tests/
└── driver_tests.rs # 32 integration tests using embedded-hal-mock
examples/
└── esp32_read.rs   # ESP32 usage example (commented code, needs esp-idf toolchain)
```

## Architecture

- Platform-agnostic driver using `embedded_hal::i2c::I2c` trait
- Single `Ina228<I2C>` generic struct holding I2C bus, address, calibration state
- Mixed register sizes: `read_u16`, `read_u24`, `read_u40` internal helpers
- 20-bit signed values use sign extension from bit 19 (`read_i20`)
- 40-bit signed values use sign extension from bit 39 (`read_i40`)

## Key Constants

- Default address: `0x40` (valid range `0x40..=0x4F`)
- Manufacturer ID: `0x5449` ("TI")
- Device ID: `0x2280`

## Calibration

- `CURRENT_LSB = max_current / 2^19`
- `SHUNT_CAL = 13107.2e6 × CURRENT_LSB × R_SHUNT` (×4 for 40mV range)
- Must call `calibrate()` before reading current/power/energy/charge

## Testing

- 32 integration tests in `tests/driver_tests.rs` using `embedded-hal-mock` (eh1 feature)
- Uses I2C mock with expected transactions to verify all register reads/writes
- Covers: construction (valid/invalid addresses), reset, configure, set_adc_range, calibrate (including assertions), all measurement reads (bus voltage, shunt voltage, current, power, energy, charge, temperature), sign extension for negative values (20-bit and 40-bit), temp compensation, reset accumulators, conversion ready, manufacturer/device IDs, read_all

## Dependencies

- `embedded-hal = "1.0"` (runtime)
- `embedded-hal-mock = { version = "0.11", features = ["eh1"] }` (dev/test)
