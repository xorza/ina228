# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

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

- `src/lib.rs` — the `Ina228<I2C>` struct and all public API. Methods take `&mut self` and an `embedded-hal` 1.0 `I2c` bus. Errors propagate as `I2C::Error`.
- `src/registers.rs` — `Register` address enum plus the public configuration enums (`AdcRange`, `ConversionTime`, `AveragingCount`, `OperatingMode`) re-exported from `lib.rs`.

Key state held in `Ina228`: `current_lsb` and `shunt_resistance_ohm` (set by `calibrate()`) and `adc_range`. Several scaling computations (current, power, energy, charge, SHUNT_CAL) depend on these, so when changing calibration logic check all readout methods.

`set_adc_range()` automatically rewrites SHUNT_CAL because the calibration constant differs by 4× between the two ranges. Preserve this invariant for any new range/calibration paths.

`AlertConfig` is a struct passed to `configure_alerts()`; fields default to `false` so callers use struct-update syntax. Don't reintroduce the previous boolean-parameter form (see commit a5a0125).

## Testing

Tests use `embedded-hal-mock` (eh1 feature) to script expected I2C transactions. When adding a method, add a test in `tests/driver_tests.rs` that asserts the exact register read/write bytes — the mock will fail loudly on any deviation. Tests double as the spec for register encoding.

## Conventions

- The crate is `no_std`; do not pull in `std` or `alloc`.
- Public enums use explicit `#[repr(u8)]` / `#[repr(u16)]` with discriminants matching the datasheet bit patterns — keep them aligned with the INA228 datasheet, not renumbered for ergonomics.
- MSRV / edition: Rust 2024.
- Maintain a `CHANGELOG.md` entry for user-visible changes; bump version in `Cargo.toml` and update README install snippet on release.
