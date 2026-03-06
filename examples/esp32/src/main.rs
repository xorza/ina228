use esp_idf_hal::i2c::{I2cConfig, I2cDriver};
use esp_idf_hal::peripherals::Peripherals;
use esp_idf_hal::units::Hertz;
use ina228::{
    AveragingCount, ConversionTime, Ina228, OperatingMode, DEFAULT_ADDRESS, DEVICE_ID,
    MANUFACTURER_ID,
};

fn main() {
    esp_idf_svc::sys::link_patches();
    esp_idf_svc::log::EspLogger::initialize_default();

    let peripherals = Peripherals::take().unwrap();
    let i2c = I2cDriver::new(
        peripherals.i2c0,
        peripherals.pins.gpio21, // SDA
        peripherals.pins.gpio22, // SCL
        &I2cConfig::new().baudrate(Hertz(400_000)),
    )
    .unwrap();

    let mut ina = Ina228::new(i2c, DEFAULT_ADDRESS);

    // Verify chip identity
    let mfr = ina.manufacturer_id();
    let dev = ina.device_id();
    let rev = ina.die_revision();
    log::info!("Manufacturer ID: 0x{:04X} (expect 0x{:04X})", mfr, MANUFACTURER_ID);
    log::info!("Device ID: 0x{:03X} rev {} (expect 0x{:03X})", dev, rev, DEVICE_ID);
    assert_eq!(mfr, MANUFACTURER_ID, "wrong manufacturer ID");
    assert_eq!(dev, DEVICE_ID, "wrong device ID");

    // Configure: continuous all, 1052µs conversion, 64x averaging
    ina.configure(
        OperatingMode::ContinuousAll,
        ConversionTime::Us1052,
        ConversionTime::Us1052,
        ConversionTime::Us1052,
        AveragingCount::N64,
    );

    // Calibrate for 10A max current, 10mΩ shunt resistor
    ina.calibrate(10.0, 0.01);

    log::info!("INA228 initialized, reading measurements...");

    loop {
        while !ina.conversion_ready() {
            std::thread::sleep(std::time::Duration::from_millis(10));
        }

        let m = ina.read_all();
        log::info!(
            "Bus={:.3}V Shunt={:.6}V I={:.4}A P={:.4}W T={:.1}°C",
            m.bus_voltage_v,
            m.shunt_voltage_v,
            m.current_a,
            m.power_w,
            m.die_temp_c,
        );

        std::thread::sleep(std::time::Duration::from_secs(1));
    }
}
