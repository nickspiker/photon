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

    /// The conversation panel — the orb's destination inside ANY conversation, whatever its size (docs/conversations.md Phase 1). Mirrors the Settings screen: same layout, nav rail with pages, scrolling content. The open conversation rides `active_conversation`; the rail's pinned Back returns to it (from a Person page, back to Bonds).
    ConversationPanel(ConversationPage),

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
    /// Handle / avatar / pubkey — the identity read-out page. On ferros it lives in `ahau`, the OS capsule, not here (`ALL`).
    You,
    /// Bound-device list + add / rename / retire — the multi-device page. On ferros it lives in `ahau`, the OS capsule, not here (`ALL`).
    Fleet,
    /// Lock / retire / shred — the destructive-actions page. On ferros it lives in `ahau`, the OS capsule, not here (`ALL`).
    Security,
    /// Custodian opt-in + identity backup — the getting-back-in page. On ferros it lives in `ahau`, the OS capsule, not here (`ALL`).
    Recovery,
    /// Theme / party colours / zoom / calibration.
    Appearance,
    /// Chime toggles + presence visibility.
    Notifications,
    /// Auto-update toggle.
    Updates,
    /// The on-device VSF log: clear / snapshot / submit.
    Diagnostics,
    /// Ideas & fixes: anonymous feedback to the developer, with the sender's own list and each one's status (docs/ideas.md).
    Ideas,
    /// The device vault's physique — capacity, odometer, live set, health (read-only, refresh on demand). On ferros the vault is the kernel's and this lives in the OS, not here (`ALL`).
    Vault,
    /// THE WAVE PAGE (Nick 2026-09-16, "a wave config screen"): this device's hearing (the earpiece trim in stops, the route, the rocker), its voice (the per-mic calibration profiles, forget / measure now), the last wave's stats, and the wave preferences (ring, vibrate, hold every wave, plaid off-LAN).
    Wave,
    /// THE BEAM PAGE (Nick 2026-10-26, "a beam settings screen to control bitrate/size/quality/HDRness… show some test gradients there too"): what this device asks of the beams it sends (HDR, size, frame rate, quality, the data ceiling, what a busy link sheds), this device only or following the fleet, and test patterns that check this screen's HDR against white and brightest and show its banding.
    Beam,
    /// UI-language picker — one button per language, labelled in itself (autonyms).
    Language,
    /// The number base: the dozenal toggle, why, the digit cheat sheet, and the DMS time-ago legend. Its rail label reads "Dozenal" or "Arabic" per the current choice (Nick 2026-09-09).
    Dozenal,
    /// Conversations (docs/molecules.md §10): how groups work, in plain words; the default newcomer-history policy for groups you found; the groups you stand in with a Mute pill each.
    Conversations,
    /// Explainer / philosophy / version / feedback / credits.
    About,
}

impl SettingsPage {
    /// All pages in rail order — the nav rail and the tab-cycle iterate this. Appearance is COMMENTED OUT of the rail (Nick 2026-09-01) — the variant + its render arm stay compiled so restoring it is a one-line uncomment; the dozenal (base-twelve) toggle lives on About. The Wave calibration page is GONE (2026-09-07): the v-chirp connect probe measures every route at every wave start, so there is no ritual for a page to hold.
    #[cfg(not(target_os = "ferros"))]
    pub const ALL: [SettingsPage; 15] = [
        SettingsPage::You,
        SettingsPage::Fleet,
        SettingsPage::Security,
        SettingsPage::Recovery,
        // SettingsPage::Appearance,
        SettingsPage::Conversations,
        SettingsPage::Wave,
        SettingsPage::Beam,
        SettingsPage::Notifications,
        SettingsPage::Updates,
        SettingsPage::Diagnostics,
        SettingsPage::Ideas,
        SettingsPage::Vault,
        SettingsPage::Language,
        SettingsPage::Dozenal,
        SettingsPage::About,
    ];

    /// On ferros the person's identity is the OS's, not Photon's (2026-10-27, the owner via the ferros session: "Photon is the comms app; the identity/fleet/You surfaces belong to the OS"): You, Fleet, Security and Recovery live in `ahau`, the OS capsule, there, so the rail shows the messenger's pages alone. The Vault page goes too: the device vault is the kernel's there (kete's ferros backend answers no stats), so device storage health lives in the OS. Their render arms and the logic behind them stay compiled — the pages are unreachable, not removed — so ahau can take the logic over.
    #[cfg(target_os = "ferros")]
    pub const ALL: [SettingsPage; 10] = [
        SettingsPage::Conversations,
        SettingsPage::Wave,
        SettingsPage::Beam,
        SettingsPage::Notifications,
        SettingsPage::Updates,
        SettingsPage::Diagnostics,
        SettingsPage::Ideas,
        SettingsPage::Language,
        SettingsPage::Dozenal,
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
            SettingsPage::Ideas => "Ideas",
            SettingsPage::Vault => "Vault",
            SettingsPage::Wave => "Wave",
            SettingsPage::Beam => "Beam",
            SettingsPage::Language => "Language",
            // The base page is "Base" (Nick 2026-09-10) — the pills on it show the choice.
            SettingsPage::Dozenal => "Base",
            SettingsPage::Conversations => "Conversations",
            SettingsPage::About => "About",
        }
    }
}

/// Pages of the conversation panel — one set for every conversation at any size (docs/conversations.md Phase 1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConversationPage {
    /// Everyone in it and everyone you could bring in, as a contacts-style list with a checkbox each; tapping a member opens their Person page. Invite edits go out when you leave the page.
    Bonds,
    /// The conversation in numbers — counts, span, delivery, history, the key ladder, connection. These rows should CONVERGE across our fleet devices; divergence is a sync bug made visible.
    Stats,
    /// The title, the history policy and era, mute, clearing, and leaving.
    Manage,
    /// One person in it (drilled into from Bonds, never on the rail): their avatar, what they share with us, their identity, and the friendship's own actions.
    Person([u8; 32]),
}

impl ConversationPage {
    /// The rail's pages, in order.
    pub const RAIL: [ConversationPage; 3] = [ConversationPage::Bonds, ConversationPage::Stats, ConversationPage::Manage];
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

    /// The worker refused the announce because this device is fleet-locked (treat-as-stolen). A terminal brick, NOT an editable error: the handle is correct and the identity is real, but the fleet owner marked this hardware stolen, so no handle re-type helps — only the owner unlocking it from another device does. The screen is a dead end (no textbox, no button), and because the worker is the authority this state re-appears on every attest even after a local wipe.
    Locked,
}

impl LaunchState {
    /// Check if we're in a state where the user can type in the handle textbox
    pub fn can_edit_handle(&self) -> bool {
        !matches!(self, LaunchState::Attesting | LaunchState::Locked)
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
    /// The port at `local_ip` (PeerRecord::lan_port — the stated one, else PHOTON_PORT; never the public port).
    pub lan_port: u16,
}

#[derive(Debug, Clone)]
pub enum SearchResult {
    Found(FoundPeer),
    NotFound,
    Error(String),
}
