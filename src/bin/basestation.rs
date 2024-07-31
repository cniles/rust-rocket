use std::{
    cell::RefCell,
    error::Error,
    rc::Rc,
    str::FromStr,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, RecvTimeoutError, Sender},
        Arc, Mutex,
    },
    time::Duration,
};

use embedded_graphics::{
    geometry::Point,
    mono_font::{ascii::FONT_6X9, MonoTextStyle},
    pixelcolor::{Rgb565, RgbColor},
    prelude::*,
    primitives::{Line, PrimitiveStyle, Rectangle},
    text::Text,
};

use esp_idf_hal::{
    delay::NON_BLOCK,
    gpio::{Gpio0, Gpio1, Gpio3},
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
        EspWifi,
    },
    ws::FrameType,
};

use ez_cyd_rs::CydDisplay;
use rocket::{
    altimeter::calc_altitude,
    control_panel::init_control_panel,
    datalink::ByteSerialize,
    keypad::init_keypad,
    telemetry::Telemetry,
    ui::{text::Text as UiText, ui::Ui},
};

const STACK_SIZE: usize = 10240;
const WEB_SERVICES_ON: bool = false;

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

fn draw_telemetry(telemetry: &Telemetry, display: &mut CydDisplay) {
    let style = MonoTextStyle::new(&FONT_6X9, Rgb565::GREEN);
    Rectangle::new((25, 0).into(), Size::new(80, 13))
        .into_styled(PrimitiveStyle::with_fill(Rgb565::BLACK))
        .draw(display)
        .unwrap();
    let text = format!("Alt: {:.2}", telemetry.altitude);
    Text::new(&text, Point::new(5, 12), style)
        .draw(display)
        .map_err(|_| Box::<dyn Error>::from("draw hello"))
        .unwrap();

    Rectangle::new((25, 14).into(), Size::new(50, 13))
        .into_styled(PrimitiveStyle::with_fill(Rgb565::BLACK))
        .draw(display)
        .unwrap();
    let text = format!(" V+: {:.2}", telemetry.battery_voltage);
    Text::new(&text, Point::new(5, 26), style)
        .draw(display)
        .map_err(|_| Box::<dyn Error>::from("draw hello"))
        .unwrap();

    Rectangle::new((25, 28).into(), Size::new(100, 13))
        .into_styled(PrimitiveStyle::with_fill(Rgb565::BLACK))
        .draw(display)
        .unwrap();
    let text = format!("Prs: {:.2}", telemetry.pressure);
    Text::new(&text, Point::new(5, 40), style)
        .draw(display)
        .map_err(|_| Box::<dyn Error>::from("draw hello"))
        .unwrap();
}

