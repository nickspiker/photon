//! Photon-specific UI colours — AUTHORED in VSF RGB `g2t` (Nick 2026-10-20: UI colours are VSF RGB g2t, step 1 of an HDR stack), resolved to the platform's display target once (LazyLock) at first use.
//!
//! A `g2t` code is light = atanh(code / 256)², bucket bottom: diffuse white is 0xC2 (194), and the codes above it are headroom past white, which the γ2 buffer clips until the HDR plan's later steps give it somewhere to go.
//! The conversion is fluor's ([`fluor::theme::vsf`]), the one every fluor constant takes too: integer only, the current colorimetric matrix (no chromatic adaptation since vsf 02076e6).
//! macOS ships VSF RGB as is (its surface is ICC-tagged VSF RGB and the compositor converts); everywhere else VSF RGB → Rec.2020, written γ2 into the BT.2020-tagged (or assumed) surface.
//! Never pessimise to sRGB: an untagged panel is on some vendor default anyway, and a user who wants real calibration supplies an ICC.
//!
//! After conversion the value takes fluor's α+darkness storage via `dark(fmt(..))` — `fmt` is identity on desktop and an R↔B swap on Android; `dark` flips visible RGB → darkness and sets α=0xFF.
//! Use sites deref (`*theme::NAME`) — the statics resolve once and read like the old consts.
//! The α-only watermark tints (version, zoom, separator, rail) are diffuse white at partial opacity thru `fluor::theme::white_at` — never a bare darkness 0, which since the `g2t` buffer is the top code, 9.7× white, a glow on an HDR surface.
//! Linux refinement to come: poll the panel's ICC (X11 `_ICC_PROFILE` / colord; Wayland color-management-v1 once winit exposes it) and convert to the real profile instead of the BT.2020 assumption.

use std::sync::LazyLock;

/// The full authored→stored pipeline: fluor's display conversion (`g2t` → the surface's primaries and γ2 buffer code), then its byte order + darkness flip.
fn c(hex: u32) -> u32 {
    fluor::theme::dark(fluor::theme::fmt(fluor::theme::vsf(hex)))
}

/// One γ2 code (0..=255, 255 = white) → the `g2t` code holding the same light, by VSF's truncation rule (the bucket the light falls in): for runtime colours computed on the γ2 scale.
fn g2t_from_gamma2(v: u8) -> u32 {
    let v = v as u32;
    fluor::theme::g2t_code((v * v * 65536 + 65025 / 2) / 65025)
}

/// Error-state message colour for the Launch screen's error slot — bright red, fully opaque.
pub static ERROR_TEXT_COLOUR: LazyLock<u32> = LazyLock::new(|| c(0x00_C2_4D_4D));
/// The fullscreen viewer/reader backdrop — near-black, authored VSF like everything else (it was a raw darkness literal bypassing the palette, one of the 2026-09-12 colour crimes).
pub static VIEWER_BG_COLOUR: LazyLock<u32> = LazyLock::new(|| c(0x00_0B_0B_0B));
/// The link-consent panel backdrop — dark blue-grey, formerly the raw darkness literal 0xE8E2D8.
pub static CONSENT_BG_COLOUR: LazyLock<u32> = LazyLock::new(|| c(0x00_17_1D_26));

/// Colour for the dozenal version glyphs at the bottom of the screen: diffuse white (`fluor::theme::white_at`, never darkness 0, the 9.7× top code), α = 32 = 1/8 opacity. `draw_text_center_u32` multiplies the glyph coverage into this α, so the version reads as a faint watermark over the background noise.
pub const VERSION_COLOUR: u32 = fluor::theme::white_at(0x20);

/// Colour for the zoom-percentage watermark at the top of the screen: pure white, α = 64 = 1/4 opacity (twice [`VERSION_COLOUR`]'s 1/8). Painted before the background noise so it reads as a faint top-centre indicator of the current `ru` zoom factor.
pub const ZOOM_COLOUR: u32 = fluor::theme::white_at(0x40);

