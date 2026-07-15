# AGENTS.md

Repository guidance for contributors working with this codebase.

## Project

Platform-agnostic `no_std` Rust driver for the TI INA228 power/energy/charge monitor, built on `embedded-hal` 1.0. Published as the `ina228` crate.

## Commands

```sh
cargo build                              # build the library
cargo build --release
cargo test                               # run integration tests in tests/driver_tests.rs
cargo test --test driver_tests <name>    # run a single test by name
cargo clippy --all-targets
cargo fmt
```

ESP32 example (separate workspace under `examples/esp32/`, requires the ESP-IDF Rust toolchain):

```sh
cd examples/esp32
cargo run --release        # default target is ESP32-C6; edit .cargo/config.toml + MCU env for other RISC-V variants
```

The example is excluded from the published crate via `exclude = ["examples/"]` in `Cargo.toml` and is not part of the main `cargo` workspace — `cargo test` from the repo root does **not** build it.

## Architecture

Two-file driver:

- `src/lib.rs` — the `Ina228<I2C>` struct and all public API. Methods take `&mut self` and an `embedded-hal` 1.0 `I2c` bus. Fallible methods return `Error<I2C::Error>`, separating bus failures from invalid physical configuration.
- `src/registers.rs` — the internal `Register` address enum plus the public configuration enums (`AdcRange`, `ConversionTime`, `AveragingCount`, `OperatingMode`) re-exported from `lib.rs`.

Key state held in `Ina228`: `calibration: Option<Calibration>` (containing `current_lsb` and `shunt_resistance_ohm`) and `adc_range`. Construction reads CONFIG and synchronizes `adc_range` with the live device. Calibration is absent after construction, reset, or a failed range-dependent SHUNT_CAL rewrite. Current, power, energy, charge, power-limit, and SHUNT_CAL calculations depend on it.

`set_adc_range()` precomputes the new SHUNT_CAL because the calibration constant differs by 4× between ranges, writes CONFIG, then rewrites SHUNT_CAL. A failed CONFIG write preserves the old state; a failed SHUNT_CAL write preserves the new range and invalidates calibration. Preserve this transition for new range/calibration paths.

Physical-unit setters validate finite and representable inputs before I2C, then round to the nearest register value. Temperature compensation writes SHUNT_TEMPCO before enabling TEMPCOMP so a partial failure cannot activate a stale coefficient.

`AdcConfig` and `AlertConfig` provide named fields for register configuration. `AdcConfig::default()` matches the datasheet ADC_CONFIG reset value. Keep these APIs named rather than reintroducing positional parameters with repeated types.

## Testing

Tests use `embedded-hal-mock` (eh1 feature) to script expected I2C transactions. When adding a method, add a test in `tests/driver_tests.rs` that asserts the exact register read/write bytes — the mock will fail loudly on any deviation. Tests double as the spec for register encoding.

## Conventions

- The crate is `no_std`; do not pull in `std` or `alloc`.
- Public enums use explicit `#[repr(u8)]` / `#[repr(u16)]` with discriminants matching the datasheet bit patterns — keep them aligned with the INA228 datasheet, not renumbered for ergonomics.
- MSRV / edition: Rust 2024.
- Maintain a `CHANGELOG.md` entry for user-visible changes; bump version in `Cargo.toml` and update README install snippet on release.
