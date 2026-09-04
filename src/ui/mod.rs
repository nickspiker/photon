// All platforms share the fluor-hosted UI stack: `photon_app::PhotonApp` runs under `fluor::host::android::AndroidShell` on Android and `fluor::host::app::run_app` on desktop.
// The legacy Android compositor (app / compositing / drawing / keyboard / mouse / text_editing / text_rasterizing / renderer_android) was deleted once fully retired — text measurement, editing, and rendering now live in fluor.

pub mod avatar;
/// Avatar sharing over scoped blobs — one ciphertext, a private slot per reader (docs/scoped-blobs.md). Replaces the bearer pin.
pub mod avatar_scoped;
pub mod display_profile;
pub mod lms2006so;
pub mod state;

// Chromatic wave (sine-modulated visible-spectrum bar). Reads LMS2006SO; writes α + darkness pixels.
pub mod chromatic_wave;

// "Photon" wordmark — port of legacy `compositing.rs::draw_logo_text` with glow + highlight + sharp body in α + darkness format. Oxanium 800.
pub mod photon_logo;

// Launch-screen layout calculator — proportional slicing port from legacy `app::Layout::new`.
pub mod launch_layout;

// Ready-screen layout calculator — slice-based port of legacy `app::ContactsUnifiedLayout`.
pub mod ready_layout;

// VSF RGB → BT.2020 RGB conversion for display output on Android (γ=2.0 end-to-end).
pub mod colour_convert;

// Avatar paint — Mitchell resize + AA textured circle into a fluor `Canvas`.
pub mod avatar_render;

// The incoming-wave rim — the contact's bell casting drawn as an undulating Fourier contour around the avatar.
pub mod ring_rim;

pub use state::{AppState, FoundPeer, LaunchState, SearchResult, SettingsPage};


// Settings-panel layout calculator — nav-rail-vs-content split and stacked content rows via fluor's `Region`.
pub mod settings_layout;

// Photon-specific UI colour palette (the app's own colours, mirroring `fluor::theme`).
pub mod theme;

// The language catalog — every user-facing string is a `lang::Msg` variant, each language an exhaustive match (see lang.rs doctrine).
pub mod lang;

// The fluor-hosted `FluorApp` impl. Drives desktop via `host-winit` and Android via `host-android`.
pub mod photon_app;
pub use photon_app::PhotonApp;

/// Custom events for cross-thread communication with the event loop. On desktop, background tasks clone the `EventLoopProxy<PhotonEvent>` from `PhotonApp::set_event_proxy` and call `send_event` to wake the UI thread; on Android the same proxy type exists (data-only) but background work pokes the activity via JNI callbacks instead — the variants stay shared so the FluorApp::on_user_event handler is the same code on both platforms.
#[derive(Debug, Clone)]
pub enum PhotonEvent {
    /// FGTW connectivity status changed
    ConnectivityChanged(bool),
    /// Attestation completed (background thread finished)
    AttestationComplete,
    /// Message received from peer (future use)
    MessageReceived,
    /// Network update available (status, CLUTCH, avatar, etc.) - wake event loop
    NetworkUpdate,
    /// Background CLUTCH keypair generation completed
    ClutchKeygenComplete,
    /// Background CLUTCH KEM encapsulation completed
    ClutchKemEncapComplete,
    ClutchKemDecapComplete,
    /// Background CLUTCH ceremony completion (avalanche_expand) completed
    ClutchCeremonyComplete,
    /// A second launch handed off "surface yourself" over the control channel (desktop resident mode) — the handler un-hides the window via `EventResponse::ShowWindow`.
    ShowWindow,
}