/// Contact name text on the Ready list — near-white.
pub static CONTACT_NAME_COLOUR: LazyLock<u32> = LazyLock::new(|| c(0x00_BC_BC_BC));
/// Hairline separating the user section from the contact list — pure white at 1/4 opacity (α=64), the same translucent treatment as the hints + zoom watermark.
pub const SEPARATOR_COLOUR: u32 = fluor::theme::white_at(0x40);
/// Settings-rail ACTIVE row hint — half the separator's opacity (α=32 = 1/8): the full-column bar behind the selected page name (Nick 2026-09-02: the α=64 version read too bright).
pub const RAIL_ACTIVE_COLOUR: u32 = fluor::theme::white_at(0x20);

/// Presence-ring tiers (user spec, VSF RGB): how you are connected, at a glance — cyan = direct in the same room (LAN), green = direct across the WAN, amber = relay-only (never mistakable for direct), grey = offline.
pub static RING_LAN_COLOUR: LazyLock<u32> = LazyLock::new(|| c(0x00_00_C2_C2));
/// Wi-Fi Direct: radio-direct, zero infrastructure — the VSF AGB primary blue (the 462nm monochromatic blue primary, authored as pure B in VSF RGB; user spec 2026-08-31).
pub static RING_WFD_COLOUR: LazyLock<u32> = LazyLock::new(|| c(0x00_00_00_C2));
pub static RING_ONLINE_COLOUR: LazyLock<u32> = LazyLock::new(|| c(0x00_00_C2_00));
/// Offline = PERFECT BLACK (Nick 2026-09-02: the grey ring interfered with the avatar visually — offline should read as the ring's ABSENCE, not a competing grey). One constant, every ring/dot site inherits via conn_tier_colour.
pub static RING_OFFLINE_COLOUR: LazyLock<u32> = LazyLock::new(|| c(0x00_00_00_00));
/// 0xFFB000 amber — the long-standing 0xB0FF00 lime was this value with its bytes swapped, never a deliberate lime.
pub static RING_RELAY_COLOUR: LazyLock<u32> = LazyLock::new(|| c(0x00_C2_99_00));
/// Unread cue: a magenta ring on the OUTSIDE of the presence ring (a new-message badge that never competes with the connectivity tiers). Event-shown, cleared on conversation-open.
pub static RING_UNREAD_COLOUR: LazyLock<u32> = LazyLock::new(|| c(0x00_C2_00_C2));
/// The message-details copy pill, ready state — cyan ("tap me"); flips to SEARCH_FOUND green + the label "copied" once the text lands on the clipboard. Event-cleared: reverts when the selection moves/closes, never on a timer.
pub static COPY_PILL_COLOUR: LazyLock<u32> = LazyLock::new(|| c(0x00_00_C2_C2));
pub static SEARCH_RELAY_COLOUR: LazyLock<u32> = LazyLock::new(|| c(0x00_C2_99_00));
/// Add-friend result text + the in-flight hourglass: green on success, red on not-found/error.
pub static SEARCH_FOUND_COLOUR: LazyLock<u32> = LazyLock::new(|| c(0x00_3E_B4_3E));
pub static SEARCH_FAIL_COLOUR: LazyLock<u32> = LazyLock::new(|| c(0x00_B4_3E_3E));
/// Hyperlink text — the primary VSF blue (the 462nm monochromatic AGB primary, authored as pure B in VSF RGB; same primary the WFD ring wears).
pub static LINK_COLOUR: LazyLock<u32> = LazyLock::new(|| c(0x00_00_00_C2));
/// Message links (Nick 2026-09-09): purple, in the compose box, on the link button, on the caret while typing a link, and bold in every bubble.
pub static LINK_PURPLE: LazyLock<u32> = LazyLock::new(|| c(0x00_8E_4D_C2));
/// The unticked dozenal checkbox's shame fill — Zil.lun red (half-intensity, dozenal 0;6): red enough to disapprove, dark enough to stay a box.
/// Transport tier of a live path, shown as a dot beside a device or contact — SAME semantics as the presence ring above (one language: cyan = LAN/same room, green = direct WAN, orange = relay). The dot and ring shipped INVERTED for weeks (dot green=LAN/cyan=WAN) and the field caught it as "fleet page cyan, bridge green, same peer, same moment" (2026-08-30).
pub static PATH_LAN_COLOUR: LazyLock<u32> = LazyLock::new(|| c(0x00_00_AC_B4));
pub static PATH_WAN_COLOUR: LazyLock<u32> = LazyLock::new(|| c(0x00_3E_B4_3E));
pub static PATH_RELAY_COLOUR: LazyLock<u32> = LazyLock::new(|| c(0x00_C2_91_00));

