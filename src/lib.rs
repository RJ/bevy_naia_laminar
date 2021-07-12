use std::net::SocketAddr;

#[cfg(not(target_arch = "wasm32"))]
pub mod server;

pub mod client;

// for our connection tracking. we are hiding laminars connection events and exposing our
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum DisconnectReason {
    Timedout,
    // another peer connection from same src addr replaced us
    // Replaced,
}
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum ConnectionState {
    Uninitialized,
    Connecting,
    Connected,
    Timeout,
    Disconnected,
}

// opaque handle to this peer that our api exposes, but it's just the socketaddr for now.
pub type PeerHandle = SocketAddr;


// pub enum PeerEvent {
    // Packet(LaminarPacket),
// }


// pub struct PeerEventsss {
//     handle: PeerHandle,
//     state: ConnectionState,
// }

#[derive(Debug)]
pub enum PeerEvent {
    Status(PeerHandle, ConnectionState),
    Packet(laminar::Packet),
}

pub mod prelude {
    pub use super::{DisconnectReason, ConnectionState, PeerHandle, PeerEvent};

    pub trait GetPeerHandleFromLaminarPacket {
        fn peer_handle(&self) -> PeerHandle;
    }
    
    impl GetPeerHandleFromLaminarPacket for laminar::Packet {
        fn peer_handle(&self) -> PeerHandle {
            self.addr()
        }
    }
}