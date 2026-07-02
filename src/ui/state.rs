//! Shared state types — `AppState`, `LaunchState`, network search result types. Kept in their own module (not on `PhotonApp`) because non-ui code (`network/handle_query`, `platform/jni_android`) depends on them; import from `ui::state` or the `crate::ui` re-exports.

use crate::types::HandleText;

/// Top-level app screen — drives both rendering routing (which Container::visit branch runs) and network-state machine transitions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AppState {
    /// Launch screen states (before main messenger UI)
    Launch(LaunchState),

    /// Main messenger - ready to search peers and chat
    Ready,

    /// Searching for a peer handle (computing handle_proof in background)
    Searching,

    /// Viewing conversation with a contact (contact index stored separately)
    Conversation,

    /// Existing (attested) device adding another device to the fleet: a words-entry screen — the NEW device displays its pairing words, the user types them here, and a match lights the Bind affordance (orb tap). Entered by tapping the orb on Ready.
    AddDevice,

    /// Active P2P conversation (legacy - may remove)
    Connected { peer_handle: String },
}

impl Default for AppState {
    fn default() -> Self {
        AppState::Launch(LaunchState::Fresh)
    }
}

/// Sub-states for the launch screen
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LaunchState {
    /// Ready to attest - show handle input + "Attest" button
    Fresh,

    /// Computing handle_proof + announcing to FGTW Show loading spinner, no button
    Attesting,

    /// Attestation failed - show error message, no button User can edit textbox to return to Fresh
    Error(String),
}

impl LaunchState {
    /// Check if we're in a state where the user can type in the handle textbox
    pub fn can_edit_handle(&self) -> bool {
        !matches!(self, LaunchState::Attesting)
    }

    /// Check if we're waiting for a network response
    pub fn is_loading(&self) -> bool {
        matches!(self, LaunchState::Attesting)
    }
}

/// Result of searching for a handle
#[derive(Debug, Clone)]
pub struct FoundPeer {
    pub handle: HandleText,
    pub handle_proof: [u8; 32], // Cached handle_proof (expensive - ~1 second to compute)
    pub device_pubkey: crate::types::DevicePubkey,
    pub ip: std::net::SocketAddr, // Public (WAN) address from FGTW
    pub local_ip: Option<std::net::IpAddr>, // Same-LAN address from FGTW, for hairpin-NAT direct connect
}

#[derive(Debug, Clone)]
pub enum SearchResult {
    Found(FoundPeer),
    NotFound,
    Error(String),
}
