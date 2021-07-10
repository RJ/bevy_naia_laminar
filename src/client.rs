use bevy::{
    log,
    app::{AppBuilder, EventWriter, Plugin},
    ecs::prelude::*,
};
use crossbeam_channel::{unbounded, Receiver, Sender};
use std::net::SocketAddr;
use instant::Instant;

// "use" with Laminar/Naia prefix as needed, since both have Packets and similar concepts.

use naia_client_socket::{
    ClientSocketTrait as NaiaClientSocketTrait,
    MessageSender as NaiaMessageSender,
    Packet as NaiaPacket,
    ClientSocket as NaiaSocket,
    LinkConditionerConfig,
};

pub use laminar::{
    Config as LaminarConfig,
    Connection as LaminarConnection,
    ConnectionMessenger as LaminarConnectionMessenger,
    Packet as LaminarPacket,
    VirtualConnection as LaminarVirtualConnection,
    SocketEvent as LaminarSocketEvent,
};

pub mod prelude {
    pub use super::{LaminarConfig, LaminarPacket, LaminarSocketEvent};
    pub use naia_client_socket::LinkConditionerConfig;
    pub use super::NetworkResource;
    pub use super::ClientNetworkingPlugin;
}

// If we want to allow connections to multiple laminar servers, we'll have to expose PeerConnections.
// for now we just support connecting to 1 server, and expose everything through NetworkResource functions
// that delegate to the sole PeerConnection.
struct PeerConnection {
    server_addr: SocketAddr,
    naia_socket: Box<dyn NaiaClientSocketTrait>,
    laminar_vconnection: LaminarVirtualConnection,
    laminar_messenger: LaminarConnectionMessengerForNaia,
    laminar_event_receiver: Receiver<ReceiveEvent>,
}

impl PeerConnection {
    pub fn new(
        config: LaminarConfig,
        mut naia_socket: Box<dyn NaiaClientSocketTrait>,
        server_socket_address: &SocketAddr,
    ) -> Self {

        let naia_message_sender = naia_socket.get_sender();

        let (laminar_event_sender, laminar_event_receiver) = unbounded();
        
        let mut laminar_messenger = LaminarConnectionMessengerForNaia {
            config,
            naia_message_sender,
            laminar_event_sender,
        };
        
        let laminar_vconnection = LaminarVirtualConnection::create_connection(
            &mut laminar_messenger,
            *server_socket_address,
            Instant::now(),
        );
        
        PeerConnection {
            naia_socket,
            server_addr: *server_socket_address,
            laminar_vconnection,
            laminar_messenger,
            laminar_event_receiver,
        }
    }

    /// gets laminar event receiver
    pub fn event_receiver(&mut self) -> &Receiver<ReceiveEvent> {
        &self.laminar_event_receiver
    }

    /// get address of server we've connected to
    pub fn server_addr(&self) -> &SocketAddr {
        &self.server_addr
    }

    /// sends a LaminarPacket on the laminar virtual connection
    pub fn send(&mut self, event: LaminarPacket) {
        self.laminar_vconnection.process_event(&mut self.laminar_messenger, event, Instant::now());
    }

    /// recv incoming packets from naia, and hand on to laminar
    pub fn poll(&mut self, time: Instant) {
        loop {
            match self.naia_socket.receive() {
                Ok(event) => match event {
                    Some(packet) => {
                        log::info!("process_incoming: {:?}", String::from_utf8_lossy(packet.payload()));
                        // Bytes::copy_from_slice(packet.payload())
                        self.laminar_vconnection.process_packet(&mut self.laminar_messenger, packet.payload(), time);
                    },
                    None => {
                        break;
                    },
                },
                Err(err) => {
                    log::error!("Error process_incoming for {}", err);
                }
            }
        }
        // TODO should perhaps separate the following call?
        //      maybe poll_recv in preupdate and then vconnection update in post?
        self.laminar_vconnection.update(&mut self.laminar_messenger, time);
    }
}

#[cfg(target_arch = "wasm32")]
unsafe impl Send for PeerConnection {}

#[cfg(target_arch = "wasm32")]
unsafe impl Sync for PeerConnection {}



#[derive(Default)]
pub struct ClientNetworkingPlugin {
    pub link_conditioner: Option<LinkConditionerConfig>,
}

