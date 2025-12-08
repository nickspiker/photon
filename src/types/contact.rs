use super::{DevicePubkey, Seed};
use crate::crypto::clutch::{ClutchAllKeypairs, ClutchFullOfferPayload, ClutchKemSharedSecrets};
use std::net::{Ipv4Addr, SocketAddr};

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
/// Full 8-algorithm CLUTCH flow:
/// 1. Pending: Contact added, no ceremony started yet
/// 2. KeysGenerated: All 8 ephemeral keypairs generated (~548KB pubkeys)
/// 3. OfferSent: We sent our full offer, waiting for theirs
/// 4. OfferReceived: We received their offer, sending ours
/// 5. OffersExchanged: Both offers exchanged, generating KEM response
/// 6. KemSent: We sent our KEM response, waiting for theirs
/// 7. KemReceived: We received their KEM response
/// 8. Complete: Both KEM responses exchanged, pads derived, can message
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ClutchState {
    #[default]
    Pending, // Contact added, no ceremony started
    KeysGenerated,   // All 8 ephemeral keypairs generated
    OfferSent,       // We sent our full offer (~548KB)
    OfferReceived,   // We received their full offer
    OffersExchanged, // Both offers exchanged
    KemSent,         // We sent our KEM response (~17KB)
    KemReceived,     // We received their KEM response
    Complete,        // CLUTCH done, can message
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
    pub send_pad: Option<Vec<u8>>, // 1MB rolling pad for sending messages
    pub recv_pad: Option<Vec<u8>>, // 1MB rolling pad for receiving messages
    pub clutch_state: ClutchState,

    // Full 8-algorithm CLUTCH state
    pub clutch_our_keypairs: Option<ClutchAllKeypairs>, // Our 8 ephemeral keypairs (~512KB secret keys)
    pub clutch_their_offer: Option<ClutchFullOfferPayload>, // Their 8 public keys (~548KB)
    pub clutch_our_kem_secrets: Option<ClutchKemSharedSecrets>, // Secrets from our encapsulations (we→them)
    pub clutch_their_kem_secrets: Option<ClutchKemSharedSecrets>, // Secrets from their encapsulations (them→us)

    // Legacy X25519-only fields (kept for backward compat with existing contacts)
    pub clutch_our_ephemeral_secret: Option<[u8; 32]>, // Our ephemeral X25519 secret (zeroize after use)
    pub clutch_our_ephemeral_pubkey: Option<[u8; 32]>, // Our ephemeral X25519 pubkey (needed for parallel seed derivation)
    pub clutch_their_ephemeral_pubkey: Option<[u8; 32]>, // Their ephemeral for CLUTCH

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
            send_pad: None, // Generated from CLUTCH after ceremony
            recv_pad: None, // Generated from CLUTCH after ceremony
            clutch_state: ClutchState::Pending,
            // Full 8-algorithm CLUTCH fields
            clutch_our_keypairs: None,
            clutch_their_offer: None,
            clutch_our_kem_secrets: None,
            clutch_their_kem_secrets: None,
            // Legacy X25519-only fields
            clutch_our_ephemeral_secret: None,
            clutch_our_ephemeral_pubkey: None,
            clutch_their_ephemeral_pubkey: None,
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

    pub fn can_be_custodian(&self) -> bool {
        matches!(self.trust_level, TrustLevel::Trusted | TrustLevel::Inner)
    }
}
