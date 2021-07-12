// we want to use bevy::log everywhere, but need log::Level to set console level to debug
#[cfg(target_arch = "wasm32")]
extern crate log as log_fac;
#[cfg(target_arch = "wasm32")]
use log_fac::Level as LogLevel;

use bevy::prelude::*;
use bevy::log::{self, LogPlugin};
use bevy::app::{ScheduleRunnerSettings, EventReader};

use bevy_naia_laminar::prelude::*;
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
    let link_conditioner = Some(LinkConditionerConfig {
        // Delay to receive incoming messages in milliseconds
        incoming_latency: 10, //50,  // and 50 on server too
        // The maximum additional random latency to delay received incoming messages in milliseconds.
        // This may be added OR subtracted from the latency determined in the incoming_latency property above
        incoming_jitter: 5, //5,
        // The % chance that an incoming packet will be dropped. Represented as a value between 0 and 1
        incoming_loss: 0.2,
        // The % chance that an incoming packet will have a single bit tampered with.
        // Represented as a value between 0 and 1
        // SET TO ZERO - webrtc does checksumming and corruption manifests as packet loss
        incoming_corruption: 0.0,           
    });
    let link_conditioner = None;
    let net_plugin = ClientNetworkingPlugin{
        link_conditioner,
        ..Default::default()
    };

    App::build()
        // minimal plugins necessary for timers + headless loop
        .insert_resource(ScheduleRunnerSettings::run_loop(Duration::from_secs_f64(
            1.0 / 60.0,
        )))
        .add_plugins(MinimalPlugins)
        .add_plugin(LogPlugin)
        // The NetworkingPlugin
        .add_plugin(net_plugin)
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

fn send_packets(mut net: ResMut<NetworkResource>, time: Res<Time>, mut n: Local<u32>, mut ttp: Local<f64>) {
    if !net.initialized() {
        return;
    }
    if net.connection_state() != ConnectionState::Connected {
        return;
    }
    *ttp += time.delta_seconds_f64();
    if *ttp >= 1.0 {
        *ttp = 0.0;
        *n += 1;
        let payload = format!("🏓 PING {}", *n).into_bytes();
        log::info!("<<< Sending payload: {:?}", String::from_utf8_lossy(&payload));
        
        let dst_addr = net.server_addr();
        let packet = LaminarPacket::unreliable(*dst_addr, payload);
        net.send(packet);
    }
}

fn handle_packets(
    // mut net: ResMut<NetworkResource>,
    mut peer_events: EventReader<PeerEvent>,
) {
    for event in peer_events.iter() {
        if let PeerEvent::Packet(packet) = event {
            log::info!(">>> PACKET from {:?}, payload len:{} str:{}",
                packet.addr(),
                packet.payload().len(),
                String::from_utf8_lossy(packet.payload())
            );
        } else {
            log::info!(">>> PEER EVENT {:?}", event);
        }
    }
}
