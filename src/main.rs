use std::{
    io::{self, Read},
    sync::{Arc, Mutex},
    time::Duration,
};

use altimeter::Altimeter;
use buzzer::Buzzer;
use esp_idf_hal::{
    i2c::{I2cConfig, I2cDriver},
    peripherals::Peripherals,
};
use esp_idf_hal::{prelude::*, sys::EspError};
use esp_idf_svc::{
    espnow::PeerInfo,
    eventloop::EspSystemEventLoop,
    netif::{EspNetif, NetifConfiguration},
    nvs::EspDefaultNvsPartition,
    wifi::{
        BlockingWifi, ClientConfiguration, Configuration::Client, EspWifi, WifiDeviceId, WifiDriver,
    },
};

#[derive(Debug)]
struct State {}

impl Default for State {
    fn default() -> Self {
        State {}
    }
}

mod altimeter;
mod buzzer;

fn main() {
    // It is necessary to call this function once. Otherwise some patches to the runtime
    // implemented by esp-idf-sys might not link properly. See https://github.com/esp-rs/esp-idf-template/issues/71
    esp_idf_svc::sys::link_patches();
    // Bind the log crate to the ESP Logging facilities
    esp_idf_svc::log::EspLogger::initialize_default();

    log::info!("Num cpus: {}", num_cpus::get());

    let state = Arc::new(Mutex::new(State::default()));

    let peripherals = Peripherals::take().expect("Failed to obtain peripherals");

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
    let mut altimeter = Altimeter::new(Arc::new(Mutex::new(i2c_driver)));

    // wifi connect
    let (espnow, wifi) = {
        let sys_loop = EspSystemEventLoop::take().unwrap();
        let nvs = EspDefaultNvsPartition::take().unwrap();

        // let wifi = WifiDriver::new(peripherals.modem, sys_loop.clone(), Some(nvs));
        // let wifi = configure_wifi(wifi.unwrap()).unwrap();
        let wifi = EspWifi::new(peripherals.modem, sys_loop.clone(), Some(nvs)).unwrap();
        let mut wifi = BlockingWifi::wrap(wifi, sys_loop).expect("Failed to create wifi");
        let configuration = Client(ClientConfiguration::default());
        wifi.set_configuration(&configuration).unwrap();

        let ap_mac = wifi.wifi().get_mac(WifiDeviceId::Ap).unwrap();
        let sta_mac = wifi.wifi().get_mac(WifiDeviceId::Sta).unwrap();

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

    let telemetry_addr = Arc::new(Mutex::new(Option::<[u8; 6]>::None));

    espnow
        .register_recv_cb(|mac: &[u8], data: &[u8]| {
            let data = std::str::from_utf8(data).unwrap().to_string();

            if data.starts_with("beep") {
                log::info!("beeping");
                buzzer.once();
                buzzer.start();
            }

            if data.starts_with("telemetry_on") {
                log::info!("streaming telemetry");
                let mut telemetry_addr = telemetry_addr.lock().unwrap();
                let mut mac_arr = [0u8; 6];
                mac_arr.copy_from_slice(mac);
                *telemetry_addr = Some(mac_arr);
            }

            if data.starts_with("telemetry_off") {
                log::info!("disabling telemetry");
                let mut telemetry_addr = telemetry_addr.lock().unwrap();
                *telemetry_addr = None;
            }

            if data.starts_with("inhg") {
                let parts: Vec<&str> = data.trim().split(' ').collect();

                if parts.len() < 2 {
                    log::info!("No pressure provided");
                } else {
                    let sea_level_pressure = parts[1].parse::<f64>();

                    if sea_level_pressure.is_ok() {
                        log::info!("Inhg updated");
                        altimeter.sea_level_pressure(sea_level_pressure.unwrap());
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
        .expect("failed to register callback");

    // Start main loop
    loop {
        altimeter.update_stats();
        {
            let stats = { altimeter.stats.lock().unwrap().clone() };
            let mut guard = telemetry_addr.lock().unwrap();
            if let Some(ref mut addr) = *guard {
                let mut peer_addr = [0u8; 6];
                peer_addr.copy_from_slice(addr);
                if !espnow.peer_exists(peer_addr).unwrap() {
                    let mut peer_info = PeerInfo::default();
                    peer_info.peer_addr.copy_from_slice(addr);
                    espnow.add_peer(peer_info).unwrap();
                }
                // let metrics = "Hello world";
                let metrics = format!(
                    "metrics: {:.2}ft ({:.2}ft/{:.2}ft)",
                    stats.altitude, stats.minimum_altitude, stats.maximum_altitude
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
                    *guard = None;
                }
            }
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn configure_wifi(wifi: WifiDriver<'static>) -> Result<EspWifi<'static>, EspError> {
    let ap_netif = EspNetif::new(esp_idf_svc::netif::NetifStack::Ap).unwrap();
    let sta_netif = EspNetif::new(esp_idf_svc::netif::NetifStack::Sta).unwrap();

    let wifi = EspWifi::wrap_all(wifi, sta_netif, ap_netif);

    wifi
}