/// Hourglass tint while the search is in flight (orange).
pub static HOURGLASS_COLOUR: LazyLock<u32> = LazyLock::new(|| c(0x00_C2_91_00));

/// Security-page destructiveness ramp — pill fills `(idle, held)`, least to most destructive: green (Lock: reversible by re-typing the handle) → yellow (fleet self-removal) → orange (Shred: wipe this device) → red (Remove & shred: sign out of the fleet AND wipe). Same luminance discipline as BUTTON_FILL/HELD (dark idle, ~2× brighter held).
pub static PILL_GREEN: LazyLock<(u32, u32)> =
    LazyLock::new(|| (c(0x00_14_3B_1C), c(0x00_2D_7C_3E)));
pub static PILL_YELLOW: LazyLock<(u32, u32)> =
    LazyLock::new(|| (c(0x00_3D_37_10), c(0x00_7F_75_25)));
pub static PILL_ORANGE: LazyLock<(u32, u32)> =
    LazyLock::new(|| (c(0x00_46_29_0E), c(0x00_8E_5A_21)));
/// JOINER SELECTED flood — the whole-surface green a just-bound device shows while its sponsor confirms (docs/lifecycle.md). Opaque takeover, like the red.
pub static SELECTED_FLOOD: LazyLock<u32> = LazyLock::new(|| c(0x00_08_37_12));
/// LAST RITES flood — the whole-surface deep red the final-exit interstitial paints under its text (docs/lifecycle.md D3). Opaque: this is a takeover screen, not a tint.
pub static LASTRITES_FLOOD: LazyLock<u32> = LazyLock::new(|| c(0x00_2F_06_06));
pub static PILL_RED: LazyLock<(u32, u32)> = LazyLock::new(|| (c(0x00_4B_14_14), c(0x00_96_2D_2D)));
/// Updates page: amber (latest dev — matches the dev build's amber theme) + inert dark grey ("already on this version" — present but not an action).
pub static PILL_AMBER: LazyLock<(u32, u32)> =
    LazyLock::new(|| (c(0x00_42_2F_08), c(0x00_A3_7C_18)));
pub static PILL_GREY: LazyLock<(u32, u32)> = LazyLock::new(|| (c(0x00_23_23_27), c(0x00_23_23_27)));
/// Updates-page download bar: lime progress over a black track. The fill paints FIRST (under-blend, first-wins) and the track sweeps the remainder.
pub static PROGRESS_FILL: LazyLock<u32> = LazyLock::new(|| c(0x00_76_C2_00));
pub static PROGRESS_TRACK: LazyLock<u32> = LazyLock::new(|| c(0x00_00_00_00));
/// A file row whose transfer finished this session: a translucent green veil under the whole row (Nick 2026-10-07). Stored α + darkness in the surface's byte order — visible green (0, 255, 0) is darkness (255, 0, 255), at α 0x30.
pub static ATTACH_DONE_TINT: LazyLock<u32> = LazyLock::new(|| fluor::theme::fmt(fluor::theme::darkness_from_gamma2(0x30_FF_00_FF)));
/// Send-button arrowhead glyph — light grey.
/// The send arrowhead — GREEN over the purple pill (Nick 2026-09-12); the edit check keeps its own green by shape.
pub static SEND_ARROW_COLOUR: LazyLock<u32> = LazyLock::new(|| c(0x00_2F_C2_2F));
/// Hover fill for the send / plus action buttons — a SUBTLE neutral brightening of BUTTON_FILL (0x1A224E), reproducing the pre-fluor QUERY_BUTTON_HOVER feel rather than the shared BUTTON_HOVER's saturated-blue shift. A small delta also keeps the overlay from cooking the near-white arrowhead.
pub static SEND_BUTTON_HOVER: LazyLock<u32> = LazyLock::new(|| c(0x00_24_2C_55));
/// Wave buttons wear the traffic light (Nick 2026-09-08): answer/keep = green pill, decline/delete/end = red pill. Dark-pill fills + hover/held ramps mirroring SEND_BUTTON's structure, not the bright text greens/reds.
pub static WAVE_ACCEPT_FILL: LazyLock<u32> = LazyLock::new(|| c(0x00_1A_4B_21));
pub static WAVE_ACCEPT_HOVER: LazyLock<u32> = LazyLock::new(|| c(0x00_24_55_2C));
pub static WAVE_DANGER_FILL: LazyLock<u32> = LazyLock::new(|| c(0x00_4B_1A_1A));
pub static WAVE_DANGER_HOVER: LazyLock<u32> = LazyLock::new(|| c(0x00_55_24_24));
/// Noise-background base tint when the dual-ring vault flagged this session degraded — warning orange.
/// This is a NOISE-MATH colour (visible-RGB space, like fluor's `BG_BASE`), so `fmt` not `dark`; passed to `background_noise` in place of its default base.
pub static BG_BASE_WARNING: LazyLock<u32> =
    LazyLock::new(|| fluor::theme::fmt(fluor::theme::vsf(0x00_2F_10_00)));
