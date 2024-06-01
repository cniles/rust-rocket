use std::{
    borrow::Borrow,
    io::{self, Read},
    sync::{
        atomic::{AtomicBool, AtomicI32, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};

use bmp390::bmp390;

use esp_idf_hal::{
    gpio::{Gpio0, Gpio35, IOPin, InputPin, OutputPin, PinDriver},
    i2c::{I2c, I2cConfig, I2cDriver, I2C0},
    peripheral::Peripheral,
    peripherals::Peripherals,
    prelude::*,
    timer::Timer,
};

fn height(t: u32) -> f32 {
    let t = t as f32;
    10f32 * t - 0.5 * t * t
}

fn altitude(pressure: f64, sea_level_atmospheres: f64) -> f64 {
    (1_f64 - (pressure / sea_level_atmospheres).powf(0.190284_f64)) * 145366.45_f64
}

#[derive(Debug)]
struct State {
    fire: bool,
    max_alt: f64,
    max_temp: f64,
    min_temp: f64,
    sea_level_atmospheres: f64,
    temp: f64,
    pres: f64,
}

fn altimeter_monitor<'d, D: OutputPin + InputPin, C: OutputPin + InputPin, I2C: I2c>(
    i2c: impl Peripheral<P = I2C> + 'd,
    sda: impl Peripheral<P = D> + 'd,
    scl: impl Peripheral<P = C> + 'd,
    state: Arc<Mutex<State>>,
) {
    // let samples = [0f64; 576000];

    let config = I2cConfig::new().baudrate(400.kHz().into());

    let i2c = I2cDriver::new(i2c, sda, scl, &config);

    if i2c.is_err() {
        println!("couldn't get i2c driver");
        return;
    }

    let i2c = i2c.unwrap();

    let i2c_mu = Arc::new(Mutex::new(i2c));

    let sensor = bmp390::BMP390::new(i2c_mu.clone(), bmp390::DeviceAddr::AD0);

    if sensor.is_err() {
        println!("couldn't get sensor");
        return;
    }

    let mut sensor = sensor.unwrap();

    sensor
        .write_register(
            bmp390::Register::Osr,
            bmp390::Osr::Select(bmp390::OsrTemp::x32, bmp390::OsrPress::x2).value(),
        )
        .unwrap();

    loop {
        // println!("triggering read");
        sensor
            .write_register(
                bmp390::Register::PwrCtrl,
                bmp390::PwrCtrl::Forced {
                    press_en: true,
                    temp_en: true,
                }
                .value(),
            )
            .unwrap();

        std::thread::sleep(Duration::from_millis(100));
        let temperature = sensor.read_temperature();

        if let Ok(temperature) = temperature {
            if let Ok(pressure) = sensor.read_pressure(temperature) {
                let mut guard = state.lock().unwrap();
                let a = altitude(pressure, guard.sea_level_atmospheres);

                guard.max_temp = guard.max_temp.max(temperature);
                guard.min_temp = guard.min_temp.min(temperature);

                // println!("altitude: {}", a);

                guard.pres = a;
                guard.temp = temperature;

                if guard.max_alt < a {
                    guard.max_alt = a;
                }

                if guard.max_alt - a >= 20f64 {
                    guard.fire = true;
                }
            } else {
                println!("couldn't read sensor")
            }
        } else {
            println!("couldn't read sensor");
        }
    }
}

fn buzz_test<'d, T: OutputPin>(p: impl Peripheral<P = T> + 'd) {
    let mut buzzer = PinDriver::output(p).unwrap();

    let f = 4186f64; // frequency in hz.
    let p = 1.0f64 / f;
    let p2 = p * 0.5f64;

    let p2 = (p2 * 1000000f64) as u64;

    // play sound for 1/20th second
    let duration = 20f64.recip();

    let loops = (duration / p) as u64;

    loop {
        for i in 0..loops {
            buzzer.set_high();
            std::thread::sleep(Duration::from_micros(p2));
            buzzer.set_low();
            std::thread::sleep(Duration::from_micros(p2));
        }

        std::thread::sleep(Duration::from_secs(1));
    }
}

