use bevy::{
    log,
    app::{AppBuilder, EventWriter, Plugin},
    ecs::prelude::*,
    tasks::{IoTaskPool, TaskPool, Task},
};

use crossbeam_channel::{
    unbounded,
    Receiver,
    Sender,
    TryRecvError as CrossbeamTryRecvError,
    RecvError as CrossbeamRecvError,
    SendError as CrossbeamSendError,
};
use laminar::{Connection, Socket};

use std::{
    fmt::Debug,
    net::SocketAddr,
    io,
    collections::HashMap,
};

use instant::Instant;

use naia_server_socket::{
    // MessageSender as NaiaMessageSender,
    ServerSocket as NaiaServerSocket,
    Packet as NaiaPacket,
    LinkConditionerConfig
};

use futures_lite::future::block_on;

pub use naia_server_socket::find_my_ip_address;

pub use laminar::{
    Config as LaminarConfig,
    ConnectionManager as LaminarConnectionManager,
    DatagramSocket as LaminarDatagramSocket,
    Packet as LaminarPacket,
    SocketEvent as LaminarSocketEvent,
    VirtualConnection as LaminarVirtualConnection,
};

use crate::prelude::*;

pub mod prelude {
    pub use super::{LaminarConfig, LaminarPacket, LaminarSocketEvent};
    pub use super::find_my_ip_address;
    pub use naia_client_socket::LinkConditionerConfig;
    pub use super::NetworkResource;
    pub use super::ServerNetworkingPlugin;
}

#[derive(Debug)]
pub struct LaminarDatagramSocketForNaia {
    pub bind_address: SocketAddr,
    pub naia_packet_receiver: Receiver<NaiaPacket>,
    pub naia_payload_sender: Sender<NaiaPacket>,
}

impl LaminarDatagramSocket for LaminarDatagramSocketForNaia {
    fn send_packet(&mut self, addr: &SocketAddr, payload: &[u8]) -> io::Result<usize> {
        match self.naia_payload_sender.send(NaiaPacket::new(*addr, payload.to_vec())) {
            Ok(()) => Ok(payload.len()),
            Err(err) => {
                    log::error!("Failed sending packet to naia sender?");
                    assert!(false, "Crossbeam SendError for {:?}", err);
                    // this shouldn't really happen, but if the cb channel is closed
                    // i don't want to panic. it's kinda like a broken pipe right? :)
                    Err(io::Error::new(io::ErrorKind::BrokenPipe, err))
            }
        }
    }

    fn receive_packet<'a>(&mut self, buffer: &'a mut [u8]) -> io::Result<(&'a [u8], SocketAddr)> {
        match self.naia_packet_receiver.try_recv() {
            Ok(packet) => {
                let payload = packet.payload();
                buffer[..payload.len()].clone_from_slice(payload);
                Ok((&buffer[..payload.len()], packet.address()))
            }
            Err(error) => match error {
                CrossbeamTryRecvError::Empty => {
                    Err(io::Error::new(io::ErrorKind::WouldBlock, error))
                },
                CrossbeamTryRecvError::Disconnected => {
                    log::error!("Crossbeam channel for naia is Disconnected?");
                    assert!(false, "Crossbeam channel for naia is Disconnected? {:?}", error);
                    Err(io::Error::new(io::ErrorKind::NotConnected, error))
                }
            },
        }
    }

    fn local_addr(&self) -> io::Result<SocketAddr> {
        Ok(self.bind_address)
    }

    fn is_blocking_mode(&self) -> bool {
        false
    }
}


#[derive(Default)]
pub struct ServerNetworkingPlugin {
    pub link_conditioner: Option<LinkConditionerConfig>,
}

impl Plugin for ServerNetworkingPlugin {
    fn build(&self, app: &mut AppBuilder) {
        let task_pool = app
            .world()
            .get_resource::<IoTaskPool>()
            .expect("`IoTaskPool` resource not found.")
            .0
            .clone();

        app
        .insert_resource(NetworkResource::new(
            task_pool,
            self.link_conditioner.clone(),
        ))
        .add_event::<LaminarPacket>()
        .add_event::<PeerEvent>()
        .add_system(laminar_poller.system())
        ;
    }
}



// peers, keyed by their socketaddr
#[derive(Debug)]
pub struct Peer {
    pub epoch: Instant,
    pub socket_addr: SocketAddr,
    connection_state: ConnectionState,
    event_sender: Sender<LaminarPacket>,
}

impl Peer {
    fn new(socket_addr: SocketAddr, event_sender: Sender<LaminarPacket>) -> Self {
        Self {
            epoch: Instant::now(),
            socket_addr,
            connection_state: ConnectionState::Connecting,
            event_sender,
        }
    }

    pub fn addr(&self) -> SocketAddr {
        self.socket_addr
    }

