use std::sync::mpsc::{Receiver, Sender};

use esp_idf_hal::modem::WifiModemPeripheral;
use esp_idf_svc::{
    espnow::PeerInfo,
    eventloop::EspSystemEventLoop,
    nvs::EspDefaultNvsPartition,
    wifi::{BlockingWifi, ClientConfiguration, Configuration, EspWifi, WifiDeviceId},
};

pub struct Datalink {
    pub command_receiver: Option<Receiver<([u8; 6], Vec<u8>)>>,
    pub data_sender: Sender<([u8; 6], Vec<u8>)>,
}

pub trait ByteSerialize<T> {
    fn as_bytes(&self, buffer: &mut [u8]) -> Result<(), ()>;
    fn from_bytes(buffer: &[u8]) -> Result<T, ()>;
}

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

impl Datalink {
    pub fn new<M: WifiModemPeripheral + 'static>(modem: M) -> Self {
        let mut wifi = {
            let sys_loop = EspSystemEventLoop::take().unwrap();
            let nvs = EspDefaultNvsPartition::take().unwrap();

            let wifi = EspWifi::new(modem, sys_loop.clone(), Some(nvs)).unwrap();

            let mut wifi = BlockingWifi::wrap(wifi, sys_loop).expect("Failed to create wifi");
            let configuration = Configuration::Client(ClientConfiguration::default());
            wifi.set_configuration(&configuration).unwrap();

            wifi
        };

        print_mac_addrs(&wifi);

        let (command_sender, command_receiver) = std::sync::mpsc::channel();

        let (data_sender, data_receiver) = std::sync::mpsc::channel::<([u8; 6], Vec<u8>)>();

        let espnow = esp_idf_svc::espnow::EspNow::take().unwrap();
        espnow
            .register_recv_cb(move |mac: &[u8], data: &[u8]| {
                let mut mac_arr = [0u8; 6];
                mac_arr.copy_from_slice(mac);
                let mut vec_data = Vec::new();
                vec_data.extend_from_slice(data);
                command_sender.send((mac_arr, vec_data)).unwrap();
            })
            .unwrap();

        std::thread::spawn(move || loop {
            wifi.start().unwrap();
            let (peer_addr, data) = data_receiver.recv().unwrap();
            // todo: better handling on error conditions or at least an except
            if !espnow.peer_exists(peer_addr).unwrap() {
                let mut peer_info = PeerInfo::default();
                peer_info.peer_addr.copy_from_slice(&peer_addr);
                espnow.add_peer(peer_info).unwrap();
            }
            if let Err(e) = espnow.send(peer_addr, &data) {
                log::error!(
                    "Failed to send to {:X}:{:X}:{:X}:{:X}:{:X}:{:X}: to {:}",
                    peer_addr[0],
                    peer_addr[1],
                    peer_addr[2],
                    peer_addr[3],
                    peer_addr[4],
                    peer_addr[5],
                    e
                );
            }
        });

        Datalink {
            command_receiver: Some(command_receiver),
            data_sender,
        }
    }
}
