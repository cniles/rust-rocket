use std::{
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use esp_idf_hal::{
    gpio::{Output, OutputPin, PinDriver},
    peripheral::Peripheral,
};

#[derive(Copy, Clone)]
pub enum BuzzPattern {
    Beep { frequency: u32, duration: u32 },
    Quiet,
}

fn buzz<'d, T: OutputPin>(buzzer: &mut PinDriver<'d, T, Output>, frequency: u32, duration: u32) {
    let f = frequency as f64; // frequency in hz.
    let p = 1.0f64 / f;
    let p2 = p * 0.5f64;

    let p2 = (p2 * 1000000f64) as u64;

    // play sound for 1/20th second
    let duration = duration as f64 / 1000f64;

    let loops = (duration / p) as u64;

    for _ in 0..loops {
        buzzer.set_high().unwrap();
        std::thread::sleep(Duration::from_micros(p2));
        buzzer.set_low().unwrap();
        std::thread::sleep(Duration::from_micros(p2));
    }
}

struct BuzzerState {
    playing: bool,
    pattern: BuzzPattern,
    period: u32,
    once: bool,
}

pub struct Buzzer {
    state: Arc<Mutex<BuzzerState>>,
}

impl Buzzer {
    pub fn pattern(&self, buzz_pattern: BuzzPattern) {
        self.state.lock().unwrap().pattern = buzz_pattern;
    }

    pub fn start(&self) {
        self.state.lock().unwrap().playing = true;
    }

    pub fn once(&self) {
        let mut state = self.state.lock().unwrap();
        state.once = true;
        state.playing = true;
    }

    pub fn stop(&self) {
        self.state.lock().unwrap().playing = false;
    }

    pub fn period(&self, period: u32) {
        self.state.lock().unwrap().period = period;
    }

    pub fn new<O: OutputPin>(gpio: impl Peripheral<P = O> + 'static + Send) -> Buzzer {
        let buzzer = Buzzer {
            state: Arc::new(Mutex::new(BuzzerState {
                playing: false,
                pattern: BuzzPattern::Quiet,
                period: 5000,
                once: false,
            })),
        };

        let state = buzzer.state.clone();
        let mut pin_driver = PinDriver::output(gpio).unwrap();

        std::thread::spawn(move || loop {
            let start = std::time::Instant::now();

            let (playing, pattern, period, once) = {
                let state = state.lock().unwrap();
                (state.playing, state.pattern, state.period, state.once)
            };

            if playing {
                match pattern {
                    BuzzPattern::Beep {
                        frequency,
                        duration,
                    } => buzz(&mut pin_driver, frequency, duration),
                    BuzzPattern::Quiet => (),
                }

                if once {
                    let mut state = state.lock().unwrap();
                    state.playing = false;
                    state.once = false;
                }
            }

            let period = Duration::from_millis(period as u64);

            let elapsed = start.elapsed();

            if elapsed < period {
                std::thread::sleep(period - elapsed);
            }
        });

        buzzer
    }
}