    pub fn send(&self, packet: LaminarPacket) -> Result<(), CrossbeamSendError<LaminarPacket>> {
        self.event_sender.send(packet)
    }

    pub fn state(&self) -> ConnectionState {
        self.connection_state
    }
    
    fn set_state(&mut self, new_state: ConnectionState) -> bool {
        let changed = new_state != self.connection_state;
        self.connection_state = new_state;
        changed
    }
}

pub struct NetworkResource {
    task_pool: TaskPool,
    listeners: Vec<ServerListener>,
    manager: Option<LaminarConnectionManager<LaminarDatagramSocketForNaia, LaminarVirtualConnection>>,
    link_conditioner: Option<LinkConditionerConfig>,
    peers: HashMap<SocketAddr, Peer>,
}

// just used to keep tasks in scope so they aren't dropped
#[allow(dead_code)]
struct ServerListener {
    socket_address: SocketAddr,
    tasks: Vec<Task<()>>,
}

impl NetworkResource {
    pub fn new( task_pool: TaskPool,
                link_conditioner: Option<LinkConditionerConfig>,
            ) -> Self
    {
        NetworkResource {
            task_pool,
            link_conditioner,
            listeners: Vec::new(),
            manager: None,
            peers: HashMap::new(),
        }
    }

    fn new_peer(&self, addr: SocketAddr) -> Peer {
        Peer::new(addr, self.event_sender().clone())
    }

    pub fn peer(&self, handle: PeerHandle) -> Option<&Peer> {
        self.peers.get(&handle)
    }

    pub fn peer_mut(&mut self, handle: PeerHandle) -> Option<&mut Peer> {
        self.peers.get_mut(&handle)
    }

    pub fn num_peers(&self) -> usize {
        self.peers.len()
    }

    // fn peers_mut(&mut self) -> &mut HashMap<SocketAddr, Peer> {
    //     &mut self.peers
    // }

    // pub fn peers(&self) -> &HashMap<SocketAddr, Peer> {
    //     &self.peers
    // }

    /// everything is going to crash with an assert unless this returns true
    pub fn initialized(&self) -> bool {
        self.manager.is_some()
    }

    pub fn manager(&self) -> &LaminarConnectionManager<LaminarDatagramSocketForNaia, LaminarVirtualConnection> {
        assert!(self.initialized(), "manager not initialised yet");
        self.manager.as_ref().unwrap()
    }

    pub fn manager_mut(&mut self) -> &mut LaminarConnectionManager<LaminarDatagramSocketForNaia, LaminarVirtualConnection> {
        assert!(self.initialized(), "manager not initialised yet");
        self.manager.as_mut().unwrap()
    }

    pub fn poll(&mut self) {
        assert!(self.initialized(), "manager not initialised yet");
        self.manager_mut().manual_poll(Instant::now());
    }

    pub fn send(&self, packet: LaminarPacket) -> Result<(), CrossbeamSendError<LaminarPacket>> {
        self.event_sender().send(packet)
    }

    pub fn event_sender(&self) -> &Sender<LaminarPacket> {
        assert!(self.initialized(), "manager not initialised yet");
        self.manager().event_sender()
    }

    pub fn event_receiver(&self) -> &Receiver<LaminarSocketEvent> {
        assert!(self.initialized(), "manager not initialised yet");
        self.manager().event_receiver()
    }

    /// The 3 listening addresses aren't strictly necessary, you can put the same IP address with
    /// a different port for the socket address; Unless you have some configuration issues with 
    /// public and private addresses that need to be connected to.
    /// They also aren't necessary if you're using UDP, so you can put anything if that's the case.
    pub fn listen(
        &mut self,
        laminar_config: LaminarConfig,
        socket_address: SocketAddr,
        webrtc_listen_address: Option<SocketAddr>,
        public_webrtc_address: Option<SocketAddr>,
    ) {
        let mut server_socket = {
            let webrtc_listen_address = webrtc_listen_address.unwrap_or_else(|| {
                let mut listen_addr = socket_address;
                listen_addr.set_port(socket_address.port() + 1);
                listen_addr
            });
            let public_webrtc_address = public_webrtc_address.unwrap_or(webrtc_listen_address);
            let socket = block_on(NaiaServerSocket::listen(
                socket_address,
                webrtc_listen_address,
                public_webrtc_address,
            ));

            if let Some(ref conditioner) = self.link_conditioner {
                socket.with_link_conditioner(conditioner)
            } else {
                socket
            }
        };
        
        // all packets from naia, regardless of src_addr, are sent to laminar, which is responsible
        // for assigning them to virtual connections, channels, etc.

        // naia_packet_tx and naia_payload_rx are moved to async tasks below
        let (naia_packet_tx, naia_packet_rx) = unbounded::<NaiaPacket>();
        let (naia_payload_tx, naia_payload_rx ) = unbounded::<NaiaPacket>();
        
        let mut naia_sender = server_socket.get_sender();

        self.manager = Some(LaminarConnectionManager::new(
            LaminarDatagramSocketForNaia {
                bind_address: socket_address,
                naia_packet_receiver: naia_packet_rx,
                naia_payload_sender: naia_payload_tx,
            },
            laminar_config
        ));

        let receiver_task = self.task_pool.spawn(async move {
            loop {
                // when naia-socket receives a packet, deliver it to laminar channel
                match server_socket.receive().await {
                    Ok(naia_packet) => {
                        let address = naia_packet.address();
                        if let Err(error) = naia_packet_tx.send(naia_packet) {
                            log::error!("Error processing naia packet from {} - {}", address, error)
                        }
                    }
                    Err(error) => {
                        log::error!("Server Receive Error: {}", error);
                    }
                }
            }
        });

        let sender_task = self.task_pool.spawn(async move {
            loop {
                // when laminars publishes a packet to send, give it to naia-socket to send
                match naia_payload_rx.recv() {
                    Ok(naia_packet) => {
                        if let Err(error) = naia_sender.send(naia_packet).await {
                            log::error!("Error sending payload to naia_sender {}", error);
                        }
                    },
                    Err(CrossbeamRecvError) => {
                        log::error!("Crossbeam RecvError in sender_task! Oh dear");
                        assert!(false, "Crossbeam RecvError in sender_task!");
                    }
                }
            }
        });

        self.listeners.push(ServerListener {
            socket_address,
            tasks: vec![ receiver_task, sender_task ],
        });
    }
}

