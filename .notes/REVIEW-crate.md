# Crate review — `ina228` (`src/*.rs`)

**Delete each item as you address it.** This file lists open findings only; a resolved
finding is removed, not annotated.

Scope: the published crate (`src/`). `tests/driver_tests.rs` and
`examples/esp32-c6-test-suite/` are test harnesses and were not reviewed — they can be
rewritten to fit whatever shape production takes.

---

## Bit state is expanded into wide, undocumented structs at the API boundary

- [ ] `DiagnosticFlags::from_bits` decodes DIAG_ALRT into eleven `bool` fields with
      eleven hand-written `word & Self::MASK != 0` lines (`src/diag_alrt.rs:84-94`) — an
      11-byte struct and eleven test-and-store sequences to carry two bytes of device
      state on an MCU target.
- [ ] Ten of `DiagnosticFlags`'s eleven public fields are undocumented
      (`src/diag_alrt.rs:53-65`); only `memory_ok` has a `///`. Two of
      `AccumulatorSnapshot`'s three are undocumented (`src/ina228.rs:79-80`). All variants
      of `ConversionTime`, `AveragingCount`, and `OperatingMode` are undocumented
      (`src/adc_config.rs:11-56`) — `TriggeredTempBus` vs `TriggeredTempShunt` is not
      self-explaining. The crate has no `#![deny(missing_docs)]` to catch this.
- [ ] The `AlertConfig` example is a ```` ```ignore ```` doctest (`src/diag_alrt.rs:11`) —
      never compiled, free to rot.

## Error types cannot be used for anything but `Debug` printing

- [ ] None of `Error`, `ConfigurationError`, or `InitializationError` implements
      `core::fmt::Display` or `core::error::Error`, both of which are available in
      `no_std` on edition 2024. Callers cannot print a failure or propagate it into any
      error-handling crate.
- [ ] `InitializationError` derives `Debug` with the derive's implicit bounds
      (`src/error.rs:40-41`), so it is only `Debug` when the I2C bus type is. The README's own
      usage example has to work around this with
      `.unwrap_or_else(|_| panic!("failed to read INA228 CONFIG"))` instead of `.unwrap()`.
- [ ] `InitializationError<I2C: I2c>` carries the bound on the type definition, forcing it
      onto every signature that names the type.

## Read-back and identity gaps push work onto callers

- [ ] `device_id` and `die_revision` (`src/ina228.rs:370-377`) each issue a separate I2C read
      of the same register 0x3F; a caller wanting both pays two transactions for two
      halves of one word.
- [ ] `MANUFACTURER_ID` and `DEVICE_ID` are exported but never used by the driver, so the
      identity check they exist for is re-implemented by every consumer — including the
      hardware suite in this repo.
- [ ] The cached `adc_range` is not exposed; a caller cannot ask which range is active
      without reading and decoding CONFIG themselves. Likewise there is no read-back of
      ADC_CONFIG as an `AdcConfig`, nor of the raw DIAG_ALRT word.
- [ ] The only way to poll for conversion-ready is `take_diagnostic_flags`, which
      acknowledges every other latched threshold alert as a side effect
      (`src/ina228.rs:298`). The README's own polling loop demonstrates the hazard and warns
      about it in a comment rather than the API preventing it.
- [ ] `set_adc_range` clobbers SOVL and SUVL to their extremes and never restores them
      (`src/ina228.rs:147-148`), discarding caller configuration the driver has enough
      information to rescale — both shunt limit LSBs are already known to `AdcRange`.
- [ ] `configure` writes ADC_CONFIG (register 0x01) but is named for the CONFIG register
      (0x00) that it does not touch; the register actually called CONFIG is written by
      `set_adc_range`, `set_temp_compensation`, and `reset_accumulators`.
