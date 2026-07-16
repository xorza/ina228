# ESP32-C6 + INA228 hardware test suite

This firmware exercises the complete public INA228 driver API against a real device and prints a pass/fail result for each phase. It continues after ordinary test failures so the final summary shows every independent phase that could run.

ADC modes are validated from their measured conversion duration and recurrence: one-, two-, and three-channel modes have distinct completion windows, continuous modes must produce a second conversion, and triggered modes must remain idle after their first conversion. Conversion-time and averaging encodings are measured independently with amplified test configurations so adjacent enum values cannot share the same timing window.

## Fixture

- ESP32-C6 GPIO8 to INA228 SDA
- ESP32-C6 GPIO9 to INA228 SCL
- ESP32-C6 GPIO7 to INA228 ALERT
- A 4.7 kΩ to 10 kΩ pull-up from ALERT to the ESP32-C6 3.3 V rail
- Common ground and a valid INA228 supply
- INA228 A0 and A1 tied to ground for I2C address `0x40`
- A 2 mΩ shunt installed in the positive current direction
- Monitored current below 10 A, keeping the shunt voltage inside ±20 mV
- A powered monitored bus above 0.1 V
- A steady positive-direction load above 0.05 W

The last two requirements provide real stimulus for the bus-voltage and power-alert checks. Change the fixture constants at the top of `src/suite/cases.rs` if the board uses a different shunt or current range.

The suite validates ALERT active-low and active-high polarity, transparent and latched behavior, acknowledgement, and conversion-ready assertion on GPIO7. Slow-alert timing is reported as skipped because the steady fixture has no controllable transient source; its control-bit encoding remains covered by the host-side `configure_alerts_encodes_each_control_bit` test.

## Run

Install the ESP-IDF Rust prerequisites and connect the ESP32-C6 over USB, then run:

```sh
cd examples/esp32-c6-test-suite
cargo run --release
```

The monitor ends with a summary such as `58 passed, 0 failed, 1 skipped`. ADC modes, conversion times, averaging counts, invalid-input cases, and alert thresholds each receive their own result and device preparation boundary, so an early failure does not suppress later cases. Any failure includes the operation, measured value, and expected invariant. The firmware panics after the summary if one or more phases failed, which makes the result visible to automated serial-log runners.
