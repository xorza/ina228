## Unreleased

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
