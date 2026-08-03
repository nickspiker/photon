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

    /// Serialise the store for the vault by reusing the GOSSIP MESSAGE codec verbatim — a persisted phonebook is literally a `PhonebookResponse` addressed to ourselves.
    ///
    /// One codec, not two. The 7-value positional peer row carries tensors (the v4/v6 IP octets), and the schema-builder codec cannot round-trip that shape — a hand-rolled second encoder here failed on exactly that, and would have been a second thing to keep in step with the wire forever. `FgtwMessage` already encodes a signed peer list into a complete VSF document and parses it back, so disk and mesh agree by construction rather than by discipline. It also adds no raw-parse site (scripts/lib/vsf-gate.sh).
    ///
    /// Unsigned rows are DROPPED. Everything from FGTW arrives unsigned (`serialize_peer_list` emits no signature field), and `merge_peer` refuses anything failing `verify()` — so an unsigned row can never be gossiped or trusted by a peer, and persisting it would only grow the file.
    pub fn to_vsf_bytes(&self, device_key: &super::Keypair) -> Result<Vec<u8>, String> {
        use super::protocol::FgtwMessage;
        let signed: Vec<PeerRecord> = self.peers.iter().filter(|p| p.verify()).cloned().collect();

        // Sign our own snapshot, the same way a gossip responder signs the list it serves.
        let provenance_hash = *blake3::hash(b"PHOTON_PEERSTORE_SNAPSHOT_v1").as_bytes();
        let sig = device_key.sign(&provenance_hash);
        let mut signature = [0u8; 64];
        signature.copy_from_slice(&sig.to_bytes());

        let msg = FgtwMessage::PhonebookResponse {
            timestamp: vsf::eagle_time_oscillations(),
            responder_pubkey: DevicePubkey::from_bytes(*device_key.public.as_bytes()),
            provenance_hash,
            signature,
            peers: signed,
        };
        let bytes = msg.to_vsf_bytes();
        if bytes.is_empty() {
            return Err("phonebook encode produced no bytes".into());
        }
        Ok(bytes)
    }

    /// Read a persisted phonebook back. Each row must still self-verify — a vault a disk error touched cannot inject a peer, and a row that no longer verifies is not a peer.
    pub fn from_vsf_bytes(bytes: &[u8]) -> Result<Self, String> {
        use super::protocol::FgtwMessage;
        let peers = match FgtwMessage::from_vsf_bytes(bytes)? {
            FgtwMessage::PhonebookResponse { peers, .. } => peers,
            _ => return Err("persisted phonebook is not a peer list".into()),
        };
        let mut store = PeerStore::new();
        for rec in peers {
            // Same gate as the mesh.
            if rec.verify() {
                store.add_peer(rec);
            }
        }
        Ok(store)
    }

    /// Binary search for handle_proof, returns index where it would be inserted
    fn find_position(&self, handle_proof: &[u8; 32]) -> usize {
        self.peers
            .binary_search_by(|p| p.handle_proof.cmp(handle_proof))
            .unwrap_or_else(|i| i)
    }

    /// Add or update a peer record If device already exists for this handle, update it Otherwise insert at sorted position
    pub fn add_peer(&mut self, peer: PeerRecord) {
        // Refuse a record whose address is unspecified, at EVERY ingest (gossip, fetch, load): the signing-point guard only protects records made by current builds, and a record signed before it existed — or by a retired device that will never republish — otherwise circulates its 0.0.0.0 claim forever.
        if crate::network::traverse::gather::is_bogus_addr(&peer.ip) {
            return;
        }
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
        self.merge_peer_bound(peer, None)
    }

    /// Merge a gossiped record, optionally gated on a KNOWN fleet for its `handle_proof`.
    ///
    /// The self-signature proves the holder of a device key CLAIMS an address under some `handle_proof`. It does not prove the device belongs to that identity's fleet — nothing in the record binds the two, so a stranger's device can mint a validly-signed row for any `handle_proof` it scraped off the open phonebook. On the FGTW path the SERVER closes this (`handle_announce` refuses a device the fleet chain doesn't fold as a member); the mesh has no server, and that is the load-bearing gap in Phase A of docs/peers-are-fgtw.md.
    ///
    /// `known_fleet` is the caller's ground truth — the folded member set for that identity, which photon already holds for every contact. When supplied, a device outside it is refused: gossip about someone you know is checked against their chain.
    ///
    /// `None` means the caller has no chain for this identity, which is the normal case for the OPEN phonebook — enumerating strangers is the point. Those rows are accepted as unverified DIRECTORY data: routable, never trusted. They cannot escalate, because reaching a device still requires passing the contact gate (ping/pong and every CLUTCH handler drop non-contacts) and completing CLUTCH, which needs the handle out-of-band.
    ///
    /// The durable fix is the phonebook's primary registry (`fgtw::phonebook`), where a current member's signed placement binds device to identity without any chain fetch. Until that is deployed, this is the strongest check available from local state.
    pub fn merge_peer_bound(&mut self, peer: PeerRecord, known_fleet: Option<&[[u8; 32]]>) -> bool {
        // Gossip records are only trusted if they self-verify — a relay can carry a device's entry but can't forge or redirect it.
        // An unsigned / forged record is dropped here, so nothing untrusted ever enters the store via the mesh. (The FGTW-authoritative path uses add_peer, not this.)
        if !peer.verify() {
            return false;
        }
        // Where we DO hold the identity's chain, a device it doesn't fold as a member is refused — the binding the record itself cannot carry.
        if let Some(fleet) = known_fleet {
            if !fleet.iter().any(|m| m == peer.device_pubkey.as_bytes()) {
                return false;
            }
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

    /// Distinct fresh identities EXCLUDING `own` — the peers-as-PEOPLE count for the title bar. Our own fleet siblings ride this same store (that's how they get addresses for direct routing), but we are not our own peer: without the exclusion, a 2-device fleet reads "1 peer" to itself.
    pub fn handle_count_excluding(&self, own: &[u8; 32]) -> usize {
        let now = vsf::eagle_time_oscillations();
        let mut count = 0;
        let mut prev_handle: Option<[u8; 32]> = None;

        for p in &self.peers {
            if p.handle_proof != *own && now.saturating_sub(p.last_seen) < PEER_EXPIRY_OSC {
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

    // A properly SELF-SIGNED record: the device key is derived from `device`, its verifying key IS the device_pubkey, and we sign after setting last_seen so the signature covers it. merge_peer rejects anything that doesn't verify, so test records must be signed like the real ones.
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
        assert!(
            r.verify(),
            "self-signed record verifies against its own device_pubkey"
        );

        // Tampering with any signed field breaks the signature (the whole point — a relay can't redirect the address).
        let mut tampered = r.clone();
        tampered.ip = "198.51.100.9:4383".parse().unwrap();
        assert!(
            !tampered.verify(),
            "address tamper invalidates the signature"
        );

        let mut tampered2 = r.clone();
        tampered2.last_seen += 1;
        assert!(
            !tampered2.verify(),
            "last_seen tamper invalidates the signature"
        );

        // A record signed by a DIFFERENT key but claiming our device_pubkey fails (forgery guard).
        let attacker = SigningKey::from_bytes(&[9u8; 32]);
        let mut forged = PeerRecord::new([1u8; 32], r.device_pubkey.clone(), addr);
        forged.sign(&attacker); // attacker signs, but device_pubkey is the victim's
        assert!(
            !forged.verify(),
            "signature by a non-matching key must not verify"
        );
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

    /// `rec` with a CURRENT last_seen. Both `peer_count` and `get_all_peers` filter on the 7-day expiry window, so a synthetic `last_seen` of 100 is eagle-time ~1969 and reads as decades stale — invisible to every reader even though it is stored.
    fn fresh(handle: u8, device: u8, offset: i64) -> PeerRecord {
        rec(handle, device, vsf::eagle_time_oscillations() + offset)
    }

    /// The gate that closes the Phase A binding gap where we HAVE ground truth. A self-signature proves a device claims an address under some handle_proof; it does not prove the device is in that identity's fleet. On the FGTW path the server refuses a non-member; the mesh has no server, so a stranger can mint a valid row for any scraped handle_proof.
    #[test]
    fn merge_gated_on_a_known_fleet_refuses_a_non_member() {
        let mut store = PeerStore::new();
        let r = fresh(1, 1, 0);
        let member = *r.device_pubkey.as_bytes();

        // The device IS in the fleet we know → accepted.
        assert!(
            store.merge_peer_bound(r.clone(), Some(&[member])),
            "a folded member merges"
        );

        // A different device claiming the SAME handle_proof, correctly self-signed, but absent from the fleet → refused. This is the impersonation the record itself cannot rule out.
        let intruder = fresh(1, 99, 1);
        assert!(
            intruder.verify(),
            "the intruder's own signature is genuinely valid"
        );
        assert!(
            !store.merge_peer_bound(intruder, Some(&[member])),
            "a validly-signed non-member must not enter the store"
        );
    }

    /// `None` is the OPEN-phonebook case: we hold no chain for this identity, which is normal when enumerating strangers. Those rows are directory data — routable, never trusted — and cannot escalate, because reaching a device still requires the contact gate and CLUTCH.
    #[test]
    fn merge_without_a_known_fleet_accepts_as_directory_data() {
        let mut store = PeerStore::new();
        assert!(
            store.merge_peer_bound(fresh(7, 7, 0), None),
            "unknown identity → directory data"
        );
        // get_all_peers, not peer_count: the latter filters on the 7-day expiry window, and these fixtures carry a synthetic last_seen. The assertion here is "is it stored".
        assert_eq!(store.get_all_peers().len(), 1);
    }

    /// Persistence uses the SAME codec as the gossip wire, so a row off disk is byte-identical to one off the mesh. Unsigned rows are dropped on the way out: they can never be gossiped (merge rejects them) so persisting them would only grow the file.
    #[test]
    fn persisted_phonebook_round_trips_signed_rows_only() {
        let mut store = PeerStore::new();
        store.merge_peer(fresh(1, 1, 0));
        store.merge_peer(fresh(2, 2, 1));
        let mut unsigned = fresh(3, 3, 2);
        unsigned.signature = [0u8; 64];
        store.add_peer(unsigned); // authoritative path takes it; persistence must not
        assert_eq!(store.get_all_peers().len(), 3);

        let kp = super::super::Keypair::from_seed(&[9u8; 32]);
        let bytes = store.to_vsf_bytes(&kp).expect("encode");
        assert!(
            bytes.starts_with(b"R\xc3\x85<"),
            "a complete VSF file, not a bare section"
        );

        let back = PeerStore::from_vsf_bytes(&bytes).expect("decode");
        assert_eq!(
            back.get_all_peers().len(),
            2,
            "the unsigned row must not persist"
        );
        for r in back.get_all_peers() {
            assert!(r.verify(), "every persisted row self-verifies");
        }
    }

    /// A vault a disk error touched must not inject peers — the verified read refuses it rather than silently returning a short list.
    #[test]
    fn corrupt_persisted_phonebook_is_refused() {
        let mut store = PeerStore::new();
        store.merge_peer(fresh(1, 1, 0));
        let kp = super::super::Keypair::from_seed(&[9u8; 32]);
        let mut bytes = store.to_vsf_bytes(&kp).expect("encode");
        let n = bytes.len();
        bytes[n / 2] ^= 0xFF;
        assert!(
            PeerStore::from_vsf_bytes(&bytes).is_err(),
            "tampered vault entry must not load"
        );
        assert!(PeerStore::from_vsf_bytes(b"garbage").is_err());
    }

    #[test]
    fn handle_count_excluding_subtracts_self_and_dedups_devices() {
        let now = vsf::eagle_time_oscillations();
        let mut store = PeerStore::new();
        // Our own fleet: two sibling devices under OUR handle (handle 1).
        store.add_peer(rec(1, 1, now));
        store.add_peer(rec(1, 2, now));
        // A friend with two devices (handle 2) and a single-device friend (handle 3).
        store.add_peer(rec(2, 3, now));
        store.add_peer(rec(2, 4, now));
        store.add_peer(rec(3, 5, now));
        // Peers are PEOPLE: multi-device friends dedup to one, and we are not our own peer.
        assert_eq!(store.handle_count(), 3, "three identities in the store");
        assert_eq!(
            store.handle_count_excluding(&[1u8; 32]),
            2,
            "excluding ours leaves the two friends"
        );
        // Excluding a handle that isn't in the store changes nothing.
        assert_eq!(store.handle_count_excluding(&[9u8; 32]), 3);
        // A stale sibling record still doesn't resurrect us, and stale friends age out of the count.
        let mut stale = PeerStore::new();
        stale.add_peer(rec(1, 1, now));
        stale.add_peer(rec(2, 3, now - crate::PEER_EXPIRY_OSC - 1));
        assert_eq!(
            stale.handle_count_excluding(&[1u8; 32]),
            0,
            "only a stale friend and ourselves → zero peers"
        );
    }
}
