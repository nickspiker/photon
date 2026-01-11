use super::DevicePubkey;
use std::net::SocketAddr;

#[derive(Clone, Debug)]
pub struct Peer {
    pub public_identity: DevicePubkey,
    pub address: SocketAddr,
    pub last_seen: f64,
    pub connection_state: ConnectionState,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConnectionState {
    Disconnected,
    Connecting,
    Connected,
    Authenticated,
}

#[derive(Clone, Debug)]
pub struct DhtAnnouncement {
    pub public_key: [u8; 32],
    pub port: u16,
    pub timestamp: f64,
    pub signature: Vec<u8>, // 64 bytes
}

impl Peer {
    pub fn new(public_identity: DevicePubkey, address: SocketAddr) -> Self {
        Self {
            public_identity,
            address,
            last_seen: vsf::eagle_time_nanos(),
            connection_state: ConnectionState::Disconnected,
        }
    }

    pub fn update_connection_state(&mut self, state: ConnectionState) {
        self.connection_state = state;
        if state == ConnectionState::Connected || state == ConnectionState::Authenticated {
            self.last_seen = vsf::eagle_time_nanos();
        }
    }

    pub fn is_online(&self) -> bool {
        matches!(
            self.connection_state,
            ConnectionState::Connected | ConnectionState::Authenticated
        )
    }
}
