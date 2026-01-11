use super::{CeremonyId, DevicePubkey, FriendshipId, Seed};
use crate::crypto::clutch::{
    ClutchAllKeypairs, ClutchKemResponsePayload, ClutchKemSharedSecrets, ClutchOfferPayload,
};
use std::net::{Ipv4Addr, SocketAddr};

/// A slot in the CLUTCH ceremony, indexed by sorted handle_hash position.
/// Same indexing on all devices in the ceremony (N-party ready).
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

    /// Check if this slot has all required data for ceremony completion.
    /// Each slot needs offer + ONE KEM contribution (either direction):
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
    pub timestamp: f64,    // Eagle time (seconds since Apollo 11 landing)
    pub is_outgoing: bool, // true = we sent it, false = they sent it
    pub delivered: bool,   // true = confirmed delivered to recipient
}

impl ChatMessage {
    pub fn new(content: String, is_outgoing: bool) -> Self {
        Self {
            content,
            timestamp: vsf::eagle_time_nanos(),
            is_outgoing,
            delivered: false,
        }
    }

    /// Create a message with a specific timestamp (for received messages with known eagle_time)
    pub fn new_with_timestamp(content: String, is_outgoing: bool, timestamp: f64) -> Self {
        Self {
            content,
            timestamp,
            is_outgoing,
            delivered: false,
        }
    }
}

/// A handle name stored as VSF text (normalized Unicode, unambiguous)
/// Wrapper around String that represents a VSF x-type text value
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

/// State of the CLUTCH key ceremony for a contact
///
/// Slot-based design: each party has a slot indexed by sorted handle_hash position.
/// Ceremony completes when all slots have both offer and kem_secrets filled,
/// AND both parties have exchanged matching eggs_proof values.
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
    pub ip: Option<SocketAddr>, // Last known IP:port from FGTW or direct (public IP)
    pub local_ip: Option<Ipv4Addr>, // LAN IP discovered via broadcast (for hairpin NAT workaround)
    pub local_port: Option<u16>, // LAN port discovered via broadcast
    pub relationship_seed: Option<Seed>,
    pub friendship_id: Option<FriendshipId>, // Links to friendship storage (chains live there)
    pub clutch_state: ClutchState,

    // Slot-based CLUTCH state (N-party support)
    /// Our 8 ephemeral keypairs (~512KB secret keys) - generated once per ceremony
    pub clutch_our_keypairs: Option<ClutchAllKeypairs>,
    /// Party slots indexed by sorted handle_hash position
    /// Each slot contains offer + kem_secrets for one party (including self)
    pub clutch_slots: Vec<PartySlot>,
    /// Cached ceremony_id - computed from handle_hashes + sorted ping provenances.
    /// Uses spaghettify for mixing (no memory-hard step needed).
    /// Unique per ceremony due to ping timestamp entropy.
    pub ceremony_id: Option<[u8; 32]>,
    /// Pending KEM response received before our keygen completed
    /// Stored here and processed when ceremony_id becomes available
    pub clutch_pending_kem: Option<ClutchKemResponsePayload>,
    /// PT transfer ID for our outbound offer (None = not sent, Some = in flight or ACKed)
    /// We check PT's transfer state to know if peer ACKed our offer
    pub clutch_offer_transfer_id: Option<usize>,
    /// Our computed eggs_proof (stored while awaiting peer's proof for verification)
    pub clutch_our_eggs_proof: Option<[u8; 32]>,
    /// Peer's eggs_proof if received before we computed ours
    pub clutch_their_eggs_proof: Option<[u8; 32]>,
    /// Flag to prevent multiple concurrent keygens (race condition guard)
    pub clutch_keygen_in_progress: bool,
    /// Flag to prevent multiple concurrent KEM encapsulations
    pub clutch_kem_encap_in_progress: bool,
    /// Flag to prevent multiple concurrent ceremony completions (avalanche_expand)
    pub clutch_ceremony_in_progress: bool,
    /// HQC public key prefix from peer's last completed ceremony.
    /// Used for stale detection: if received offer has same HQC prefix, it's a
    /// PT retransmission (stale), not a legitimate re-key request.
    /// Stored at completion time, cleared when accepting new ceremony.
    pub completed_their_hqc_prefix: Option<[u8; 8]>,
    /// Collected offer provenances for ceremony nonce derivation.
    /// Each offer's VSF header has hp = BLAKE3(signer_pubkey || creation_time_nanos).
    /// Sorted and combined via spaghettify to derive unique ceremony_id.
    /// Cleared when CLUTCH ceremony completes.
    pub offer_provenances: Vec<[u8; 32]>,

    pub trust_level: TrustLevel,
    pub added: f64,
    pub last_seen: Option<f64>,
    pub is_online: bool, // True when we have confirmed bidirectional comms
    pub messages: Vec<ChatMessage>, // Conversation history
    pub message_scroll_offset: f32, // Vertical scroll offset for message area (pixels)
    pub prev_is_online: bool, // For differential rendering (not persisted)
    pub indicator_x: usize, // Cached indicator dot X position (set during draw)
    pub indicator_y: usize, // Cached indicator dot Y position (set during draw)
    pub text_x: f32,     // Cached text X position (set during draw)
    pub text_y: f32,     // Cached text Y position (set during draw)
    // Avatar cache - fetched from FGTW by handle
    // Storage key is deterministic: BLAKE3(BLAKE3(handle) || "avatar")
    pub avatar_pixels: Option<Vec<u8>>, // Full 256x256 VSF RGB pixels (cached)
    pub avatar_scaled: Option<Vec<u8>>, // Pre-scaled to current display size
    pub avatar_scaled_diameter: usize,  // Diameter the scaled pixels were rendered for
}

