// Legacy desktop UI stack — owns the per-platform renderer trio + 5 837-line compositing.rs + 6 081-line app.rs + the four extension impls (mouse, keyboard, text_editing, text_rasterizing). Cfg-gated to Android only as of Phase 0; desktop runs through `photon_app` (the fluor-hosted FluorApp impl) below. Android keeps the legacy stack untouched until fluor grows `host-android`, at which point a separate plan migrates it.
#[cfg(target_os = "android")]
pub mod app;
#[cfg(target_os = "android")]
mod colour;
#[cfg(target_os = "android")]
mod compositing;
#[cfg(target_os = "android")]
pub mod drawing;
#[cfg(target_os = "android")]
mod keyboard;
#[cfg(target_os = "android")]
mod mouse;
#[cfg(target_os = "android")]
mod text_editing;
#[cfg(target_os = "android")]
mod text_rasterizing;

// `avatar` is shared between Android (legacy stack uses it directly) and desktop (the Phase 2 Avatar widget will wrap its LRU cache). Stays unconditional.
pub mod avatar;
pub mod display_profile;
pub mod lms2006so;
pub mod state;
pub mod theme;

// Desktop chromatic wave (sine-modulated visible-spectrum bar). Reads LMS2006SO; writes α + darkness pixels. Android keeps the legacy `draw_spectrum` in `compositing.rs` until the Android port lands.
#[cfg(not(target_os = "android"))]
pub mod chromatic_wave;

// Desktop "Photon" wordmark — port of legacy `compositing.rs::draw_logo_text` with glow + highlight + sharp body in α + darkness format. Oxanium 800.
#[cfg(not(target_os = "android"))]
pub mod photon_logo;

// Desktop Launch-screen layout calculator — proportional slicing port from legacy `app::Layout::new`.
#[cfg(not(target_os = "android"))]
pub mod launch_layout;

// Desktop Ready-screen layout calculator — slice-based port of legacy `app::ContactsUnifiedLayout`.
#[cfg(not(target_os = "android"))]
pub mod ready_layout;

pub use state::{AppState, FoundPeer, LaunchState, SearchResult};

// Android renderer survives until fluor grows host-android. Renderer trio for desktop (linux_softbuffer / linux_wgpu / windows / macos / macos_softbuffer / redox) is gated below the Android-only block since they were only consumed by the legacy `app::PhotonApp`.
#[cfg(target_os = "android")]
pub mod renderer_android;

#[cfg(all(target_os = "android"))]
pub use renderer_android as renderer;

#[cfg(target_os = "android")]
pub use app::PhotonApp;

// Desktop (everything except Android): the fluor-hosted `FluorApp` impl. Phase 0c scaffold renders chrome + blank inside; Phase 1+ rebuilds each screen as widgets.
#[cfg(not(target_os = "android"))]
pub mod photon_app;
#[cfg(not(target_os = "android"))]
pub use photon_app::PhotonApp;

/// Custom events for cross-thread communication with the event loop. Wired through `FluorApp::on_user_event` on desktop (background tasks clone the `EventLoopProxy<PhotonEvent>` from `PhotonApp::set_event_proxy` and call `send_event` to wake the UI thread); Android uses its own JNI bridge.
#[cfg(not(target_os = "android"))]
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
    /// Background CLUTCH ceremony completion (avalanche_expand) completed
    ClutchCeremonyComplete,
}