/// Thin white rule between conversation messages.
pub static DIVIDER_COLOUR: LazyLock<u32> = LazyLock::new(|| c(0x00_C2_C2_C2));
/// Attachment resample overlay: the near-black scene wash behind the card.
pub static OVERLAY_DIM: LazyLock<u32> = LazyLock::new(|| c(0x00_08_08_0A));
/// Attachment resample overlay: the card panel fill.
pub static CARD_BG: LazyLock<u32> = LazyLock::new(|| c(0x00_16_1A_1F));
/// Dim grey for the compose-box placeholder text.
pub static LABEL_COLOUR: LazyLock<u32> = LazyLock::new(|| c(0x00_76_76_76));

/// Filled-pip colours by level — warm orange (low) → amber (mid) → green (high); empty pips use [`POSTURE_OFF_COLOUR`].
pub static POSTURE_LOW_COLOUR: LazyLock<u32> = LazyLock::new(|| c(0x00_B4_69_2F));
pub static POSTURE_MID_COLOUR: LazyLock<u32> = LazyLock::new(|| c(0x00_B4_A3_2F));
pub static POSTURE_HIGH_COLOUR: LazyLock<u32> = LazyLock::new(|| c(0x00_3E_B4_3E));
pub static POSTURE_OFF_COLOUR: LazyLock<u32> = LazyLock::new(|| c(0x00_3E_3E_3E));

/// Status-message colour for the "Attesting…" indicator that occupies the error slot while a handle query is in flight. Pure visible white, fully opaque — same slot as [`ERROR_TEXT_COLOUR`] but white instead of red so the user reads it as "neutral status" rather than "something went wrong".
pub static STATUS_TEXT_COLOUR: LazyLock<u32> = LazyLock::new(|| c(0x00_C2_C2_C2));

// ── Stray-colour consolidation (2026-07-31): every non-dynamic colour lives HERE. Party/message colours stay algorithmic (party_colour / self_colour in photon_app.rs — iso-luminance hue rays, not palette entries); everything below is a palette entry and was previously hand-buried at its use site.

