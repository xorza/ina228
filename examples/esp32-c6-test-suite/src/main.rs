mod i2c;
mod suite;

use crate::i2c::EspI2c;

fn main() {
    esp_idf_sys::link_patches();

    suite::run(EspI2c::new().expect("initialize ESP32-C6 I2C master"));
}