/// Contact identifier - BLAKE3 hash of the contact's public identity key
/// This provides deterministic, collision-resistant identification
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
        // Compute private handle_hash using VSF normalization
        // Formula: BLAKE3(VsfType::x(handle).flatten())
        // This ensures consistent hashing regardless of Unicode representation
        // This is PRIVATE and used for seed derivation (NOT the public handle_proof!)
        let vsf_bytes = vsf::VsfType::x(handle.as_str().to_string()).flatten();
        let handle_hash = *blake3::hash(&vsf_bytes).as_bytes();

        Self {
            id: ContactId::from_pubkey(&public_identity),
            handle,
            handle_proof,
            handle_hash,
            public_identity,
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
            clutch_offer_transfer_id: None, // PT transfer ID for tracking offer delivery
            clutch_our_eggs_proof: None, // Our proof (stored while awaiting peer's)
            clutch_their_eggs_proof: None, // Peer's proof (if received early)
            clutch_keygen_in_progress: false, // No keygen running yet
            clutch_kem_encap_in_progress: false, // No KEM encap running yet
            clutch_ceremony_in_progress: false, // No ceremony completion running yet
            completed_their_hqc_prefix: None, // Set when CLUTCH completes, persisted
            offer_provenances: Vec::new(), // Collected offer provenances for ceremony nonce
            trust_level: TrustLevel::Stranger,
            added: vsf::eagle_time_nanos(),
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
        }
    }

    pub fn with_ip(mut self, ip: SocketAddr) -> Self {
        self.ip = Some(ip);
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

    pub fn update_last_seen(&mut self, timestamp: f64) {
        self.last_seen = Some(timestamp);
    }

    /// Get the best address to reach this contact.
    /// If we share the same public IP (same NAT), use their local_ip to bypass AP isolation.
    /// Otherwise use their public IP.
    pub fn best_addr(&self, our_public_ip: Option<std::net::IpAddr>) -> Option<std::net::SocketAddr> {
        let public_addr = self.ip?;

        // If peer has a local_ip and we share the same public IP, use local_ip
        if let (Some(local_v4), Some(our_ip)) = (self.local_ip, our_public_ip) {
            if public_addr.ip() == our_ip {
                // Same public IP = same NAT, use local_ip to bypass AP isolation
                return Some(std::net::SocketAddr::new(
                    std::net::IpAddr::V4(local_v4),
                    public_addr.port(),
                ));
            }
        }

        Some(public_addr)
    }

    pub fn can_be_custodian(&self) -> bool {
        matches!(self.trust_level, TrustLevel::Trusted | TrustLevel::Inner)
    }

    /// Get the cached ceremony ID for CLUTCH with this contact.
    ///
    /// The ceremony_id is computed once during background keygen and cached.
    /// It's deterministic from sorted handle_hashes, so both parties compute same value.
    /// Returns None if keygen hasn't completed yet.
    pub fn get_ceremony_id(&self) -> Option<CeremonyId> {
        self.ceremony_id.map(CeremonyId::from_bytes)
    }

    /// Initialize CLUTCH slots for a 2-party ceremony.
    /// Slots are indexed by sorted handle_hash position.
    pub fn init_clutch_slots(&mut self, our_handle_hash: [u8; 32]) {
        let mut hashes = vec![our_handle_hash, self.handle_hash];
        hashes.sort();

        self.clutch_slots = hashes.into_iter().map(PartySlot::new).collect();
    }

    /// Get the slot index for a given handle_hash.
    /// Returns None if the handle_hash is not in the ceremony.
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

    /// Check if all slots are complete (ceremony can finish).
    /// For 2-party: both slots have offer + both KEM secret directions.
    pub fn all_slots_complete(&self) -> bool {
        !self.clutch_slots.is_empty() && self.clutch_slots.iter().all(|s| s.is_complete())
    }

    /// Insert a message in sorted order by timestamp (oldest first).
    /// Uses binary search for O(log n) position finding.
    pub fn insert_message_sorted(&mut self, msg: ChatMessage) {
        // Binary search for insertion point (maintains ascending timestamp order)
        let pos = self
            .messages
            .binary_search_by(|m| {
                m.timestamp
                    .partial_cmp(&msg.timestamp)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .unwrap_or_else(|pos| pos);
        self.messages.insert(pos, msg);
    }
}