/// Chrome orb ring, online. Hand-authored in DARKNESS space (pre-inverted — no `dark()` at the use site), visible RGB(64, 224, 64) green; still needs `fluor::theme::fmt()` for the platform channel-order pass (the Android red↔blue-swapped-ring bug was skipping it).
pub const ORB_ONLINE: u32 = 0xFF_BF_1F_BF;
/// Chrome orb ring, offline. Darkness-space, visible RGB(224, 64, 64) red; needs `fmt()` at the use site like ORB_ONLINE.
pub const ORB_OFFLINE: u32 = 0xFF_1F_BF_BF;
/// Degraded-vault banner text: visible RGB(255, 140, 0) amber — pairs with BG_BASE_WARNING's warm background tint. Thru `c()` like every palette entry: the old hand-inverted const skipped the display-conversion + channel-order passes, so the "amber" banner shipped CYAN (raw 0x0073FF read as visible RGB — field 2026-08-25).
pub static DEGRADED_TEXT: LazyLock<u32> = LazyLock::new(|| c(0x00_C2_7F_00));
/// Clock-skew banner text — same amber as DEGRADED_TEXT on purpose (both are "something is off with this device" warnings), named separately so they can diverge without a hunt.
pub static CLOCK_TEXT: LazyLock<u32> = LazyLock::new(|| c(0x00_C2_7F_00));
/// Settings back-button idle fill (held state uses fluor::theme::BUTTON_HELD).
pub const BACK_BUTTON_IDLE_FILL: u32 = 0x80_FF_FF_FF;

/// Dim a stored α+darkness colour to ~half opacity — the undelivered-outgoing message treatment (delivered = the same colour at full α; the RGB never changes, only presence). The stored high byte is opacity, so halving it makes the glyph fainter against the background.
/// A colour from three 0..=255 components on the γ2 scale (255 = white; the wave card's band-coloured bars) thru the same VSF-RGB path every theme constant takes. Each component moves to the `g2t` code holding its light first, so a bar keeps the brightness it had: read as `g2t`, 255 would be 9.7× white.
pub fn rgb_colour(r: u8, g: u8, b: u8) -> u32 {
    c((g2t_from_gamma2(r) << 16) | (g2t_from_gamma2(g) << 8) | g2t_from_gamma2(b))
}

/// The colour's hue at a fraction of its brightness, alpha kept — a NEAR-BLACK button fill that still says which verb it is (Nick 2026-09-12: "buttons need to be very dark colours, almost black"). Scales the display bytes, so `keep` is in gamma space: 0.15 reads as a few percent of the light.
pub fn near_black(c: u32, keep: f32) -> u32 {
    // The stored bytes are DARKNESS: keeping `keep` of the VISIBLE brightness means pulling darkness TOWARD 255 — scaling the stored bytes down was brightening toward white, which made every "near-black tint" a near-WHITE tint (the blinding buttons, 2026-09-12).
    // `keep` is a fraction of the light; with keep ∈ [0, 1] each channel lands in [dark, 255], inside its byte. Outside that range the packed u32 would bleed channels, so it fails here, loud.
    assert!((0.0..=1.0).contains(&keep), "near_black: keep {keep} is not a fraction");
    let ch = |shift: u32| {
        let dark = ((c >> shift) & 0xFF) as f32;
        ((255.0 - (255.0 - dark) * keep).round() as u32) << shift
    };
    (c & 0xFF00_0000) | ch(16) | ch(8) | ch(0)
}

pub fn dim_colour(c: u32) -> u32 {
    let a = ((c >> 24) & 0xFF) >> 2;
    (c & 0x00FF_FFFF) | (a << 24)
}

