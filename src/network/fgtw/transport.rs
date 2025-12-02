use super::PeerStore;
use crate::types::DevicePubkey;
use std::sync::{Arc, Mutex};

/// FGTW peer store wrapper
/// Actual FGTW communication happens via HTTPS in bootstrap.rs
pub struct FgtwTransport {
    peer_store: Arc<Mutex<PeerStore>>,
}

impl FgtwTransport {
    pub fn new(_our_pubkey: DevicePubkey, _port: u16) -> Self {
        Self {
            peer_store: Arc::new(Mutex::new(PeerStore::new())),
        }
    }

    pub fn peer_store(&self) -> Arc<Mutex<PeerStore>> {
        self.peer_store.clone()
    }
}