fn disable_wdt() {
    unsafe {
        esp_idf_svc::sys::esp_task_wdt_delete(esp_idf_svc::sys::xTaskGetIdleTaskHandleForCPU(
            esp_idf_hal::cpu::core() as u32,
        ));
    }
}

fn read_input(state: Arc<Mutex<State>>) {
    disable_wdt();
    let mut io = io::stdin();
    let mut cmd = String::new();
    loop {
        let mut str = [0u8; 1];
        if let Ok(_) = io.read(&mut str) {
            cmd.push_str(std::str::from_utf8(&str).unwrap());
        }

        if cmd.ends_with("\n") {
            let mut guard = state.lock().unwrap();
            println!("state: {:?}", guard);
            if cmd.starts_with("psl") {
                println!("setting pressure: {}", cmd);
                let parts: Vec<&str> = cmd.split(" ").collect();

                if parts.len() >= 2 {
                    if let Ok(f) = parts[1].trim().parse::<f64>() {
                        println!("Setting sea level atmosphere to {}", f);
                        guard.sea_level_atmospheres = f
                    } else {
                        println!("couldn't parse pressure: {}", parts[1]);
                    }
                } else {
                    println!("Need to provide pressure");
                }
            }
            if cmd.starts_with("reset") {
                guard.fire = false;
                guard.max_alt = f64::MIN;
                println!("reset");
            }
            if cmd.starts_with("?") {
                println!("commands: ");
                println!("reset");
                println!("psl PRESSURE_IN_PA");
            }

            if cmd.starts_with("beep") {
                println!("beeping");
                guard.fire = true;
            }

            cmd = String::new();
        }
    }
}

fn main() {
    // It is necessary to call this function once. Otherwise some patches to the runtime
    // implemented by esp-idf-sys might not link properly. See https://github.com/esp-rs/esp-idf-template/issues/71
    esp_idf_svc::sys::link_patches();

    // Bind the log crate to the ESP Logging facilities
    esp_idf_svc::log::EspLogger::initialize_default();

    let peripherals = Peripherals::take();

    if peripherals.is_err() {
        println!("couldn't get peripherals");
        return;
    }

    let peripherals = peripherals.unwrap();

    let gp = Arc::new(Mutex::new(peripherals.pins.gpio4));
    let gp2 = gp.clone();

    let gp21 = Arc::new(Mutex::new(peripherals.pins.gpio21));
    let gp22 = Arc::new(Mutex::new(peripherals.pins.gpio22));
    let i2c0 = Arc::new(Mutex::new(peripherals.i2c0));

    let state = State {
        fire: true,
        max_alt: f64::MIN,
        sea_level_atmospheres: 101320.74891,
        temp: 0f64,
        pres: 0f64,
        max_temp: f64::MIN,
        min_temp: f64::MAX,
    };

    let state = Arc::new(Mutex::new(state));

    {
        let state = state.clone();
        std::thread::spawn(move || loop {
            let i2c0 = i2c0.lock().unwrap();
            let gp21 = gp21.lock().unwrap();
            let gp22 = gp22.lock().unwrap();
            altimeter_monitor(i2c0, gp21, gp22, state.clone());
        });
    }

    let state2 = state.clone();
    std::thread::spawn(move || loop {
        println!("locking gp2");
        let gp = gp2.lock().unwrap();

        let fire = state2.lock().unwrap().fire;
        if fire {
            println!("buzz engaged");
            buzz_test(gp);
            std::thread::sleep(Duration::from_secs(1));
        }
        std::thread::sleep(Duration::from_millis(20));
    });

    read_input(state);

    loop {
        std::thread::sleep(Duration::from_secs(1));
    }

    // read in pressure.
    // calc altitude.
    // if above arm altitude, set armed
    // if below activate altitude, set start beeping.
}
