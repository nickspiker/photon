pub mod app;
pub mod avatar;
mod colour;
mod compositing;
pub mod display_profile;
pub mod drawing;
#[cfg(not(target_os = "android"))]
mod keyboard;
#[cfg(not(target_os = "android"))]
mod mouse;
mod text_editing;
mod text_rasterizing;

#[cfg(target_os = "windows")]
mod renderer_windows;

#[cfg(any(target_os = "linux", target_os = "redox"))]
mod renderer_linux;

#[cfg(target_os = "macos")]
mod renderer_macos;

#[cfg(target_os = "android")]
pub mod renderer_android;

#[cfg(target_os = "windows")]
use renderer_windows as renderer;

#[cfg(any(target_os = "linux", target_os = "redox"))]
use renderer_linux as renderer;

#[cfg(target_os = "macos")]
use renderer_macos as renderer;

#[cfg(target_os = "android")]
pub use renderer_android as renderer;

pub mod theme;

pub use app::{AppState, LaunchState, PhotonApp};

/// Custom events for cross-thread communication with the event loop
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
