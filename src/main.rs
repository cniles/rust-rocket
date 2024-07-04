use std::sync::{Arc, Mutex};

use altimeter::Altimeter;

pub(crate) use buzzer::Buzzer;
use datalink::ByteSerialize;
use esp_idf_hal::prelude::*;
use esp_idf_hal::{
    i2c::{I2cConfig, I2cDriver},
    peripherals::Peripherals,
};
use telemetry::Telemetry;

use crate::datalink::Datalink;

#[derive(Debug)]
struct State {
    telemetry_addr: Option<[u8; 6]>,
    streaming: bool,
}

// struct Rocket<I2C, C, T>
// where
//     C: Pin + InputPin,
//     T: ADCPin,
// {
//     altimeter: Altimeter<I2C>,
//     battery: Battery<C, T>,
//     buzzer: Buzzer,
// }

impl Default for State {
    fn default() -> Self {
        State {
            telemetry_addr: None,
            streaming: false,
        }
    }
}

mod altimeter;
mod battery;
mod buzzer;
mod datalink;
mod kalman;
mod telemetry;

fn main() {
    // It is necessary to call this function once. Otherwise some patches to the runtime
    // implemented by esp-idf-sys might not link properly. See https://github.com/esp-rs/esp-idf-template/issues/71
    esp_idf_svc::sys::link_patches();
    // Bind the log crate to the ESP Logging faciliies
    esp_idf_svc::log::EspLogger::initialize_default();

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

    let recording = Arc::new(Mutex::new(Vec::<Telemetry>::with_capacity(900)));
    let recording2 = recording.clone();
    let data_sender = datalink.data_sender.clone();

    std::thread::spawn(move || {
        let mut altimeter = altimeter2;
        let state = state2;
        let recording = recording2;

        loop {
            let (mac_arr, data) = command_receiver.recv().unwrap();

            let data = if let Ok(data) = String::from_utf8(data) {
                log::info!("received command: {}", data);
                data
            } else {
                log::warn!("unable to read command");
                continue;
            };

            if data.starts_with("tone") {
                log::info!("tone");
                buzzer.once();
                buzzer.start();
            }

            if data.starts_with("ton") {
                log::info!("streaming telemetry");
                {
                    let mut guard = recording.lock().unwrap();
                    guard.clear();
                }
                {
                    let mut state = state.lock().unwrap();
                    state.streaming = true;
                    state.telemetry_addr = Some(mac_arr);
                }
            }

            if data.starts_with("toff") {
                log::info!("disabling telemetry");
                let mut state = state.lock().unwrap();
                state.streaming = false;
            }

            if data.starts_with("re_tx") {
                let parts: Vec<&str> = data.trim().split(' ').collect();
                if parts.len() >= 2 {
                    let num = parts[1].parse::<usize>();

                    if let Ok(num) = num {
                        let mut buffer = [0u8; 33];

                        let telemetry = {
                            let recording = recording.lock().unwrap();
                            let telemetry_option = recording.get(num);
                            if let Some(telemetry) = telemetry_option {
                                Some(telemetry.clone())
                            } else {
                                None
                            }
                        };

                        if let Some(telemetry) = telemetry {
                            log::info!("retransmitting {}", num);
                            let state = state.lock().unwrap();
                            if let Some(addr) = state.telemetry_addr {
                                telemetry.as_bytes(&mut buffer).unwrap();

                                let data_vec = Vec::from(buffer);

                                data_sender.send((addr, data_vec)).unwrap();
                            } else {
                                log::info!("no peer addr to retransmit to");
                            }
                        } else {
                            log::info!("telemetry missing");
                        }
                    }
                }
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
    let start = std::time::Instant::now();

    println!("size of telemetry: {}", std::mem::size_of::<Telemetry>());

    loop {
        let update_result = altimeter.update_stats();

        if let Err(e) = update_result {
            log::error!("Failed to update altimeter: {:?}", e);
            // todo send a message to base station :(
        } else {
            let stats = { altimeter_stats.lock().unwrap().clone() };
            let mut guard = state.lock().unwrap();
            if guard.streaming {
                if let Some(ref mut addr) = guard.telemetry_addr {
                    let mut peer_addr = [0u8; 6];
                    peer_addr.copy_from_slice(addr);

                    log::info!("altitude: {}", stats.altitude);

                    let mut telemetry = Telemetry::from((stats, battery.stats().unwrap()));
                    telemetry.time = start.elapsed().as_millis() as u32;

                    if {
                        // perform scoped so as to prevent holding lock through tx.
                        let mut guard = recording.lock().unwrap();
                        if guard.len() < guard.capacity() {
                            guard.push(telemetry);
                            true
                        } else {
                            false
                        }
                    } {
                        let mut buffer = [0u8; 33];

                        telemetry.as_bytes(&mut buffer).unwrap();

                        let data_vec = Vec::from(buffer);

                        datalink.data_sender.send((peer_addr, data_vec)).ok();
                    }
                }
            }
        }
        // update_stats on altimeter will sleep for 100ms
        // std::thread::sleep(Duration::from_millis(50));
    }
}
