//! Photon-specific UI colours.
//!
//! The app-level palette, in the same spirit as [`fluor::theme`] but for colours that are Photon's
//! own rather than fluor widget defaults. Everything here is stored in fluor's α+darkness format
//! (via `fluor::theme::dark(fluor::theme::fmt(visible_argb))`) unless noted — `fmt` is identity on
//! desktop and an R↔B swap on Android (RGBA_8888 byte order); `dark` flips visible RGB → darkness
//! and sets α=0xFF. The `_COLOUR` values with a bare `0xAA_00_00_00` are α-only watermark tints
//! (pure white at partial opacity), also already in α+darkness format.

/// Error-state message colour for the Launch screen's error slot — visible RGB (255, 80, 80), bright red, fully opaque.
pub const ERROR_TEXT_COLOUR: u32 = fluor::theme::dark(fluor::theme::fmt(0x00_FF_50_50));

/// Colour for the dozenal version glyphs at the bottom of the screen: pure white (darkness 0 across all channels), α = 32 = 1/8 opacity. `draw_text_center_u32` multiplies the glyph coverage into this α, so the version reads as a faint watermark over the background noise.
pub const VERSION_COLOUR: u32 = 0x20_00_00_00;

/// Colour for the zoom-percentage watermark at the top of the screen: pure white, α = 64 = 1/4 opacity (twice [`VERSION_COLOUR`]'s 1/8). Painted before the background noise so it reads as a faint top-centre indicator of the current `ru` zoom factor.
pub const ZOOM_COLOUR: u32 = 0x40_00_00_00;

/// Contact name text on the Ready list — near-white. α+darkness (the format fluor's text/shape rasterizers expect — visible-RGB is not interchangeable here).
pub const CONTACT_NAME_COLOUR: u32 = fluor::theme::dark(fluor::theme::fmt(0x00_F0_F0_F0));
/// Hairline separating the user section from the contact list — pure white at 1/4 opacity (α=64), the same translucent treatment as the hints + zoom watermark.
pub const SEPARATOR_COLOUR: u32 = 0x40_00_00_00;
/// Contact presence ring around a row avatar: green online, grey offline. α+darkness.
pub const RING_ONLINE_COLOUR: u32 = fluor::theme::dark(fluor::theme::fmt(0x00_00_C0_00));
pub const RING_OFFLINE_COLOUR: u32 = fluor::theme::dark(fluor::theme::fmt(0x00_28_28_28));
/// Presence when reachable ONLY via the FGTW relay (no direct path — the asymmetric-reachability case: one peer IPv6-only, the other IPv4-only behind symmetric NAT). Lime-yellow (0xB0FF00), deliberately NOT the direct-connection green, so a relayed link never masquerades as a direct one. α+darkness, same discipline as the online/offline rings.
pub const RING_RELAY_COLOUR: u32 = fluor::theme::dark(fluor::theme::fmt(0x00_B0_FF_00));
pub const SEARCH_RELAY_COLOUR: u32 = fluor::theme::dark(fluor::theme::fmt(0x00_B0_FF_00));
/// Add-friend result text + the in-flight hourglass: green on success, red on not-found/error. α+darkness.
pub const SEARCH_FOUND_COLOUR: u32 = fluor::theme::dark(fluor::theme::fmt(0x00_40_E0_40));
pub const SEARCH_FAIL_COLOUR: u32 = fluor::theme::dark(fluor::theme::fmt(0x00_E0_40_40));
/// Hourglass tint while the search is in flight (orange). α+darkness.
pub const HOURGLASS_COLOUR: u32 = fluor::theme::dark(fluor::theme::fmt(0x00_FF_A5_00));

