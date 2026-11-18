// All platforms share the fluor-hosted UI stack: `photon_app::PhotonApp` runs under `fluor::host::android::AndroidShell` on Android and `fluor::host::app::run_app` on desktop.
// The legacy Android compositor (app / compositing / drawing / keyboard / mouse / text_editing / text_rasterizing / renderer_android) was deleted once fully retired — text measurement, editing, and rendering now live in fluor.

/// The pixel depth Photon renders at on this target: fluor's 16-bit `G2t16` on every host that takes it (since fluor e9cedec all of them: winit/softbuffer desktops, Android, ferros), each target its own line so one can go back to fluor's 8-bit `Argb8` alone.
/// Every buffer, cache and kernel of Photon's own paint is written against fluor's `Pixel` trait thru this name, so flipping a target is its one line here.
#[cfg(target_os = "ferros")]
pub type Px = fluor::pixel::G2t16;
/// See the ferros arm.
#[cfg(target_os = "android")]
pub type Px = fluor::pixel::G2t16;
/// See the ferros arm.
#[cfg(not(any(target_os = "ferros", target_os = "android")))]
pub type Px = fluor::pixel::G2t16;

/// fluor's canvas at Photon's depth: every `&mut Canvas` in Photon's paint code is a canvas of [`Px`].
pub type Canvas<'a> = fluor::canvas::Canvas<'a, Px>;

/// What Photon's paint needs of a depth beyond fluor's `Pixel`: colour in from the display's γ2 RGB, the tapuwae mark at the depth, and the bridge for a painter that only draws 8-bit (opsin's viewer).
pub mod depth;
pub use depth::Depth;

pub mod avatar;
/// Avatar sharing over scoped blobs — one ciphertext, a private slot per reader (docs/scoped-blobs.md). Replaces the bearer pin.
pub mod avatar_scoped;
pub mod display_profile;
pub mod lms2006so;
pub mod state;

/// THE SAFE CIRCLE (ferros, WINDOWS.md there, "Drawn under the controls"): the frame fills the glass and the system's controls float in front of its corners (the console's circles at the top, the helm's wedges at the bottom), so Photon keeps its own buttons and text inside this circle.
/// Centre and radius in the frame's pixels; the circle is wider than the glass, so it bounds the top and the bottom and leaves the sides to the frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SafeCircle {
    pub cx: f32,
    pub cy: f32,
    pub r: f32,
}

impl SafeCircle {
    /// The circle's top row, just under the camera on ferros's glass.
    pub fn top(&self) -> f32 {
        self.cy - self.r
    }

    /// The circle's bottom row, between the helm's Back and Deck.
    pub fn bottom(&self) -> f32 {
        self.cy + self.r
    }

    /// The display's own circle for a `w × h` frame before it tells one, as ferros_sys's `active_circle` makes it: tangent to the camera hole's bottom and to the glass's bottom, `r = (h − cutout) / 2`, centred on the width.
    /// The cutout's bottom is a measured fact of each panel (ferros_sys `CUTOUT_BOTTOM`, the Pixel 8 Pro at 1344 × 2992; `CUTOUT_BOTTOM_AKITA`, the Pixel 8a at 1080 × 2400); another panel takes the Pro's as a share of its height until its display tells it.
    pub fn of_panel(w: usize, h: usize) -> Self {
        const PRO: (usize, usize, f32) = (1344, 2992, 118.5610);
        const AKITA: (usize, usize, f32) = (1080, 2400, 101.8303);
        let cutout = if (w, h) == (AKITA.0, AKITA.1) { AKITA.2 } else { PRO.2 * h as f32 / PRO.1 as f32 };
        let r = (h as f32 - cutout) * 0.5;
        SafeCircle { cx: w as f32 * 0.5, cy: cutout + r, r }
    }
}

/// The safe circle as three f32 bit patterns, and whether one is set: written by the app thread when the host tells it (or the first frame guesses it), read by every layout on the same thread.
static SAFE_CIRCLE: [std::sync::atomic::AtomicU32; 3] = [const { std::sync::atomic::AtomicU32::new(0) }; 3];
static SAFE_CIRCLE_SET: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// The frame's safe circle, on a host that has one (ferros); None elsewhere, where the whole frame is Photon's.
/// Every ferros-only placement reads this, so a host that never tells one (the desktop, Android) lays out exactly as before.
pub fn safe_circle() -> Option<SafeCircle> {
    use std::sync::atomic::Ordering::Relaxed;
    SAFE_CIRCLE_SET.load(Relaxed).then(|| SafeCircle {
        cx: f32::from_bits(SAFE_CIRCLE[0].load(Relaxed)),
        cy: f32::from_bits(SAFE_CIRCLE[1].load(Relaxed)),
        r: f32::from_bits(SAFE_CIRCLE[2].load(Relaxed)),
    })
}

