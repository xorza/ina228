# ESP32-C6 + INA228 hardware test suite

This firmware exercises the complete public INA228 driver API against a real device and prints a pass/fail result for each phase. It continues after ordinary test failures so the final summary shows every independent phase that could run.

## Fixture

- ESP32-C6 GPIO8 to INA228 SDA
- ESP32-C6 GPIO9 to INA228 SCL
- Common ground and a valid INA228 supply
- INA228 A0 and A1 tied to ground for I2C address `0x40`
- A 2 mΩ shunt installed in the positive current direction
- Monitored current below 10 A, keeping the shunt voltage inside ±20 mV
- A powered monitored bus above 0.1 V
- A steady positive-direction load above 0.05 W

The last two requirements provide real stimulus for the bus-voltage and power-alert checks. Change the fixture constants at the top of `src/suite/cases.rs` if the board uses a different shunt or current range.

## Run

Install the ESP-IDF Rust prerequisites and connect the ESP32-C6 over USB, then run:

```sh
cd examples/esp32-c6-test-suite
cargo run --release
```

The monitor ends with a summary such as `11 passed, 0 failed`. Any failure includes the operation, measured value, and expected invariant. The firmware panics after the summary if one or more phases failed, which makes the result visible to automated serial-log runners.
