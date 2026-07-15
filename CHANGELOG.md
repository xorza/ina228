## Unreleased

### Changed
- **Breaking:** `Ina228::new()` now reads CONFIG to synchronize the active ADC range and returns `Result<Self, InitializationError<I2C>>`; both invalid addresses and CONFIG-read failures return ownership of the I2C bus.
- **Breaking:** `configure()` now accepts an `AdcConfig` struct with named fields instead of five positional parameters.
- `AdcConfig::default()` matches the datasheet ADC_CONFIG reset value.
- Renamed `DiagnosticFlags::memory_status` to `memory_ok` and removed its `Default` implementation.
- `configure_alerts()` now writes DIAG_ALRT directly; configuring alerts acknowledges any latched alert flags.
- Calibration is now tracked explicitly and all calibration-dependent operations enforce their precondition in release builds.
- **Breaking:** ADC range changes now suspend conversions and disable the range-dependent shunt alert thresholds before updating CONFIG and SHUNT_CAL.
- **Breaking:** Replaced `conversion_ready()` and `diagnostic_flags()` with the explicit `take_diagnostic_flags()` acknowledgement operation.
- **Breaking:** Replaced separate `energy()` and `charge()` reads with `take_accumulator_snapshot()`, which returns both values and their pre-read diagnostic state in `AccumulatorSnapshot`.
- **Breaking:** `take_accumulator_snapshot()` now returns `ConfigurationError::AccumulatorMode` outside continuous conversion modes and briefly suspends conversion for a coherent capture.
- **Breaking:** Fallible methods now return `Error<I2C::Error>`, distinguishing I2C failures from `ConfigurationError` values.
- Physical-unit setters round to the nearest register value instead of truncating.

### Fixed
- Range-dependent readings and calibration now use the ADC range already active when the driver is constructed.
- Corrected the DIAG_ALRT bit mapping for alert latching, memory status, energy and charge overflow, and math overflow.
- ADC range changes now validate SHUNT_CAL before changing CONFIG and invalidate calibration if the SHUNT_CAL rewrite fails.
- Rejected non-finite and unrepresentable calibration, threshold, power-limit, and temperature-coefficient inputs before I2C access.
- Programmed SHUNT_TEMPCO before enabling temperature compensation so partial failures cannot activate a stale coefficient.
- Calibration now resets ENERGY and CHARGE before becoming valid, preventing accumulated samples from being interpreted with a different `CURRENT_LSB` scale.
- ADC range transitions no longer allow conversions to run with mismatched ADCRANGE and SHUNT_CAL values.
- Calibration and temperature-compensation changes now suspend conversions and restart active modes, clearing stale conversion-ready state before new-scale results are consumed.
- Both ADC_CONFIG shutdown encodings are preserved across calibration, range, and temperature-compensation changes.
- Diagnostic and accumulator reads no longer discard clear-on-read status without returning the captured flags.
- Accumulator snapshots no longer race continuous updates between DIAG_ALRT, ENERGY, and CHARGE reads.

## 0.2.0 - 2026-04-27

### Changed
- **Breaking:** `configure_alerts(latch, active_high, conversion_ready, slow_alert)` now takes a single `AlertConfig` struct with named fields and a `Default` impl, replacing four positional `bool` parameters.

### Added
- `AlertConfig` struct in the public API.
- Tests for previously unasserted `DiagnosticFlags` bits (`memory_status`, `energy_overflow`, `math_overflow`, `shunt_under_limit`, `bus_under_limit`, `charge_overflow`), the 14-bit mask in `set_temp_compensation`, and additional `configure_alerts` cases.

## 0.1.4

Baseline of the public API:

- `Ina228::new` / `release`, `reset`, `configure`, `calibrate`, `set_adc_range`.
- Measurement readers: `bus_voltage`, `shunt_voltage`, `current`, `power`, `energy`, `charge`, `die_temperature`.
- Accumulator: `reset_accumulators`.
- Conversion / diagnostics: `conversion_ready`, `diagnostic_flags`, `DiagnosticFlags`.
- Alerts: `configure_alerts` (positional bool parameters), `set_shunt_overvoltage_limit`, `set_shunt_undervoltage_limit`, `set_bus_overvoltage_limit`, `set_bus_undervoltage_limit`, `set_temperature_limit`, `set_power_limit`.
- Temperature compensation: `set_temp_compensation`, `disable_temp_compensation`.
- Identification: `manufacturer_id`, `device_id`, `die_revision`, plus `DEFAULT_ADDRESS`, `MANUFACTURER_ID`, `DEVICE_ID` constants.
- Enums: `AdcRange`, `OperatingMode`, `ConversionTime`, `AveragingCount`.
