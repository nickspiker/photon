use super::protocol::PeerRecord;
use crate::types::DevicePubkey;

use crate::PEER_EXPIRY_OSC;

/// In-memory peer storage for FGTW Stores PeerRecords in a sorted Vec (by handle_proof) for O(log n) lookup Multiple devices per handle are supported (consecutive records with same handle_proof)
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

    /// Add or update a peer record If device already exists for this handle, update it Otherwise insert at sorted position
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

    /// Merge a peer record learned from GOSSIP (a phonebook exchange with another peer), keeping the record with the newer `last_seen` (Eagle-time oscillations). Unlike [`add_peer`], which is the FGTW-authoritative path and blindly overwrites, gossip can deliver an OLDER copy of a device (the peer we asked hadn't heard from that device as recently as we have), and that must not clobber our fresher record — otherwise gossiping with a stale peer would rot the phonebook.
    /// Returns `true` if the store actually changed (newer record adopted, or a brand-new device inserted), so the caller can persist + log only on real updates.
    ///
    /// This is the convergence rule of the peers-are-FGTW mesh: every device's freshest-known address wins, so asking everyone and merging by Eagle-time pulls the whole network toward agreement.
    pub fn merge_peer(&mut self, peer: PeerRecord) -> bool {
        // Gossip records are only trusted if they self-verify — a relay can carry a device's entry but can't forge or redirect it.
        // An unsigned / forged record is dropped here, so nothing untrusted ever enters the store via the mesh. (The FGTW-authoritative path uses add_peer, not this.)
        if !peer.verify() {
            return false;
        }
        let pos = self.find_position(&peer.handle_proof);

        let mut i = pos;
        while i < self.peers.len() && self.peers[i].handle_proof == peer.handle_proof {
            if self.peers[i].device_pubkey.as_bytes() == peer.device_pubkey.as_bytes() {
                // Same device already known — adopt the incoming copy ONLY if it's strictly newer.
                if peer.last_seen > self.peers[i].last_seen {
                    self.peers[i] = peer;
                    return true;
                }
                return false;
            }
            i += 1;
        }

        // Device we'd never heard of — insert it (gossip discovered a new device for this handle).
        self.peers.insert(pos, peer);
        debug_assert!(
            self.peers
                .windows(2)
                .all(|w| w[0].handle_proof <= w[1].handle_proof),
            "PeerStore sort order violated after merge"
        );
        true
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::DevicePubkey;
    use std::net::SocketAddr;

    // A properly SELF-SIGNED record: the device key is derived from `device`, its verifying key IS the device_pubkey, and we sign after setting last_seen so the signature covers it.
    // merge_peer rejects anything that doesn't verify, so test records must be signed like the real ones.
    fn rec(handle: u8, device: u8, last_seen: i64) -> PeerRecord {
        use ed25519_dalek::SigningKey;
        let addr: SocketAddr = "127.0.0.1:4383".parse().unwrap();
        let sk = SigningKey::from_bytes(&[device; 32]);
        let pubkey = DevicePubkey::from_bytes(sk.verifying_key().to_bytes());
        let mut r = PeerRecord::new([handle; 32], pubkey, addr);
        r.last_seen = last_seen;
        r.sign(&sk);
        r
    }

    #[test]
    fn peer_record_self_signs_and_verifies() {
        use ed25519_dalek::SigningKey;
        let addr: SocketAddr = "203.0.113.7:4383".parse().unwrap();
        // device_pubkey MUST be the verifying half of the signing key (a device signs only its own).
        let sk = SigningKey::from_bytes(&[7u8; 32]);
        let device = DevicePubkey::from_bytes(sk.verifying_key().to_bytes());

        let mut r = PeerRecord::new([1u8; 32], device, addr);
        assert!(!r.verify(), "unsigned record must not verify");
        r.sign(&sk);
        assert!(r.verify(), "self-signed record verifies against its own device_pubkey");

        // Tampering with any signed field breaks the signature (the whole point — a relay can't redirect the address).
        let mut tampered = r.clone();
        tampered.ip = "198.51.100.9:4383".parse().unwrap();
        assert!(!tampered.verify(), "address tamper invalidates the signature");

        let mut tampered2 = r.clone();
        tampered2.last_seen += 1;
        assert!(!tampered2.verify(), "last_seen tamper invalidates the signature");

        // A record signed by a DIFFERENT key but claiming our device_pubkey fails (forgery guard).
        let attacker = SigningKey::from_bytes(&[9u8; 32]);
        let mut forged = PeerRecord::new([1u8; 32], r.device_pubkey.clone(), addr);
        forged.sign(&attacker); // attacker signs, but device_pubkey is the victim's
        assert!(!forged.verify(), "signature by a non-matching key must not verify");
    }

    #[test]
    fn merge_peer_keeps_newer_by_eagle_time() {
        let mut store = PeerStore::new();

        // First sighting of handle 1 / device 1 at t=100.
        assert!(store.merge_peer(rec(1, 1, 100)));
        assert_eq!(store.peers.len(), 1);

        // A NEWER copy of the same device (t=200) wins and is adopted.
        assert!(store.merge_peer(rec(1, 1, 200)));
        assert_eq!(store.peers[0].last_seen, 200);
        assert_eq!(store.peers.len(), 1, "same device, not duplicated");

        // An OLDER copy (t=150 < 200) is rejected — gossip with a stale peer must not rot the store.
        assert!(!store.merge_peer(rec(1, 1, 150)));
        assert_eq!(store.peers[0].last_seen, 200);

        // An EQUAL copy (t=200) is also a no-op (strictly-newer only).
        assert!(!store.merge_peer(rec(1, 1, 200)));

        // A different DEVICE for the same handle inserts (multi-device support).
        assert!(store.merge_peer(rec(1, 2, 50)));
        assert_eq!(store.peers.len(), 2);

        // A different HANDLE inserts and stays sorted.
        assert!(store.merge_peer(rec(0, 9, 10)));
        assert_eq!(store.peers.len(), 3);
        assert!(store
            .peers
            .windows(2)
            .all(|w| w[0].handle_proof <= w[1].handle_proof));
    }
}
