use super::{PublicIdentity, Seed};
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;

/// A chat message in a conversation (UI-level representation)
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ChatMessage {
    pub content: String,
    pub timestamp: u64,      // Eagle time (seconds since Apollo 11 landing)
    pub is_outgoing: bool,   // true = we sent it, false = they sent it
    pub delivered: bool,     // true = confirmed delivered to recipient
}

impl ChatMessage {
    pub fn new(content: String, is_outgoing: bool) -> Self {
        Self {
            content,
            timestamp: vsf::eagle_time_nanos() as u64,
            is_outgoing,
            delivered: false,
        }
    }
}

/// A handle name stored as VSF text (normalized Unicode, unambiguous)
/// Wrapper around String that represents a VSF x-type text value
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
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

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Contact {
    pub id: ContactId,
    pub handle: HandleText, // VSF-style text for unambiguous handle storage
    pub public_identity: PublicIdentity,
    pub ip: Option<SocketAddr>, // Last known IP:port from FGTW or direct
    pub relationship_seed: Option<Seed>,
    pub trust_level: TrustLevel,
    pub added_timestamp: u64,
    pub last_seen: Option<u64>,
    pub is_online: bool,      // True when we have confirmed bidirectional comms
    pub messages: Vec<ChatMessage>, // Conversation history
    #[serde(skip)]
    pub prev_is_online: bool, // For differential rendering (not persisted)
    #[serde(skip)]
    pub indicator_x: usize,   // Cached indicator dot X position (set during draw)
    #[serde(skip)]
    pub indicator_y: usize,   // Cached indicator dot Y position (set during draw)
    #[serde(skip)]
    pub text_x: f32,          // Cached text X position (set during draw)
    #[serde(skip)]
    pub text_y: f32,          // Cached text Y position (set during draw)
    // Avatar cache - fetched from FGTW by handle
    // Storage key is deterministic: BLAKE3(BLAKE3(handle) || "avatar")
    #[serde(skip)]
    pub avatar_pixels: Option<Vec<u8>>,      // Full 256x256 VSF RGB pixels (cached)
    #[serde(skip)]
    pub avatar_scaled: Option<Vec<u8>>,      // Pre-scaled to current display size
    #[serde(skip)]
    pub avatar_scaled_diameter: usize,       // Diameter the scaled pixels were rendered for
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ContactId([u8; 16]);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum TrustLevel {
    Stranger,
    Known,
    Trusted,
    Inner,
}

impl ContactId {
    pub fn new() -> Self {
        let mut id = [0u8; 16];
        rand::RngCore::fill_bytes(&mut rand::thread_rng(), &mut id);
        Self(id)
    }

    pub fn from_bytes(bytes: [u8; 16]) -> Self {
        Self(bytes)
    }

    pub fn as_bytes(&self) -> &[u8; 16] {
        &self.0
    }
}

impl Default for ContactId {
    fn default() -> Self {
        Self::new()
    }
}

impl Contact {
    pub fn new(handle: HandleText, public_identity: PublicIdentity) -> Self {
        Self {
            id: ContactId::new(),
            handle,
            public_identity,
            ip: None,
            relationship_seed: None,
            trust_level: TrustLevel::Stranger,
            added_timestamp: vsf::eagle_time_nanos() as u64,
            last_seen: None,
            is_online: false,      // Starts offline until we confirm comms
            messages: Vec::new(),  // No messages yet
            prev_is_online: false, // Match initial state
            indicator_x: 0,        // Set during first draw
            indicator_y: 0,        // Set during first draw
            text_x: 0.0,           // Set during first draw
            text_y: 0.0,           // Set during first draw
            avatar_pixels: None,   // Fetched from FGTW by handle when online
            avatar_scaled: None,   // Scaled on demand for display
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

    pub fn update_last_seen(&mut self, timestamp: u64) {
        self.last_seen = Some(timestamp);
    }

    pub fn can_be_custodian(&self) -> bool {
        matches!(self.trust_level, TrustLevel::Trusted | TrustLevel::Inner)
    }
}