fn main() {
    esp_idf_svc::sys::link_patches();

    // Bind the log crate to the ESP Logging facilities
    esp_idf_svc::log::EspLogger::initialize_default();

    let (command_sender, command_receiver) = mpsc::channel();

    let peripherals = Peripherals::take().unwrap();

    let (mut cyd, peripherals) = ez_cyd_rs::Cyd::new(peripherals).unwrap();

    let touch_calibration = cyd.calibrate_touch();

    // let uart_driver = make_uart_driver(
    //     peripherals.uart0,
    //     peripherals.pins.gpio1,
    //     peripherals.pins.gpio3,
    // );
    // let uart_driver = Arc::new(Mutex::new(uart_driver));
    // spawn thread to read commands from UART
    // {
    //     let uart_driver = uart_driver.clone();
    //     std::thread::spawn(move || loop {
    //         let s = read_input(&uart_driver);
    //         command_sender.send(s).unwrap();
    //     });
    // }

    let client_connections = ClientConnectionList::new();

    let http_server = wifi_thread(
        peripherals.modem,
        client_connections.clone(),
        command_receiver,
    );

    let draw_client = client_connections.add_client();

    if WEB_SERVICES_ON {
        http_server
            .unwrap()
            .ws_handler("/ws/telemetry", move |ws| {
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

    cyd.display
        .clear(Rgb565::BLACK)
        .map_err(|_| Box::<dyn Error>::from("clear display"))
        .unwrap();

    let mut chart_x = 0;
    let mut chart_y = 0;

    let mut ui = Ui::new(320, 240);

    ui.touch_calibration(touch_calibration.unwrap());
    ui.add_element(Box::new(UiText::new("0".to_string(), Point::new(0, 0))));

    let (mut clear_flag, mut psl_flag) = init_control_panel(command_sender.clone(), &mut ui);
    let psl = Rc::new(RefCell::new(101230.0));

    let psl_set_flag = Rc::new(RefCell::new(false));

    loop {
        if psl_flag.load(Ordering::Relaxed) {
            // show psl keypad
            ui.clear();
            let psl_set_flag1 = psl_set_flag.clone();
            let psl_set_flag2 = psl_set_flag.clone();
            let psl = psl.clone();
            init_keypad(
                &mut ui,
                Box::new(move |psl_str: &str| {
                    *psl_set_flag1.borrow_mut() = true;
                    if let Ok(psl_val) = f64::from_str(psl_str) {
                        println!("setting PSL to {}", psl_str);
                        *psl.borrow_mut() = psl_val;
                    } else {
                        println!("PSL Format error");
                    }
                }),
                Box::new(move || {
                    *psl_set_flag2.borrow_mut() = true;
                }),
            );
            ui.dirty_all();
            cyd.display.clear(Rgb565::BLACK).unwrap();
        }

        if clear_flag.load(Ordering::Relaxed) {
            cyd.display.clear(Rgb565::BLACK).unwrap();
            ui.dirty_all();
        }

        if *psl_set_flag.borrow() {
            cyd.display.clear(Rgb565::BLACK).unwrap();
            ui.dirty_all();
            ui.clear();
            let (f1, f2) = init_control_panel(command_sender.clone(), &mut ui);
            clear_flag = f1;
            psl_flag = f2;
        }

        psl_flag.store(false, Ordering::Relaxed);
        clear_flag.store(false, Ordering::Relaxed);
        *psl_set_flag.borrow_mut() = false;

        let touch = cyd.try_touch().unwrap();
        ui.handle_touch((touch.0, touch.1, touch.2));

        ui.draw(&mut cyd.display);

        loop {
            let telemetry = draw_client.recv_timeout(Duration::from_millis(10));

            // Create text style
            if let Ok(mut telemetry) = telemetry {
                let altitude = calc_altitude(telemetry.pressure as f64, *psl.borrow() as f64);

                telemetry.altitude = altitude as f32;

                draw_telemetry(&telemetry, &mut cyd.display);
                let altitude = altitude / 2.0;
                Line::new(
                    Point::new(chart_x, 210 - altitude as i32),
                    Point::new(chart_x, 210 - chart_y),
                )
                .into_styled(PrimitiveStyle::with_stroke(Rgb565::GREEN, 1))
                .draw(&mut cyd.display)
                .map_err(|_| Box::<dyn Error>::from("draw chart"))
                .unwrap();

                chart_x = (chart_x + 1) % 320;
                chart_y = altitude as i32;

                Line::new(Point::new(chart_x, 210), Point::new(chart_x, 60))
                    .into_styled(PrimitiveStyle::with_stroke(Rgb565::BLACK, 1))
                    .draw(&mut cyd.display)
                    .map_err(|_| Box::<dyn Error>::from("draw chart"))
                    .unwrap();
            } else {
                break;
            }
        }
    }
}

fn wifi_thread(
    modem: esp_idf_hal::modem::Modem,
    client_connections: ClientConnectionList,
    command_receiver: Receiver<String>,
) -> Option<EspHttpServer<'static>> {
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
            // log::info!("{:?}", telemetry);

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
                        log::error!("Failed to send: {}", e);
                    } else {
                        log::info!("Sent {} bytes", s.len());
                    }
                }
            }
            std::thread::sleep(Duration::from_millis(63));
        }
    });

    if WEB_SERVICES_ON {
        let http_server_config = esp_idf_svc::http::server::Configuration {
            stack_size: STACK_SIZE,
            ..Default::default()
        };

        Some(EspHttpServer::new(&http_server_config).unwrap())
    } else {
        None
    }
}
