use bevy::prelude::*;
use bevy::log::{self, LogPlugin};
use bevy::app::{ScheduleRunnerSettings, EventReader};

use bevy_naia_laminar::prelude::*;
use bevy_naia_laminar::server::prelude::*;

use std::os::unix::net;
use std::{net::SocketAddr, time::Duration};

const SERVER_PORT: u16 = 14191;

fn main() {

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
    let net_plugin = ServerNetworkingPlugin{
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
        .add_plugin(net_plugin)
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
    net: Res<NetworkResource>,
    mut peer_events: EventReader<PeerEvent>,
) {

    for event in peer_events.iter() {
        match event {
            PeerEvent::Packet(packet) => {
                let peer = net.peer(packet.peer_handle()).unwrap();
            
                log::info!(">>> ({:?}) [#peers:{}] {:?}", peer.state(), net.num_peers(), packet);
                log::info!(">>> Payload.str: {}", String::from_utf8_lossy(packet.payload()));
                let payload = format!("You sent: {}", String::from_utf8_lossy(packet.payload())).into_bytes();
                
                let response_packet = LaminarPacket::unreliable(
                    packet.addr(),
                    payload
                );
                net.send(response_packet).unwrap();
            },
            PeerEvent::Status(handle, state) => {
                log::info!("PEER_EVENT {:?} = {:?}", handle, state);
                log::info!("Num peers: {}", net.num_peers());

                if *state == ConnectionState::Connected {
                    net.send(LaminarPacket::reliable_unordered(*handle, Vec::from("Welcome, friend"))).unwrap();
                }
            }
        }
    }

}