fn laminar_poller(
    mut net: ResMut<NetworkResource>,
    mut peer_events: EventWriter<PeerEvent>,
){
    if !net.initialized() {
        return;
    }

    net.poll();

    let event_receiver = net.event_receiver().clone();

    // publish to bevy events - we won't expose the event_receiver
    // also adding in a bit of connection tracking.
    while let Ok(event) = event_receiver.try_recv() {
        match event {
            LaminarSocketEvent::Connect(addr) => {
                if let Some(existing_peer) = net.peers.get_mut(&addr) {
                    assert_eq!(existing_peer.state(), ConnectionState::Connecting);
                    // log::warn!("Connect event for existing peer on {}, in state: {:?} (setting to connected)", addr, existing_peer.state());
                    let new_state = ConnectionState::Connected;
                    if existing_peer.set_state(new_state) {
                        peer_events.send(PeerEvent::Status(addr, new_state));
                    }
                } else {
                    log::warn!("Laminar connect event but no known peer {}", addr);
                }
            },
            LaminarSocketEvent::Disconnect(addr) => {
                if let Some(mut existing_peer) = net.peers.remove(&addr) {
                    let state = ConnectionState::Disconnected;
                    if existing_peer.set_state(state) {
                        peer_events.send(PeerEvent::Status(addr, existing_peer.state()));
                    }
                } else {
                    log::warn!("Got laminar disconnected event for unknown peer {}", addr);
                }
            },
            LaminarSocketEvent::Timeout(addr) => {
                // laminar will send disconnect right after timeout, so no removal here
                if let Some(existing_peer) = net.peers.get_mut(&addr) {
                    let state = ConnectionState::Timeout;
                    if existing_peer.set_state(state) {
                        peer_events.send(PeerEvent::Status(addr, existing_peer.state()));
                    }
                } else {
                    log::warn!("Got laminar timeout event for unknown peer {}", addr);
                }
            },
            LaminarSocketEvent::Packet(packet) => {
                // log::info!(">>packet, str: '{}'", String::from_utf8_lossy(packet.payload()));
                // NB: peer_handle() is added by our trait, just returns the socket addr
                if let Some(existing_peer) = net.peers.get_mut(&packet.peer_handle()) {
                    // if we are getting a packet from a peer still in Connecting state, we need to send them a packet
                    // in order to put the connection into a connected state.
                    if existing_peer.state() == ConnectionState::Connecting && packet.payload().len() == 0 {
                        // still sending empty welcome/handshake packets
                        continue;
                    }
                    assert_eq!(existing_peer.state(), ConnectionState::Connected);
                    peer_events.send(PeerEvent::Packet(packet));
                } else {
                    // got a packet from an unknown peer, must be a new connection.
                    let pc = net.new_peer(packet.addr());
                    let initial_state = pc.state();
                    net.peers.insert(packet.addr(), pc);
                    // send a welcome packet.
                    let welcome_packet = LaminarPacket::reliable_unordered(packet.addr(), vec![]);
                    log::info!("New peer detected! Welcoming {}", packet.addr());
                    net.send(welcome_packet).unwrap_or_default();
                    peer_events.send(PeerEvent::Status(packet.peer_handle(), initial_state));
                    // welcome packets are 0 len
                    assert_eq!(packet.payload().len(), 0);
                    // dont publish welcome packets
                    // packet_events.send(packet);
                }
                
            },
        }
   }
}
