use super::{CeremonyId, DevicePubkey, FriendshipId, Seed};
use crate::crypto::clutch::{
    ClutchAllKeypairs, ClutchKemResponsePayload, ClutchKemSharedSecrets, ClutchOfferPayload,
};
use std::net::{Ipv4Addr, SocketAddr};

/// A slot in the CLUTCH ceremony, indexed by sorted handle_hash position. Same indexing on all devices in the ceremony (N-party ready).
#[derive(Clone, Debug)]
pub struct PartySlot {
    /// Handle hash identifying this party (sorted position determines slot index)
    pub handle_hash: [u8; 32],
    /// 8 public keys from ClutchOffer for this slot's party
    pub offer: Option<ClutchOfferPayload>,
    /// Secrets from encapsulation FROM this slot's party (they encapsulated to local, local decapsulated)
    pub kem_secrets_from_them: Option<ClutchKemSharedSecrets>,
    /// Secrets from encapsulation TO this slot's party (local encapsulated to them)
    pub kem_secrets_to_them: Option<ClutchKemSharedSecrets>,
    /// KEM response payload for re-send if peer missed it
    pub kem_response_for_resend: Option<ClutchKemResponsePayload>,
}

impl PartySlot {
    /// Create empty slot for a party
    pub fn new(handle_hash: [u8; 32]) -> Self {
        Self {
            handle_hash,
            offer: None,
            kem_secrets_from_them: None,
            kem_secrets_to_them: None,
            kem_response_for_resend: None,
        }
    }

    /// Check if this slot has all required data for ceremony completion. Each slot needs offer + ONE KEM contribution (either direction):
    /// - Local slot: kem_secrets_to_them (local encapsulated)
    /// - Remote slots: kem_secrets_from_them (remote encapsulated)
    pub fn is_complete(&self) -> bool {
        self.offer.is_some()
            && (self.kem_secrets_from_them.is_some() || self.kem_secrets_to_them.is_some())
    }
}

/// A chat message in a conversation (UI-level representation)
#[derive(Clone, Debug)]
pub struct ChatMessage {
    pub content: String,
    pub timestamp: i64, // Eagle time oscillations (i64, ~1.42 GHz since Apollo 11 landing)
    pub is_outgoing: bool, // true = we sent it, false = they sent it
    pub delivered: bool, // true = confirmed delivered to recipient
    /// For RECEIVED messages: the ACK plaintext_hash (blake3 of the full decrypted payload) we sent back when we first processed this message. Stored so a duplicate retransmit (our ACK was lost) can be re-ACKed with the SAME hash instead of being silently dropped — the sender's chain only advances on a matching ACK, so a lost ACK would otherwise stall it forever. `None` for outgoing messages and for received messages stored before this field existed.
    pub ack_hash: Option<[u8; 32]>,
}

impl ChatMessage {
    pub fn new(content: String, is_outgoing: bool) -> Self {
        Self {
            content,
            timestamp: vsf::eagle_time_oscillations(),
            is_outgoing,
            delivered: false,
            ack_hash: None,
        }
    }

    /// Create a message with a specific timestamp (for received messages with known eagle_time)
    pub fn new_with_timestamp(content: String, is_outgoing: bool, timestamp: i64) -> Self {
        Self {
            content,
            timestamp,
            is_outgoing,
            delivered: false,
            ack_hash: None,
        }
    }

    /// Builder: attach the ACK hash (the plaintext_hash we ACK this message with). Used on the receive path so a later duplicate can be re-ACKed from storage.
    pub fn with_ack_hash(mut self, ack_hash: [u8; 32]) -> Self {
        self.ack_hash = Some(ack_hash);
        self
    }
}

/// A handle name stored as VSF text (normalized Unicode, unambiguous) Wrapper around String that represents a VSF x-type text value
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HandleText(String);

