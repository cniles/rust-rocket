use std::{
    str::FromStr,
    sync::{
        atomic::AtomicU32,
        mpsc::{self, Receiver, RecvTimeoutError, Sender},
        Arc, Mutex,
    },
    time::Duration,
};

use esp_idf_hal::{
    delay::NON_BLOCK,
    gpio::{Gpio0, Gpio1, Gpio3},
    io::Write,
    peripherals::Peripherals,
    sys::EspError,
    uart::{config::Config, UartDriver, UART0},
    units::Hertz,
};
use esp_idf_svc::{
    espnow::PeerInfo,
    eventloop::EspSystemEventLoop,
    http::server::EspHttpServer,
    nvs::EspDefaultNvsPartition,
    wifi::{
        AccessPointConfiguration, AuthMethod, BlockingWifi, ClientConfiguration, Configuration,
        EspWifi, WifiDeviceId,
    },
    ws::FrameType,
};
use heapless::mpmc;
use rocket::{datalink::ByteSerialize, telemetry::Telemetry};

const STACK_SIZE: usize = 10240;

#[derive(Clone)]
struct ClientConnection {
    sender: Sender<Telemetry>,
}

#[derive(Clone)]
struct ClientConnectionList {
    clients: Arc<Mutex<Vec<ClientConnection>>>,
}

impl ClientConnectionList {
    fn new() -> Self {
        ClientConnectionList {
            clients: Arc::new(Mutex::new(Vec::new())),
        }
    }

    fn add_client(&self) -> Receiver<Telemetry> {
        let mut guard = self.clients.lock().unwrap();
        let (sender, receiver) = mpsc::channel();
        guard.push(ClientConnection { sender });
        receiver
    }
}

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

    let (command_sender, command_receiver) = mpsc::channel();

    let peripherals = Peripherals::take().unwrap();

    let uart_driver = make_uart_driver(
        peripherals.uart0,
        peripherals.pins.gpio1,
        peripherals.pins.gpio3,
    );

    let uart_driver = Arc::new(Mutex::new(uart_driver));

    // spawn thread to read commands from UART
    {
        let uart_driver = uart_driver.clone();
        std::thread::spawn(move || loop {
            let s = read_input(&uart_driver);
            command_sender.send(s).unwrap();
        });
    }

    let client_connections = ClientConnectionList::new();

    let mut http_server = wifi_thread(
        peripherals.modem,
        client_connections.clone(),
        command_receiver,
    );

    let msg = "<h1>Hello world</h1>";

    {
        http_server
            .fn_handler("/stats", esp_idf_svc::http::Method::Get, |req| {
                req.into_ok_response()
                    .unwrap()
                    .write_all(msg.as_bytes())
                    .unwrap();
                Ok::<(), EspError>(())
            })
            .unwrap()
            .ws_handler("/ws/test", move |ws| {
                if ws.is_new() {
                    println!("new ws connection");
                    let mut ws = ws.create_detached_sender().unwrap();
                    let telemetry_receiver = client_connections.clone().add_client();
                    std::thread::spawn(move || {
                        loop {
                            if ws.is_closed() {
                                break;
                            }

                            let telemetry =
                                telemetry_receiver.recv_timeout(Duration::from_millis(50));
                            let mut buffer = [0u8; std::mem::size_of::<Telemetry>()];

                            if ws.is_closed() {
                                break;
                            }

                            let telemetry = match telemetry {
                                Err(e) => match e {
                                    RecvTimeoutError::Timeout => {
                                        continue;
                                    }
                                    RecvTimeoutError::Disconnected => {
                                        break;
                                    }
                                },
                                Ok(telemetry) => telemetry,
                            };

                            telemetry
                                .as_bytes(&mut buffer)
                                .expect("serialize should work");

                            if ws.send(FrameType::Binary(false), &buffer).is_err() {
                                break;
                            }
                        }
                        println!("ws connection closed");
                    });
                }
                Ok::<(), EspError>(())
            })
            .unwrap();
    }

    loop {
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn wifi_thread(
    modem: esp_idf_hal::modem::Modem,
    client_connections: ClientConnectionList,
    command_receiver: Receiver<String>,
) -> EspHttpServer<'static> {
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
            let data = Vec::from(data);

            // only send if we have a listener or
            let telemetry = Telemetry::from_bytes(&data).unwrap();
            log::info!("{:?}", telemetry);

            let mut guard = client_connections.clients.lock().unwrap();

            let mut i = 0;

            while i < guard.len() {
                if guard
                    .get(i)
                    .unwrap()
                    .sender
                    .send(telemetry.clone())
                    .is_err()
                {
                    guard.remove(i);
                } else {
                    i += 1;
                }
            }
        })
        .unwrap();

    let peer: [u8; 6] = [0xD4, 0xD4, 0xDA, 0xAA, 0x27, 0x5C];

    let mut peer_info = PeerInfo::default();

    peer_info.channel = 1;
    peer_info.peer_addr = peer;
    peer_info.encrypt = false;

    espnow.add_peer(peer_info).unwrap();

    std::thread::spawn(move || {
        let _wifi = wifi;
        loop {
            if let Ok(s) = command_receiver.try_recv() {
                if s.len() != 0 {
                    if let Err(e) = espnow.send(peer, s.as_bytes()) {
                        log::error!("failed to send: {}", e);
                    } else {
                        log::info!("Sent {} bytes", s.len());
                    }
                }
            }
            std::thread::sleep(Duration::from_millis(63));
        }
    });

    let http_server_config = esp_idf_svc::http::server::Configuration {
        stack_size: STACK_SIZE,
        ..Default::default()
    };

    EspHttpServer::new(&http_server_config).unwrap()
}