/// Half the alpha — the REFERENCED-message treatment (a reply's target snippet, the armed compose strip). Quarter (`dim_colour`) stays the sent-not-yet-ACKed signal; half reads as "context, not content", and the two must stay visually distinct.
pub fn half_colour(c: u32) -> u32 {
    let a = ((c >> 24) & 0xFF) >> 1;
    (c & 0x00FF_FFFF) | (a << 24)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The γ2 pipeline the palette used to take, on the CURRENT matrix: γ2 decode, VSF RGB → the surface's primaries (none on macOS), γ2 buffer code.
    fn old_pipeline(hex: u32) -> [u32; 3] {
        let ch = |c: u32| ((c * c) as i64 * 65536 + 65025 / 2) / 65025;
        let (r, g, b) = (ch((hex >> 16) & 0xFF), ch((hex >> 8) & 0xFF), ch(hex & 0xFF));
        #[cfg(target_os = "macos")]
        let (r, g, b) = (r, g, b);
        #[cfg(not(target_os = "macos"))]
        let (r, g, b) = {
            let m = fluor::theme::VSF_RGB2REC2020_Q16;
            ((m[0] * r + m[3] * g + m[6] * b) >> 16, (m[1] * r + m[4] * g + m[7] * b) >> 16, (m[2] * r + m[5] * g + m[8] * b) >> 16)
        };
        [fluor::theme::buffer_code(r), fluor::theme::buffer_code(g), fluor::theme::buffer_code(b)]
    }

    /// The palette moved from γ2 to `g2t` authoring with its light kept (Nick 2026-10-20): each pair is (the γ2 value it had, the `g2t` value it has now), and the old pipeline and fluor's land within 3 buffer codes. Not 0: `g2t` spends 194 codes below white where γ2 spent 255, and VSF's rule takes the bucket below.
    #[test]
    fn the_g2t_palette_keeps_the_gamma2_palettes_light() {
        const PAIRS: &[(u32, u32)] = &[
            (0x000000, 0x000000),
            (0x0000FF, 0x0000C2),
            (0x00D0E0, 0x00ACB4),
            (0x00FF00, 0x00C200),
            (0x00FFFF, 0x00C2C2),
            (0x08080A, 0x08080A),
            (0x083812, 0x083712),
            (0x0B0B0B, 0x0B0B0B),
            (0x143C1C, 0x143B1C),
            (0x161A20, 0x161A1F),
            (0x171D27, 0x171D26),
            (0x1A4E22, 0x1A4B21),
            (0x242428, 0x232327),
            (0x252D59, 0x242C55),
            (0x25592D, 0x24552C),
            (0x2E8840, 0x2D7C3E),
            (0x300606, 0x2F0606),
            (0x301000, 0x2F1000),
            (0x30FF30, 0x2FC22F),
            (0x3E3810, 0x3D3710),
            (0x404040, 0x3E3E3E),
            (0x40E040, 0x3EB43E),
            (0x443008, 0x422F08),
            (0x482A0E, 0x46290E),
            (0x4E1414, 0x4B1414),
            (0x4E1A1A, 0x4B1A1A),
            (0x592525, 0x552424),
            (0x808080, 0x767676),
            (0x80FF00, 0x76C200),
            (0x8C7E26, 0x7F7525),
            (0xA050FF, 0x8E4DC2),
            (0xA05E22, 0x8E5A21),
            (0xAC2E2E, 0x962D2D),
            (0xC08818, 0xA37C18),
            (0xE04040, 0xB43E3E),
            (0xE07030, 0xB4692F),
            (0xE0C030, 0xB4A32F),
            (0xF0F0F0, 0xBCBCBC),
            (0xFF00FF, 0xC200C2),
            (0xFF5050, 0xC24D4D),
            (0xFF8C00, 0xC27F00),
            (0xFFA500, 0xC29100),
            (0xFFB000, 0xC29900),
            (0xFFFFFF, 0xC2C2C2),
        ];
        for &(old, new) in PAIRS {
            let got = fluor::theme::vsf(new);
            let got = [(got >> 16) & 0xFF, (got >> 8) & 0xFF, got & 0xFF];
            let want = old_pipeline(old);
            for i in 0..3 {
                assert!((got[i] as i32 - want[i] as i32).abs() <= 3, "{old:06X} → {new:06X}: {got:?} vs {want:?}");
            }
        }
    }

    /// A runtime γ2 component lands in the `g2t` bucket holding its light: the code's bucket bottom is at or below it, the next code's above, and γ2 white is `g2t` white (0xC2).
    #[test]
    fn gamma2_components_keep_their_light() {
        for v in 0..=255u8 {
            let light = (v as u32 * v as u32 * 65536 + 65025 / 2) / 65025;
            let code = g2t_from_gamma2(v) as usize;
            assert!(fluor::theme::G2T_LIGHT_Q16[code] <= light, "{v}");
            assert!(code == 255 || fluor::theme::G2T_LIGHT_Q16[code + 1] > light, "{v}");
        }
        assert_eq!(g2t_from_gamma2(255), 0xC2);
        assert_eq!(g2t_from_gamma2(0), 0);
    }
}
