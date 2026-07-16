pub(crate) mod cases;

use std::fmt::Debug;

use embedded_hal::digital::InputPin;
use embedded_hal::i2c::I2c;
use ina228::{DEFAULT_ADDRESS, Ina228, InitializationError};

pub(crate) type TestResult<T = ()> = Result<T, String>;

pub(crate) trait ResultContext<T> {
    fn context(self, operation: &str) -> TestResult<T>;
}

impl<T, E: Debug> ResultContext<T> for Result<T, E> {
    fn context(self, operation: &str) -> TestResult<T> {
        self.map_err(|error| format!("{operation}: {error:?}"))
    }
}

#[derive(Debug, Default)]
struct Summary {
    passed: u32,
    failed: u32,
    skipped: u32,
}

impl Summary {
    fn record(&mut self, name: &str, result: TestResult) {
        match result {
            Ok(()) => {
                self.passed += 1;
                println!("[PASS] {name}");
            }
            Err(message) => {
                self.failed += 1;
                println!("[FAIL] {name}: {message}");
            }
        }
    }

    fn run<I2C, F>(&mut self, name: &str, ina: &mut Ina228<I2C>, test: F)
    where
        I2C: I2c,
        I2C::Error: Debug,
        F: FnOnce(&mut Ina228<I2C>) -> TestResult,
    {
        println!("[RUN ] {name}");
        self.record(name, test(ina));
    }

    fn skip(&mut self, name: &str, reason: &str) {
        self.skipped += 1;
        println!("[SKIP] {name}: {reason}");
    }

    fn finish(&self) {
        println!(
            "INA228 hardware test suite complete: {} passed, {} failed, {} skipped",
            self.passed, self.failed, self.skipped
        );
        assert_eq!(self.failed, 0, "INA228 hardware test suite failed");
    }
}

pub(crate) fn require(condition: bool, message: impl Into<String>) -> TestResult {
    if condition {
        Ok(())
    } else {
        Err(message.into())
    }
}

pub(crate) fn run<I2C, ALERT>(i2c: I2C, mut alert: ALERT)
where
    I2C: I2c,
    I2C::Error: Debug,
    ALERT: InputPin,
    ALERT::Error: Debug,
{
    println!("Starting ESP32-C6 + INA228 hardware test suite");
    let mut summary = Summary::default();

    let i2c = match Ina228::new(i2c, DEFAULT_ADDRESS - 1) {
        Err(InitializationError::InvalidAddress { i2c, address }) => {
            summary.record(
                "constructor rejects invalid address",
                require(
                    address == DEFAULT_ADDRESS - 1,
                    format!("returned address 0x{address:02X}"),
                ),
            );
            i2c
        }
        Err(InitializationError::I2c { i2c, error }) => {
            summary.record(
                "constructor rejects invalid address",
                Err(format!("unexpected I2C access: {error:?}")),
            );
            i2c
        }
        Ok(ina) => {
            summary.record(
                "constructor rejects invalid address",
                Err("accepted address below 0x40".into()),
            );
            ina.release()
        }
    };

    let mut ina = match Ina228::new(i2c, DEFAULT_ADDRESS) {
        Ok(ina) => {
            summary.record("constructor reads live CONFIG", Ok(()));
            ina
        }
        Err(InitializationError::InvalidAddress { .. }) => {
            summary.record(
                "constructor reads live CONFIG",
                Err("rejected DEFAULT_ADDRESS".into()),
            );
            summary.finish();
            unreachable!();
        }
        Err(InitializationError::I2c { error, .. }) => {
            summary.record(
                "constructor reads live CONFIG",
                Err(format!("could not communicate with INA228: {error:?}")),
            );
            summary.finish();
            unreachable!();
        }
    };

    summary.run("identity registers", &mut ina, cases::identity);
    summary.run("reset and default conversion", &mut ina, cases::reset);
    summary.run("ADC shutdown mode", &mut ina, cases::adc_shutdown);
    for case in cases::ADC_MODE_CASES {
        let name = format!("ADC mode {}", case.name);
        summary.run(&name, &mut ina, |ina| cases::adc_mode(ina, case));
    }
    for case in cases::CONVERSION_TIME_CASES {
        let name = format!("ADC conversion time {}", case.name);
        summary.run(&name, &mut ina, |ina| cases::adc_conversion_time(ina, case));
    }
    for case in cases::AVERAGING_CASES {
        let name = format!("ADC averaging {}", case.name);
        summary.run(&name, &mut ina, |ina| cases::adc_averaging(ina, case));
    }
    summary.run(
        "ADC ranges and calibration",
        &mut ina,
        cases::ranges_and_calibration,
    );
    summary.run(
        "temperature compensation and measurements",
        &mut ina,
        cases::measurements,
    );
    summary.run(
        "energy and charge accumulators",
        &mut ina,
        cases::accumulators,
    );
    summary.run("ALERT pin active-low transparent", &mut ina, |ina| {
        cases::alert_active_low(ina, &mut alert)
    });
    summary.run("ALERT pin active-high transparent", &mut ina, |ina| {
        cases::alert_active_high(ina, &mut alert)
    });
    summary.run("ALERT pin latch and acknowledge", &mut ina, |ina| {
        cases::alert_latch(ina, &mut alert)
    });
    summary.run("ALERT pin conversion-ready", &mut ina, |ina| {
        cases::alert_conversion_ready(ina, &mut alert)
    });
    summary.skip(
        "ALERT pin slow-alert timing",
        "fixture has no controllable transient; control-bit encoding is covered by host tests",
    );
    for case in cases::ALERT_THRESHOLD_CASES {
        let name = format!("alert threshold flag: {}", case.name());
        summary.run(&name, &mut ina, |ina| cases::alert_threshold(ina, case));
    }

    let _i2c = ina.release();
    summary.record("release returns the I2C bus", Ok(()));
    summary.finish();
}
