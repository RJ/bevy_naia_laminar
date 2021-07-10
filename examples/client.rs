// we want to use bevy::log everywhere, but need log::Level to set console level to debug
#[cfg(target_arch = "wasm32")]
extern crate log as log_fac;
#[cfg(target_arch = "wasm32")]
use log_fac::Level as LogLevel;

use bevy::prelude::*;
use bevy::log::{self, LogPlugin};
use bevy::app::{ScheduleRunnerSettings, EventReader};

use bevy_naia_laminar::client::prelude::*;

use std::{net::SocketAddr, time::Duration};

const SERVER_PORT: u16 = 14191;

fn main() {
    cfg_if::cfg_if! {
        if #[cfg(target_arch = "wasm32")] {
            std::panic::set_hook(Box::new(console_error_panic_hook::hook));
            console_log::init_with_level(LogLevel::Debug).expect("cannot initialize console_log");
        }
    }

    App::build()
        // minimal plugins necessary for timers + headless loop
        .insert_resource(ScheduleRunnerSettings::run_loop(Duration::from_secs_f64(
            1.0 / 60.0,
        )))
        .add_plugins(MinimalPlugins)
        .add_plugin(LogPlugin)
        // The NetworkingPlugin
        .add_plugin(ClientNetworkingPlugin::default())
        // Our networking
        .add_startup_system(startup.system())
        .add_system(send_packets.system())
        .add_system(handle_packets.system())
        .run();
}

fn startup(mut net: ResMut<NetworkResource>) {
    // compile_error!("Set the ip to the one your server is listening on");
    log::warn!("I hope you edited the examples/client.rs and set the IP address");
    let server_address: SocketAddr = "10.0.0.123:14191".parse().expect("can't parse server addr"); 
    log::info!("Starting client (--> {:?})", server_address);

    let config = LaminarConfig {
        idle_connection_timeout: Duration::from_millis(3000),
        heartbeat_interval: Some(Duration::from_millis(1000)),
        ..Default::default()
    };

    net.connect(server_address, config);
}

fn send_packets(mut net: ResMut<NetworkResource>, time: Res<Time>) {
    if !net.initialized() {
        return;
    }
    if (time.seconds_since_startup() * 60.) as i64 % 60 == 0 {
        let payload = format!("PING {}", time.seconds_since_startup()).into_bytes();
        log::info!("Sending payload: {:?}", payload);
        
        let dst_addr = net.server_addr();
        let packet = LaminarPacket::unreliable(*dst_addr, payload);
        net.send(packet);
    }
}

fn handle_packets(
    // mut net: ResMut<NetworkResource>,
    mut laminar_events: EventReader<LaminarSocketEvent>,
) {
    for event in laminar_events.iter() {
        match event {
            LaminarSocketEvent::Packet(packet) => {
                log::info!(">>> LaminarPacket: {:?}", packet);
                log::info!(">>> Payload string: {}", String::from_utf8_lossy(packet.payload()));
            },
            LaminarSocketEvent::Connect(_addr) => {
                log::info!(">>> {:?}", event);
            },
            LaminarSocketEvent::Disconnect(_addr) => {
                log::info!(">>> {:?}", event);
            },
            LaminarSocketEvent::Timeout(_addr) => {
                log::info!(">>> {:?}", event);
            },
        }
    }
}
