# ina228

<img src="docs/ina228.png" alt="INA228 breakout board" width="400" />

Platform-agnostic, `no_std` Rust driver for the [TI INA228](https://www.ti.com/product/INA228) high-side power/energy/charge monitor, built on [`embedded-hal`](https://crates.io/crates/embedded-hal) 1.0.

The INA228 measures bus voltage (0-85V), shunt voltage, current, power, energy, and charge over I2C with 20-bit ADC resolution.

## Installation

```toml
[dependencies]
ina228 = "0.2"
```

## Usage

```rust
use ina228::{
    AdcConfig, AveragingCount, Ina228, DEFAULT_ADDRESS,
};

let mut ina = Ina228::new(i2c, DEFAULT_ADDRESS)
    .unwrap_or_else(|_| panic!("failed to read INA228 CONFIG"));

// Configure: continuous bus+shunt+temp, 1052µs conversion, 64x averaging
ina.configure(AdcConfig {
    averaging: AveragingCount::N64,
    ..Default::default()
})
.unwrap();

// Calibrate for 10A max expected current, 2mΩ shunt resistor
ina.calibrate(10.0, 0.002).unwrap();

// Polling acknowledges every snapshot; production code must handle every returned flag.
loop {
    let flags = ina.take_diagnostic_flags().unwrap();
    if flags.conversion_ready {
        break;
    }
}
let voltage = ina.bus_voltage().unwrap();
let current = ina.current().unwrap();
let power = ina.power().unwrap();
let temp = ina.die_temperature().unwrap();
```

## Features

- `no_std` compatible — works on any platform with `embedded-hal` 1.0 I2C
- Bus voltage, shunt voltage, current, power, energy, and charge measurements
- Configurable ADC conversion time and averaging
- Two shunt voltage ranges: ±163.84mV and ±40.96mV
- Alert thresholds for shunt/bus voltage, temperature, and power
- Diagnostic flags for overflow and limit detection
- Shunt temperature compensation
- Energy and charge accumulators with reset

## Calibration

Call `calibrate(max_current_a, shunt_resistance_ohm)` before reading current, power, energy, or charge. The `max_current_a` parameter sets the measurement resolution — use the maximum current your load will draw, not the theoretical maximum of the shunt.

Calibration suspends conversions, writes SHUNT_CAL, resets the energy and charge accumulators, and restores the previous ADC configuration. Restoring a running mode starts a fresh conversion and clears the old conversion-ready flag. `calibrate()` does not wait synchronously; poll `take_diagnostic_flags()` until `conversion_ready` before consuming the new-scale measurements. A previous shutdown mode remains in shutdown, so call `configure()` with a conversion-producing mode before polling. If SHUNT_CAL is written but the accumulator reset fails, calibration-dependent operations remain unavailable until `calibrate()` succeeds again.

Enabling, changing, or disabling shunt temperature compensation uses the same suspend-and-restore transition. Poll for conversion-ready before consuming newly compensated measurements; if the ADC was already shut down, configure or trigger a conversion first.

If you change the ADC range via `set_adc_range()` after calling `calibrate()`, the SHUNT_CAL register is automatically recalculated. Range changes suspend conversions and disable the shunt over- and under-voltage alerts because those thresholds use a range-dependent scale; configure both thresholds again afterward. The previous ADC configuration is restored on success. `set_adc_range()` does not wait for fresh data, so the caller must wait for a new conversion before reading measurements produced under the new range.

An I2C failure during calibration, range, or temperature-compensation changes after conversions are suspended leaves the ADC in shutdown mode. A range failure may also have disabled one or both shunt alerts. Call `configure()` to resume conversions. If the range update succeeds but the SHUNT_CAL write fails, call `calibrate()` again before using current, power, energy, charge, or power-limit operations.

`take_diagnostic_flags()` reads and acknowledges DIAG_ALRT, including conversion-ready and any latched threshold alerts. `take_accumulator_snapshot()` returns energy, charge, and the complete diagnostic snapshot captured before reading ENERGY and CHARGE clears their overflow indicators.

Fallible methods return `Error<I2C::Error>`. Invalid, non-finite, or unrepresentable physical configuration values return `Error::InvalidConfiguration`; bus failures return `Error::I2c`. Thresholds are rounded to the nearest register value.

Construction reads CONFIG over I2C so the driver uses the ADC range already active in the device. `Ina228::new()` rejects addresses outside `0x40..=0x4F` with `InitializationError::InvalidAddress` and reports CONFIG read failures with `InitializationError::I2c`. Both variants return ownership of the I2C bus so the caller can recover the peripheral or retry initialization.

`AdcConfig::default()` matches the datasheet ADC_CONFIG reset value: continuous conversion of all channels, 1052 µs conversion times, and one-sample averaging.

## I2C Addresses

The INA228 supports 16 addresses (0x40-0x4F) configured via A0 and A1 pins:

| A1  | A0  | Address |
|-----|-----|---------|
| GND | GND | 0x40    |
| GND | VS  | 0x41    |
| GND | SDA | 0x42    |
| GND | SCL | 0x43    |
| VS  | GND | 0x44    |
| VS  | VS  | 0x45    |
| VS  | SDA | 0x46    |
| VS  | SCL | 0x47    |
| SDA | GND | 0x48    |
| SDA | VS  | 0x49    |
| SDA | SDA | 0x4A    |
| SDA | SCL | 0x4B    |
| SCL | GND | 0x4C    |
| SCL | VS  | 0x4D    |
| SCL | SDA | 0x4E    |
| SCL | SCL | 0x4F    |

## ESP32 Example

A complete ESP32 example is in [`examples/esp32/`](examples/esp32/). It requires the ESP-IDF Rust toolchain and an ESP32 with an INA228 connected via I2C (GPIO8 SDA, GPIO9 SCL). The example's `.cargo/config.toml` is preconfigured for ESP32-C6; adjust `target` and `MCU` for other RISC-V variants (e.g. C3, C2).

```sh
cd examples/esp32
cargo run --release
```

## License

Licensed under either of [Apache License, Version 2.0](http://www.apache.org/licenses/LICENSE-2.0) or [MIT license](http://opensource.org/licenses/MIT) at your option.
