mod suite;

use esp_idf_hal::i2c::{I2cConfig, I2cDriver};
use esp_idf_hal::peripherals::Peripherals;
use esp_idf_hal::units::Hertz;

const I2C_BAUDRATE_HZ: u32 = 400_000;

fn main() {
    esp_idf_sys::link_patches();

    let peripherals = Peripherals::take().unwrap();
    let i2c = I2cDriver::new(
        peripherals.i2c0,
        peripherals.pins.gpio8,
        peripherals.pins.gpio9,
        &I2cConfig::new().baudrate(Hertz(I2C_BAUDRATE_HZ)),
    )
    .unwrap();

    suite::run(i2c);
}
