//! ESP32 example: continuously read INA228 measurements over I2C.
//!
//! Wiring (ESP32 to INA228):
//!   - GPIO21 (SDA) -> SDA
//!   - GPIO22 (SCL) -> SCL
//!   - 3.3V -> VS
//!   - GND -> GND
//!   - A0, A1 -> GND (address 0x40)
//!
//! This example requires esp-idf-hal and esp-idf-svc dependencies.
//! Add to Cargo.toml:
//!
//! ```toml
//! [dependencies]
//! ina228 = { path = "." }
//! esp-idf-hal = "0.44"
//! esp-idf-svc = "0.49"
//! log = "0.4"
//! ```

fn main() {
    // NOTE: This example won't compile without esp-idf toolchain and dependencies.
    // It serves as a reference for wiring up the INA228 driver on ESP32.
    //
    // ```rust
    // use esp_idf_hal::i2c::{I2cConfig, I2cDriver};
    // use esp_idf_hal::peripherals::Peripherals;
    // use esp_idf_hal::units::Hertz;
    // use ina228::{Ina228, OperatingMode, ConversionTime, AveragingCount, DEFAULT_ADDRESS};
    //
    // esp_idf_svc::sys::link_patches();
    // esp_idf_svc::log::EspLogger::initialize_default();
    //
    // let peripherals = Peripherals::take().unwrap();
    // let i2c = I2cDriver::new(
    //     peripherals.i2c0,
    //     peripherals.pins.gpio21, // SDA
    //     peripherals.pins.gpio22, // SCL
    //     &I2cConfig::new().baudrate(Hertz(400_000)),
    // ).unwrap();
    //
    // let mut ina = Ina228::new(i2c, DEFAULT_ADDRESS);
    //
    // // Verify chip identity
    // assert_eq!(ina.manufacturer_id(), 0x5449, "wrong manufacturer ID");
    // assert_eq!(ina.device_id(), 0x2280, "wrong device ID");
    //
    // // Configure: continuous bus+shunt+temp, 1052µs conversion, 64x averaging
    // ina.configure(
    //     OperatingMode::ContinuousAll,
    //     ConversionTime::Us1052,
    //     ConversionTime::Us1052,
    //     ConversionTime::Us1052,
    //     AveragingCount::N64,
    // );
    //
    // // Calibrate for 10A max current, 10mΩ shunt resistor
    // ina.calibrate(10.0, 0.01);
    //
    // loop {
    //     // Wait for conversion to complete
    //     while !ina.conversion_ready() {}
    //
    //     let m = ina.read_all();
    //     log::info!(
    //         "Bus={:.3}V Shunt={:.6}V I={:.4}A P={:.4}W T={:.1}°C",
    //         m.bus_voltage_v,
    //         m.shunt_voltage_v,
    //         m.current_a,
    //         m.power_w,
    //         m.die_temp_c,
    //     );
    //
    //     // Also read accumulated values
    //     let energy_j = ina.energy();
    //     let charge_c = ina.charge();
    //     log::info!("Energy={:.6}J Charge={:.6}C", energy_j, charge_c);
    //
    //     std::thread::sleep(std::time::Duration::from_secs(1));
    // }
    // ```

    println!("This example requires ESP32 esp-idf toolchain. See source for usage.");
}