impl Plugin for ClientNetworkingPlugin {
    fn build(&self, app: &mut AppBuilder) {
        let net_resource = NetworkResource::new(
            self.link_conditioner.clone(),
        );
        app
        .add_event::<LaminarSocketEvent>()
        .insert_resource(net_resource)
        .add_system(laminar_poller.system())
        ;
    }
}

type ReceiveEvent = <LaminarVirtualConnection as LaminarConnection>::ReceiveEvent;

struct LaminarConnectionMessengerForNaia {
    config: LaminarConfig,
    naia_message_sender: NaiaMessageSender,
    laminar_event_sender: Sender<ReceiveEvent>,
}

impl LaminarConnectionMessenger<ReceiveEvent> for LaminarConnectionMessengerForNaia {
    fn config(&self) -> &LaminarConfig {
        &self.config
    }

    // publishes laminar connection event
    fn send_event(&mut self, _address: &SocketAddr, event: ReceiveEvent) {
        log::info!("PacketMessenger::send_event {:?}", event);
        self.laminar_event_sender.send(event).expect("send_event failed?");
    }

    // sends packet
    fn send_packet(&mut self, _address: &SocketAddr, payload: &[u8]) {
        log::info!("PacketMessenger::send_packet {}", payload.len());
        self.naia_message_sender
            .send(NaiaPacket::new(payload.to_vec()))
            .expect("send packet error");
    }
}


#[derive(Default)]
pub struct NetworkResource {
    connection: Option<PeerConnection>,
    link_conditioner: Option<LinkConditionerConfig>,
}

#[cfg(target_arch = "wasm32")]
unsafe impl Send for NetworkResource {}

#[cfg(target_arch = "wasm32")]
unsafe impl Sync for NetworkResource {}

// this is the bevy resource you summon in your systems to interact with laminar.
// mostly delegate to the sole private PeerConnection. in future we might
// expose multiple peerconnections if multiple server connections required.
impl NetworkResource {
    pub fn new(link_conditioner: Option<LinkConditionerConfig>) -> Self
    {
        Self {
            link_conditioner,
            connection: None,
        }
    }

    /// Until initialized() is true, most other functions will crash with an assert.
    pub fn initialized(&self) -> bool {
        self.connection.is_some()
    }

    /// Get Laminar event receiver
    fn event_receiver(&mut self) -> &Receiver<ReceiveEvent> {
        assert!(self.initialized(), "not initialized!");
        self.connection_mut().event_receiver()
    }

    /// Get SocketAddr of server we are connected to
    pub fn server_addr(&self) -> &SocketAddr {
        assert!(self.initialized(), "not initialized!");
        self.connection().server_addr()
    }

    /// send a LaminarPacket
    pub fn send(&mut self, packet: LaminarPacket) {
        assert!(self.initialized(), "not initialized!");
        self.connection_mut().send(packet);
    }

    /// calls laminar's manual_poll - do per tick
    pub fn poll(&mut self) {
        assert!(self.initialized(), "not initialized!");
        let now = Instant::now();
        self.connection_mut().poll(now);
    }

    pub fn connect_with_defaults(&mut self, socket_address: SocketAddr) {
        self.connect(socket_address, LaminarConfig::default());
    }

    /// connect to server. sets initialized() to true.
    pub fn connect(&mut self, socket_address: SocketAddr, config: LaminarConfig) {
        let naia_socket = {
            let socket = NaiaSocket::connect(socket_address);

            if let Some(ref conditioner) = self.link_conditioner {
                socket.with_link_conditioner(conditioner)
            } else {
                socket
            }
        };
        self.connection = Some(
            PeerConnection::new(
                config,
                naia_socket,
                &socket_address,
            )
        );

        // send an initial empty packet to make sure the connection gets marked as connected
        let hello_packet = LaminarPacket::reliable_unordered(socket_address, vec![]);
        self.send(hello_packet);
    }

    fn connection(&self) -> &PeerConnection {
        self.connection.as_ref().unwrap()
    }

    fn connection_mut(&mut self) -> &mut PeerConnection {
        self.connection.as_mut().unwrap()
    }
}

fn laminar_poller(
    mut net: ResMut<NetworkResource>,
    mut lam_evs: EventWriter<LaminarSocketEvent>,
){
    if !net.initialized() {
        return;
    }

    net.poll();

    let event_receiver = net.event_receiver();

    // publish laminar socket events to bevy events - we won't expose the event_receiver.
    while let Ok(event) = event_receiver.try_recv() {
        lam_evs.send(event);
    }
}