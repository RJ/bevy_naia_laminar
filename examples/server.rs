use bevy::prelude::*;
use bevy::log::{self, LogPlugin};
use bevy::app::{ScheduleRunnerSettings, EventReader};

use bevy_naia_laminar::server::prelude::*;

use std::{net::SocketAddr, time::Duration};

const SERVER_PORT: u16 = 14191;

fn main() {

    App::build()
        // minimal plugins necessary for timers + headless loop
        .insert_resource(ScheduleRunnerSettings::run_loop(Duration::from_secs_f64(
            1.0 / 60.0,
        )))
        .add_plugins(MinimalPlugins)
        .add_plugin(LogPlugin)
        .add_plugin(ServerNetworkingPlugin::default())
        .add_startup_system(startup.system())
        .add_system(handle_packets.system())
        .run();
}

fn startup(mut net: ResMut<NetworkResource>) {
    let ip_address = find_my_ip_address().expect("can't find ip address");
    let server_address = SocketAddr::new(ip_address, SERVER_PORT);

    #[cfg(feature = "use-webrtc")]
    log::info!("Starting WebRTC server on {:?}", server_address);
    #[cfg(feature = "use-udp")]
    log::info!("Starting native UDP server on {:?}", server_address);

    let config = LaminarConfig {
        idle_connection_timeout: Duration::from_millis(3000),
        heartbeat_interval: Some(Duration::from_millis(1000)),
        ..Default::default()
    };

    net.listen(config, server_address, None, None);
}

fn handle_packets(
    mut net: ResMut<NetworkResource>,
    mut laminar_events: EventReader<LaminarSocketEvent>,
) {
    let mut to_send = Vec::new();

    for event in laminar_events.iter() {
        match event {
            LaminarSocketEvent::Packet(packet) => {
                log::info!(">>> LaminarPacket: {:?}", packet);
                log::info!(">>> Payload string: {}", String::from_utf8_lossy(packet.payload()));
                let payload = format!("You sent: {}", String::from_utf8_lossy(packet.payload())).into_bytes();
                let response_packet = LaminarPacket::unreliable(
                    packet.addr(),
                    payload
                );
                to_send.push(response_packet);
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

    for packet in to_send {
        log::info!("<<< {:?}", packet);
        log::info!("<<< Payload string: {}", String::from_utf8_lossy(packet.payload()));
        net.send(packet).unwrap();
    }
}
