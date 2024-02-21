#![no_std]
#![no_main]

use hal::{clock::ClockControl, peripherals::Peripherals, prelude::*, systimer::SystemTimer, Rng};
use embedded_io::*;
use embedded_svc::{
    ipv4::Interface,
    wifi::{AccessPointInfo, AuthMethod, ClientConfiguration, Configuration, Wifi},
};

use esp_backtrace as _;
use esp_println::{print, println};
use esp_wifi::{
    current_millis, initialize,
    wifi::{utils::create_network_interface, WifiError, WifiStaDevice},
    wifi_interface::WifiStack,
    EspWifiInitFor,
};
use smoltcp::{
    iface::SocketStorage,
    wire::{IpAddress, Ipv4Address},
};

const SSID: &str = env!("SSID");
const PASSWORD: &str = env!("PASSWORD");

#[entry]
fn main() -> ! {
    let peripherals = Peripherals::take();
    let system = peripherals.SYSTEM.split();
    let clocks = ClockControl::max(system.clock_control).freeze();

    let timer = SystemTimer::new(peripherals.SYSTIMER).alarm0;
    let init = initialize(
        EspWifiInitFor::Wifi,
        timer,
        Rng::new(peripherals.RNG),
        system.radio_clock_control,
        &clocks,
    ).unwrap();

    let wifi = peripherals.WIFI;
    let mut socket_set_entries: [SocketStorage; 3] = Default::default();
    let (iface, device, mut controller, sockets) =
        create_network_interface(&init, wifi, WifiStaDevice, &mut socket_set_entries).unwrap();
    let wifi_stack = WifiStack::new(iface, device, sockets, current_millis);

    let mut auth_method = AuthMethod::WPA2Personal;
    let mut channel = None;
    if PASSWORD.is_empty() {
        auth_method = AuthMethod::None;
        channel = Some(6);
    }

    let client_config = Configuration::Client(ClientConfiguration {
        ssid: SSID.try_into().unwrap(),
        password: PASSWORD.try_into().unwrap(),
        auth_method,
        channel,
        ..Default::default()
    });

    let res = controller.set_configuration(&client_config);
    println!("Wi-fi set_configuration returned {:?}", res);

    controller.start().unwrap();
    println!("Is Wi-fi started: {:?}", controller.is_started());

    println!("Start Wi-fi scan");
    let res: Result<(heapless::Vec<AccessPointInfo, 10>, usize), WifiError> = controller.scan_n();
    if let Ok((res, _count)) = res {
        for ap in res {
            println!("{:?}", ap);
        }
    }

    println!("{:?}", controller.get_capabilities());
    println!("Wi-Fi connect: {:?}", controller.connect());

    println!("Wait to get connected");
    loop {
        let res = controller.is_connected();
        match res {
            Ok(connected) => {
                if connected {
                    break;
                }
            }
            Err(e) => {
                println!("Error: {:?}", e);
            }
        }
    }

    println!("{:?}", controller.is_connected());
    println!("Wait to get an IP address");
    loop {
        wifi_stack.work();

        if wifi_stack.is_iface_up() {
            println!("Got IP {:?}", wifi_stack.get_ip_info());
            break;
        }
    }

    println!("Start busy loop on main");

    let mut rx_buffer = [0u8; 1536];
    let mut tx_buffer = [0u8; 1536];
    let mut socket = wifi_stack.get_socket(&mut rx_buffer, &mut tx_buffer);

    loop {
        println!("Making HTTP request");
        socket.work();

        socket.open(IpAddress::Ipv4(Ipv4Address::new(192,168,50,64)), 8222).unwrap();
        socket.write(b"GET / HTTP/1.0\r\nHost: 0.0.0.0\r\nConnection: close\r\n\r\n").unwrap();
        socket.flush().unwrap();

        let wait_end = current_millis() + 5000;
        loop {
            let mut buffer = [0; 256];
            match socket.read(&mut buffer) {
                Ok(0) => {
                    println!("Connection closed");
                    break;
                }
                Ok(n) => {
                    print!("{}", core::str::from_utf8(&buffer[..n]).unwrap());
                }
                Err(e) => {
                    println!("Error: {:?}", e);
                    break;
                }
            }
            if current_millis() > wait_end {
                println!("Timeout");
                break;
            }
        }
        println!("Closing socket");
        socket.disconnect();

        let wait_end = current_millis() + 5 * 1000;
        while current_millis() < wait_end {
            socket.work();
        }

    }
}
