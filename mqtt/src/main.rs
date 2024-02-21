#![no_std]
#![no_main]


use hal::{clock::ClockControl, peripherals::Peripherals, prelude::*, systimer::SystemTimer, Rng, Delay};
use embedded_io::*;
use embedded_svc::{
    ipv4::Interface,
    wifi::{AccessPointInfo, AuthMethod, ClientConfiguration, Configuration, Wifi},
};

use esp_backtrace as _;
use esp_println::{println, logger::init_logger_from_env};
use esp_wifi::{
    current_millis, initialize,
    wifi::{utils::create_network_interface, WifiError, WifiStaDevice},
    wifi_interface::WifiStack,
    EspWifiInitFor,
};
use mqttrust::encoding::v4::Pid;
use smoltcp::{
    iface::SocketStorage,
    wire::{IpAddress, Ipv4Address},
};
use crate::tiny_mqtt::TinyMqtt;
mod tiny_mqtt;

const SSID: &str = env!("SSID");
const PASSWORD: &str = env!("PASSWORD");

#[entry]
fn main() -> ! {
    init_logger_from_env();

    let peripherals = Peripherals::take();
    let system = peripherals.SYSTEM.split();
    let clocks = ClockControl::max(system.clock_control).freeze();
    let mut delay = Delay::new(&clocks);

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

    println!("Setting Wi-fi configuration for SSID: {}", SSID);
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
    match controller.scan_n::<10>() {
        Ok((res, _count)) =>
            for ap in res {
                println!(" - {:?}", ap);
            }
        Err(e) =>
            println!("Error scanning Wi-Fi: {:?}", e),
    }

    println!("{:?}", controller.get_capabilities());
    println!("Wi-Fi connect: {:?}", controller.connect());

    println!("Wait to get connected");
    loop {
        match controller.is_connected() {
            Ok(connected) => {
                if connected {
                    break;
                }
            }
            Err(e) => {
                delay.delay_ms(1000u16);
                println!("Wi-Fi connect: {:?}", controller.connect());
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

    // println!("Start busy loop on main");

    let mut rx_buffer = [0u8; 1536];
    let mut tx_buffer = [0u8; 1536];
    let socket = wifi_stack.get_socket(&mut rx_buffer, &mut tx_buffer);

    let mut mqtt = TinyMqtt::new("esp32", socket, current_millis, None);

    loop {
        println!("Connecting to MQTT server");
        // TODO Why disconnect??
        // mqtt.disconnect().ok();
        match mqtt.connect(
            IpAddress::Ipv4(Ipv4Address::new(192,168,50,64)), // nats mqtt
            1883,
            10,
            None,
            None,
        ) {
            Ok(()) => {
                println!("Connected to MQTT server");
                break;
            }
            Err(e) => {
                println!("Error connecting to MQTT server: {:?}", e);
                delay.delay_ms(1000u16);
            }
        }
    }

    let mut pkt_num = 0u16;
    loop {
        match mqtt.poll() {
            Ok(()) =>
                println!("Polled MQTT"),
            Err(e) =>
                panic!("Error polling MQTT: {:?}", e),
        }

        match pkt_num.checked_add(1) {
            Some(n) => pkt_num = n,
            None => panic!("overflow message id"),
        }

        println!("Publishing MQTT message");
        println!("Publishing MQTT message {}", pkt_num);
        let res = mqtt
            .publish_with_pid(
                Some(Pid::try_from(pkt_num).unwrap()),
                "test.topic",
                "test-msg".as_bytes(),
                mqttrust::QoS::AtLeastOnce,
            );


        println!("Publish result: {:?}", res);
        delay.delay_ms(1000u16);
    }

    panic!("End of main");
}
