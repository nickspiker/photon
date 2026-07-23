//! Photon-specific UI colours — AUTHORED in VSF RGB, resolved to the platform's display target once (LazyLock) at first use.
//!
//! Colour doctrine (all platforms): assume a WIDE-GAMUT panel and tag the surface as BT.2020, γ≈2 — then ship values in the surface's declared space. Never pessimise to sRGB: an untagged panel is on some vendor default that's wonked anyway, and a user who wants real calibration supplies an ICC (which overrides the assumption). The transfer is always **sqrt (γ2)** — never the piecewise sRGB OETF, which costs 50-100 cycles/pixel to the sqrt's 2-4.
//!
//! [`to_display`] therefore has two shapes, both γ2-in / γ2-out:
//! - **macOS** (surface tagged with the full VSF-RGB ICC via `renderer_wgpu`'s `setColorspace:`): the surface IS VSF RGB, so ship RAW — identity, no matrix, no re-encode. CoreGraphics colour-manages VSF→panel itself.
//! - **Android + Linux/Windows** (BT.2020-tagged, or assumed so): VSF primaries → Rec.2020 primaries (`VSF_RGB2REC2020`, E→D65 baked in), sqrt transfer. Android tags γ2.2 and eats a slight darkening (no `TRANSFER_GAMMA2_0` constant exists); the pixels stay γ2.0.
//!
//! After conversion the value takes fluor's α+darkness storage via `dark(fmt(..))` — `fmt` is identity on desktop and an R↔B swap on Android; `dark` flips visible RGB → darkness and sets α=0xFF.
//! Use sites deref (`*theme::NAME`) — the statics resolve once and read like the old consts.
//! The `_COLOUR` values with a bare `0xAA_00_00_00` are α-only watermark tints (pure white at partial opacity) — white is gamut-invariant, so no conversion, still plain consts.
//! Linux refinement to come: poll the panel's ICC (X11 `_ICC_PROFILE` / colord; Wayland color-management-v1 once winit exposes it) and convert to the real profile instead of the BT.2020 assumption.

use std::sync::LazyLock;

/// VSF-RGB visible hex → the platform display target, still visible-RGB hex (α byte passes thru).
fn to_display(hex: u32) -> u32 {
    // macOS: the surface is ICC-tagged VSF RGB (renderer_wgpu), so the authored value IS the display value — ship raw.
    #[cfg(target_os = "macos")]
    {
        hex
    }
    // Android + Linux/Windows: convert VSF → Rec.2020 primaries and sqrt-encode into the (assumed / tagged) BT.2020 γ2 surface.
    #[cfg(not(target_os = "macos"))]
    {
        let r = ((hex >> 16) & 0xFF) as f32 / 255.0;
        let g = ((hex >> 8) & 0xFF) as f32 / 255.0;
        let b = (hex & 0xFF) as f32 / 255.0;
        let lin = [r * r, g * g, b * b]; // γ=2.0 authoring transfer (decode)
        let out = vsf::colour::convert::apply_matrix_3x3_f32(&vsf::colour::VSF_RGB2REC2020, &lin);
        let e = |x: f32| (x.clamp(0.0, 1.0).sqrt() * 255.0).round().clamp(0.0, 255.0) as u32; // γ2 encode — sqrt, never sRGB OETF
        (hex & 0xFF00_0000) | (e(out[0]) << 16) | (e(out[1]) << 8) | e(out[2])
    }
}

/// The full authored→stored pipeline: display conversion, then fluor's byte order + darkness flip.
fn c(hex: u32) -> u32 {
    fluor::theme::dark(fluor::theme::fmt(to_display(hex)))
}

/// Error-state message colour for the Launch screen's error slot — bright red, fully opaque.
pub static ERROR_TEXT_COLOUR: LazyLock<u32> = LazyLock::new(|| c(0x00_FF_50_50));

/// Colour for the dozenal version glyphs at the bottom of the screen: pure white (darkness 0 across all channels), α = 32 = 1/8 opacity. `draw_text_center_u32` multiplies the glyph coverage into this α, so the version reads as a faint watermark over the background noise.
pub const VERSION_COLOUR: u32 = 0x20_00_00_00;

/// Colour for the zoom-percentage watermark at the top of the screen: pure white, α = 64 = 1/4 opacity (twice [`VERSION_COLOUR`]'s 1/8). Painted before the background noise so it reads as a faint top-centre indicator of the current `ru` zoom factor.
pub const ZOOM_COLOUR: u32 = 0x40_00_00_00;

/// Contact name text on the Ready list — near-white.
pub static CONTACT_NAME_COLOUR: LazyLock<u32> = LazyLock::new(|| c(0x00_F0_F0_F0));
/// Hairline separating the user section from the contact list — pure white at 1/4 opacity (α=64), the same translucent treatment as the hints + zoom watermark.
pub const SEPARATOR_COLOUR: u32 = 0x40_00_00_00;

/// Presence-ring tiers (user spec, VSF RGB): how you are connected, at a glance —
/// cyan = direct in the same room (LAN), green = direct across the WAN, amber = relay-only (never mistakable for direct), grey = offline.
pub static RING_LAN_COLOUR: LazyLock<u32> = LazyLock::new(|| c(0x00_00_FF_FF));
pub static RING_ONLINE_COLOUR: LazyLock<u32> = LazyLock::new(|| c(0x00_00_FF_00));
pub static RING_OFFLINE_COLOUR: LazyLock<u32> = LazyLock::new(|| c(0x00_28_28_28));
/// 0xFFB000 amber — the long-standing 0xB0FF00 lime was this value with its bytes swapped, never a deliberate lime.
pub static RING_RELAY_COLOUR: LazyLock<u32> = LazyLock::new(|| c(0x00_FF_B0_00));
pub static SEARCH_RELAY_COLOUR: LazyLock<u32> = LazyLock::new(|| c(0x00_FF_B0_00));
/// Add-friend result text + the in-flight hourglass: green on success, red on not-found/error.
pub static SEARCH_FOUND_COLOUR: LazyLock<u32> = LazyLock::new(|| c(0x00_40_E0_40));
pub static SEARCH_FAIL_COLOUR: LazyLock<u32> = LazyLock::new(|| c(0x00_E0_40_40));
/// Hourglass tint while the search is in flight (orange).
pub static HOURGLASS_COLOUR: LazyLock<u32> = LazyLock::new(|| c(0x00_FF_A5_00));

