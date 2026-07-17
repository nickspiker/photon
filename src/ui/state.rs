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

    /// Settings / About / Help panel — the orb's real destination on Ready. Carries the currently-selected page so the render + layout know which page body to draw. STUB: every page + control renders, but no behaviour is wired.
    Settings(SettingsPage),

    /// Active P2P conversation (legacy - may remove)
    Connected { peer_handle: String },
}

impl Default for AppState {
    fn default() -> Self {
        AppState::Launch(LaunchState::Fresh)
    }
}

/// The nine top-level pages of the settings panel — the value carried by [`AppState::Settings`] naming the page whose body is currently drawn. The left nav rail lists all nine; clicking one swaps this value. Order here is the render order in the rail.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsPage {
    /// Handle / avatar / pubkey — the identity read-out page.
    You,
    /// Bound-device list + add / rename / retire — the multi-device page.
    Fleet,
    /// Lock / retire / shred — the destructive-actions page.
    Security,
    /// Custodian opt-in + identity backup — the getting-back-in page.
    Recovery,
    /// Theme / party colours / zoom / calibration.
    Appearance,
    /// Chime toggles + presence visibility.
    Notifications,
    /// Auto-update toggle.
    Updates,
    /// The on-device VSF log: clear / snapshot / submit.
    Diagnostics,
    /// Explainer / philosophy / version / feedback / credits.
    About,
}

impl SettingsPage {
    /// All pages in rail order — the nav rail and the tab-cycle iterate this.
    pub const ALL: [SettingsPage; 9] = [
        SettingsPage::You,
        SettingsPage::Fleet,
        SettingsPage::Security,
        SettingsPage::Recovery,
        SettingsPage::Appearance,
        SettingsPage::Notifications,
        SettingsPage::Updates,
        SettingsPage::Diagnostics,
        SettingsPage::About,
    ];

    /// Human-readable rail label for this page.
    pub fn label(self) -> &'static str {
        match self {
            SettingsPage::You => "You",
            SettingsPage::Fleet => "Fleet",
            SettingsPage::Security => "Security",
            SettingsPage::Recovery => "Recovery",
            SettingsPage::Appearance => "Appearance",
            SettingsPage::Notifications => "Notifications",
            SettingsPage::Updates => "Updates",
            SettingsPage::Diagnostics => "Diagnostics",
            SettingsPage::About => "About",
        }
    }
}

/// Sub-states for the launch screen
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LaunchState {
    /// Ready to attest - show handle input + "Attest" button
    Fresh,

    /// Permanence interstitial after the first Attest press. A claimed handle has no password, no reset, and no recovery — more permanent than any account — so the first press arms this warning and only a second deliberate press fires the query. Any edit to the handle drops back to `Fresh` (same cancel path as `Error`).
    Confirm,

    /// The collision-ambiguous branch (docs/lifecycle.md D1): the probed handle has a fleet whose genesis binds to this handle's identity — which is EITHER the user's own other device OR a total stranger who typed the same name (the derivation makes them indistinguishable). The screen speaks to both readers with two affordances: pick another name (back to Fresh) / it's mine (→ pairing words). Nothing posts to the network — no bind request, no beacon — until "it's mine" is pressed. Editing the handle cancels back to `Fresh` like `Confirm`.
    KnownHandle,

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
