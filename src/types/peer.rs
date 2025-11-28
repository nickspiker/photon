use super::PublicIdentity;
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use std::time::{SystemTime, UNIX_EPOCH};

fn eagle_time_secs() -> u64 {
    const EAGLE_TO_UNIX_OFFSET: u64 = 14182940;
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
        + EAGLE_TO_UNIX_OFFSET
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Peer {
    pub public_identity: PublicIdentity,
    pub address: SocketAddr,
    pub last_seen: u64,
    pub connection_state: ConnectionState,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ConnectionState {
    Disconnected,
    Connecting,
    Connected,
    Authenticated,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DhtAnnouncement {
    pub public_key: [u8; 32],
    pub port: u16,
    pub timestamp: u64,
    pub signature: Vec<u8>, // 64 bytes
}

impl Peer {
    pub fn new(public_identity: PublicIdentity, address: SocketAddr) -> Self {
        Self {
            public_identity,
            address,
            last_seen: eagle_time_secs(),
            connection_state: ConnectionState::Disconnected,
        }
    }

    pub fn update_connection_state(&mut self, state: ConnectionState) {
        self.connection_state = state;
        if state == ConnectionState::Connected || state == ConnectionState::Authenticated {
            self.last_seen = eagle_time_secs();
        }
    }

    pub fn is_online(&self) -> bool {
        matches!(
            self.connection_state,
            ConnectionState::Connected | ConnectionState::Authenticated
        )
    }
}
