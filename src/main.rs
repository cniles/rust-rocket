use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

use altimeter::Altimeter;
use battery::Battery;
use buzzer::Buzzer;
use esp_idf_hal::{
    gpio::{ADCPin, InputPin, Pin},
    prelude::*,
};
use esp_idf_hal::{
    i2c::{I2cConfig, I2cDriver},
    peripherals::Peripherals,
};

use crate::datalink::Datalink;

#[derive(Debug)]
struct State {
    telemetry_addr: Option<[u8; 6]>,
}

struct Rocket<I2C, C, T>
where
    C: Pin + InputPin,
    T: ADCPin,
{
    altimeter: Altimeter<I2C>,
    battery: Battery<C, T>,
    buzzer: Buzzer,
}

impl Default for State {
    fn default() -> Self {
        State {
            telemetry_addr: None,
        }
    }
}

mod altimeter;
mod battery;
mod buzzer;
mod datalink;

fn main() {
    // It is necessary to call this function once. Otherwise some patches to the runtime
    // implemented by esp-idf-sys might not link properly. See https://github.com/esp-rs/esp-idf-template/issues/71
    esp_idf_svc::sys::link_patches();
    // Bind the log crate to the ESP Logging faciliies
    esp_idf_svc::log::EspLogger::initialize_default();

    log::info!("Num cpus: {}", num_cpus::get());

    let state = Arc::new(Mutex::new(State::default()));

    let peripherals = Peripherals::take().expect("Failed to obtain peripherals");

    // Create battery driver
    let mut battery = battery::Battery::new(
        peripherals.pins.gpio34,
        peripherals.adc1,
        peripherals.pins.gpio35,
    )
    .unwrap();

    // Create buzzer driver
    let buzzer = Buzzer::new(peripherals.pins.gpio4);
    buzzer.period(100);
    buzzer.pattern(buzzer::BuzzPattern::Beep {
        frequency: 4186,
        duration: 50,
    });

    // Create i2C driver
    let i2c_config = I2cConfig::new().baudrate(400.kHz().into());
    let i2c_driver = I2cDriver::new(
        peripherals.i2c0,
        peripherals.pins.gpio21,
        peripherals.pins.gpio22,
        &i2c_config,
    )
    .unwrap();

    // Create altimeter driver
    let mut altimeter = Altimeter::new(Arc::new(Mutex::new(i2c_driver))).unwrap();

    let mut datalink = Datalink::new(peripherals.modem);
    let command_receiver = datalink.command_receiver.take().unwrap();

    let altimeter_stats = altimeter.stats.clone();
    let state2 = state.clone();
    let altimeter2 = altimeter.clone();
    std::thread::spawn(move || {
        let mut altimeter = altimeter2;
        let state = state2;

        loop {
            let (mac_arr, data) = command_receiver.recv().unwrap();
            let data = String::from_utf8(data).unwrap();

            if data.starts_with("tone") {
                log::info!("tone");
                buzzer.once();
                buzzer.start();
            }

            if data.starts_with("telemetry_on") {
                log::info!("streaming telemetry");
                let mut state = state.lock().unwrap();
                state.telemetry_addr = Some(mac_arr);
            }

            if data.starts_with("telemetry_off") {
                log::info!("disabling telemetry");
                let mut state = state.lock().unwrap();
                state.telemetry_addr = None;
            }

            if data.starts_with("inhg") {
                let parts: Vec<&str> = data.trim().split(' ').collect();

                if parts.len() < 2 {
                    log::info!("No pressure provided");
                } else {
                    let sea_level_pressure = parts[1].parse::<f64>();

                    if let Ok(sea_level_pressure) = sea_level_pressure {
                        log::info!("Inhg updated");
                        altimeter.sea_level_pressure(sea_level_pressure);
                        return;
                    } else {
                        log::info!("Failed to parse pressure");
                    }
                }

                log::info!("pressure not set");
            }

            if data.starts_with("reset") {
                altimeter.reset_stats();
            }
        }
    });
    // Start main loop

    loop {
        let update_result = altimeter.update_stats();

        if let Err(e) = update_result {
            log::error!("Failed to update altimeter: {:?}", e);
            // todo send a message to base station :(
        } else {
            let stats = { altimeter_stats.lock().unwrap().clone() };
            let mut guard = state.lock().unwrap();
            if let Some(ref mut addr) = guard.telemetry_addr {
                let mut peer_addr = [0u8; 6];
                peer_addr.copy_from_slice(addr);

                let metrics = format!(
                    "metrics: {:.2}ft ({:.2}ft/{:.2}ft) (diff: {:.2}) charging/voltage: {}/{:.3}",
                    stats.altitude,
                    stats.minimum_altitude,
                    stats.maximum_altitude,
                    stats.maximum_altitude - stats.minimum_altitude,
                    battery.charging(),
                    battery.voltage().unwrap(),
                );

                let mut data_vec = Vec::new();

                data_vec.extend_from_slice(metrics.as_bytes());

                datalink.data_sender.send((peer_addr, data_vec)).unwrap();
            }
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}