/// Security-page destructiveness ramp — pill fills `(idle, held)`, least to most destructive: green (Lock: reversible by re-typing the handle) → yellow (fleet self-removal) → orange (Shred: wipe this device) → red (Remove & shred: sign out of the fleet AND wipe). Same luminance discipline as BUTTON_FILL/HELD (dark idle, ~2× brighter held).
pub static PILL_GREEN: LazyLock<(u32, u32)> =
    LazyLock::new(|| (c(0x00_14_3C_1C), c(0x00_2E_88_40)));
pub static PILL_YELLOW: LazyLock<(u32, u32)> =
    LazyLock::new(|| (c(0x00_3E_38_10), c(0x00_8C_7E_26)));
pub static PILL_ORANGE: LazyLock<(u32, u32)> =
    LazyLock::new(|| (c(0x00_48_2A_0E), c(0x00_A0_5E_22)));
/// JOINER SELECTED flood — the whole-surface green a just-bound device shows while its sponsor confirms (docs/lifecycle.md). Opaque takeover, like the red.
pub static SELECTED_FLOOD: LazyLock<u32> = LazyLock::new(|| c(0x00_08_38_12));
/// LAST RITES flood — the whole-surface deep red the final-exit interstitial paints under its text (docs/lifecycle.md D3). Opaque: this is a takeover screen, not a tint.
pub static LASTRITES_FLOOD: LazyLock<u32> = LazyLock::new(|| c(0x00_30_06_06));
pub static PILL_RED: LazyLock<(u32, u32)> = LazyLock::new(|| (c(0x00_4E_14_14), c(0x00_AC_2E_2E)));
/// Updates page: amber (latest dev — matches the dev build's amber theme) + inert dark grey ("already on this version" — present but not an action).
pub static PILL_AMBER: LazyLock<(u32, u32)> =
    LazyLock::new(|| (c(0x00_44_30_08), c(0x00_C0_88_18)));
pub static PILL_GREY: LazyLock<(u32, u32)> = LazyLock::new(|| (c(0x00_24_24_28), c(0x00_24_24_28)));
/// Updates-page download bar: lime progress over a black track. The fill paints FIRST (under-blend, first-wins) and the track sweeps the remainder.
pub static PROGRESS_FILL: LazyLock<u32> = LazyLock::new(|| c(0x00_80_FF_00));
pub static PROGRESS_TRACK: LazyLock<u32> = LazyLock::new(|| c(0x00_00_00_00));
/// Send-button arrowhead glyph — light grey.
pub static SEND_ARROW_COLOUR: LazyLock<u32> = LazyLock::new(|| c(0x00_D0_D0_D0));
/// Hover fill for the send / plus action buttons — a SUBTLE neutral brightening of BUTTON_FILL (0x1A224E), reproducing the pre-fluor QUERY_BUTTON_HOVER feel rather than the shared BUTTON_HOVER's saturated-blue shift. A small delta also keeps the overlay from cooking the near-white arrowhead.
pub static SEND_BUTTON_HOVER: LazyLock<u32> = LazyLock::new(|| c(0x00_25_2D_59));
/// Noise-background base tint when the dual-ring vault flagged this session degraded — warning orange.
/// This is a NOISE-MATH colour (visible-RGB space, like fluor's `BG_BASE`), so `fmt` not `dark`; passed to `background_noise` in place of its default base.
pub static BG_BASE_WARNING: LazyLock<u32> =
    LazyLock::new(|| fluor::theme::fmt(to_display(0x00_30_10_00)));
/// Thin white rule between conversation messages.
pub static DIVIDER_COLOUR: LazyLock<u32> = LazyLock::new(|| c(0x00_FF_FF_FF));
/// Dim grey for the compose-box placeholder text.
pub static LABEL_COLOUR: LazyLock<u32> = LazyLock::new(|| c(0x00_80_80_80));

/// Filled-pip colours by level — warm orange (low) → amber (mid) → green (high); empty pips use [`POSTURE_OFF_COLOUR`].
pub static POSTURE_LOW_COLOUR: LazyLock<u32> = LazyLock::new(|| c(0x00_E0_70_30));
pub static POSTURE_MID_COLOUR: LazyLock<u32> = LazyLock::new(|| c(0x00_E0_C0_30));
pub static POSTURE_HIGH_COLOUR: LazyLock<u32> = LazyLock::new(|| c(0x00_40_E0_40));
pub static POSTURE_OFF_COLOUR: LazyLock<u32> = LazyLock::new(|| c(0x00_40_40_40));

/// Status-message colour for the "Attesting…" indicator that occupies the error slot while a handle query is in flight. Pure visible white, fully opaque — same slot as [`ERROR_TEXT_COLOUR`] but white instead of red so the user reads it as "neutral status" rather than "something went wrong".
pub static STATUS_TEXT_COLOUR: LazyLock<u32> = LazyLock::new(|| c(0x00_FF_FF_FF));
