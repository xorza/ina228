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
        peripherals.pins.gpio8, // SDA
        peripherals.pins.gpio9, // SCL
        &I2cConfig::new().baudrate(Hertz(400_000)),
    )
    .unwrap();

    let mut ina = Ina228::new(i2c, DEFAULT_ADDRESS);

    // Verify chip identity
    let mfr = ina.manufacturer_id().unwrap();
    let dev = ina.device_id().unwrap();
    let rev = ina.die_revision().unwrap();
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
    )
    .unwrap();

    // Calibrate for 10A max current, 2mΩ shunt resistor (R002)
    ina.calibrate(10.0, 0.002).unwrap();

    log::info!("INA228 initialized, reading measurements...");

    loop {
        while !ina.conversion_ready().unwrap() {
            std::thread::sleep(std::time::Duration::from_millis(10));
        }

        let bus_v = ina.bus_voltage().unwrap();
        let shunt_v = ina.shunt_voltage().unwrap();
        let current = ina.current().unwrap();
        let power = ina.power().unwrap();
        let temp = ina.die_temperature().unwrap();
        log::info!(
            "Bus={bus_v:.3}V Shunt={shunt_v:.6}V I={current:.4}A P={power:.4}W T={temp:.1}°C",
        );

        std::thread::sleep(std::time::Duration::from_secs(1));
    }
}
