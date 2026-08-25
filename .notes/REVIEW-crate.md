# Crate review — `ina228` (`src/lib.rs`, `src/registers.rs`, `src/config.rs`, `src/scale.rs`)

**Delete each item as you address it.** This file lists open findings only; a resolved
finding is removed, not annotated.

Scope: the published crate (`src/`). `tests/driver_tests.rs` and
`examples/esp32-c6-test-suite/` are test harnesses and were not reviewed — they can be
rewritten to fit whatever shape production takes.

---

## A failed restore discards a completed accumulator capture

- [ ] `take_accumulator_snapshot` builds a complete, valid `AccumulatorSnapshot`
      (`src/lib.rs:423-432`) and then drops it when the ADC_CONFIG restore fails
      (`src/lib.rs:607`). The clear-on-read side effects already happened, so those counts
      are gone for good. `Result<AccumulatorSnapshot, _>` can carry the snapshot or the
      restore error, not both.

## The ADC_CONFIG mode field is re-encoded in four places

- [ ] The field's `<< 12` position is hardcoded in `configure` (`src/lib.rs:285`) and
      baked separately into `MODE_MASK`, `ALTERNATE_SHUTDOWN_MODE`, and
      `FIRST_CONTINUOUS_MODE` (`src/registers.rs:29-31`), which store pre-shifted values
      while `OperatingMode`'s discriminants are unshifted.
- [ ] There is no decode from a read-back ADC_CONFIG word to `OperatingMode`, so mode
      semantics are re-derived from raw bits with two different ad-hoc predicates:
      `mode == 0 || mode == ALTERNATE_SHUTDOWN_MODE` in `with_conversions_suspended`
      (`src/lib.rs:601`) and `mode < FIRST_CONTINUOUS_MODE` in
      `take_accumulator_snapshot` (`src/lib.rs:418`).
- [ ] CONFIG has a typed representation in `src/config.rs` while ADC_CONFIG has none:
      its three bit constants sit loose in `src/registers.rs` (`36-38`) and `configure`
      re-encodes the mode field inline. Two registers of the same device, modelled two
      different ways.

## Register-word access helpers are four near-copies

- [ ] `read_u24` (`src/lib.rs:623`) and `read_u40` (`src/lib.rs:635`) differ only in
      buffer length and the byte offset the read lands at.
- [ ] `read_i20` (`src/lib.rs:630`) and `read_i40` (`src/lib.rs:642`) differ only in the
      sign-extension shift width.
- [ ] The private I2C helpers disagree on where the bus error is widened: `read_u16`
      returns the raw `I2C::Error` (`src/lib.rs:610`), `write_u16` wraps it with
      `.map_err(Error::I2c)` (`src/lib.rs:616`), and `read_u24` / `read_u40` wrap via `?`.

## Missing calibration is enforced by four hand-written panics

- [ ] `.expect("call calibrate() before …")` is repeated with four different messages at
      `src/lib.rs:392`, `401`, `415`, `522`. The precondition is a property of the driver's
      state, restated once per consumer.
- [ ] The crate is inconsistent about how misuse is reported: an out-of-range temperature
      coefficient is `Error::InvalidConfiguration`, but reading `current()` uncalibrated
      is a panic in release builds. Both are caller sequencing errors.

## One 646-line `lib.rs` holds every type in the crate

Against the stated conventions in `CLAUDE.md`:

- [ ] `Ina228`, `AdcConfig`, `AlertConfig`, `DiagnosticFlags`, `AccumulatorSnapshot`,
      `Calibration`, `ConfigurationError`, `Error`, and `InitializationError` all live
      in `src/lib.rs` (`Config` and the scale factors now have their own files) — "one major struct, one file, same
      name" and "`error.rs` is for errors only" are both unmet, and there is no `error.rs`.
- [ ] `src/registers.rs` is a grab bag: the private `Register` address map, two inline
      bit-constant modules, and four public configuration enums with the per-range scale
      derivations hung off `AdcRange`. The file name describes only the first of these.
- [ ] `src/lib.rs` and `src/registers.rs` have no `//!` module doc, though `src/scale.rs`
      and `src/config.rs` set the precedent. `lib.rs` opens straight into `#![no_std]`, so docs.rs has no
      landing page for the crate.

## Bit state is expanded into wide, undocumented structs at the API boundary

- [ ] `take_diagnostic_flags` decodes DIAG_ALRT into eleven `bool` fields with eleven
      hand-written `d & MASK != 0` lines (`src/lib.rs:452-462`) — an 11-byte struct and
      eleven test-and-store sequences to carry two bytes of device state on an MCU target.
- [ ] Ten of `DiagnosticFlags`'s eleven public fields are undocumented
      (`src/lib.rs:219-230`); only `memory_ok` has a `///`. Two of
      `AccumulatorSnapshot`'s three are undocumented (`src/lib.rs:237-238`). All variants
      of `ConversionTime`, `AveragingCount`, and `OperatingMode` are undocumented
      (`src/registers.rs:127-175`) — `TriggeredTempBus` vs `TriggeredTempShunt` is not
      self-explaining. The crate has no `#![deny(missing_docs)]` to catch this.
- [ ] The `AlertConfig` example is a ```` ```ignore ```` doctest (`src/lib.rs:201`) —
      never compiled, free to rot.

## Error types cannot be used for anything but `Debug` printing

- [ ] None of `Error`, `ConfigurationError`, or `InitializationError` implements
      `core::fmt::Display` or `core::error::Error`, both of which are available in
      `no_std` on edition 2024. Callers cannot print a failure or propagate it into any
      error-handling crate.
- [ ] `InitializationError` derives `Debug` with the derive's implicit bounds
      (`src/lib.rs:53-54`), so it is only `Debug` when the I2C bus type is. The README's own
      usage example has to work around this with
      `.unwrap_or_else(|_| panic!("failed to read INA228 CONFIG"))` instead of `.unwrap()`.
- [ ] `InitializationError<I2C: I2c>` carries the bound on the type definition, forcing it
      onto every signature that names the type.

## Read-back and identity gaps push work onto callers

- [ ] `device_id` and `die_revision` (`src/lib.rs:538-545`) each issue a separate I2C read
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
      (`src/lib.rs:449`). The README's own polling loop demonstrates the hazard and warns
      about it in a comment rather than the API preventing it.
- [ ] `set_adc_range` clobbers SOVL and SUVL to their extremes and never restores them
      (`src/lib.rs:310-311`), discarding caller configuration the driver has enough
      information to rescale — both shunt limit LSBs are already known to `AdcRange`.
- [ ] `configure` writes ADC_CONFIG (register 0x01) but is named for the CONFIG register
      (0x00) that it does not touch; the register actually called CONFIG is written by
      `set_adc_range`, `set_temp_compensation`, and `reset_accumulators`.
