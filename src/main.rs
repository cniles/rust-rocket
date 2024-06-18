use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

use altimeter::Altimeter;
use buzzer::Buzzer;
use esp_idf_hal::{gpio::PinDriver, prelude::*};
use esp_idf_hal::{
    i2c::{I2cConfig, I2cDriver},
    peripherals::Peripherals,
};
use esp_idf_svc::{
    espnow::PeerInfo,
    eventloop::EspSystemEventLoop,
    nvs::EspDefaultNvsPartition,
    wifi::{BlockingWifi, ClientConfiguration, Configuration::Client, EspWifi, WifiDeviceId},
};

#[derive(Debug)]
struct State {
    telemetry_addr: Option<[u8; 6]>,
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

fn print_mac_addrs(wifi: &BlockingWifi<EspWifi<'_>>) {
    let ap_mac = wifi
        .wifi()
        .get_mac(WifiDeviceId::Ap)
        .expect("should have ap");
    let sta_mac = wifi
        .wifi()
        .get_mac(WifiDeviceId::Sta)
        .expect("should have station");

    log::info!(
        "ap mac: {:X}:{:X}:{:X}:{:X}:{:X}:{:X}",
        ap_mac[0],
        ap_mac[1],
        ap_mac[2],
        ap_mac[3],
        ap_mac[4],
        ap_mac[5]
    );
    log::info!(
        "sta mac: {:X}:{:X}:{:X}:{:X}:{:X}:{:X}",
        sta_mac[0],
        sta_mac[1],
        sta_mac[2],
        sta_mac[3],
        sta_mac[4],
        sta_mac[5]
    );
}

fn main() {
    // It is necessary to call this function once. Otherwise some patches to the runtime
    // implemented by esp-idf-sys might not link properly. See https://github.com/esp-rs/esp-idf-template/issues/71
    esp_idf_svc::sys::link_patches();
    // Bind the log crate to the ESP Logging facilities
    esp_idf_svc::log::EspLogger::initialize_default();

    log::info!("Num cpus: {}", num_cpus::get());

    let state = Arc::new(Mutex::new(State::default()));

    let peripherals = Peripherals::take().expect("Failed to obtain peripherals");
    let charge_pin = PinDriver::input(peripherals.pins.gpio34).unwrap();

    let mut battery = battery::Battery::new(charge_pin, peripherals.adc1, peripherals.pins.gpio35);

    // Create buzzer interface
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
    .expect("Failed to obtain I2C Driver");

    // Create altimeter
    let mut altimeter =
        Altimeter::new(Arc::new(Mutex::new(i2c_driver))).expect("altimeter available");

    // wifi connect
    let (espnow, _wifi) = {
        let sys_loop = EspSystemEventLoop::take().unwrap();
        let nvs = EspDefaultNvsPartition::take().unwrap();

        let wifi = EspWifi::new(peripherals.modem, sys_loop.clone(), Some(nvs)).unwrap();
        let mut wifi = BlockingWifi::wrap(wifi, sys_loop).expect("Failed to create wifi");
        let configuration = Client(ClientConfiguration::default());
        wifi.set_configuration(&configuration).unwrap();

        print_mac_addrs(&wifi);

        wifi.start().unwrap();

        // Start up EspNow
        let espnow = esp_idf_svc::espnow::EspNow::take();
        match espnow {
            Ok(_) => {
                log::info!("got esp now");
            }
            Err(e) => {
                log::error!("failed to get esp now: {}", e);
            }
        }

        let espnow = espnow.unwrap();

        (espnow, wifi)
    };

    espnow
        .register_recv_cb(|mac: &[u8], data: &[u8]| {
            let data = std::str::from_utf8(data).unwrap().to_string();

            if data.starts_with("tone") {
                log::info!("tone");
                buzzer.once();
                buzzer.start();
            }

            if data.starts_with("telemetry_on") {
                log::info!("streaming telemetry");
                let mut state = state.lock().unwrap();
                let mut mac_arr = [0u8; 6];
                mac_arr.copy_from_slice(mac);
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
        })
        .expect("callback registration should succeed");

    // Start main loop
    loop {
        let update_result = altimeter.update_stats();

        if let Err(e) = update_result {
            log::error!("Failed to update altimeter: {:?}", e);
            // todo send a message to base station :(
        } else {
            let stats = { altimeter.stats.lock().unwrap().clone() };
            let mut guard = state.lock().unwrap();
            if let Some(ref mut addr) = guard.telemetry_addr {
                let mut peer_addr = [0u8; 6];
                peer_addr.copy_from_slice(addr);

                // todo: better handling on error conditions or at least an except
                if !espnow.peer_exists(peer_addr).unwrap() {
                    let mut peer_info = PeerInfo::default();
                    peer_info.peer_addr.copy_from_slice(addr);
                    espnow.add_peer(peer_info).unwrap();
                }

                let metrics = format!(
                    "metrics: {:.2}ft ({:.2}ft/{:.2}ft) charging/voltage: {}/{:.3}",
                    stats.altitude,
                    stats.minimum_altitude,
                    stats.maximum_altitude,
                    battery.charging().unwrap(),
                    battery.voltage().unwrap(),
                );

                let data = metrics.as_bytes();
                if let Err(e) = espnow.send(*addr, data) {
                    log::error!(
                        "Failed to send to {:X}:{:X}:{:X}:{:X}:{:X}:{:X}: to {:}",
                        addr[0],
                        addr[1],
                        addr[2],
                        addr[3],
                        addr[4],
                        addr[5],
                        e
                    );
                    guard.telemetry_addr = None;
                }
            }
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}