/// Security-page destructiveness ramp — pill fills `(idle, held)`, least to most destructive: green (Lock: reversible by re-typing the handle) → yellow (fleet self-removal) → orange (Shred: wipe this device) → red (Remove & shred: sign out of the fleet AND wipe). Same luminance discipline as BUTTON_FILL/HELD (dark idle, ~2× brighter held).
pub const PILL_GREEN: (u32, u32) = (
    fluor::theme::dark(fluor::theme::fmt(0x00_14_3C_1C)),
    fluor::theme::dark(fluor::theme::fmt(0x00_2E_88_40)),
);
pub const PILL_YELLOW: (u32, u32) = (
    fluor::theme::dark(fluor::theme::fmt(0x00_3E_38_10)),
    fluor::theme::dark(fluor::theme::fmt(0x00_8C_7E_26)),
);
pub const PILL_ORANGE: (u32, u32) = (
    fluor::theme::dark(fluor::theme::fmt(0x00_48_2A_0E)),
    fluor::theme::dark(fluor::theme::fmt(0x00_A0_5E_22)),
);
/// JOINER SELECTED flood — the whole-surface green a just-bound device shows while its sponsor confirms (docs/lifecycle.md). Opaque takeover, like the red.
pub const SELECTED_FLOOD: u32 = fluor::theme::dark(fluor::theme::fmt(0x00_08_38_12));
/// LAST RITES flood — the whole-surface deep red the final-exit interstitial paints under its text (docs/lifecycle.md D3). Opaque: this is a takeover screen, not a tint.
pub const LASTRITES_FLOOD: u32 = fluor::theme::dark(fluor::theme::fmt(0x00_30_06_06));
pub const PILL_RED: (u32, u32) = (
    fluor::theme::dark(fluor::theme::fmt(0x00_4E_14_14)),
    fluor::theme::dark(fluor::theme::fmt(0x00_AC_2E_2E)),
);
/// Updates page: amber (latest dev — matches the dev build's amber theme) + inert dark grey ("already on this version" — present but not an action).
pub const PILL_AMBER: (u32, u32) = (
    fluor::theme::dark(fluor::theme::fmt(0x00_44_30_08)),
    fluor::theme::dark(fluor::theme::fmt(0x00_C0_88_18)),
);
pub const PILL_GREY: (u32, u32) = (
    fluor::theme::dark(fluor::theme::fmt(0x00_24_24_28)),
    fluor::theme::dark(fluor::theme::fmt(0x00_24_24_28)),
);
/// Updates-page download bar: lime progress over a black track. α+darkness like the pills; the fill paints FIRST (under-blend, first-wins) and the track sweeps the remainder.
pub const PROGRESS_FILL: u32 = fluor::theme::dark(fluor::theme::fmt(0x00_80_FF_00));
pub const PROGRESS_TRACK: u32 = fluor::theme::dark(fluor::theme::fmt(0x00_00_00_00));
/// Send-button arrowhead glyph — light grey, α+darkness format for under-blend (visible-RGB is not usable on this canvas).
pub const SEND_ARROW_COLOUR: u32 = fluor::theme::dark(fluor::theme::fmt(0x00_D0_D0_D0));
/// Hover fill for the send / plus action buttons — a SUBTLE neutral brightening of BUTTON_FILL (0x1A224E), reproducing the pre-fluor QUERY_BUTTON_HOVER feel rather than the shared BUTTON_HOVER's saturated-blue shift. A small delta also keeps the overlay from cooking the near-white arrowhead.
pub const SEND_BUTTON_HOVER: u32 = fluor::theme::dark(fluor::theme::fmt(0x00_25_2D_59));
/// Noise-background base tint when the dual-ring vault flagged this session degraded — warning orange.
/// This is a NOISE-MATH colour (visible-RGB space, like fluor's `BG_BASE`), so `fmt` not `dark`; passed
/// to `background_noise` in place of its default base.
pub const BG_BASE_WARNING: u32 = fluor::theme::fmt(0x00_30_10_00);
/// Thin white rule between conversation messages. α+darkness.
pub const DIVIDER_COLOUR: u32 = fluor::theme::dark(fluor::theme::fmt(0x00_FF_FF_FF));
/// Dim grey for the compose-box placeholder text. α+darkness.
pub const LABEL_COLOUR: u32 = fluor::theme::dark(fluor::theme::fmt(0x00_80_80_80));

/// Filled-pip colours by level — warm orange (low) → amber (mid) → green (high); empty pips use [`POSTURE_OFF_COLOUR`]. α+darkness format (opaque), the space the shape rasterizers expect.
pub const POSTURE_LOW_COLOUR: u32 = fluor::theme::dark(fluor::theme::fmt(0x00_E0_70_30));
pub const POSTURE_MID_COLOUR: u32 = fluor::theme::dark(fluor::theme::fmt(0x00_E0_C0_30));
pub const POSTURE_HIGH_COLOUR: u32 = fluor::theme::dark(fluor::theme::fmt(0x00_40_E0_40));
pub const POSTURE_OFF_COLOUR: u32 = fluor::theme::dark(fluor::theme::fmt(0x00_40_40_40));

/// Status-message colour for the "Attesting…" indicator that occupies the error slot while a handle query is in flight. Pure visible white, fully opaque — same slot as [`ERROR_TEXT_COLOUR`] but white instead of red so the user reads it as "neutral status" rather than "something went wrong".
pub const STATUS_TEXT_COLOUR: u32 = fluor::theme::dark(fluor::theme::fmt(0x00_FF_FF_FF));
