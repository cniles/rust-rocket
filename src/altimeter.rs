use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

use bmp390::{
    self,
    bmp390::{Bmp390Error, DeviceAddr, Osr, OsrPress, OsrTemp, PwrCtrl, Register},
};
use embedded_hal::i2c::I2c;

#[derive(Copy, Clone, Debug)]
pub struct KalmanState {
    n: u32,
    x: f64,
    next_x: f64,
    p: f64,
    next_p: f64,
}

impl KalmanState {
    fn new(p_0: f64, x_0: f64) -> Self {
        KalmanState {
            n: 0,
            x: x_0,
            next_x: x_0, // static
            p: p_0,
            next_p: p_0,
        }
    }
    fn update(&mut self, r_n: f64, z_n: f64, q: f64) {
        // update

        // kalman gain
        let k_n = self.next_p / (self.next_p + r_n);

        // estimate current state
        self.x = self.next_x + k_n * (z_n - self.next_x);
        // estimate current variance
        self.p = self.next_p * (1.0f64 - k_n);

        self.n += 1;

        self.next_x = self.x;
        self.next_p = self.p + q;
    }
}

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

    pub filtered_pressure: f64,
    kalman_state: KalmanState,
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
            filtered_pressure: 0.0f64,

            kalman_state: KalmanState::new(102178.0, 2500.0),
        }
    }
}

pub struct Altimeter<I2C> {
    sensor: bmp390::BMP390<I2C>,
    pub stats: Arc<Mutex<AltimeterStats>>,
    sea_level_pressure: Arc<Mutex<f64>>,
}

impl<I2C> Clone for Altimeter<I2C> {
    fn clone(&self) -> Self {
        Self {
            sensor: self.sensor.clone(),
            stats: self.stats.clone(),
            sea_level_pressure: self.sea_level_pressure.clone(),
        }
    }
}

#[derive(Copy, Clone, Debug)]
pub enum AltimeterError<I2C> {
    SensorError(Bmp390Error<I2C>),
}

impl<I2C> Altimeter<I2C>
where
    I2C: I2c,
{
    pub fn new(i2c_driver: Arc<Mutex<I2C>>) -> Result<Altimeter<I2C>, AltimeterError<I2C::Error>> {
        let mut sensor = bmp390::BMP390::new(i2c_driver, DeviceAddr::AD0)
            .map_err(AltimeterError::SensorError)?;
        let stats = Arc::new(Mutex::new(AltimeterStats::default()));

        sensor
            .write_register(Register::Config, 0b0010)
            .map_err(AltimeterError::SensorError)?;

        Ok(Altimeter {
            sensor,
            stats,
            sea_level_pressure: Arc::new(Mutex::new(102030.0)),
        })
    }

    pub fn sea_level_pressure(&mut self, sea_level_pressure: f64) {
        *self.sea_level_pressure.lock().unwrap() = sea_level_pressure;
    }

    pub fn reset_stats(&mut self) {
        let mut stats = self.stats.lock().expect("mutex is never closed");
        *stats = AltimeterStats::default();
    }

    pub fn update_stats(&mut self) -> Result<(), AltimeterError<I2C::Error>> {
        // Read from sensor
        self.sensor
            .write_register(
                Register::Osr,
                Osr::Select(OsrTemp::x1, OsrPress::x8).value(),
            )
            .map_err(AltimeterError::SensorError)?;

        self.sensor
            .write_register(
                Register::PwrCtrl,
                PwrCtrl::Forced {
                    press_en: true,
                    temp_en: true,
                }
                .value(),
            )
            .map_err(AltimeterError::SensorError)?;

        std::thread::sleep(Duration::from_millis(200));

        let temperature = self
            .sensor
            .read_temperature()
            .map_err(AltimeterError::SensorError)?;

        let pressure = self
            .sensor
            .read_pressure(temperature)
            .map_err(AltimeterError::SensorError)?;

        // Update stats and filter pressure

        let mut stats = self.stats.lock().unwrap();

        stats.temperature = temperature;
        stats.pressure = pressure;

        stats.kalman_state.update(1.0f64, pressure, 0.4f64);
        stats.filtered_pressure = stats.kalman_state.x;

        let altitude = calc_altitude(
            stats.filtered_pressure,
            *self.sea_level_pressure.lock().unwrap(),
        );

        stats.altitude = altitude;

        stats.maximum_temperature = stats.maximum_temperature.max(temperature);
        stats.minimum_temperature = stats.minimum_temperature.min(temperature);

        stats.minimum_altitude = stats.minimum_altitude.min(altitude);
        stats.maximum_altitude = stats.maximum_altitude.max(altitude);

        stats.maximum_pressure = stats.maximum_pressure.max(pressure);
        stats.minimum_pressure = stats.minimum_pressure.min(pressure);
        Ok(())
    }
}

fn calc_altitude(pressure: f64, sea_level_atmospheres: f64) -> f64 {
    (1_f64 - (pressure / sea_level_atmospheres).powf(0.190284_f64)) * 145366.45_f64
}
