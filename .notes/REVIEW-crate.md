# Crate review — `ina228` (`src/*.rs`)

**Delete each item as you address it.** This file lists open findings only; a resolved
finding is removed, not annotated.

Scope: the published crate (`src/`). `tests/driver_tests.rs` and
`examples/esp32-c6-test-suite/` are test harnesses and were not reviewed — they can be
rewritten to fit whatever shape production takes.

---

## Read-back and identity gaps push work onto callers

- [ ] `device_id` and `die_revision` (`src/ina228.rs:350-357`) each issue a separate I2C read
      of the same register 0x3F; a caller wanting both pays two transactions for two
      halves of one word.
- [ ] `MANUFACTURER_ID` and `DEVICE_ID` are exported but never used by the driver, so the
      identity check they exist for is re-implemented by every consumer — including the
      hardware suite in this repo.
- [ ] The cached `adc_range` is not exposed; a caller cannot ask which range is active
      without reading and decoding CONFIG themselves, and no read-back of ADC_CONFIG as
      an `AdcConfig`.
- [ ] The only way to poll for conversion-ready is `take_diagnostic_flags`, which
      acknowledges every other latched threshold alert as a side effect
      (`src/ina228.rs:278`). The README's own polling loop demonstrates the hazard and warns
      about it in a comment rather than the API preventing it.
- [ ] `set_adc_range` clobbers SOVL and SUVL to their extremes and never restores them
      (`src/ina228.rs:127-128`), discarding caller configuration the driver has enough
      information to rescale — both shunt limit LSBs are already known to `AdcRange`.
- [ ] `configure` writes ADC_CONFIG (register 0x01) but is named for the CONFIG register
      (0x00) that it does not touch; the register actually called CONFIG is written by
      `set_adc_range`, `set_temp_compensation`, and `reset_accumulators`.