impl HandleText {
    pub fn new(s: &str) -> Self {
        HandleText(s.to_string())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<&str> for HandleText {
    fn from(s: &str) -> Self {
        HandleText::new(s)
    }
}

impl std::fmt::Display for HandleText {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Reserved sentinel content for the hidden chain-weave probe message. After CLUTCH reaches Complete, each device sends exactly one message with this exact content to validate the ratchet end-to-end. The receive path recognises it, advances/ACKs the chain like any message, but suppresses the chat bubble. The control bytes (SOH/STX around the tag) make a collision with a real user message effectively impossible.
pub const CHAIN_PROBE_MARKER: &str = "\u{1}\u{2}photon-chain-probe\u{2}\u{1}";

/// State of the CLUTCH key ceremony for a contact
///
/// Slot-based design: each party has a slot indexed by sorted handle_hash position. Ceremony completes when all slots have both offer and kem_secrets filled, AND both parties have exchanged matching eggs_proof values.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ClutchState {
    #[default]
    Pending, // Slots not all filled yet
    AwaitingProof, // Eggs computed, sent our proof, waiting for peer's proof
    Complete,      // Proofs exchanged and verified
}

#[derive(Clone, Debug)]
pub struct Contact {
    pub id: ContactId,
    pub handle: HandleText, // VSF-style text for unambiguous handle storage
    pub handle_proof: [u8; 32], // Cached handle_proof (expensive to compute - ~1 second) - PUBLIC
    pub handle_hash: [u8; 32], // BLAKE3(handle) - PRIVATE, used for seed derivation
    pub public_identity: DevicePubkey,
    /// The friend's CURRENT fleet device set, folded from their public membership chain (`fleet::current_members` by `handle_proof`). A contact is an IDENTITY, not a device: pings/pongs/offers/messages are honoured from ANY member here, not just `public_identity` (which is only the device we happened to meet first). Empty until the first refresh; refreshed on load and on a `fleet` bump for this contact. Runtime cache — re-fetched from the network, never authoritative on disk.
    pub fleet_members: Vec<[u8; 32]>,
    pub ip: Option<SocketAddr>, // Last known IP:port from FGTW or direct (public IP)
    pub local_ip: Option<Ipv4Addr>, // LAN IP discovered via broadcast (for hairpin NAT workaround)
    pub local_port: Option<u16>, // LAN port discovered via broadcast
    pub relationship_seed: Option<Seed>,
    pub friendship_id: Option<FriendshipId>, // Links to friendship storage (chains live there)
    pub clutch_state: ClutchState,

    // Slot-based CLUTCH state (N-party support)
    /// Our 8 ephemeral keypairs (~512KB secret keys) - generated once per ceremony
    pub clutch_our_keypairs: Option<ClutchAllKeypairs>,
    /// Party slots indexed by sorted handle_hash position Each slot contains offer + kem_secrets for one party (including self)
    pub clutch_slots: Vec<PartySlot>,
    /// Cached ceremony_id - computed from handle_hashes + sorted ping provenances. Uses spaghettify for mixing (no memory-hard step needed). Unique per ceremony due to ping timestamp entropy.
    pub ceremony_id: Option<[u8; 32]>,
    /// Pending KEM response received before our keygen completed Stored here and processed when ceremony_id becomes available
    pub clutch_pending_kem: Option<ClutchKemResponsePayload>,
    /// Track if we've sent our offer (to avoid resending)
    pub clutch_offer_sent: bool,
    /// Our computed eggs_proof (stored while awaiting peer's proof for verification)
    pub clutch_our_eggs_proof: Option<[u8; 32]>,
    /// Peer's eggs_proof if received before we computed ours
    pub clutch_their_eggs_proof: Option<[u8; 32]>,
    /// Bounded retransmit budget for our ClutchComplete proof.
    /// The proof is a small single-packet UDP send with no PT retransmit, so one drop would strand the peer in AwaitingProof forever (the asymmetric-completion bug: we go Complete via early-proof, null our state, and never resend).
    /// Set to a small N when we compute our eggs proof; ping_contacts re-sends the proof and decrements each cycle until it hits 0, so both sides converge even on a lossy or just-refreshed path.
    /// Runtime-only — not persisted (a resumed Complete contact needs no further proof sends).
    pub clutch_proof_resends_left: u8,
    /// Flag to prevent multiple concurrent keygens (race condition guard)
    pub clutch_keygen_in_progress: bool,
    /// Flag to prevent multiple concurrent KEM encapsulations
    pub clutch_kem_encap_in_progress: bool,
    /// Flag to prevent multiple concurrent ceremony completions (avalanche_expand)
    pub clutch_ceremony_in_progress: bool,
    /// HQC public key prefix from peer's last completed ceremony. Used for stale detection: if received offer has same HQC prefix, it's a PT retransmission (stale), not a legitimate re-key request. Stored at completion time, cleared when accepting new ceremony.
    pub completed_their_hqc_prefix: Option<[u8; 8]>,
    /// Collected offer provenances for ceremony nonce derivation. Each offer's VSF header has hp = BLAKE3(signer_pubkey || creation_time_nanos). Sorted and combined via spaghettify to derive unique ceremony_id. Cleared when CLUTCH ceremony completes.
    pub offer_provenances: Vec<[u8; 32]>,

    pub trust_level: TrustLevel,
    pub added: i64,
    pub last_seen: Option<i64>,
    pub is_online: bool, // True when we have confirmed bidirectional comms
    pub messages: Vec<ChatMessage>, // Conversation history
    pub message_scroll_offset: f32, // Vertical scroll offset for message area (pixels)
    pub prev_is_online: bool, // For differential rendering (not persisted)
    pub indicator_x: usize, // Cached indicator dot X position (set during draw)
    pub indicator_y: usize, // Cached indicator dot Y position (set during draw)
    pub text_x: f32,     // Cached text X position (set during draw)
    pub text_y: f32,     // Cached text Y position (set during draw)
    // Avatar cache - fetched from FGTW by handle Storage key is deterministic: BLAKE3(BLAKE3(handle) || "avatar")
    pub avatar_pixels: Option<Vec<u8>>, // Full 256x256 VSF RGB pixels (cached)
    pub avatar_scaled: Option<Vec<u8>>, // Pre-scaled to current display size
    pub avatar_scaled_diameter: usize,  // Diameter the scaled pixels were rendered for

    // Chain weave probe — after CLUTCH reaches Complete, both devices auto-exchange one hidden probe chat message each way to prove the ratchet works end-to-end.
    // Once proven, the ceremony proof rebroadcast is cancelled (clutch_proof_resends_left = 0).
    // Runtime-only, not persisted: a resumed Complete contact already has a working chain and needs no re-probe.
    /// The chain has been validated end-to-end (our probe/message got ACKed AND we saw theirs). Gates the status line from "weaving the chain" to "secured" and stops the ceremony rebroadcast.
    pub chain_woven: bool,
    /// We've already sent (or queued) our chain-weave probe for this contact — send it once only.
    pub probe_sent: bool,
    /// We've received the peer's chain-weave probe (their TX chain / our RX proven for at least one hop).
    pub their_probe_seen: bool,
    /// Our own TX chain has advanced via a matching ACK at least once (our TX / their RX proven). Sealed with `their_probe_seen` this is the both-directions-proven condition for `chain_woven`.
    pub chain_advanced_by_ack: bool,
}

/// Contact identifier - BLAKE3 hash of the contact's public identity key This provides deterministic, collision-resistant identification
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ContactId([u8; 32]);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrustLevel {
    Stranger,
    Known,
    Trusted,
    Inner,
}

impl ContactId {
    /// Create ContactId from public identity key (deterministic)
    pub fn from_pubkey(pubkey: &DevicePubkey) -> Self {
        Self(*blake3::hash(pubkey.as_bytes()).as_bytes())
    }

    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

impl Contact {
    pub fn new(handle: HandleText, handle_proof: [u8; 32], public_identity: DevicePubkey) -> Self {
        // Private handle_hash for local seed derivation (NOT the public handle_proof). Delegates to `ihi::handle_to_hash` — VsfType::x pre-hash + BLAKE3, same canonical answer as handle_to_proof's first stage.
        let handle_hash = crate::types::Handle::to_identity_seed(handle.as_str());

        Self {
            id: ContactId::from_pubkey(&public_identity),
            handle,
            handle_proof,
            handle_hash,
            public_identity,
            fleet_members: Vec::new(), // Folded from the friend's chain on the first refresh
            ip: None,
            local_ip: None,   // Discovered via LAN broadcast
            local_port: None, // Discovered via LAN broadcast
            relationship_seed: None,
            friendship_id: None, // Set after CLUTCH ceremony completes
            clutch_state: ClutchState::Pending,
            // Slot-based CLUTCH fields
            clutch_our_keypairs: None,
            clutch_slots: Vec::new(),    // Initialized when ceremony starts
            ceremony_id: None,           // Computed from handle_hashes + ping provenances
            clutch_pending_kem: None,    // KEM response received before keygen completed
            clutch_offer_sent: false,    // Track if we've sent our offer
            clutch_our_eggs_proof: None, // Our proof (stored while awaiting peer's)
            clutch_their_eggs_proof: None, // Peer's proof (if received early)
            clutch_proof_resends_left: 0, // Bounded proof-retransmit budget (runtime only)
            clutch_keygen_in_progress: false, // No keygen running yet
            clutch_kem_encap_in_progress: false, // No KEM encap running yet
            clutch_ceremony_in_progress: false, // No ceremony completion running yet
            completed_their_hqc_prefix: None, // Set when CLUTCH completes, persisted
            offer_provenances: Vec::new(), // Collected offer provenances for ceremony nonce
            trust_level: TrustLevel::Stranger,
            added: vsf::eagle_time_oscillations(),
            last_seen: None,
            is_online: false,           // Starts offline until we confirm comms
            messages: Vec::new(),       // No messages yet
            message_scroll_offset: 0.0, // Starts at top (scrolled to latest when messages added)
            prev_is_online: false,      // Match initial state
            indicator_x: 0,             // Set during first draw
            indicator_y: 0,             // Set during first draw
            text_x: 0.0,                // Set during first draw
            text_y: 0.0,                // Set during first draw
            avatar_pixels: None,        // Fetched from FGTW by handle when online
            avatar_scaled: None,        // Scaled on demand for display
            avatar_scaled_diameter: 0,
            chain_woven: false,           // Chain not yet proven end-to-end (probe pending)
            probe_sent: false,            // Chain-weave probe not sent yet
            their_probe_seen: false,      // Haven't seen their chain-weave probe yet
            chain_advanced_by_ack: false, // Our TX chain not yet ACK-advanced
        }
    }

    pub fn with_ip(mut self, ip: SocketAddr) -> Self {
        self.ip = Some(ip);
        self
    }

    /// Record the same-LAN address FGTW reported for this device's last announce, alongside the public address from [`with_ip`](Self::with_ip). Lets the offer/KEM send race the LAN path against the WAN path without waiting for LAN multicast discovery (which routers often drop).
    /// Only IPv4 LAN addresses are stored — `local_ip`/`local_port` are typed for the v4 hairpin case; a v6 local address is left unset and the WAN path carries it.
    pub fn with_local_ip(mut self, local_ip: Option<std::net::IpAddr>, port: u16) -> Self {
        if let Some(std::net::IpAddr::V4(v4)) = local_ip {
            self.local_ip = Some(v4);
            self.local_port = Some(port);
        }
        self
    }

    pub fn with_seed(mut self, seed: Seed) -> Self {
        self.relationship_seed = Some(seed);
        self
    }

    pub fn with_trust_level(mut self, level: TrustLevel) -> Self {
        self.trust_level = level;
        self
    }

    pub fn update_last_seen(&mut self, timestamp: i64) {
        self.last_seen = Some(timestamp);
    }

    /// Get the best address to reach this contact. If we share the same public IP (same NAT), use their local_ip to bypass AP isolation. Otherwise use their public IP.
    pub fn best_addr(
        &self,
        our_public_ip: Option<std::net::IpAddr>,
    ) -> Option<std::net::SocketAddr> {
        let public_addr = self.ip?;

        // If peer has a USABLE local_ip and we share the same public IP, use local_ip (skip CLAT/service-continuity noise).
        if let (Some(local_v4), Some(our_ip)) = (self.local_ip, our_public_ip) {
            if crate::network::udp::is_usable_lan_ipv4(local_v4) && public_addr.ip() == our_ip {
                // Same public IP = same NAT, use local_ip to bypass AP isolation
                return Some(std::net::SocketAddr::new(
                    std::net::IpAddr::V4(local_v4),
                    public_addr.port(),
                ));
            }
        }

        Some(public_addr)
    }

    /// Returns the (primary, alternate) address pair for racing a CLUTCH transfer across both the same-LAN and public paths. Primary is the LAN address (preferred — no router hairpin, no AP isolation), alternate is the public address. When no LAN address is known, primary is the public address and alternate is `None`. PT sends the SPEC to both and locks onto whichever ACKs first (see [`crate::network::pt::PtManager::send_with_pubkey_and_alt`]).
    pub fn race_addrs(&self) -> Option<(SocketAddr, Option<SocketAddr>)> {
        let public_addr = self.ip?;
        if let (Some(local_v4), Some(local_port)) = (self.local_ip, self.local_port) {
            // Skip an unreachable LAN candidate (464XLAT CLAT `192.0.0.4` and friends) — racing it just burns the retry budget before the WAN path wins. A peer on cellular has no real LAN address; the public path is the only one.
            if crate::network::udp::is_usable_lan_ipv4(local_v4) {
                let lan = SocketAddr::new(std::net::IpAddr::V4(local_v4), local_port);
                if lan != public_addr {
                    return Some((lan, Some(public_addr)));
                }
            }
        }
        Some((public_addr, None))
    }

    /// True once the CLUTCH ceremony is Complete — which is cryptographically impossible unless BOTH parties ran it, so it doubles as the mutual-consent signal ("we each added the other"). Used to gate friend-only behaviour like the direct peer-to-peer avatar exchange.
    pub fn is_mutual(&self) -> bool {
        self.clutch_state == ClutchState::Complete
    }

    /// Does `device_pubkey` belong to this contact's identity? True for the first-met device (`public_identity`) OR any current member of the friend's folded fleet. This is the fold-and-honour gate: a friend's second phone is a different device key we've never seen, but it's a valid member of their chain, so its pings/offers/messages must be honoured. `public_identity` stays in the check so a not-yet-refreshed contact still answers its known device.
    pub fn knows_device(&self, device_pubkey: &[u8; 32]) -> bool {
        self.public_identity.key == *device_pubkey || self.fleet_members.contains(device_pubkey)
    }

    /// Every device pubkey we'll answer for this contact — `public_identity` unioned with the folded fleet. Feeds the status checker's answerable-pubkey set so pongs from any of the friend's devices are honoured.
    pub fn answerable_pubkeys(&self) -> Vec<[u8; 32]> {
        let mut v = Vec::with_capacity(self.fleet_members.len() + 1);
        v.push(self.public_identity.key);
        for m in &self.fleet_members {
            if *m != self.public_identity.key {
                v.push(*m);
            }
        }
        v
    }

    pub fn can_be_custodian(&self) -> bool {
        matches!(self.trust_level, TrustLevel::Trusted | TrustLevel::Inner)
    }

    /// Get the cached ceremony ID for CLUTCH with this contact.
    ///
    /// The ceremony_id is computed once during background keygen and cached. It's deterministic from sorted handle_hashes, so both parties compute same value. Returns None if keygen hasn't completed yet.
    pub fn get_ceremony_id(&self) -> Option<CeremonyId> {
        self.ceremony_id.map(CeremonyId::from_bytes)
    }

    /// Initialize CLUTCH slots for a 2-party ceremony. Slots are indexed by sorted handle_hash position.
    pub fn init_clutch_slots(&mut self, our_handle_hash: [u8; 32]) {
        let mut hashes = vec![our_handle_hash, self.handle_hash];
        hashes.sort();

        self.clutch_slots = hashes.into_iter().map(PartySlot::new).collect();
    }

    /// Get the slot index for a given handle_hash. Returns None if the handle_hash is not in the ceremony.
    pub fn get_slot_index(&self, handle_hash: &[u8; 32]) -> Option<usize> {
        self.clutch_slots
            .iter()
            .position(|s| &s.handle_hash == handle_hash)
    }

    /// Get mutable reference to the slot for a given handle_hash.
    pub fn get_slot_mut(&mut self, handle_hash: &[u8; 32]) -> Option<&mut PartySlot> {
        self.clutch_slots
            .iter_mut()
            .find(|s| &s.handle_hash == handle_hash)
    }

    /// Get reference to the slot for a given handle_hash.
    pub fn get_slot(&self, handle_hash: &[u8; 32]) -> Option<&PartySlot> {
        self.clutch_slots
            .iter()
            .find(|s| &s.handle_hash == handle_hash)
    }

    /// Check if all slots are complete (ceremony can finish). For 2-party: both slots have offer + both KEM secret directions.
    pub fn all_slots_complete(&self) -> bool {
        !self.clutch_slots.is_empty() && self.clutch_slots.iter().all(|s| s.is_complete())
    }

    /// The eight independent KEM "eggs" a CLUTCH braids, across three security families — a compromise of any one family still leaves the shared secret protected by the others. Named here so the status line can say WHAT is being exchanged, not just "pending".
    /// 4 elliptic-curve (x25519, P-384, secp256k1, P-256) · 2 lattice (Frodo-976, NTRU-701) · 2 code-based (McEliece-460896, HQC-256).
    pub const CLUTCH_EGGS: usize = 8;

    /// Total ceremony milestones for a 2-party CLUTCH — the denominator of the status fraction. Keygen, our offer, their offer, our KEM, their KEM, braid, their proof, verify.
    pub const CLUTCH_STEPS: u8 = 8;

    /// Where the CLUTCH ceremony actually is, as `step/total · what's happening (which eggs)` — for the conversation status line and asymmetric-completion debugging. The fraction is monotonic toward `secured`; the label names the crypto in flight (the egg families) instead of a flat "pending".
    pub fn clutch_status_detail(&self) -> String {
        let n = Self::CLUTCH_STEPS;
        match self.clutch_state {
            ClutchState::Complete => {
                if self.chain_woven {
                    "secured".to_string()
                } else {
                    "testing · weaving the chain".to_string()
                }
            }
            ClutchState::AwaitingProof => {
                if self.clutch_their_eggs_proof.is_some() {
                    format!("{n}/{n} · verifying proof")
                } else {
                    format!("7/{n} · braided · awaiting their proof")
                }
            }
            ClutchState::Pending => {
                let their_offer = self.clutch_slots.iter().any(|s| s.offer.is_some() && s.handle_hash != self.handle_hash);
                let all_kem = self.all_slots_complete();
                // Walk the milestones in order; report the earliest one not yet reached.
                if self.clutch_ceremony_in_progress {
                    format!("6/{n} · braiding {} eggs", Self::CLUTCH_EGGS)
                } else if self.clutch_our_keypairs.is_none() {
                    // Keygen (McEliece dominates the ~1-2s) — name the egg families being forged.
                    format!("1/{n} · forging {} eggs (4 EC · 2 lattice · 2 code)", Self::CLUTCH_EGGS)
                } else if self.clutch_kem_encap_in_progress {
                    format!("4/{n} · sealing KEMs (McEliece·HQC·Frodo·NTRU)")
                } else if all_kem {
                    format!("6/{n} · braiding {} eggs", Self::CLUTCH_EGGS)
                } else if their_offer || self.clutch_pending_kem.is_some() {
                    format!("5/{n} · awaiting their KEMs")
                } else if self.clutch_offer_sent {
                    format!("3/{n} · awaiting their eggs")
                } else {
                    format!("2/{n} · sending offer")
                }
            }
        }
    }

    /// Insert a message in sorted order by timestamp (oldest first). Uses binary search for O(log n) position finding.
    pub fn insert_message_sorted(&mut self, msg: ChatMessage) {
        // Binary search for insertion point (maintains ascending timestamp order)
        let pos = self
            .messages
            .binary_search_by(|m| m.timestamp.cmp(&msg.timestamp))
            .unwrap_or_else(|pos| pos);
        self.messages.insert(pos, msg);
    }
}

#[cfg(test)]
mod fold_honour_tests {
    use super::*;

    fn contact_with(pk: [u8; 32]) -> Contact {
        Contact::new(
            HandleText::new("friend"),
            [0x11; 32],
            DevicePubkey::from_bytes(pk),
        )
    }

    #[test]
    fn knows_first_met_device_before_any_refresh() {
        let c = contact_with([1u8; 32]);
        assert!(c.knows_device(&[1u8; 32]), "the device we met is always known");
        assert!(!c.knows_device(&[2u8; 32]), "a stranger device is not");
    }

    #[test]
    fn knows_any_folded_member_and_answerable_dedups() {
        let mut c = contact_with([1u8; 32]);
        c.fleet_members = vec![[1u8; 32], [2u8; 32], [3u8; 32]];
        assert!(c.knows_device(&[2u8; 32]), "a sibling device folds in");
        assert!(c.knows_device(&[3u8; 32]));
        assert!(!c.knows_device(&[9u8; 32]), "still rejects a non-member");
        // public_identity (1) appears once, siblings 2 and 3 follow — no duplicate of 1.
        let ans = c.answerable_pubkeys();
        assert_eq!(ans.len(), 3, "first-met + 2 distinct siblings, deduped: {ans:?}");
        assert_eq!(ans[0], [1u8; 32], "first-met device leads");
    }
}
