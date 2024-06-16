use std::{
    fmt::Display,
    sync::{Arc, Mutex},
    time::Duration,
};

use bmp390::{
    self,
    bmp390::{DeviceAddr, Osr, OsrPress, OsrTemp, PwrCtrl, Register},
};
use esp_idf_hal::i2c::I2cDriver;

#[derive(Copy, Clone, Debug)]
pub struct AltimeterStats {
    pub maximum_altitude: f64,
    pub minimum_altitude: f64,
    pub maximum_temperature: f64,
    pub minimum_temperature: f64,
    pub maximum_pressure: f64,
    pub minimum_pressure: f64,
    pub altitude: f64,
    pub temperature: f64,
    pub pressure: f64,
}

impl Default for AltimeterStats {
    fn default() -> Self {
        AltimeterStats {
            maximum_altitude: f64::MIN,
            minimum_altitude: f64::MAX,
            maximum_temperature: f64::MIN,
            minimum_temperature: f64::MAX,
            maximum_pressure: f64::MIN,
            minimum_pressure: f64::MAX,
            altitude: 0.0f64,
            temperature: 0.0f64,
            pressure: 0.0f64,
        }
    }
}

pub struct Altimeter<'d> {
    sensor: bmp390::BMP390<I2cDriver<'d>>,
    pub stats: Arc<Mutex<AltimeterStats>>,
    sea_level_pressure: f64,
}

impl<'d> Altimeter<'d> {
    pub fn new(i2c_driver: Arc<Mutex<I2cDriver<'d>>>) -> Altimeter {
        let sensor = bmp390::BMP390::new(i2c_driver, DeviceAddr::AD0).unwrap();
        let stats = Arc::new(Mutex::new(AltimeterStats::default()));

        Altimeter {
            sensor,
            stats,
            sea_level_pressure: 101320.75,
        }
    }

    pub fn sea_level_pressure(&mut self, sea_level_pressure: f64) {
        self.sea_level_pressure = sea_level_pressure;
    }

    pub fn reset_stats(&mut self) {
        let mut stats = self.stats.lock().unwrap();
        *stats = AltimeterStats::default();
    }

    pub fn update_stats(&mut self) {
        self.sensor
            .write_register(
                Register::Osr,
                Osr::Select(OsrTemp::x32, OsrPress::x2).value(),
            )
            .unwrap();

        self.sensor
            .write_register(
                Register::PwrCtrl,
                PwrCtrl::Forced {
                    press_en: true,
                    temp_en: true,
                }
                .value(),
            )
            .unwrap();

        std::thread::sleep(Duration::from_millis(100));
        let temperature = self.sensor.read_temperature();

        if let Ok(temperature) = temperature {
            if let Ok(pressure) = self.sensor.read_pressure(temperature) {
                let mut stats = self.stats.lock().unwrap();
                let altitude = calc_altitude(pressure, self.sea_level_pressure);

                stats.altitude = altitude;
                stats.temperature = temperature;
                stats.pressure = pressure;

                stats.maximum_temperature = stats.maximum_temperature.max(temperature);
                stats.minimum_temperature = stats.minimum_temperature.min(temperature);

                stats.minimum_altitude = stats.minimum_altitude.min(altitude);
                stats.maximum_altitude = stats.maximum_altitude.max(altitude);

                stats.maximum_pressure = stats.maximum_pressure.max(pressure);
                stats.minimum_pressure = stats.minimum_pressure.min(pressure);
            } else {
                println!("couldn't read sensor")
            }
        } else {
            println!("couldn't read sensor");
        }
    }
}

fn calc_altitude(pressure: f64, sea_level_atmospheres: f64) -> f64 {
    (1_f64 - (pressure / sea_level_atmospheres).powf(0.190284_f64)) * 145366.45_f64
}
