use std::{
    str::FromStr,
    sync::{
        mpsc::{self, Receiver, Sender},
        Arc, Mutex,
    },
    time::Duration,
};

use esp_idf_hal::{
    delay::NON_BLOCK,
    gpio::{Gpio0, Gpio1, Gpio3},
    peripherals::Peripherals,
    uart::{config::Config, UartDriver, UART0},
    units::Hertz,
};
use esp_idf_svc::{
    espnow::PeerInfo,
    eventloop::EspSystemEventLoop,
    nvs::EspDefaultNvsPartition,
    wifi::{
        AccessPointConfiguration, AuthMethod, BlockingWifi, ClientConfiguration, Configuration,
        EspWifi, WifiDeviceId,
    },
};

fn read_input(uart_driver: &Arc<Mutex<UartDriver>>) -> String {
    let mut result = String::new();

    loop {
        let mut buf = [0_u8; 1];
        let c = {
            let uart_driver = uart_driver.lock().unwrap();
            uart_driver.read(&mut buf, NON_BLOCK).unwrap()
        };

        let s = std::str::from_utf8(&buf).unwrap();

        if s == "\n" || s == "\r" {
            return result;
        }

        if c != 0 {
            result.push_str(s);
        }

        std::thread::sleep(Duration::from_millis(50));
    }
}

fn make_uart_driver(uart0: UART0, gpio1: Gpio1, gpio3: Gpio3) -> UartDriver<'static> {
    let config = Config::default().baudrate(Hertz(115200));
    UartDriver::new::<UART0>(
        uart0,
        gpio1,
        gpio3,
        Option::<Gpio0>::None,
        Option::<Gpio1>::None,
        &config,
    )
    .unwrap()
}

fn main() {
    esp_idf_svc::sys::link_patches();

    // Bind the log crate to the ESP Logging facilities
    esp_idf_svc::log::EspLogger::initialize_default();

    let (sender, receiver) = mpsc::channel();

    let peripherals = Peripherals::take().unwrap();

    let uart_driver = make_uart_driver(
        peripherals.uart0,
        peripherals.pins.gpio1,
        peripherals.pins.gpio3,
    );

    let uart_driver = Arc::new(Mutex::new(uart_driver));

    {
        let uart_driver = uart_driver.clone();
        std::thread::spawn(move || loop {
            let s = read_input(&uart_driver);
            sender.send(s).unwrap();
        });
    }

    let sender = {
        let (sender, receiver) = mpsc::channel();
        std::thread::spawn(move || loop {
            let message: String = receiver.recv().unwrap();
            log::info!("Message received: {}", message);
        });
        sender
    };

    wifi_thread(peripherals.modem, sender, receiver);
}

fn wifi_thread(
    modem: esp_idf_hal::modem::Modem,
    sender: Sender<String>,
    receiver: Receiver<String>,
) {
    let sys_loop = EspSystemEventLoop::take().unwrap();
    let nvs = EspDefaultNvsPartition::take().unwrap();
    let esp_wifi = EspWifi::new(modem, sys_loop.clone(), Some(nvs)).unwrap();

    let mut wifi = BlockingWifi::wrap(esp_wifi, sys_loop).unwrap();

    let (mut client_config, mut ap_config) = (
        ClientConfiguration::default(),
        AccessPointConfiguration::default(),
    );

    client_config.channel = Some(1);

    ap_config.ssid = heapless::String::<32>::from_str("omega9").unwrap();
    ap_config.password = heapless::String::<64>::from_str("knock it off").unwrap();
    ap_config.channel = 1;
    ap_config.auth_method = AuthMethod::WPA3Personal;
    ap_config.ssid_hidden = false;

    wifi.set_configuration(&Configuration::Mixed(
        client_config.clone(),
        ap_config.clone(),
    ))
    .unwrap();

    wifi.start().unwrap();

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

    espnow
        .register_recv_cb(move |_mac: &[u8], data: &[u8]| {
            sender
                .send(std::str::from_utf8(data).unwrap().to_string())
                .unwrap();
        })
        .unwrap();

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

    let peer: [u8; 6] = [0xD4, 0xD4, 0xDA, 0xAA, 0x27, 0x5C];

    let mut peer_info = PeerInfo::default();

    peer_info.channel = 1;
    peer_info.peer_addr = peer;
    peer_info.encrypt = false;

    espnow.add_peer(peer_info).unwrap();

    loop {
        if let Ok(s) = receiver.try_recv() {
            if s.len() != 0 {
                let data = s.as_bytes();
                if let Err(e) = espnow.send(peer, data) {
                    log::error!("failed to send: {}", e);
                } else {
                    log::info!("Sent {} bytes", s.len());
                }
            }
        }
        std::thread::sleep(Duration::from_millis(63));
    }
}
