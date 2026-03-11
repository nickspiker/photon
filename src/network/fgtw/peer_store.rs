use super::protocol::PeerRecord;
use crate::types::DevicePubkey;

use crate::PEER_EXPIRY_OSC;

/// In-memory peer storage for FGTW
/// Stores PeerRecords in a sorted Vec (by handle_proof) for O(log n) lookup
/// Multiple devices per handle are supported (consecutive records with same handle_proof)
pub struct PeerStore {
    /// Sorted by handle_proof for binary search
    peers: Vec<PeerRecord>,
}

impl PeerStore {
    pub fn new() -> Self {
        Self { peers: Vec::new() }
    }

    /// Binary search for handle_proof, returns index where it would be inserted
    fn find_position(&self, handle_proof: &[u8; 32]) -> usize {
        self.peers
            .binary_search_by(|p| p.handle_proof.cmp(handle_proof))
            .unwrap_or_else(|i| i)
    }

    /// Add or update a peer record
    /// If device already exists for this handle, update it
    /// Otherwise insert at sorted position
    pub fn add_peer(&mut self, peer: PeerRecord) {
        // Find where this handle_proof starts
        let pos = self.find_position(&peer.handle_proof);

        // Check if this device already exists (scan consecutive matching handle_proofs)
        let mut i = pos;
        while i < self.peers.len() && self.peers[i].handle_proof == peer.handle_proof {
            if self.peers[i].device_pubkey.as_bytes() == peer.device_pubkey.as_bytes() {
                // Update existing device
                self.peers[i] = peer;
                return;
            }
            i += 1;
        }

        // Insert new device at sorted position
        self.peers.insert(pos, peer);

        // Debug: verify sort order is maintained
        debug_assert!(
            self.peers
                .windows(2)
                .all(|w| w[0].handle_proof <= w[1].handle_proof),
            "PeerStore sort order violated after insert"
        );
    }

    /// Get all devices for a specific handle proof (binary search + scan)
    pub fn get_devices_for_handle(&self, handle_proof: &[u8; 32]) -> Vec<PeerRecord> {
        let now = vsf::eagle_time_oscillations();
        let pos = self.find_position(handle_proof);

        let mut result = Vec::new();
        let mut i = pos;
        while i < self.peers.len() && self.peers[i].handle_proof == *handle_proof {
            if now.saturating_sub(self.peers[i].last_seen) < PEER_EXPIRY_OSC {
                result.push(self.peers[i].clone());
            }
            i += 1;
        }
        result
    }

    /// Get all peer records (all handles, all devices)
    pub fn get_all_peers(&self) -> Vec<PeerRecord> {
        let now = vsf::eagle_time_oscillations();
        self.peers
            .iter()
            .filter(|p| now.saturating_sub(p.last_seen) < PEER_EXPIRY_OSC)
            .cloned()
            .collect()
    }

    /// Get total count of active peer records
    pub fn peer_count(&self) -> usize {
        let now = vsf::eagle_time_oscillations();
        self.peers
            .iter()
            .filter(|p| now.saturating_sub(p.last_seen) < PEER_EXPIRY_OSC)
            .count()
    }

    /// Get count of unique handles
    pub fn handle_count(&self) -> usize {
        let now = vsf::eagle_time_oscillations();
        let mut count = 0;
        let mut prev_handle: Option<[u8; 32]> = None;

        for p in &self.peers {
            if now.saturating_sub(p.last_seen) < PEER_EXPIRY_OSC {
                if prev_handle.map_or(true, |h| h != p.handle_proof) {
                    count += 1;
                    prev_handle = Some(p.handle_proof);
                }
            }
        }
        count
    }

    /// Update last_seen for a specific device
    pub fn update_peer_seen(&mut self, handle_proof: &[u8; 32], device_pubkey: &DevicePubkey) {
        let pos = self.find_position(handle_proof);
        let mut i = pos;
        while i < self.peers.len() && self.peers[i].handle_proof == *handle_proof {
            if self.peers[i].device_pubkey.as_bytes() == device_pubkey.as_bytes() {
                self.peers[i].last_seen = vsf::eagle_time_oscillations();
                return;
            }
            i += 1;
        }
    }

    /// Remove stale peers (older than 7 days)
    pub fn cleanup_stale(&mut self) -> usize {
        let now = vsf::eagle_time_oscillations();
        let before = self.peers.len();
        self.peers
            .retain(|p| now.saturating_sub(p.last_seen) < PEER_EXPIRY_OSC);
        before - self.peers.len()
    }
}

impl Default for PeerStore {
    fn default() -> Self {
        Self::new()
    }
}
