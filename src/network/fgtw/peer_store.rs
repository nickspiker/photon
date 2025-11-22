use super::protocol::PeerRecord;
use crate::types::PublicIdentity;
use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

const PEER_EXPIRY_SECONDS: f64 = 604800.0; // 7 days

/// In-memory peer storage for FGTW
/// Stores PeerRecords keyed by handle_proof
/// Multiple devices per handle are supported (Vec<PeerRecord>)
pub struct PeerStore {
    /// Map: handle_proof -> list of devices for that handle
    peers: HashMap<[u8; 32], Vec<PeerRecord>>,
}

impl PeerStore {
    pub fn new() -> Self {
        Self {
            peers: HashMap::new(),
        }
    }

    /// Add or update a peer record
    /// If device already exists for this handle, update it
    /// Otherwise add new device to the handle's device list
    pub fn add_peer(&mut self, peer: PeerRecord) {
        let devices = self.peers.entry(peer.handle_proof).or_insert_with(Vec::new);

        // Check if this device already exists
        if let Some(existing) = devices
            .iter_mut()
            .find(|p| p.device_pubkey.as_bytes() == peer.device_pubkey.as_bytes())
        {
            // Update existing device
            *existing = peer;
        } else {
            // Add new device
            devices.push(peer);
        }
    }

    /// Get all devices for a specific handle proof
    pub fn get_devices_for_handle(&self, handle_proof: &[u8; 32]) -> Vec<PeerRecord> {
        let now = current_timestamp();
        self.peers
            .get(handle_proof)
            .map(|devices| {
                devices
                    .iter()
                    .filter(|p| now - p.last_seen < PEER_EXPIRY_SECONDS)
                    .cloned()
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Get all peer records (all handles, all devices)
    pub fn get_all_peers(&self) -> Vec<PeerRecord> {
        let now = current_timestamp();
        self.peers
            .values()
            .flat_map(|devices| devices.iter())
            .filter(|p| now - p.last_seen < PEER_EXPIRY_SECONDS)
            .cloned()
            .collect()
    }

    /// Get total count of active peer records
    pub fn peer_count(&self) -> usize {
        let now = current_timestamp();
        self.peers
            .values()
            .flat_map(|devices| devices.iter())
            .filter(|p| now - p.last_seen < PEER_EXPIRY_SECONDS)
            .count()
    }

    /// Get count of unique handles
    pub fn handle_count(&self) -> usize {
        let now = current_timestamp();
        self.peers
            .iter()
            .filter(|(_, devices)| {
                devices
                    .iter()
                    .any(|p| now - p.last_seen < PEER_EXPIRY_SECONDS)
            })
            .count()
    }

    /// Update last_seen for a specific device
    pub fn update_peer_seen(&mut self, handle_proof: &[u8; 32], device_pubkey: &PublicIdentity) {
        if let Some(devices) = self.peers.get_mut(handle_proof) {
            if let Some(peer) = devices
                .iter_mut()
                .find(|p| p.device_pubkey.as_bytes() == device_pubkey.as_bytes())
            {
                peer.last_seen = current_timestamp();
            }
        }
    }

    /// Remove stale peers (older than PEER_EXPIRY_SECONDS)
    pub fn cleanup_stale(&mut self) -> usize {
        let now = current_timestamp();
        let mut removed_count = 0;

        // Remove stale devices from each handle
        self.peers.retain(|_, devices| {
            let before = devices.len();
            devices.retain(|p| now - p.last_seen < PEER_EXPIRY_SECONDS);
            removed_count += before - devices.len();
            !devices.is_empty() // Remove handle if no devices left
        });

        removed_count
    }
}

impl Default for PeerStore {
    fn default() -> Self {
        Self::new()
    }
}

fn current_timestamp() -> f64 {
    const EAGLE_TO_UNIX_OFFSET: f64 = 14182940.0;
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs_f64()
        + EAGLE_TO_UNIX_OFFSET
}