/// Set the frame's safe circle (the host's word, or the first frame's guess); returns whether it changed.
pub fn set_safe_circle(c: SafeCircle) -> bool {
    use std::sync::atomic::Ordering::Relaxed;
    if safe_circle() == Some(c) {
        return false;
    }
    SAFE_CIRCLE[0].store(c.cx.to_bits(), Relaxed);
    SAFE_CIRCLE[1].store(c.cy.to_bits(), Relaxed);
    SAFE_CIRCLE[2].store(c.r.to_bits(), Relaxed);
    SAFE_CIRCLE_SET.store(true, Relaxed);
    true
}

/// THE SAFE TOP (Nick 2026-09-17, "the top bit of Android is now the notification bar"): with the surface edge to edge, the status bar (or camera cutout) sits over the first N pixels of every screen. Every top-anchored layout starts its block here — the launch stack, the ready screen's avatar row, the settings header, the conversation's back arrow and call pill — and the chrome orb insets by the same amount. Zero everywhere but Android, where the Activity mirrors the OS inset.
pub fn safe_top_px() -> usize {
    #[cfg(target_os = "android")]
    {
        crate::platform::jni_android::top_inset_px().max(0) as usize // WHY/PROOF: the Kotlin inset, cast to usize — see jni_android
    }
    #[cfg(not(target_os = "android"))]
    {
        0
    }
}

/// THE SAFE BOTTOM (Nick 2026-09-30): the Android navigation bar (gesture pill or three buttons) over the last N rows. Zero everywhere but Android.
pub fn safe_bottom_px() -> usize {
    #[cfg(target_os = "android")]
    {
        crate::platform::jni_android::bottom_inset_px().max(0) as usize // WHY/PROOF: the Kotlin inset, cast to usize — see jni_android
    }
    #[cfg(not(target_os = "android"))]
    {
        0
    }
}

/// THE SAFE LEFT (Nick 2026-09-28, landscape cutouts): the camera cutout or a side nav bar over the first N columns. Zero everywhere but Android.
pub fn safe_left_px() -> usize {
    #[cfg(target_os = "android")]
    {
        crate::platform::jni_android::left_inset_px().max(0) as usize // WHY/PROOF: the Kotlin inset, cast to usize — see jni_android
    }
    #[cfg(not(target_os = "android"))]
    {
        0
    }
}

/// THE SAFE RIGHT: as [`safe_left_px`], the right edge.
pub fn safe_right_px() -> usize {
    #[cfg(target_os = "android")]
    {
        crate::platform::jni_android::right_inset_px().max(0) as usize // WHY/PROOF: as above
    }
    #[cfg(not(target_os = "android"))]
    {
        0
    }
}

/// THE SAFE SIDE: the larger of the left and right insets, for layouts whose one horizontal padding serves both edges (a landscape phone turned either way puts its camera on one side or the other).
pub fn safe_side_px() -> usize {
    #[cfg(target_os = "android")]
    {
        crate::platform::jni_android::left_inset_px().max(crate::platform::jni_android::right_inset_px()).max(0) as usize // WHY/PROOF: as above
    }
    #[cfg(not(target_os = "android"))]
    {
        0
    }
}

// Chromatic wave (sine-modulated visible-spectrum bar). Reads LMS2006SO; writes α + darkness pixels.
pub mod chromatic_wave;

// "Photon" wordmark — port of legacy `compositing.rs::draw_logo_text` with glow + highlight + sharp body in α + darkness format. Oxanium 800.
pub mod photon_logo;

// Launch-screen layout calculator — proportional slicing port from legacy `app::Layout::new`.
pub mod launch_layout;

// Ready-screen layout calculator — slice-based port of legacy `app::ContactsUnifiedLayout`.
pub mod ready_layout;

// Photon round the safe circle (ferros): the orb at its top under the camera, Wave and Beam on its shoulders.
pub mod safe_layout;

// VSF RGB → BT.2020 RGB conversion for display output on Android (γ=2.0 end-to-end).
pub mod colour_convert;

// Avatar paint — Mitchell resize + AA textured circle into a fluor `Canvas`.
pub mod avatar_render;
/// Attachment kind sniff + micro preview minting, off the UI thread (typed attachments 2026-09-10).
pub mod attach_preview;

// The incoming-wave rim — the contact's bell casting drawn as an undulating Fourier contour around the avatar.
pub mod ring_rim;

pub use state::{AppState, FoundPeer, LaunchState, SearchResult, SettingsPage};


// Settings-panel layout calculator — nav-rail-vs-content split and stacked content rows via fluor's `Region`.
pub mod settings_layout;

// Photon-specific UI colour palette (the app's own colours, mirroring `fluor::theme`).
pub mod theme;

// The language catalog — every user-facing string is a `lang::Msg` variant, each language an exhaustive match (see lang.rs doctrine).
pub mod lang;
/// Release notes compiled in from RELEASE_NOTES.md (the Updates page's "what's new").
pub mod release_notes;
/// The bundled font set — one loader shared by the app and the font-contract KAT.
pub mod fonts;

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
    /// A full-UI launch asks the LIFELINE instance to release the single-instance lock (docs/headless-lifeline.md): the lifeline exits, the full instance takes over. A full-UI instance receiving this treats it as ShowWindow (it isn't yielding to anyone).
    Yield,
}
