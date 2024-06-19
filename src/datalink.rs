use std::sync::mpsc::{Receiver, Sender};

use esp_idf_hal::modem::WifiModemPeripheral;
use esp_idf_svc::{
    eventloop::EspSystemEventLoop,
    nvs::EspDefaultNvsPartition,
    wifi::{BlockingWifi, ClientConfiguration, Configuration, EspWifi},
};

struct Datalink {
    wifi: BlockingWifi<EspWifi<'static>>,
    pub command_receiver: Receiver<String>,
    command_sender: Sender<String>,
}

impl Datalink {
    pub fn new<M: WifiModemPeripheral + 'static>(modem: M) -> Self {
        let wifi = {
            let sys_loop = EspSystemEventLoop::take().unwrap();
            let nvs = EspDefaultNvsPartition::take().unwrap();

            let wifi = EspWifi::new(modem, sys_loop.clone(), Some(nvs)).unwrap();

            let mut wifi = BlockingWifi::wrap(wifi, sys_loop).expect("Failed to create wifi");
            let configuration = Configuration::Client(ClientConfiguration::default());
            wifi.set_configuration(&configuration).unwrap();

            wifi
        };

        let (command_sender, command_receiver) = std::sync::mpsc::channel();

        Datalink {
            wifi,
            command_receiver,
            command_sender,
        }
    }

    fn task(&mut self) {
        loop {
            // are we started? if not, start...
            if !self.wifi.is_started().unwrap() {
                self.wifi.start().unwrap();
            }
            self.wifi.start().unwrap();

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
        }
    }
}
