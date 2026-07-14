//! [`PhotonApp`]: the [`fluor::host::app::FluorApp`] impl that hosts Photon on desktop. Owns the app state machine (`AppState`), network handles, contact list, and the per-screen widgets (Launch / Ready / Searching / Conversation), drawing the chrome (perimeter, shadow, window buttons, app-icon orb) plus each screen's content, and routing cross-thread wake-ups thru `FluorApp::on_user_event` with the [`super::PhotonEvent`] payload.

use super::chromatic_wave::chromatic_wave;
use super::launch_layout::{AttestBlockLayout, LaunchLayout};
use super::photon_logo::paint_photon_logo;
use super::ready_layout::ReadyLayout;
use super::settings_layout::SettingsLayout;
use super::state::{AppState, LaunchState, SettingsPage};
use super::PhotonEvent;
#[cfg(not(target_os = "android"))]
use crate::network::fgtw::get_machine_fingerprint;
use crate::network::fgtw::{derive_device_keypair, PeerStore};
use crate::network::{
    ClutchCeremonyResult, ClutchKemEncapResult, ClutchKeygenResult, HandleQuery, QueryResult,
};
// Types used by the CLUTCH ceremony + message machinery extracted from app.rs (referenced bare in those blocks).
use crate::network::status::AckRequest;
use crate::types::{ChatMessage, ContactId, FriendshipChains, FriendshipId};
use fluor::canvas::{Canvas, PixelRect};
use fluor::coord::Coord;
use fluor::event::{
    CursorIcon, ElementState, Event, Ime, Key, MouseButton, MouseScrollDelta, NamedKey,
};
use fluor::geom::Viewport;
use fluor::host::app::{Context, EventResponse, FluorApp};
use fluor::host::chrome::{self, ResizeEdge};
use fluor::host::chrome_widget::DefaultChrome;
use fluor::host::widget::{self, Container, TabDir, Widget};
use fluor::paint::{self, HitId, HIT_NONE};
use fluor::widgets::{BlinkTimer, Button, Textbox};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use fluor::host::WakeSender;

/// How long after a `[`/`]` release we still treat the bracket as "held" for chord purposes. X11 fires a synthetic Release for the held bracket the instant the action key is pressed; this grace absorbs that round-trip so chords fire reliably.
const CHORD_RELEASE_GRACE: Duration = Duration::from_millis(40);

/// Error-state message colour for the Launch screen's error slot — visible RGB (255, 80, 80), bright red, fully opaque. `fluor::theme::dark(fmt(visible_argb))` does the same compile-time pack as fluor's theme constants: `fmt` is identity on desktop and an R↔B swap on Android (RGBA_8888 byte order in the ANativeWindow buffer), `dark` flips RGB → darkness and sets α=0xFF.
const ERROR_TEXT_COLOUR: u32 = fluor::theme::dark(fluor::theme::fmt(0x00_FF_50_50));

/// Colour for the dozenal version glyphs at the bottom of the screen: pure white (darkness 0 across all channels), α = 32 = 1/8 opacity. Stored directly in fluor's α+darkness format — `draw_text_center_u32` multiplies the glyph coverage into this α, so the version reads as a faint watermark over the background noise.
const VERSION_COLOUR: u32 = 0x20_00_00_00;

/// Colour for the zoom-percentage watermark at the top of the screen: pure white, α = 64 = 1/4 opacity (twice [`VERSION_COLOUR`]'s 1/8). Same α+darkness watermark scheme as the version — painted before the background noise so it reads as a faint top-centre indicator of the current `ru` zoom factor.
const ZOOM_COLOUR: u32 = 0x40_00_00_00;

/// Contact name text on the Ready list — near-white. α+darkness (the format fluor's text/shape rasterizers expect — visible-RGB is not interchangeable here).
const CONTACT_NAME_COLOUR: u32 = fluor::theme::dark(fluor::theme::fmt(0x00_F0_F0_F0));
/// Hairline separating the user section from the contact list — pure white at 1/4 opacity (α=64), the same translucent treatment as the hints + zoom watermark. The 0-height `fill_rect` lays the whole 1px line at this α, so it reads as faint light over the dark background.
const SEPARATOR_COLOUR: u32 = 0x40_00_00_00;
/// Contact presence ring around a row avatar: green online, grey offline. α+darkness.
const RING_ONLINE_COLOUR: u32 = fluor::theme::dark(fluor::theme::fmt(0x00_00_C0_00));
const RING_OFFLINE_COLOUR: u32 = fluor::theme::dark(fluor::theme::fmt(0x00_28_28_28));
/// Add-friend result text + the in-flight hourglass: green on success, red on not-found/error. α+darkness.
const SEARCH_FOUND_COLOUR: u32 = fluor::theme::dark(fluor::theme::fmt(0x00_40_E0_40));
const SEARCH_FAIL_COLOUR: u32 = fluor::theme::dark(fluor::theme::fmt(0x00_E0_40_40));
/// Hourglass tint while the search is in flight (orange). α+darkness.
const HOURGLASS_COLOUR: u32 = fluor::theme::dark(fluor::theme::fmt(0x00_FF_A5_00));
/// Send-button arrowhead glyph — light grey, α+darkness format for under-blend (visible-RGB is not usable on this canvas).
const SEND_ARROW_COLOUR: u32 = fluor::theme::dark(fluor::theme::fmt(0x00_D0_D0_D0));
/// Hover fill for the send / plus action buttons — a SUBTLE neutral brightening of BUTTON_FILL (0x1A224E), reproducing the pre-fluor QUERY_BUTTON_HOVER feel rather than the shared BUTTON_HOVER's saturated-blue shift. A small delta also keeps the overlay from cooking the near-white arrowhead.
const SEND_BUTTON_HOVER: u32 = fluor::theme::dark(fluor::theme::fmt(0x00_25_2D_59));
/// Noise-background base tint when the dual-ring vault flagged this session degraded — warning orange.
/// This is a NOISE-MATH colour (visible-RGB space, like fluor's `BG_BASE`), so `fmt` not `dark`; passed
/// to `background_noise` in place of its default base.
const BG_BASE_WARNING: u32 = fluor::theme::fmt(0x00_30_10_00);
/// Grey placeholder circle for contacts/avatars without a loaded image.
const AVATAR_PLACEHOLDER: u32 = 0xFF_C5_C5_C5;
/// Thin white rule between conversation messages. α+darkness.
const DIVIDER_COLOUR: u32 = fluor::theme::dark(fluor::theme::fmt(0x00_FF_FF_FF));
/// Dim grey for the compose-box placeholder text. α+darkness.
const LABEL_COLOUR: u32 = fluor::theme::dark(fluor::theme::fmt(0x00_80_80_80));

/// Deploy version = the crate's patch number from `Cargo.toml`, baked in at compile time. The Cargo version IS the version — `deploy.sh` bumps the patch and ships; local test/release builds inherit whatever the tree currently says, so the displayed number only advances on a real deploy. (Major/minor live in 0.0 today, so the patch is the whole counter; revisit the encoding if minor ever moves.)
fn deploy_version() -> u32 {
    env!("CARGO_PKG_VERSION_PATCH").parse().unwrap_or(0)
}

/// Render `n` in dozenal (base 12) as a string of reserved control-code bytes: digit `d` (0..11) maps to codepoint `0x10 + d` (DLE..ESC), which the Oxanium `+glyphs` face draws as the dozenal glyph Zil..Stelor. The result is meant only for that font — the bytes are non-printing control codes everywhere else. Most-significant digit first; `0` renders as a single Zil (0x10).
fn dozenal_glyphs(mut n: u32) -> String {
    if n == 0 {
        return char::from(0x10).to_string();
    }
    let mut digits = Vec::new();
    while n > 0 {
        digits.push(char::from(0x10 + (n % 12) as u8));
        n /= 12;
    }
    digits.iter().rev().collect()
}

/// Number of pips in each posture meter (Security / Recovery on the Ready strip): low / medium / high.
const POSTURE_PIPS: usize = 3;
/// Filled-pip colours by level — warm orange (low) → amber (mid) → green (high); empty pips use [`POSTURE_OFF_COLOUR`]. α+darkness format (opaque), the space the shape rasterizers expect.
const POSTURE_LOW_COLOUR: u32 = fluor::theme::dark(fluor::theme::fmt(0x00_E0_70_30));
const POSTURE_MID_COLOUR: u32 = fluor::theme::dark(fluor::theme::fmt(0x00_E0_C0_30));
const POSTURE_HIGH_COLOUR: u32 = fluor::theme::dark(fluor::theme::fmt(0x00_40_E0_40));
const POSTURE_OFF_COLOUR: u32 = fluor::theme::dark(fluor::theme::fmt(0x00_40_40_40));

/// Filled-pip colour for a meter showing `filled` of [`POSTURE_PIPS`].
fn posture_colour(filled: usize) -> u32 {
    match filled {
        0 | 1 => POSTURE_LOW_COLOUR,
        2 => POSTURE_MID_COLOUR,
        _ => POSTURE_HIGH_COLOUR,
    }
}

/// Security and Recovery posture for the current identity — each a count of filled pips out of [`POSTURE_PIPS`]. Two orthogonal axes, surfaced on the Ready-screen bottom strip: * Security — how hard it is for an attacker to steal or forge this identity. Bounded by the device root. Today every platform derives `device_secret` from a *readable* fingerprint (Linux machine-id, Windows MachineGuid, macOS IOPlatformUUID), so same-privilege code can lift it: 1 pip everywhere. A root-gated firmware fact would be 2; a hardware enclave or PIPE, 3.
/// * Recovery — how hard it is for the *owner* to lose this identity for good. For a single device it is whether the root survives a factory reset: macOS's IOPlatformUUID is firmware and re-derives after a wipe (2); Linux machine-id, Windows MachineGuid and Android's ANDROID_ID are software / reset-volatile (1). Device redundancy (Mirrored), a durable anchor (desktop/PIPE) and social vouching raise this toward 3.
///
/// This is the single seam multi-device, vouching and PIPE plug into: they change what this returns and nothing else.
fn identity_posture() -> (usize, usize) {
    let security = 1; // readable root on every platform today
    #[cfg(target_os = "macos")]
    let recovery = 2; // IOPlatformUUID is firmware — survives a factory reset
    #[cfg(not(target_os = "macos"))]
    let recovery = 1; // software / reset-volatile root, single device
    (security, recovery)
}

/// Signed distance from `(px,py)` to the capsule of radius `r` around segment `a→b`. Negative inside. The projection parameter `h` is clamped to `[0,1]` because that IS the capsule SDF — the closest point on a finite segment — not a defensive bound.
fn dist_to_capsule(px: f32, py: f32, ax: f32, ay: f32, bx: f32, by: f32, r: f32) -> f32 {
    let (pax, pay) = (px - ax, py - ay);
    let (bax, bay) = (bx - ax, by - ay);
    let denom = bax * bax + bay * bay;
    let h = if denom > 0.0 {
        ((pax * bax + pay * bay) / denom).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let (dx, dy) = (pax - bax * h, pay - bay * h);
    (dx * dx + dy * dy).sqrt() - r
}

/// Draw an hourglass (two triangles meeting at a central point) centred at `(cx,cy)`, `size` px tall-ish, rotated `angle_deg`, in `colour` (α+darkness). SDF over the six capsule edges with a 1-pixel AA band; composes via `under()`. Port of the legacy search-in-flight icon.
fn draw_hourglass(canvas: &mut Canvas, cx: f32, cy: f32, size: f32, angle_deg: f32, colour: u32) {
    use fluor::pixel::{Blend, BlendMode};
    let scale = size / 1000.0;
    let radius = (83.0 * scale) * 0.5; // stroke half-width
    let (hw, hh) = (300.0 * scale, 400.0 * scale);
    let a = (-angle_deg).to_radians();
    let (cos_a, sin_a) = (a.cos(), a.sin());
    // Six edges: top triangle (base + two sides to the centre apex) and bottom triangle (mirror).
    let edges = [
        ((-hw, -hh), (hw, -hh)),
        ((-hw, -hh), (0.0, 0.0)),
        ((hw, -hh), (0.0, 0.0)),
        ((-hw, hh), (hw, hh)),
        ((-hw, hh), (0.0, 0.0)),
        ((hw, hh), (0.0, 0.0)),
    ];
    let (w, h) = (canvas.width, canvas.height);
    let half = (size * 0.5 + 2.0) as isize;
    let x0 = (cx as isize - half).max(0) as usize;
    let x1 = ((cx as isize + half).max(0) as usize).min(w);
    let y0 = (cy as isize - half).max(0) as usize;
    let y1 = ((cy as isize + half).max(0) as usize).min(h);
    if x0 >= x1 || y0 >= y1 {
        return;
    }
    canvas.damage.add_bounds(x0, y0, x1, y1);
    let dark = colour & 0x00FF_FFFF;
    let base_a = (colour >> 24) & 0xFF;
    for py in y0..y1 {
        let row = py * w;
        for px in x0..x1 {
            // Inverse-rotate the sample into the hourglass's local frame.
            let dx = px as f32 + 0.5 - cx;
            let dy = py as f32 + 0.5 - cy;
            let lx = dx * cos_a - dy * sin_a;
            let ly = dx * sin_a + dy * cos_a;
            let mut d = f32::MAX;
            for ((ax, ay), (bx, by)) in edges {
                let e = dist_to_capsule(lx, ly, ax, ay, bx, by, radius);
                if e < d {
                    d = e;
                }
            }
            // Coverage AA across a 1px band at the zero level set (clamped to [0,1] — it's coverage, the algorithm).
            let cov = (0.5 - d).clamp(0.0, 1.0);
            if cov <= 0.0 {
                continue;
            }
            let alpha = (base_a as f32 * cov) as u32;
            if alpha == 0 {
                continue;
            }
            let idx = row + px;
            canvas.pixels[idx] = canvas.pixels[idx].under((alpha << 24) | dark, BlendMode::Normal);
        }
    }
}

/// Draw an upward-pointing arrowhead (a filled 4-vertex chevron) centred at (cx, cy), sized to a `size`×`size` box — the send-button glyph, painted OVER the already-drawn pill (the window-controls pattern: fill the button first, draw the symbol after). The four vertices: apex (top centre), right wing tip, bottom notch (centre, pulled up so it reads as a chevron with thickness, not a solid triangle), left wing tip. `colour` is α+darkness packed. Composites via source-over onto the existing (opaque pill) pixel, writing the result OPAQUE — so it CAN'T be an under() write (that would be discarded on the opaque pill). Crucially it does NOT touch the hit map: the pill already stamped the full silhouette, so the hover overlay (which wrap-adds a FILL-calibrated delta onto every hit-id pixel) tints only the pill, never the near-white glyph. Stamping the glyph's hit id here cooked the hover — don't. Coverage feathers the 1px boundary against the actual pill colour; `colour`'s α scales the glyph.
fn draw_up_arrowhead(canvas: &mut Canvas, cx: f32, cy: f32, size: f32, colour: u32) {
    // Geometry as fractions of the box: apex up top, wings at the bottom corners, notch pulled up so the shape is a chevron (^) with visible thickness.
    let half_w = size * 0.42;
    let top = cy - size * 0.34; // apex
    let bot = cy + size * 0.30; // wing tips
    let notch = cy + size * 0.02; // bottom-centre notch (above the wing tips)
    let verts = [
        (cx, top),
        (cx + half_w, bot),
        (cx, notch),
        (cx - half_w, bot),
    ];

    let (w, h) = (canvas.width, canvas.height);
    let x0 = (cx - half_w - 1.0).floor().max(0.0) as usize;
    let x1 = ((cx + half_w + 1.0).ceil() as usize).min(w);
    let y0 = (top - 1.0).floor().max(0.0) as usize;
    let y1 = ((bot + 1.0).ceil() as usize).min(h);
    if x0 >= x1 || y0 >= y1 {
        return;
    }
    canvas.damage.add_bounds(x0, y0, x1, y1);
    // Glyph darkness channels + its base α; coverage scales α for the source-over onto the pill.
    let glyph_a = ((colour >> 24) & 0xFF) as f32 / 255.0;
    let (gr, gg, gb) = (
        ((colour >> 16) & 0xFF) as f32,
        ((colour >> 8) & 0xFF) as f32,
        (colour & 0xFF) as f32,
    );

    // Even-odd inside test + distance-to-nearest-edge for 1px coverage AA.
    let inside = |px: f32, py: f32| -> bool {
        let mut wind = false;
        let mut j = verts.len() - 1;
        for i in 0..verts.len() {
            let (xi, yi) = verts[i];
            let (xj, yj) = verts[j];
            if ((yi > py) != (yj > py)) && (px < (xj - xi) * (py - yi) / (yj - yi) + xi) {
                wind = !wind;
            }
            j = i;
        }
        wind
    };
    let edge_dist = |px: f32, py: f32| -> f32 {
        let mut best = f32::MAX;
        let mut j = verts.len() - 1;
        for i in 0..verts.len() {
            let (xi, yi) = verts[i];
            let (xj, yj) = verts[j];
            let (ex, ey) = (xj - xi, yj - yi);
            let len2 = ex * ex + ey * ey;
            let t = if len2 > 0.0 {
                (((px - xi) * ex + (py - yi) * ey) / len2).clamp(0.0, 1.0)
            } else {
                0.0
            };
            let (dx, dy) = (px - (xi + t * ex), py - (yi + t * ey));
            best = best.min((dx * dx + dy * dy).sqrt());
            j = i;
        }
        best
    };

    for py in y0..y1 {
        let row = py * w;
        for px in x0..x1 {
            let fx = px as f32 + 0.5;
            let fy = py as f32 + 0.5;
            let d = edge_dist(fx, fy);
            // Coverage: 1.0 solidly inside; feather ONLY the 1px boundary band. (The old code faded interior pixels by edge distance too, carving a translucent groove along every inner edge — the "hollow" look. Interior = fully covered.)
            let cov = if inside(fx, fy) {
                (d + 0.5).min(1.0)
            } else {
                (0.5 - d).clamp(0.0, 1.0)
            };
            if cov <= 0.0 {
                continue;
            }
            let a = cov * glyph_a;
            if a <= 0.0 {
                continue;
            }
            let idx = row + px;
            let dst = canvas.pixels[idx];
            // Source-over the glyph darkness onto the pill pixel, keeping the pill's opacity. Feathers the AA edge against the ACTUAL pill colour (any hover/active state) — no halo. Does NOT touch the hit map: the pill's silhouette stamp already covers here, so the hover overlay tints only the pill, never this near-white glyph.
            let (dr, dg, db) = (
                ((dst >> 16) & 0xFF) as f32,
                ((dst >> 8) & 0xFF) as f32,
                (dst & 0xFF) as f32,
            );
            let nr = (gr * a + dr * (1.0 - a)) as u32;
            let ng = (gg * a + dg * (1.0 - a)) as u32;
            let nb = (gb * a + db * (1.0 - a)) as u32;
            canvas.pixels[idx] = (dst & 0xFF00_0000) | (nr << 16) | (ng << 8) | nb;
        }
    }
}

/// Status-message colour for the "Attesting…" indicator that occupies the error slot while a handle query is in flight. Pure visible white, fully opaque — same slot as `ERROR_TEXT_COLOUR` but white instead of red so the user reads it as "neutral status" rather than "something went wrong".
const STATUS_TEXT_COLOUR: u32 = fluor::theme::dark(fluor::theme::fmt(0x00_FF_FF_FF));

// Tiered presence-ping cadence — frequent while the user is engaged, sparse once they've walked away, so an idle/unfocused window isn't waking the radio every few seconds for rings nobody is watching. The tier is chosen by time-since-last-interaction; any interaction (input or focus gain) resets the clock AND fires an immediate sweep, so presence is always fresh the moment the user looks, regardless of how far the cadence had backed off.
/// Active tier: sweep every 5s while interacting (idle < `PRESENCE_IDLE_NEAR`).
const PRESENCE_PING_ACTIVE: std::time::Duration = std::time::Duration::from_secs(5);
/// Idle tier: sweep every 1min once idle past `PRESENCE_IDLE_NEAR`.
const PRESENCE_PING_IDLE: std::time::Duration = std::time::Duration::from_secs(60);
/// Deep-idle tier: sweep every 15min once idle past `PRESENCE_IDLE_FAR`.
const PRESENCE_PING_DEEP: std::time::Duration = std::time::Duration::from_secs(15 * 60);
/// Idle past this → drop from active (5s) to idle (1min).
const PRESENCE_IDLE_NEAR: std::time::Duration = std::time::Duration::from_secs(30);
/// Idle past this → drop from idle (1min) to deep-idle (15min).
const PRESENCE_IDLE_FAR: std::time::Duration = std::time::Duration::from_secs(10 * 60);

/// One deterministic aesthetic channel in `[0, 1]` from a relationship digest: `blake3(name ‖ digest)`, first 8 bytes as u64, divided by `u64::MAX`. Same convention as chirp's `channel_unit` (the chime derivation) — duplicated here rather than imported because chirp is desktop-gated and colour must build on every target. Keep the two in lockstep.
fn aesthetic_channel_unit(name: &str, digest: &[u8; 32]) -> f32 {
    let mut h = blake3::Hasher::new();
    h.update(name.as_bytes());
    h.update(digest);
    let mut out = [0u8; 8];
    out.copy_from_slice(&h.finalize().as_bytes()[..8]);
    (u64::from_le_bytes(out) as f64 / u64::MAX as f64) as f32
}

/// The relationship digest for party `p` as seen alongside `other`: `spaghettify(p ‖ other)`. One derivation feeds ears and eyes: the chime uses (sender ‖ receiver), colours use (party ‖ other) — both devices agree on a party's colour within a conversation, and nothing links a party across conversations.
fn relationship_digest(p: &[u8; 32], other: &[u8; 32]) -> [u8; 32] {
    let mut input = [0u8; 64];
    input[..32].copy_from_slice(p);
    input[32..].copy_from_slice(other);
    ihi::spaghettify(&input)
}

/// Encode a linear VSF RGB triple for the framebuffer: linear sRGB (caller guarantees in-gamut; clamp mops up f32 dust) → OETF → stored α+darkness.
fn vsf_rgb_to_stored(rgb_vsf: [f32; 3]) -> u32 {
    use vsf::colour::{srgb_oetf, VSF_RGB2SRGB};
    let m = &VSF_RGB2SRGB;
    let lin = [
        m[0] * rgb_vsf[0] + m[3] * rgb_vsf[1] + m[6] * rgb_vsf[2],
        m[1] * rgb_vsf[0] + m[4] * rgb_vsf[1] + m[7] * rgb_vsf[2],
        m[2] * rgb_vsf[0] + m[5] * rgb_vsf[1] + m[8] * rgb_vsf[2],
    ];
    let r = (srgb_oetf(lin[0].clamp(0.0, 1.0)) * 255.0).round() as u32;
    let g = (srgb_oetf(lin[1].clamp(0.0, 1.0)) * 255.0).round() as u32;
    let b = (srgb_oetf(lin[2].clamp(0.0, 1.0)) * 255.0).round() as u32;
    fluor::theme::dark(fluor::theme::fmt((r << 16) | (g << 8) | b))
}

/// Self renders in the system's achromatic anchor: VSF grey (0.5, 0.5, 0.5) — photopic Y = 0.5 like every contact colour, zero chroma. It is literally the chroma-0 point of every party's colour ray (Illuminant-E neutral, so a hair warm on a D65 display — that's equal-energy white, the pipeline's honest neutral).
fn self_colour() -> u32 {
    vsf_rgb_to_stored([0.5; 3])
}

/// Deterministic per-party text colour: an iso-luminance hue ray in linear VSF RGB, fed by the relationship digest (`spaghettify(party ‖ other)` — the same digest family as the chime, so ears and eyes derive from one relationship identity).
///
/// Brightness is locked at photopic Y = 0.5 LINEAR via the spectral pipeline (Stockman & Sharpe 2000 10° cone fundamentals, LMS2PHOTOPIC): photopic Y is linear in linear RGB, so the legal colours form a plane slicing the gamut cube thru grey (0.5, 0.5, 0.5). "colour hue" picks a direction in that plane (⊥ the luminance gradient), "colour chroma" (√-biased toward saturated) walks from grey toward the wall. The walk is clipped against BOTH the VSF RGB cube and the preimage of the linear sRGB cube, so the displayed colour is never gamut-clipped — the 50% promise holds on the actual screen. Returns fluor stored α+darkness.
fn party_colour(digest: &[u8; 32]) -> u32 {
    use vsf::colour::convert::vsf_rgb_to_photopic_f32;
    use vsf::colour::VSF_RGB2SRGB;

    // Luminance gradient w: photopic Y is linear in rgb, so evaluating the canonical pipeline on the three axes yields the plane normal. Tracks any future vsf observer changes automatically.
    let w = [
        vsf_rgb_to_photopic_f32(1.0, 0.0, 0.0),
        vsf_rgb_to_photopic_f32(0.0, 1.0, 0.0),
        vsf_rgb_to_photopic_f32(0.0, 0.0, 1.0),
    ];
    // Orthonormal basis (u, v) spanning the iso-luminance plane: u ⊥ w chosen with zero blue component, v = w × u.
    let norm = |a: [f32; 3]| {
        let n = (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt();
        [a[0] / n, a[1] / n, a[2] / n]
    };
    let u = norm([w[1], -w[0], 0.0]);
    let v = norm([
        w[1] * u[2] - w[2] * u[1],
        w[2] * u[0] - w[0] * u[2],
        w[0] * u[1] - w[1] * u[0],
    ]);

    let theta = aesthetic_channel_unit("colour hue", digest) * core::f32::consts::TAU;
    let (sin_t, cos_t) = theta.sin_cos();
    let dir = [
        u[0] * cos_t + v[0] * sin_t,
        u[1] * cos_t + v[1] * sin_t,
        u[2] * cos_t + v[2] * sin_t,
    ];
    let grey = [0.5f32; 3];

    // Largest t with origin + t·dir inside [0,1]³ (per-axis wall clip; dir ⊥ w keeps Y at 0.5 for every t).
    let ray_box_t = |origin: [f32; 3], d: [f32; 3]| -> f32 {
        let mut t = f32::MAX;
        for i in 0..3 {
            if d[i].abs() > 1e-9 {
                let wall = if d[i] > 0.0 { 1.0 } else { 0.0 };
                t = t.min((wall - origin[i]) / d[i]);
            }
        }
        t.max(0.0)
    };
    // Column-major 3x3 apply (matches vsf's matrix layout).
    let apply = |m: &[f32; 9], p: [f32; 3]| -> [f32; 3] {
        [
            m[0] * p[0] + m[3] * p[1] + m[6] * p[2],
            m[1] * p[0] + m[4] * p[1] + m[7] * p[2],
            m[2] * p[0] + m[5] * p[1] + m[8] * p[2],
        ]
    };

    let t_vsf = ray_box_t(grey, dir);
    // The same ray expressed in linear sRGB (linear map ⇒ still a ray): clip against the display cube too.
    let grey_s = apply(&VSF_RGB2SRGB, grey);
    let dir_s = apply(&VSF_RGB2SRGB, dir);
    let t_srgb = ray_box_t(grey_s, dir_s);
    let t_max = t_vsf.min(t_srgb);

    // √ bias: uniform chroma draws cluster greyish; sqrt pushes the population toward saturated.
    let chroma = aesthetic_channel_unit("colour chroma", digest).sqrt() * t_max;
    let rgb_vsf = [
        grey[0] + chroma * dir[0],
        grey[1] + chroma * dir[1],
        grey[2] + chroma * dir[2],
    ];

    // Display: in-cube by the dual clip; shared encoder does sRGB conversion + OETF + darkness packing.
    vsf_rgb_to_stored(rgb_vsf)
}

/// Dim a stored α+darkness colour to ~half opacity (for undelivered outgoing messages). The stored high byte is opacity (α), so scaling it down makes the glyph fainter against the background.
fn dim_colour(c: u32) -> u32 {
    let a = ((c >> 24) & 0xFF) / 2;
    (c & 0x00FF_FFFF) | (a << 24)
}

/// Debug chord bindings shown in the hint overlay while `[ + ]` are held. Keep in sync with the dispatch in `on_event`'s KeyboardInput arm — adding a row here without wiring its handler (or vice versa) silently drops the binding.
const CHORD_HINTS: &[(&str, &str)] = &[
    ("h", "Hit-mask overlay"),
    ("p", "Skip premultiply"),
    ("a", "Show alpha (cycle)"),
    ("c", "Skip chrome"),
    ("l", "Skip controls"),
    ("r", "Force redraw"),
    ("f", "FPS / per-stage timings strip"),
    ("w", "Damage rect outline (Where)"),
    ("d", "Screen-buffer decay (fade)"),
    ("b", "Finalize copy-pass blue tint"),
    ("n", "Nuke vault — keeps you attested (dev only)"),
    ("u", "Un-attest — clear session, keep vault (dev only)"),
    ("x", "Nuke vault + un-attest + wipe logs + EXIT for a clean relaunch (dev only)"),
];

/// Bounding rect the chord hint panel covers — matches `paint::draw_chord_hint`'s positioning math so `damage_rect` can union it when both brackets are held. Pulled out of the panes example with the same math; if fluor's hint geometry changes, this needs updating in lockstep.
fn chord_hint_bbox(viewport: Viewport, vw: usize, vh: usize) -> PixelRect {
    let span = viewport.effective_span();
    // Mirrors fluor's `draw_chord_hint`: `span × 0.014`, no pixel floor (kept in lockstep — see paint.rs).
    let font_size = span * 0.014;
    let line_h = font_size * 1.55;
    let pad = font_size * 1.25;
    let line_count = CHORD_HINTS.len() as f32 + 1.5;
    let panel_h = line_count * line_h + pad * 2.0;
    let panel_w = (span * 0.45).clamp(font_size * 22.0, font_size * 36.0);
    let cx = vw as f32 * 0.5;
    let cy = vh as f32 * 0.4;
    let x0 = (cx - panel_w * 0.5).max(0.0) as usize;
    let y0 = (cy - panel_h * 0.5).max(0.0) as usize;
    let x1 = ((cx + panel_w * 0.5).max(0.0) as usize).min(vw);
    let y1 = ((cy + panel_h * 0.5).max(0.0) as usize).min(vh);
    PixelRect::new(x0, y0, x1, y1)
}

/// Which textbox a registry entry is, so callers that need per-role behaviour can branch (freeze keys off Launch-vs-Contacts busy state; the launch box gates the Attest button; the contacts box filters the contact list). Generic concerns — focus, IME routing, blink — ignore the role and treat every entry the same. Add the conversation compose bar here when it lands.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TextboxRole {
    LaunchHandle,
    ContactsSearch,
    MessageCompose,
    /// The Diagnostics optional-note field — in the registry so click-to-focus raises the Android IME + blinkie like every other box.
    SettingsNote,
}

/// Photon-desktop as a `FluorApp`. Owns fluor's `DefaultChrome` (window frame), the dense hit-id counter for widget allocation, and an optional event-loop proxy clone for waking from background tasks.
///
/// `chrome` is `Option` because [`DefaultChrome::new`] needs the actual viewport size, which the host doesn't hand the app until [`FluorApp::init`] fires. `new()` is parameterless; everything else allocates in `init`.
pub struct PhotonApp {
    chrome: Option<DefaultChrome>,
    hit_counter: HitId,
    event_proxy: Option<Arc<dyn WakeSender<PhotonEvent>>>,
    /// Vertical scroll offset for the background noise — drives `paint::background_noise`'s `scroll_offset` (visually translates the noise pattern up/down), `shimmer` (noise colour bias cycle), AND the chromatic wave's phase. MouseWheel events in `on_event` mutate this; everything else reads it.
    bg_scroll: isize,
    /// Whether to paint the top-centre zoom-percentage watermark. The host swallows the zoom events (Ctrl/Cmd + scroll / ± / 0) and updates `ctx.viewport.ru` directly, so we can't observe a zoom event — instead `render` arms this when `ru` changes WHILE a zoom modifier is held, and the `ModifiersChanged` handler clears it the instant the modifier is released. Not time-based: it persists exactly as long as Ctrl/Cmd stays down after a zoom began. (Android pinch — show from two-fingers-down to release — waits on fluor's multi-touch `Touch` event, which doesn't exist yet.)
    zoom_hint: bool,
    /// Previous frame's `ru`, for the frame-to-frame change detection that arms `zoom_hint`. Seeded to 1.0 (the host's default zoom).
    last_ru: f32,
    /// Wave-phase animation accumulator for the "query in flight" cue. Advances at `2π rad/s` (1 full cycle/sec) in `tick()` while `state == LaunchState::Attesting` (or future `AppState::Searching`); held constant otherwise so the wave stays idle when the app is. Summed into the scroll-driven base phase in `render()`. Wraps mod TAU each frame so it stays in `[0, 2π)` and float precision doesn't drift over a long-running query.
    attest_anim_phase: f32,
    /// Last `tick()` timestamp; used to compute the per-frame `delta_time` that `attest_anim_phase` advances by. `None` until the first tick fires.
    last_tick: Option<Instant>,
    /// Top-level app state machine. Launch(LaunchState) at startup; transitions to Ready after a successful attestation lands via `tick`'s `HandleQuery::try_recv` poll. Cloned out of [`super::state::AppState::Default`] at construction; mutated in `on_event` (textbox edits flip `Error → Fresh`), `tick` (handle_query result drives the Launch → Ready transition), and submission (`Fresh → Attesting`).
    state: AppState,
    /// Handle textbox — sits in the launch screen's `attest_block.textbox` slot. Holds the user's typed handle until Enter or Attest-click; geometry recomputed on every resize / zoom via `update_widget_layout`. `None` until [`FluorApp::init`].
    textbox: Option<Textbox>,
    /// "Attest" button — sits in the `attest_block.attest` slot. Click fires the same submission path as Enter in the textbox. `None` until init.
    attest_btn: Option<Button>,
    /// Currently-focused widget id, or `None` when nothing's focused (Esc, background click, first launch). Source of truth for keyboard delivery — widgets' internal `focused` flags are derived state set by `widget::apply_focus_change` after this updates.
    focused: Option<HitId>,
    /// Blinkey timer for the focused textbox cursor. `tick()` polls it and writes `textbox.blinkey_visible` accordingly; resets on every keystroke so the cursor stays solid thru typing instead of strobing.
    blink_timer: BlinkTimer,
    /// Last time `tick()` ran the background presence ping sweep (`ping_contacts`). `None` until the first sweep. Paired with `last_interaction` to drive the tiered cadence (see `presence_ping_interval`): `tick()` re-pings when due and `wake_at()` schedules the next due sweep so presence refreshes even while idle. Without this, contacts only flipped online when you opened their conversation.
    last_presence_ping: Option<Instant>,
    /// Last time the user interacted with the app (any input event, or window focus-gain). `None` until the first interaction. The presence sweep tapers with idle time — frequent while you're actively using it, sparse when you've walked away — so an unfocused, untouched window isn't hitting the network every few seconds. Reset on interaction, which also triggers an immediate sweep so rings are fresh the instant you look. See `presence_ping_interval`.
    last_interaction: Option<Instant>,
    /// Last time an already-running device re-folded its OWN fleet chain to catch a device add/remove it may have missed. The hub `fleet` event is the fast path but best-effort (a dropped WebSocket = a missed add), so this periodic re-fold is the reliable doorbell: without it, an existing device never learns a newly-added sibling until relaunch — it wouldn't answer the new device's presence pings (→ shows it offline) and its Fleet list would stay stale. `None` until the first poll.
    last_fleet_refold: Option<Instant>,
    /// HandleQuery client — owns the UDP socket, device keypair, and FGTW peer store. Submission calls `handle_query.query(handle)`; `tick()` polls `try_recv()` for results. `None` until init.
    handle_query: Option<HandleQuery>,
    /// Per-contact presence + CLUTCH ceremony driver. Shares HandleQuery's UDP socket; pings contacts, receives pongs (→ `is_online`), and runs the slot-based CLUTCH offer/KEM/complete exchange. `None` until init. Ported from the retired `app.rs` — the fluor migration left this whole subsystem behind, so contacts showed offline and CLUTCH never started.
    status_checker: Option<crate::network::status::StatusChecker>,
    /// Pubkeys the status checker will answer pings from — kept in lockstep with `self.contacts` (seeded on resume-load, appended on add). Shared `Arc<Mutex<..>>` with the checker thread.
    contact_pubkeys: crate::network::status::ContactPubkeys,
    /// Last-received-message markers per conversation, for retransmit. Inert in v1 (messaging not yet ported) — an empty shared vec the checker reads and never finds anything in.
    sync_records: crate::network::status::SyncRecordsProvider,
    /// Background CLUTCH keypair-generation results (the 8 ephemeral keypairs per ceremony). Drained in `tick` → stores keypairs on the contact + flips it to a ready-to-offer state.
    clutch_keygen_tx: std::sync::mpsc::Sender<crate::network::ClutchKeygenResult>,
    clutch_keygen_rx: std::sync::mpsc::Receiver<crate::network::ClutchKeygenResult>,
    /// Background KEM-encapsulation results (responder's reply to an offer). Drained in `tick` → sends the KEM response.
    clutch_kem_encap_tx: std::sync::mpsc::Sender<crate::network::ClutchKemEncapResult>,
    clutch_kem_encap_rx: std::sync::mpsc::Receiver<crate::network::ClutchKemEncapResult>,
    /// Background ceremony-completion results (avalanche-expand → friendship chains + eggs proof). Drained in `tick` → sends complete, marks the contact CLUTCH-complete.
    clutch_ceremony_tx: std::sync::mpsc::Sender<crate::network::ClutchCeremonyResult>,
    clutch_ceremony_rx: std::sync::mpsc::Receiver<crate::network::ClutchCeremonyResult>,
    /// Peer-avatar background downloads (fetched from FGTW by handle, off the UI thread). The result carries the decoded VSF-RGB pixels (or None if the peer has no avatar / fetch failed); the drain in `check_status_updates` colour-converts and installs them on the matching contact.
    avatar_dl_tx: std::sync::mpsc::Sender<crate::ui::avatar::AvatarDownloadResult>,
    avatar_dl_rx: std::sync::mpsc::Receiver<crate::ui::avatar::AvatarDownloadResult>,
    /// Handles we've already kicked an avatar download for this session, so we don't re-spawn a fetch every time a conversation is reopened or the contact list re-renders.
    avatar_dl_started: std::collections::HashSet<[u8; 32]>,
    /// Mutual peers we've sent a direct P2P AvatarRequest to, mapped to the eagle-time we sent it. The per-tick sweep asks each mutual peer once, then — if no AvatarResponse has installed an avatar within `AVATAR_P2P_FALLBACK_OSC` — falls back to FGTW. So a friend's avatar comes from the friend first, and FGTW only covers the case where the friend is offline or avatar-less.
    avatar_req_pending: std::collections::HashMap<[u8; 32], i64>,
    /// History-serve rate limiting, keyed by conversation_token: (last-served eagle-time, recent request ids). Dedups replayed hist_req frames (the redundant alt-path copy arrives ~always) and caps the serve cadence per conversation.
    history_serve: std::collections::HashMap<[u8; 32], (i64, std::collections::VecDeque<[u8; 32]>)>,
    /// Completed friendship chains, keyed by friendship id — populated when a CLUTCH ceremony completes (the per-conversation rolling key material lives here). Persisted via `save_friendship_chains`; loaded on attest/resume.
    friendship_chains: Vec<(
        crate::types::friendship::FriendshipId,
        crate::types::friendship::FriendshipChains,
    )>,
    /// Last `[` Press timestamp; `None` until first press. Combined with `chord_lb_release` decides whether `[` is currently held — see `brackets_held`.
    chord_lb_press: Option<Instant>,
    /// Last `[` Release timestamp. `None` until first release.
    chord_lb_release: Option<Instant>,
    /// Mirror of `chord_lb_press` for `]`.
    chord_rb_press: Option<Instant>,
    /// Mirror of `chord_lb_release` for `]`.
    chord_rb_release: Option<Instant>,
    /// Toggle for the `[]h` chord — paints a per-hit-id random-colour overlay over the entire frame so widget hit zones are visually distinguishable. Synced to `paint::DEBUG_SHOW_HITMASK` for the finalize debug branch.
    show_hitmask: bool,
    /// 256-entry colour table indexed by `hit_test_map` byte. Regenerated each time `[]h` toggles on so distinct IDs get visibly distinct colours. Empty until the chord first arms; cleared back to empty has no effect (the overlay skips when empty).
    debug_hit_colours: Vec<u32>,
    /// "Were both brackets held last frame?" — read in `damage_rect` so the frame following a release still includes the chord-hint bbox (one extra paint to clear stale hint pixels), and the toggle is debounced thru a full frame.
    last_chord_held: bool,
    /// True when anything OTHER than self-damage-tracking widget state changed since the last render — screen content is immediate-mode (contact rows, bubbles, banners, toasts all re-rasterize as a function of app state), so any state change that could move content claims the full viewport in `damage_rect`. What stays narrow: pure widget frames (blinkey flips, drag-select growth) where the widgets' own `damage_rect`s are the whole story. Set by every event except `CursorMoved` (hover lives in the host overlay pass; drag-select is textbox-tracked), by every content-flavoured `needs_redraw` in `tick`, and cleared at the end of `render`. Starts true so the first frame paints everything.
    scene_dirty: bool,
    /// The device's session identity (register-shaped roots), set on `QueryResult::Success`. `None` while the user is still on Launch. Replaces the handle string — Photon never holds the plaintext handle past first attest; an optional "show my handle" label would re-prompt rather than store it.
    session: Option<tohu::SessionIdentity>,
    /// The private identity secret S — RAM-ONLY, never persisted (crypto::blind::PrivateS). Reconstituted from a friend's OTP-blinded deposit (blind_get→blind_srv) or generated fresh at first weave-seal AFTER every reachable woven friend answers found=0 (probe-before-generate: a []n-reset device must RECOVER its S, never mint a second one). Zeroized on []u/de-attest and on drop.
    private_s: crate::crypto::blind::PrivateS,
    /// True when the dual-ring vault flagged a damaged ring on open this session. Drives the persistent amber banner on the Ready screen. Sticky for the session.
    vault_degraded: bool,
    /// Green confirmation band on the Ready screen ("Device added \u{221a}"). Event-shown, interaction-cleared (clear_hints), NEVER time-based. Stacks above the amber warning bands.
    ready_toast: Option<String>,
    /// nunc-time clock sanity check: result channel + drain. The worker (one-shot, off-thread) posts the consensus-vs-system offset here; `drain_clock_check` reads it and updates `clock_off`.
    clock_check_tx: std::sync::mpsc::Sender<crate::network::ClockCheckResult>,
    clock_check_rx: std::sync::mpsc::Receiver<crate::network::ClockCheckResult>,
    /// `Some(offset_secs)` when the last consensus said the system clock is off by more than the threshold (consensus − system; positive = system behind). Drives the amber "clock off" banner. Tracks the LATEST verdict, not sticky — a corrected clock clears it on the next clean check.
    clock_off: Option<i64>,
    /// Watches the wall clock against the monotonic clock; a gross unexplained jump (NTP step, long sleep, or an adversary moving the clock after boot) triggers a fresh consensus re-check.
    clock_jump: crate::network::ClockJumpDetector,
    /// Fleet-inbox drain: a one-shot off-thread pull of this identity's pending worker-observed events (bind-attempt alerts, docs/fleet-inbox.md). `drain_fleet_inbox` reads the result and surfaces a notice. Kicked once per attest/resume.
    inbox_check_tx: std::sync::mpsc::Sender<Vec<crate::network::fgtw::FleetInboxEvent>>,
    inbox_check_rx: std::sync::mpsc::Receiver<Vec<crate::network::fgtw::FleetInboxEvent>>,
    /// FGTW connectivity state — flipped by `HandleQuery::try_recv_online`. Drives the top-left chrome orb's colour (red offline / green online). Starts false; the background worker reports the first real status within the first second of launch.
    online: bool,
    /// Contacts-page handle search/add textbox (Ready state). Distinct from `textbox` so content doesn't bleed between Launch (handle being attested) and Ready (handle being added as a contact).
    contacts_textbox: Option<Textbox>,
    /// Plus button to the right of `contacts_textbox` — clicking it (or pressing Enter in the textbox) triggers the add-contact flow (`HandleQuery::search`). Will eventually carry an idle "+" glyph and an in-progress rotating-hourglass animation (legacy port from `compositing.rs`); that lands when `ProgressButton` gets extracted to fluor.
    contacts_plus_btn: Option<Button>,
    /// Conversation-screen message compose box (Conversation state). Distinct from the launch/search boxes so content never bleeds between screens. Enter sends (`submit_message`); the contents encrypt onto the open contact's friendship chain.
    message_textbox: Option<Textbox>,
    /// Send button overlaid inside `message_textbox`'s right edge — mirrors the contacts-screen search `+` button (same size, same overlay treatment). Clicking it sends the compose box contents, same as pressing Enter.
    message_send_btn: Option<Button>,
    /// Encrypted local storage — initialized after attestation success with the device secret + handle. Held behind an `Arc` so it can be handed to the avatar background-download/sync threads (a plain `&FlatStorage` borrow can't cross `thread::spawn`); the inner `Mutex<Vault>` makes `Arc<FlatStorage>` `Send + Sync`.
    storage: Option<std::sync::Arc<crate::storage::FlatStorage>>,
    /// Contact list. Populated from `AttestationData.contacts` on attestation success and grown by `submit_add_friend` → `HandleQuery::search` results. Persisted to FlatStorage on add.
    contacts: Vec<crate::types::Contact>,
    /// `true` while an add-friend FGTW search is in flight (between `submit_add_friend` firing `hq.search` and `on_search_result` landing). Drives the rotating-hourglass-over-the-plus-button cue.
    add_in_flight: bool,
    /// Hourglass rotation in degrees, advanced with a stochastic wobble each tick while `add_in_flight`.
    hourglass_angle: f32,
    /// xorshift state for the hourglass wobble — avoids a `rand` call per frame.
    hourglass_rng: u64,
    /// Last add-friend result as (text, α+darkness colour), shown below the search box until the next search starts. `None` = nothing to show. "added {h}" green, "not found" / "error: …" red.
    search_status: Option<(String, u32)>,
    /// Device keypair injected externally (Android: from `NetworkContext` via `set_device_keypair` before `init`). When `Some`, `init` uses it directly; when `None`, `init` derives a fresh keypair from `get_machine_fingerprint` (desktop path). Android MUST set this before `init` runs — leaving it `None` on Android would silently downgrade to a zeroed placeholder keypair, which would be a critical key-derivation failure.
    device_keypair: Option<crate::network::fgtw::Keypair>,
    /// One-shot Android soft-keyboard request. `change_focus` sets `Some(true)` when focus enters a textbox and `Some(false)` when it leaves; `wants_keyboard` returns and clears the value. The Activity reads the JNI signal after each touch and calls `InputMethodManager.show/hide` accordingly. Stays `None` on idle frames so the Activity doesn't churn the IME.
    pending_keyboard_request: Option<bool>,
    /// One-shot: set true when the compose box is cleared on send, so the Android host restarts IME input and a predictive keyboard doesn't re-materialise the just-sent text. Drained by `wants_input_reset`.
    pending_input_reset: bool,
    /// AddDevice flow (EXISTING device): status line on the words-entry screen.
    add_device_status: String,
    /// AddDevice flow: the verified pending binding requests for OUR fleet — the matcher's candidate set, each with its expected word tokens + keyed display name precomputed. Refreshed by the bindreq watch thread (hub-poked + polled); the typed entry prefix-matches against these, keystroke by keystroke (docs/pairing-v2.md).
    add_device_candidates: Vec<AddCandidate>,
    /// AddDevice flow: the device pubkey whose consent-carrying bind has PUBLISHED and now awaits the human's green confirm — the two-phase gate: the fleet-key rotation is held behind that press, so a wrong bind stays a keyless ledger entry. `Some` = the confirm affordance is live and the words entry is done.
    add_device_bound: Option<[u8; 32]>,
    /// AddDevice flow: the entry text the live matcher last ran against (debounce — the match itself is cheap, but no point re-running it on ticks where nothing was typed).
    add_device_wordcheck_text: String,
    /// AddDevice flow: the first typed word that diverges from every candidate's expected words (or fails the voca spell-check while no candidates are in yet). Drives the red status line at the exact word the typo happens, instead of a silent no-match after all 23.
    add_device_typo: Option<String>,
    /// AddDevice flow: a bind or rotate is in flight (debounces spawns; cleared when its result drains).
    add_device_checking: bool,
    /// Pairing v2 shadow beacon: whether the AddDevice-screen scan is currently running (diffed against the screen state each tick — see the tick block).
    beacon_scan_active: bool,
    /// AddDevice flow: results from the off-thread candidate watch / bind / rotate (the fleet client blocks on HTTP, so it can't run on the UI thread).
    add_device_rx: Option<std::sync::mpsc::Receiver<AddDeviceUpdate>>,
    /// AddDevice flow: a clone-able sender so the watch, bind, and rotate threads report on the same channel.
    add_device_tx: Option<std::sync::mpsc::Sender<AddDeviceUpdate>>,
    /// AddDevice flow: stop flag for the bindreq watch thread — set on every flow exit so the registry polling dies with the screen.
    add_device_stop: Option<std::sync::Arc<std::sync::atomic::AtomicBool>>,
    /// AddDevice flow: hit id for the green-confirm affordance ("It's in — finish"), stamped only while `add_device_bound` is Some.
    add_confirm_hit_id: HitId,
    /// AddDevice flow: base hit id for the tappable candidate rows (BLE/tap select). Row `i` stamps `add_candidate_hit_base + i`; up to 8 rows.
    add_candidate_hit_base: HitId,
    /// AddDevice flow: the tap-to-bind (BLE/list select) path is in flight, so the Bound result shows the "did it turn green?" confirm instead of auto-rotating (that auto path is words-match only, where the typed key IS the confirmation). Reset when the flow ends.
    add_device_bind_ble: bool,
    /// Diagnostics "Submit" flow: result of the off-thread log upload to FGTW (blocking HTTP over up to 16 MiB). `Ok(())` → "Log sent" toast; `Err` → the reason. Drained in tick.
    log_submit_rx: Option<std::sync::mpsc::Receiver<Result<(), String>>>,
    log_submit_tx: Option<std::sync::mpsc::Sender<Result<(), String>>>,
    /// An upload is currently on the worker thread — Submit greys so a second press can't race a duplicate.
    log_submit_inflight: bool,
    /// `crate::log_size_bytes()` captured right after the last SUCCESSFUL submit's own log lines landed. While the live size still equals this, the log holds nothing new and Submit stays greyed (a resend would be a byte-identical duplicate); any fresh record — or a Clear — moves the size and re-arms the pill. `None` until a submit succeeds.
    log_submitted_len: Option<u64>,
    /// Stop flag for the NEW device's join thread — set true when the user cancels join mode so the thread quits re-posting its request (a zombie re-poster would race a later attempt for the inbox slot).
    add_stop: Option<std::sync::Arc<std::sync::atomic::AtomicBool>>,
    /// Session-long fleet-event subscription (hub WebSocket): receiver of event kinds ("fstate" / "fleet") filtered to OUR identity. Drained in tick — fstate triggers a roster pull (a friend added on a sibling device appears here in ~a second), fleet triggers a key/membership sync. `None` until the first attest/resume succeeds.
    fleet_evt_rx: Option<std::sync::mpsc::Receiver<(&'static str, [u8; 32])>>,
    /// Stop flag for the fleet-event subscription task (dropped app / de-attest).
    fleet_evt_stop: Option<std::sync::Arc<std::sync::atomic::AtomicBool>>,
    /// Off-thread contact-fleet refresh results: (contact handle_proof, current member pubkeys folded from their chain, chain-tip eagle time). Drained in tick into the matching contact's `fleet_members`, gated on the tip being fresher than the last adopted one, then `reseed_contact_pubkeys`. Lets a friend's NEW device be honoured — and a REMOVED device revoked — without waiting for our next launch.
    contact_members_rx: Option<std::sync::mpsc::Receiver<([u8; 32], Vec<[u8; 32]>, i64)>>,
    /// Sender half of the contact-fleet-refresh channel, kept alive so successive refreshes reuse one channel (the receiver is drained in tick).
    contact_members_tx: Option<std::sync::mpsc::Sender<([u8; 32], Vec<[u8; 32]>, i64)>>,
    /// Launch add-mode (NEW device joining a fleet): orb on Launch toggles it, and a failed attest against an existing fleet auto-enters it. Enter the handle; this device then generates + displays its pairing words and waits for the other device to match and bind.
    launch_add_mode: bool,
    /// Join flow: the handle once entered; `None` while still awaiting it.
    add_join_handle: Option<String>,
    /// Roots from the pre-attest probe, stashed so the permanence-confirm press claims WITHOUT re-deriving the ~1s proof. Set on a `Fresh` probe outcome; cleared on any handle edit (via clear_launch_error) or when the claim fires.
    probed_session: Option<tohu::SessionIdentity>,
    /// Canonical spelling of the handle `probed_session` was derived from — the confirm press fires the stashed roots ONLY when the box still canonicalizes to this, so stale roots can never attest a different identity than the one on screen.
    probed_handle: Option<String>,
    /// Join flow: status line on the add-mode launch screen.
    add_join_status: String,
    /// Join flow: the fixed-width fleet-masked words (this device's own pubkey under the identity mask) displayed for the user to type on an existing device. `Some` = the words screen is up. The screen stays up until membership folds (green = leaving this screen) or the user cancels.
    add_join_words: Option<String>,
    /// Join flow: progress from the off-thread request-post + matched/membership poll.
    add_join_rx: Option<std::sync::mpsc::Receiver<JoinUpdate>>,
    /// Fleet key received during a JOIN, held until attest sets the vault up so it can be persisted (the new device has no storage during the join thread).
    pending_fleet_key: Option<[u8; 32]>,
    /// In-flight fleet-roster pull; its `Ok` result merges into contacts, its `Err` triggers a retry — both drained in `tick`. `Some` = a pull is running, which also debounces re-spawns.
    roster_pull_rx: Option<std::sync::mpsc::Receiver<Result<fgtw::fstate::FleetState, String>>>,
    /// The linked-settings cache (per-device maps + link-to-global; docs/global-vault.md). Lazily loaded from the vault once storage + device key exist; merged from every fstate pull; every local set persists + pushes.
    fleet_settings: Option<crate::storage::fleet_settings::FleetSettings>,
    /// Set on each attest/resume: "do one roster pull as soon as the fleet key is available." The key is written by an ASYNC fan-out sync, so an immediate pull races it and loses — this flag makes tick fire the pull the moment `fleet_key_cached()` goes Some, which is the wake-up catch-up that brings a friend added on a sibling device onto this one.
    needs_initial_roster_pull: bool,
    /// Retry budget for the initial roster pull. A fresh device's pairing-recovered key is a PRE-rotation generation (adding a device rotates the fleet key via the fan-out re-key), so the first pull decrypts the current roster with a stale key and fails `aead::Error`. The in-flight `spawn_fleet_key_sync` writes the current key within ~150ms, so on a failed pull we re-arm `needs_initial_roster_pull` and retry — the pull's own ~150ms round-trip naturally spaces attempts, and this budget caps them so a genuinely-undecryptable roster gives up instead of spinning (next fleet event / relaunch re-tries).
    roster_pull_retries_left: u8,
    /// The FGTW peer rows from the most recent attest echo (device-keyed: hp + device pubkey). Retained so `reconcile_fleet_siblings` can address a freshly-created sibling contact IMMEDIATELY from the same echo — the attest-time `refresh_contact_addrs_from_peers` runs before the async fleet-fold creates the sibling, so without this the sibling has no address, its CLUTCH offer never sends (send needs `contact.ip`), and it never pings/comes-online to retry.
    last_peers: Vec<crate::network::fgtw::PeerRecord>,
    /// This device's avatar in BT.2020 γ=2.0 u8 RGB, sized `crate::avatar::AVATAR_SIZE × AVATAR_SIZE × 3`. `None` until `on_query_result` pulls one from local storage (no saved avatar = stays `None`, Ready screen falls back to the grey placeholder).
    device_avatar_pixels: Option<Vec<u8>>,
    /// Cached Mitchell resize of `device_avatar_pixels` at the current Ready-screen circle diameter. Rebuilt on diameter change (resize / zoom).
    device_avatar_scaled: Option<Vec<u8>>,
    /// Diameter (in pixels) of `device_avatar_scaled`. `0` means no cache built yet.
    device_avatar_scaled_diameter: usize,
    /// HitId reserved for the Ready-screen self-avatar circle. Allocated in `init` alongside the other widget IDs; stamped into `chrome.hit_test_map` during the Ready render so a tap on the circle dispatches to the avatar code path (open the image picker on Android).
    avatar_hit_id: HitId,
    /// One-shot Android image-picker request. Set when the user taps the avatar; consumed by the JNI poll (`nativePollAvatarPicker`) which signals the Activity to launch `ACTION_GET_CONTENT`. Stays `None` on idle frames so the Activity doesn't churn.
    pending_picker_request: bool,
    /// One-shot signal for the Android sticky session broadcast: 1=send, -1=clear, 0=nothing. Set by attest success and []n nuke.
    pending_broadcast_signal: i8,
    /// Index of the contact currently open in Conversation view, or `None` when on the Ready (contacts list) screen.
    active_contact: Option<usize>,
    /// Base hit ID for contact rows. Row `i` gets `contact_hit_base + i`. Allocated in `init` after the other widget IDs.
    contact_hit_base: HitId,
    /// Hit ID for the "← Contacts" back button on the Conversation screen.
    back_btn_hit_id: HitId,
    /// Hit ID for the "Start fresh (wipe this device)" line on the JOIN words screen — a removed device's only self-clean path (it can't attest → can't reach Security).
    join_startfresh_hit_id: HitId,
    /// Two-tap arm for "Start fresh" on the JOIN screen (destructive → confirm).
    join_startfresh_armed: bool,
    /// Contact-list scroll offset in pixels (Ready screen). 0 = top; grows as the user scrolls down. The user section (avatar/search) stays fixed; only the rows below the separator scroll. Re-clamped to the list extent each render.
    contacts_scroll: isize,
    /// Settings nav-rail vertical scroll (pixels, ≥0). The rail lists Back + 9 pages at NATURAL (unzoomed-consistent) row height — no clamp-to-fit — so at high zoom they overflow and this scrolls them. Re-clamped to the rail extent each frame.
    settings_rail_scroll: f32,
    /// Settings content-pane vertical scroll (pixels, ≥0). Page bodies lay out at natural row height (no compress-to-fit), so tall pages / high zoom overflow and this scrolls them. Reset to 0 on page switch; re-clamped to the page's extent each frame.
    settings_content_scroll: f32,
    /// `true` once the user has interacted (any click or keystroke) since the last transition into `Ready` — hides the standing avatar prompt. Hints are event-shown and interaction-cleared, never hover- or time-driven; reset to `false` on each `Ready` entry. See [`clear_hints`].
    hints_dismissed: bool,
    /// `true` while the cursor is over the Ready-screen avatar circle. Drives the "drag/drop to update avatar" hover hint.
    avatar_hovered: bool,

    // --- Settings panel (STUB) ---
    /// Base hit id for the settings nav-rail rows. Row `i` (page `SettingsPage::ALL[i]`) stamps `settings_nav_base + i`. Allocated in `init`.
    settings_nav_base: HitId,
    /// Base hit id for the settings stub action pills (immediate-mode Buttons — Add device, Lock, Shred, Snapshot, …). Each page draws its pills over a small contiguous slice of this range; clicks land here and log a stub line. Allocated in `init` with a fixed span.
    settings_btn_base: HitId,
    /// Appearance-page theme selector — a real fluor `Dropdown`. Only in the widget walk while the Settings/Appearance page is up.
    settings_theme_dropdown: Option<fluor::widgets::Dropdown>,
    /// Appearance-page zoom / text-size control — a real fluor `Slider`.
    settings_zoom_slider: Option<fluor::widgets::Slider>,
    /// Recovery-page "be a custodian" opt-in — a custom `Checkbox`.
    settings_custodian_check: Option<crate::ui::settings_widgets::Checkbox>,
    /// Notifications-page global chime on/off — a custom `Checkbox`.
    settings_chime_check: Option<crate::ui::settings_widgets::Checkbox>,
    /// Notifications-page presence-visibility toggle — a custom `Checkbox`.
    settings_presence_check: Option<crate::ui::settings_widgets::Checkbox>,
    /// Updates-page auto-update on/off — a custom `Checkbox`.
    settings_autoupdate_check: Option<crate::ui::settings_widgets::Checkbox>,
    /// Diagnostics-page optional-note field — a real fluor `Textbox` (distinct from the launch / contacts / compose boxes so content never bleeds).
    settings_note_textbox: Option<Textbox>,
    /// Fleet-page device management: the device pubkey the user tapped to select (highlighted row). `None` = nothing selected. Only OUR OTHER devices (siblings) are selectable — never this device. Remove-other retired 2026-07-13 (sovereign records: self-signed departure only; eviction = withholding at the key layer, arriving with the device-trust bundle) — selection currently feeds only the future rename.
    settings_fleet_selected: Option<[u8; 32]>,
    /// Security-page "Shred (crypto-wipe)" confirm arm: a first tap arms, a second fires `clean_device_for_reuse` (nuke vault + clear session). Event-shown, interaction-cleared.
    settings_shred_armed: bool,

    /// This node's own reflexive (public) address, learned via peer-echoed reflection (see [`crate::network::traverse::reflexive`]). `None` until the first signed pong / `ReflectResponse` echo. Fed forward to candidate gathering and the FGTW announce so our published address is the one seen on the live UDP data socket — not fgtw.org's TLS-flow `cf-connecting-ip`, which is only right for cone NATs.
    our_reflexive: Option<std::net::SocketAddr>,
}

impl PhotonApp {
    /// Construct an empty app shell. Real state (chrome, network handles, app state machine) initializes in [`FluorApp::init`] once the viewport is known.
    pub fn new() -> Self {
        Self {
            chrome: None,
            hit_counter: 0,
            event_proxy: None,
            our_reflexive: None,
            bg_scroll: 0,
            zoom_hint: false,
            last_ru: 1.0,
            attest_anim_phase: 0.,
            last_tick: None,
            state: AppState::default(),
            textbox: None,
            attest_btn: None,
            focused: None,
            blink_timer: BlinkTimer::new(),
            last_presence_ping: None,
            last_interaction: None,
            last_fleet_refold: None,
            handle_query: None,
            status_checker: None,
            contact_pubkeys: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
            sync_records: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
            clutch_keygen_tx: {
                let (tx, _) = std::sync::mpsc::channel();
                tx
            },
            clutch_keygen_rx: std::sync::mpsc::channel().1,
            clutch_kem_encap_tx: {
                let (tx, _) = std::sync::mpsc::channel();
                tx
            },
            clutch_kem_encap_rx: std::sync::mpsc::channel().1,
            clutch_ceremony_tx: {
                let (tx, _) = std::sync::mpsc::channel();
                tx
            },
            clutch_ceremony_rx: std::sync::mpsc::channel().1,
            avatar_dl_tx: {
                let (tx, _) = std::sync::mpsc::channel();
                tx
            },
            avatar_dl_rx: std::sync::mpsc::channel().1,
            avatar_dl_started: std::collections::HashSet::new(),
            avatar_req_pending: std::collections::HashMap::new(),
            history_serve: std::collections::HashMap::new(),
            friendship_chains: Vec::new(),
            chord_lb_press: None,
            chord_lb_release: None,
            chord_rb_press: None,
            chord_rb_release: None,
            show_hitmask: false,
            debug_hit_colours: Vec::new(),
            last_chord_held: false,
            scene_dirty: true,
            session: None,
            private_s: crate::crypto::blind::PrivateS::None,
            vault_degraded: false,
            ready_toast: None,
            clock_check_tx: {
                let (tx, _) = std::sync::mpsc::channel();
                tx
            },
            clock_check_rx: std::sync::mpsc::channel().1,
            clock_off: None,
            // ~1 hour of unexplained wall-vs-monotonic skew triggers a re-check (loose enough to ignore NTP steps and short sleeps, tight enough to catch a day-scale set or long sleep).
            clock_jump: crate::network::ClockJumpDetector::new(3600),
            inbox_check_tx: {
                let (tx, _) = std::sync::mpsc::channel();
                tx
            },
            inbox_check_rx: std::sync::mpsc::channel().1,
            online: false,
            contacts_textbox: None,
            message_textbox: None,
            contacts_plus_btn: None,
            message_send_btn: None,
            storage: None,
            contacts: Vec::new(),
            add_in_flight: false,
            hourglass_angle: 0.0,
            hourglass_rng: 0x9E37_79B9_7F4A_7C15,
            search_status: None,
            device_keypair: None,
            pending_keyboard_request: None,
            pending_input_reset: false,
            add_device_status: String::new(),
            add_device_candidates: Vec::new(),
            add_device_bound: None,
            beacon_scan_active: false,
            add_device_wordcheck_text: String::new(),
            add_device_typo: None,
            add_device_checking: false,
            add_device_rx: None,
            add_device_tx: None,
            add_device_stop: None,
            add_confirm_hit_id: HIT_NONE,
            add_candidate_hit_base: HIT_NONE,
            add_device_bind_ble: false,
            log_submit_rx: None,
            log_submit_tx: None,
            log_submit_inflight: false,
            log_submitted_len: None,
            add_stop: None,
            fleet_evt_rx: None,
            fleet_evt_stop: None,
            contact_members_rx: None,
            contact_members_tx: None,
            launch_add_mode: false,
            add_join_handle: None,
            probed_session: None,
            probed_handle: None,
            add_join_status: String::new(),
            add_join_words: None,
            add_join_rx: None,
            pending_fleet_key: None,
            roster_pull_rx: None,
            fleet_settings: None,
            needs_initial_roster_pull: false,
            last_peers: Vec::new(),
            roster_pull_retries_left: 0,
            device_avatar_pixels: None,
            device_avatar_scaled: None,
            device_avatar_scaled_diameter: 0,
            avatar_hit_id: HIT_NONE,
            active_contact: None,
            contact_hit_base: HIT_NONE,
            back_btn_hit_id: HIT_NONE,
            join_startfresh_hit_id: HIT_NONE,
            join_startfresh_armed: false,
            pending_picker_request: false,
            pending_broadcast_signal: 0,
            contacts_scroll: 0,
            settings_rail_scroll: 0.0,
            settings_content_scroll: 0.0,
            hints_dismissed: false,
            avatar_hovered: false,
            settings_nav_base: HIT_NONE,
            settings_btn_base: HIT_NONE,
            settings_theme_dropdown: None,
            settings_zoom_slider: None,
            settings_custodian_check: None,
            settings_chime_check: None,
            settings_presence_check: None,
            settings_autoupdate_check: None,
            settings_note_textbox: None,
            settings_fleet_selected: None,
            settings_shred_armed: false,
        }
    }

    /// Inject the device keypair before `init` runs. Used by the Android JNI shim to pass thru the keypair that `PhotonConnectionService` derives from the OS-provided device fingerprint — that fingerprint lives in Java (`Build.FINGERPRINT` / `Settings.Secure.ANDROID_ID`) and reaches the native side via `NetworkContext`. On desktop this stays unset; `init` falls back to `get_machine_fingerprint` (which reads `/etc/machine-id` etc.) and derives the keypair internally.
    pub fn set_device_keypair(&mut self, keypair: crate::network::fgtw::Keypair) {
        self.device_keypair = Some(keypair);
    }

    /// Take the one-shot image-picker request. JNI shim polls this once per frame; returns `true` exactly on the frame the user taps the avatar so the Activity launches `ACTION_GET_CONTENT` once per tap.
    pub fn take_picker_request(&mut self) -> bool {
        let req = self.pending_picker_request;
        self.pending_picker_request = false;
        req
    }

    /// One-shot poll for the Android sticky session broadcast signal. Returns `1` after a successful attest (Kotlin should call `sendSessionBroadcast()`), `-1` after a vault nuke (Kotlin should call `clearSessionBroadcast()`), `0` otherwise.
    pub fn take_broadcast_signal(&mut self) -> i8 {
        let s = self.pending_broadcast_signal;
        self.pending_broadcast_signal = 0;
        s
    }

    /// Encode + save + reload an avatar image picked from the OS image picker. Pipeline: raw file bytes → `encode_avatar_from_image` (handles JPEG/PNG/WebP and the ICC-profile colour management — VSF spectral γ=2.0 RGB out) → `save_avatar` (encrypted handle-keyed storage) → `load_avatar` (round-trip check) → `vsf_rgb_to_bt2020` (display conversion for the Android BT.2020 buffer tag) → installed as `device_avatar_pixels` with the scaled cache invalidated. Uploads to FGTW when a `handle_proof` is available so other devices can fetch it. Skipped if the user hasn't attested yet (no handle to derive the storage key from).
    pub fn set_avatar_from_file(&mut self, image_bytes: Vec<u8>) {
        let identity_seed = match &self.session {
            Some(s) => s.identity_seed,
            None => {
                crate::log("avatar picker: ignored — not attested yet");
                return;
            }
        };
        crate::log(&format!(
            "avatar picker: processing {} bytes",
            image_bytes.len()
        ));
        let av1_data = match crate::ui::avatar::encode_avatar_from_image(&image_bytes) {
            Ok(d) => d,
            Err(e) => {
                crate::log(&format!("avatar picker: encode failed: {e}"));
                return;
            }
        };
        let storage = match self.storage.clone() {
            Some(s) => s,
            None => {
                crate::log("avatar picker: ignored — storage not initialized yet");
                return;
            }
        };
        if let Err(e) =
            crate::ui::avatar::save_avatar_from_seed(&av1_data, &identity_seed, &storage)
        {
            crate::log(&format!("avatar picker: save failed: {e}"));
            return;
        }
        let Some((_, vsf_rgb)) = crate::ui::avatar::load_avatar_from_seed(&identity_seed, &storage)
        else {
            crate::log("avatar picker: post-save load failed");
            return;
        };
        self.device_avatar_pixels = Some(crate::ui::colour_convert::vsf_rgb_to_bt2020(&vsf_rgb));
        self.device_avatar_scaled = None;
        self.device_avatar_scaled_diameter = 0;
        crate::log("avatar picker: saved + installed");
        let proof = self
            .handle_query
            .as_ref()
            .and_then(|hq| hq.get_handle_proof());
        match (self.device_keypair.as_ref(), proof) {
            (Some(kp), Some(hp)) => {
                match crate::ui::avatar::upload_avatar_from_seed(
                    &kp.secret,
                    &identity_seed,
                    &hp,
                    &storage,
                ) {
                    Ok(_) => crate::log("avatar picker: FGTW upload ok"),
                    Err(e) => crate::log(&format!("avatar picker: FGTW upload failed: {e}")),
                }
            }
            _ => crate::log("avatar picker: skipping FGTW upload — keypair / proof unavailable"),
        }
    }
}

/// Map a connectivity bool to the chrome orb tint. Offline = red disk, online = green disk. Visible RGB chosen for high contrast in either light or dark chrome themes; brighten=true on the online state for the eventual icon-overlay case (no-icon today just renders as a solid coloured circle).
fn orb_tint_for(online: bool) -> fluor::host::chrome::OrbTint {
    // Visible RGB(64, 224, 64) green: darkness = (0xBF, 0x1F, 0xBF); packed α=0xFF. Visible RGB(224, 64, 64) red:   darkness = (0x1F, 0xBF, 0xBF); packed α=0xFF.
    // These are hand-authored in darkness-space (pre-inverted, so no `dark()`), but they STILL need `fmt()` — the platform channel-order pass (identity on desktop, R↔B swap on Android). Every other photon colour rides `fmt`; the orb ring skipping it was the Android "red-blue swapped ring". `fmt` only reorders RGB and preserves the α byte, so it's correct on the already-darkened constants.
    const ORB_ONLINE: u32 = 0xFF_BF_1F_BF;
    const ORB_OFFLINE: u32 = 0xFF_1F_BF_BF;
    fluor::host::chrome::OrbTint::Custom {
        ring: fluor::theme::fmt(if online { ORB_ONLINE } else { ORB_OFFLINE }),
        brighten: online,
    }
}

/// One matcher candidate on the AddDevice screen: a verified binding request plus its precomputed expected word tokens (23, lowercase — `masked_device_words` split) and keyed display name. Precomputing keeps the per-keystroke match a plain string walk.
struct AddCandidate {
    req: crate::network::fgtw::fleet::BindRequest,
    name: String,
    tokens: Vec<String>,
    /// This candidate's device pubkey is currently being heard over the BLE announce beacon — proximity confirmation (docs/pairing-v2.md, BLE transport). The candidate list marks these "nearby"; tapping any candidate binds it (BLE/tap select), typing its words still works too.
    heard_ble: bool,
}

/// Off-thread results for the AddDevice flow (candidate watch + bind + rotate), drained in `tick`.
enum AddDeviceUpdate {
    /// A fresh, signature-verified candidate set from the binding-request registry (the watch thread's periodic/hub-poked list).
    Candidates(Vec<crate::network::fgtw::fleet::BindRequest>),
    /// The consent-carrying bind PUBLISHED — this device pubkey now awaits the human's green confirm (the rotation is held behind that press).
    Bound([u8; 32]),
    /// The green-confirm rotation published — ceremony complete, the new device can recover the fleet key.
    Rotated,
    /// An error to surface in the status line.
    Failed(String),
}

/// Off-thread results for the new-device JOIN flow (binding-request post + membership poll), drained in `tick`.
enum JoinUpdate {
    /// The fleet-masked words this device displays for the user to type on an existing device.
    ShowWords(String),
    /// This device is now in the fleet — hand off to the normal attest. Carries the fleet key recovered from the fan-out (None = bound but the green-confirm rotation hasn't landed yet; the post-attest sync retries) plus the session roots derived ONCE at join start, so the attest skips the second ~1s memory-hard proof.
    Joined(Option<[u8; 32]>, tohu::SessionIdentity),
    /// An error to surface in the status line.
    Failed(String),
}

impl Default for PhotonApp {
    fn default() -> Self {
        Self::new()
    }
}

/// Walk the widget tree. Screen content yields BEFORE chrome: launch-screen content (textbox → attest button) first, then chrome's four buttons — matching the macOS / GNOME convention where Tab traverses form fields before window-frame controls. `linear_tab_next` reads this order off the visit walk; `dispatch_click` / `dispatch_key` use it to route events by id. The walk gates on `state` so off-screen widgets neither hit-test nor cycle.
impl Container for PhotonApp {
    fn visit(&mut self, f: &mut dyn FnMut(&mut dyn Widget)) {
        if matches!(self.state, AppState::Launch(_)) {
            // The attest button is only part of the tree when there's a handle to attest — same reveal as the render gate. An empty field yields just the textbox, so Tab can't land focus on a button that isn't drawn and a hit-test can't dispatch to it. Join words phase (new device displaying its pairing words): no input widgets at all — the screen is display-only until bound or cancelled.
            let join_words_up = self.launch_add_mode && self.add_join_words.is_some();
            let handle_entered = self
                .textbox
                .as_ref()
                .map(|tb| !tb.chars.is_empty())
                .unwrap_or(false);
            if !join_words_up {
                if let Some(tb) = self.textbox.as_mut() {
                    f(tb);
                }
                if handle_entered {
                    if let Some(btn) = self.attest_btn.as_mut() {
                        f(btn);
                    }
                }
            }
        }
        if matches!(self.state, AppState::AddDevice) {
            // Words-entry screen (existing device): the launch textbox instance does double duty as the entry field. Hidden once the bind published (green-confirm phase) — a hidden field must not stay focusable.
            if self.add_device_bound.is_none() {
                if let Some(tb) = self.textbox.as_mut() {
                    f(tb);
                }
            }
        }
        if matches!(self.state, AppState::Ready) {
            if let Some(tb) = self.contacts_textbox.as_mut() {
                f(tb);
            }
            if let Some(btn) = self.contacts_plus_btn.as_mut() {
                f(btn);
            }
        }
        if matches!(self.state, AppState::Conversation) {
            // The compose box is the only focusable widget in a conversation; yielding it here wires click-to-focus, Tab, and key dispatch. Only once the chain is PROVEN (chain_woven — both probe directions sealed; self-contacts exempt, they never probe) — before that the box isn't rendered, so it must not be focusable either (otherwise a click or Tab could land focus on an invisible dead input). Must mirror the render gate exactly.
            let our_handle_hash = self
                .session
                .as_ref()
                .map(|s| crate::crypto::clutch::identity_party_id(&s.identity_seed))
                .unwrap_or([0u8; 32]);
            let compose_ready = self
                .active_contact
                .and_then(|ci| self.contacts.get(ci))
                .map(|c| {
                    c.clutch_state == crate::types::ClutchState::Complete
                        && (c.chain_woven || c.handle_hash == our_handle_hash)
                })
                .unwrap_or(false);
            if compose_ready {
                if let Some(tb) = self.message_textbox.as_mut() {
                    f(tb);
                }
                if let Some(btn) = self.message_send_btn.as_mut() {
                    f(btn);
                }
            }
        }
        if let AppState::Settings(page) = self.state {
            // Only the stateful widgets on the SELECTED page enter the walk (dispatch + tab + hover + dropdown-popup). Immediate-mode action pills and the nav rail aren't Widgets — they're hit-stamped and handled directly in the Pressed arm.
            match page {
                SettingsPage::Appearance => {
                    if let Some(dd) = self.settings_theme_dropdown.as_mut() {
                        f(dd);
                        dd.visit_rows(f);
                    }
                    if let Some(sl) = self.settings_zoom_slider.as_mut() {
                        f(sl);
                    }
                }
                SettingsPage::Recovery => {
                    if let Some(cb) = self.settings_custodian_check.as_mut() {
                        f(cb);
                    }
                }
                SettingsPage::Notifications => {
                    if let Some(cb) = self.settings_chime_check.as_mut() {
                        f(cb);
                    }
                    if let Some(cb) = self.settings_presence_check.as_mut() {
                        f(cb);
                    }
                }
                SettingsPage::Updates => {
                    if let Some(cb) = self.settings_autoupdate_check.as_mut() {
                        f(cb);
                    }
                }
                SettingsPage::Diagnostics => {
                    if let Some(tb) = self.settings_note_textbox.as_mut() {
                        f(tb);
                    }
                }
                _ => {}
            }
        }
        if let Some(chrome) = self.chrome.as_mut() {
            chrome.visit(f);
        }
    }
}

impl FluorApp for PhotonApp {
    type UserEvent = PhotonEvent;

    fn title(&self) -> &str {
        // OS WINDOW title only (taskbar / alt-tab / WM) — set once at window creation via winit `with_title`. The brand name lives here. The DRAWN in-app title bar is separate: it's `chrome.set_title(...)` per-frame in `render` ("← Network" on launch, live peer count on Ready). The chrome is constructed with "Photon" too but the first render overrides it before the first rasterize, so the drawn bar never flashes the brand name.
        "Photon"
    }

    fn initial_size(&self, monitor: (u32, u32)) -> (u32, u32) {
        // Portrait launch window — matches the pre-fluor Photon dimensions: height = half the SHORTER screen axis, width = half that. Yields a tall 1:2 (w:h) rectangle on any aspect ratio. Examples: 1920×1080 → 270×540; 1080×1920 → 270×540; 2560×1440 → 360×720.
        let short = monitor.0.min(monitor.1);
        let h = short >> 1;
        let w = h >> 1;
        (w, h)
    }

    fn wants_keyboard(&mut self) -> Option<bool> {
        // Return the one-shot keyboard transition set by `change_focus` and clear it so subsequent polls see `None` until focus moves again — keeps the Android Activity from calling `InputMethodManager.show/hide` every frame.
        self.pending_keyboard_request.take()
    }

    fn wants_input_reset(&mut self) -> bool {
        // One-shot: drained after a send so the Activity restarts IME input exactly once.
        std::mem::replace(&mut self.pending_input_reset, false)
    }

    fn set_event_proxy(&mut self, proxy: Arc<dyn WakeSender<Self::UserEvent>>) {
        self.event_proxy = Some(proxy);
    }

    fn init(&mut self, ctx: &mut Context) {
        // Register Photon's Oxanium font weights with fluor's shared `TextRenderer` so the logo wordmark can resolve `Family::Name("Oxanium")`. ExtraLight/Light/Regular/Medium/SemiBold/Bold/ExtraBold = numeric weights 200/300/400/500/600/700/800. The logo uses weight 800.
        let db = ctx.text.font_system_mut().db_mut();
        db.load_font_data(include_bytes!("../../assets/Oxanium/Oxanium-ExtraLight.ttf").to_vec());
        db.load_font_data(include_bytes!("../../assets/Oxanium/Oxanium-Light.ttf").to_vec());
        // Regular weight uses the `+glyphs` superset: identical to plain Oxanium-Regular for 0x20-0x7e (normal text) but adds the dozenal digit glyphs in the reserved control-code block 0x10-0x1b (DLE..ESC = digits 0..11, Zil..Stelor). Rendering a dozenal number is then a plain draw_text of those bytes at weight 400 — no runtime SVG, no separate font family. Other weights stay on the plain faces (the dozenal glyphs only need to exist at one weight, and the version string renders at 400).
        db.load_font_data(
            include_bytes!("../../assets/Oxanium/Oxanium-Regular+glyphs.ttf").to_vec(),
        );
        db.load_font_data(include_bytes!("../../assets/Oxanium/Oxanium-Medium.ttf").to_vec());
        db.load_font_data(include_bytes!("../../assets/Oxanium/Oxanium-SemiBold.ttf").to_vec());
        db.load_font_data(include_bytes!("../../assets/Oxanium/Oxanium-Bold.ttf").to_vec());
        db.load_font_data(include_bytes!("../../assets/Oxanium/Oxanium-ExtraBold.ttf").to_vec());

        // Chrome owns its own hit-test map sized to the viewport, allocates four hit-ids for its buttons via the threaded counter, and stamps the perimeter + button rasters in `rasterize_chrome`. The Photon orb (chromatic starburst — same brand mark as the OS-level app icon) ships as a VSF image and decodes into the chrome's app_icon slot. Decode the bundled orb (the Photon brand mark, and the app_icon slot that swaps to a peer's avatar in a conversation). A decode failure logs LOUDLY instead of silently falling back to a plain coloured disk — a stale asset against a bumped vsf format is exactly how a blank orb shipped unnoticed, so make the next one scream rather than degrade in silence.
        let orb_icon = match fluor::host::icon::Icon::from_vsf_bytes(include_bytes!(
            "../../assets/photon-orb.vsf"
        )) {
            Ok(icon) => Some(icon),
            Err(e) => {
                crate::log(&format!(
                    "ORB: bundled photon-orb.vsf failed to decode ({e:?}) — orb falls back to a plain disk; the asset is likely stale against the current vsf format"
                ));
                None
            }
        };
        let mut chrome = DefaultChrome::new(
            ctx.viewport,
            "Photon",
            orb_icon,
            None,
            &mut self.hit_counter,
        );
        // Android: full-screen surface owns the whole display, so drop the desktop window chrome — no perimeter hairline, no top-right min/max/close buttons. Keeps the orb (connectivity indicator) on the top-left. set_full_edge skips draw_window_edges_and_mask; the `DEBUG_SKIP_CONTROLS` flag (also used by the desktop `[]l` chord) gates the controls-strip rasterization, so flipping it once at startup persistently suppresses the strip on Android.
        #[cfg(target_os = "android")]
        {
            chrome.set_full_edge(true);
            fluor::paint::DEBUG_SKIP_CONTROLS.store(true, std::sync::atomic::Ordering::Relaxed);
        }
        // Top-left orb's ring doubles as the FGTW connectivity indicator. Initialize red/offline; `try_recv_online` flips to green once the FGTW reports the device is reachable.
        chrome.set_orb_tint(orb_tint_for(false));
        self.chrome = Some(chrome);

        // Launch-screen widgets: handle textbox + attest button. Constructed with placeholder geometry; real geometry lands in `update_widget_layout` (called below and on every resize). Hit IDs are allocated from the shared counter AFTER chrome's four — chrome currently takes 1..=4, launch widgets get 5..=6, contacts widgets get 7..=8.
        self.textbox = Some(Textbox::new(&mut self.hit_counter, 0., 0., 1., 1., 12.));
        self.attest_btn = Some(Button::new(
            &mut self.hit_counter,
            0.,
            0.,
            1.,
            1.,
            12.,
            "Attest",
        ));
        // Contacts-page widgets — same placeholder shape; geometry set every frame via `update_widget_layout` based on ReadyLayout. The plus button label is "+" for now; the rotating-hourglass animation lands in a follow-up when we extract `ProgressButton` into fluor.
        self.contacts_textbox = Some(Textbox::new(&mut self.hit_counter, 0., 0., 1., 1., 12.));
        self.contacts_plus_btn = Some(Button::new(&mut self.hit_counter, 0., 0., 1., 1., 12., "+"));
        // Conversation compose box — placeholder geometry; positioned each frame via `update_widget_layout`.
        self.message_textbox = Some(Textbox::new(&mut self.hit_counter, 0., 0., 1., 1., 12.));
        // Send button overlaid in the compose box. ASCII ">" (not "→" U+2192 — absent from the Android font, so it rendered blank there; the contacts "+" button proves ASCII renders). Geometry set each frame in `update_widget_layout`. Empty label — the glyph is a drawn 4-vertex up arrowhead (draw_up_arrowhead), not text.
        self.message_send_btn = Some(Button::new(&mut self.hit_counter, 0., 0., 1., 1., 12., ""));
        // Specific subtle hover for the two overlay-in-textbox action buttons (pre-fluor per-control hover colours), instead of the generic saturated BUTTON_HOVER.
        if let Some(b) = self.contacts_plus_btn.as_mut() {
            b.set_hover_fill(Some(SEND_BUTTON_HOVER));
        }
        if let Some(b) = self.message_send_btn.as_mut() {
            b.set_hover_fill(Some(SEND_BUTTON_HOVER));
        }
        // Reserve a hit-id for the Ready-screen avatar circle. Not a Widget — the avatar is just a paint primitive — so click dispatch is handled directly in `on_event`'s MouseInput::Pressed arm, not thru `widget::dispatch_click`. Incrementing the shared counter keeps the contiguous-id contract intact for the `[]h` debug overlay.
        self.hit_counter = self.hit_counter.wrapping_add(1);
        self.avatar_hit_id = self.hit_counter;
        // Reserve a block of 256 hit IDs for contact rows. Row i stamps `contact_hit_base + i`.
        self.hit_counter = self.hit_counter.wrapping_add(1);
        self.contact_hit_base = self.hit_counter;
        self.hit_counter = self.hit_counter.wrapping_add(255);
        // Back button on conversation screen.
        self.hit_counter = self.hit_counter.wrapping_add(1);
        self.back_btn_hit_id = self.hit_counter;

        // "Start fresh (wipe this device)" tappable on the JOIN words screen — the only clean path for a device that was REMOVED from a fleet and so can't attest (can't reach the Security page). Two-tap confirm → clean_device_for_reuse.
        self.hit_counter = self.hit_counter.wrapping_add(1);
        self.join_startfresh_hit_id = self.hit_counter;

        // Green-confirm tappable on the AddDevice screen ("It's in — finish"): the two-phase press that releases the fleet-key rotation after the human sees the new device enrolled.
        self.hit_counter = self.hit_counter.wrapping_add(1);
        self.add_confirm_hit_id = self.hit_counter;

        // Tappable candidate rows on the AddDevice screen (BLE/list select): 8-id block, row i stamps base + i.
        self.hit_counter = self.hit_counter.wrapping_add(1);
        self.add_candidate_hit_base = self.hit_counter;
        self.hit_counter = self.hit_counter.wrapping_add(7);

        // Settings panel (STUB) hit-id blocks + widgets. Reserve a contiguous 9-id block for the nav-rail rows and a 32-id block for the immediate-mode action pills, then construct the stateful fluor widgets (dropdown / slider / textbox) and the custom checkboxes. All get placeholder geometry; `update_widget_layout` repositions the ones on the active page each frame.
        self.hit_counter = self.hit_counter.wrapping_add(1);
        self.settings_nav_base = self.hit_counter;
        self.hit_counter = self.hit_counter.wrapping_add(8); // rows 0..=8
        self.hit_counter = self.hit_counter.wrapping_add(1);
        self.settings_btn_base = self.hit_counter;
        self.hit_counter = self.hit_counter.wrapping_add(31); // pills 0..=31
        self.settings_theme_dropdown = Some(fluor::widgets::Dropdown::new(
            &mut self.hit_counter,
            0.,
            0.,
            1.,
            1.,
            12.,
            vec!["Dark chrome".to_string(), "Light chrome".to_string()],
        ));
        self.settings_zoom_slider =
            Some(fluor::widgets::Slider::new(&mut self.hit_counter, 0., 0., 1., 1., 0.5));
        self.settings_custodian_check = Some(crate::ui::settings_widgets::Checkbox::new(
            &mut self.hit_counter,
            "Be a custodian for others",
            0.,
            0.,
            1.,
            1.,
            12.,
            false,
        ));
        self.settings_chime_check = Some(crate::ui::settings_widgets::Checkbox::new(
            &mut self.hit_counter,
            "Chime on new message",
            0.,
            0.,
            1.,
            1.,
            12.,
            true,
        ));
        self.settings_presence_check = Some(crate::ui::settings_widgets::Checkbox::new(
            &mut self.hit_counter,
            "Show my presence to contacts",
            0.,
            0.,
            1.,
            1.,
            12.,
            true,
        ));
        self.settings_autoupdate_check = Some(crate::ui::settings_widgets::Checkbox::new(
            &mut self.hit_counter,
            "Install updates automatically",
            0.,
            0.,
            1.,
            1.,
            12.,
            true,
        ));
        self.settings_note_textbox = Some(Textbox::new(&mut self.hit_counter, 0., 0., 1., 1., 12.));

        self.update_widget_layout(ctx);

        // HandleQuery: device keypair is derived deterministically from the machine fingerprint (NEVER stored to disk — same machine yields the same keypair so attestations are reproducible across restarts). HandleQuery owns the UDP socket + sends/receives FGTW packets; an empty PeerStore wires the transport so query packets have somewhere to fan out to. The proxy expect is structurally safe: fluor's host calls `set_event_proxy` BEFORE `init` (see `run_app` in fluor/src/host/app.rs), so `event_proxy` is always `Some` here.
        let proxy = self
            .event_proxy
            .as_ref()
            .expect("event_proxy must be set before init (host contract)");
        // Prefer an externally-injected keypair (Android: PhotonContext sets it from NetworkContext before AndroidShell::new calls init). Fall back to deriving from the OS machine fingerprint — desktop reads /etc/machine-id etc., Android has no in-Rust fallback (Build.FINGERPRINT lives Java-side) so a missing keypair there is a panic-worthy programmer error: shipping a zero-derived keypair would silently downgrade every cryptographic identity in the app.
        let keypair = match self.device_keypair.take() {
            Some(kp) => kp,
            None => {
                #[cfg(not(target_os = "android"))]
                {
                    let fingerprint = get_machine_fingerprint()
                        .expect("device-key derivation: machine fingerprint unavailable");
                    derive_device_keypair(&fingerprint)
                }
                #[cfg(target_os = "android")]
                {
                    panic!(
                        "PhotonApp::set_device_keypair must be called before init on Android — \
                         the JNI shim wires thru the keypair derived from the OS fingerprint \
                         in PhotonConnectionService; a missing keypair here means the wiring was \
                         skipped and would produce a zeroed/insecure key derivation"
                    );
                }
            }
        };
        // Stash a clone for app-level operations that need the keypair after init (avatar upload via `upload_avatar`). The clone is cheap (Ed25519 keypair is ~64 bytes); we can't ask HandleQuery for it back because its constructor moves the keypair into the worker threads.
        self.device_keypair = Some(keypair.clone());
        #[cfg(not(target_os = "android"))]
        let hq = HandleQuery::new(keypair, proxy.clone());
        #[cfg(target_os = "android")]
        let hq = {
            let _ = proxy;
            HandleQuery::new(keypair)
        };
        let peer_store = Arc::new(Mutex::new(PeerStore::new()));
        hq.set_transport(peer_store);

        // Wire the CLUTCH job channels (replace the disconnected placeholders from `new`).
        {
            let (ktx, krx) = std::sync::mpsc::channel();
            self.clutch_keygen_tx = ktx;
            self.clutch_keygen_rx = krx;
            let (etx, erx) = std::sync::mpsc::channel();
            self.clutch_kem_encap_tx = etx;
            self.clutch_kem_encap_rx = erx;
            let (ctx_, crx) = std::sync::mpsc::channel();
            self.clutch_ceremony_tx = ctx_;
            self.clutch_ceremony_rx = crx;
            let (atx, arx) = std::sync::mpsc::channel();
            self.avatar_dl_tx = atx;
            self.avatar_dl_rx = arx;
            let (cctx, ccrx) = std::sync::mpsc::channel();
            self.clock_check_tx = cctx;
            self.clock_check_rx = ccrx;
            let (ictx, icrx) = std::sync::mpsc::channel();
            self.inbox_check_tx = ictx;
            self.inbox_check_rx = icrx;
        }

        // One-shot wall-clock sanity check via nunc-time, a few seconds behind attest (off-thread, so the several-seconds consensus query never blocks the UI). Warns via banner if the system clock is grossly wrong — never corrects it. Mid-session re-checks fire from the jump detector in `update`. On Android the wake handle is `None` (redraws come thru the JNI/Choreographer path); the result is drained on a subsequent tick.
        #[cfg(not(target_os = "android"))]
        crate::network::spawn_clock_check(self.clock_check_tx.clone(), Some(proxy.clone()));
        #[cfg(target_os = "android")]
        crate::network::spawn_clock_check(self.clock_check_tx.clone(), None);

        // One-shot fleet-inbox drain: pull any worker-observed alerts (bind attempts on our devices). Off-thread — a blocking HTTPS round trip — with the verdict drained on a later tick.
        self.spawn_inbox_drain();

        // Spawn the presence + CLUTCH status checker on HandleQuery's shared socket. Done BEFORE `hq` is moved into the field so we can take its socket. Without this the UDP recv/pong worker never runs — the socket is bound but nothing reads it or replies, so the device is invisible to every peer (no presence, no CLUTCH). The desktop and Android constructors differ only in the wake sender: desktop passes the winit event proxy; Android's redraws come thru the JNI/Choreographer path so its constructor takes none.
        #[cfg(not(target_os = "android"))]
        let checker_result = crate::network::status::StatusChecker::new(
            hq.socket(),
            self.device_keypair
                .clone()
                .expect("device_keypair set above"),
            self.contact_pubkeys.clone(),
            self.sync_records.clone(),
            proxy.clone(),
        );
        #[cfg(target_os = "android")]
        let checker_result = crate::network::status::StatusChecker::new(
            hq.socket(),
            self.device_keypair
                .clone()
                .expect("device_keypair set above"),
            self.contact_pubkeys.clone(),
            self.sync_records.clone(),
        );
        match checker_result {
            Ok(c) => {
                self.status_checker = Some(c);
                crate::log("UI: status checker started (presence + CLUTCH)");
            }
            Err(e) => crate::log(&format!("UI: status checker failed to start: {e}")),
        }

        self.handle_query = Some(hq);

        // Auto-resume from the remembered session roots. If tohu has this login's roots (persisted on a prior, FGTW-confirmed attest), paint Ready IMMEDIATELY from local state — we already own this identity, so there is no reason to block the first frame on the network. The avatar comes from a local cache file (no vault, no network); contacts + peer presence + cloud-merge arrive a beat later via the background `query_resume` and merge in thru `on_query_result`. A rejection (handle claimed by another device) bails back to the attest screen; a transient network error leaves the local session on Ready untouched. None (first run / post-logout) falls thru to the normal typed-attest flow.
        if let Some(remembered) = tohu::session() {
            self.session = Some(remembered);
            self.hints_dismissed = false; // fresh Ready entry → the avatar prompt gets a chance until first interaction
                                          // Initialize local storage and load contacts immediately so the contact list is visible before the FGTW round-trip completes.
            if let Some(kp) = &self.device_keypair {
                let device_secret = *kp.secret.as_bytes();
                // open_shared, NEVER new: query_resume below spawns the attest worker, which opens this same vault — a second independent engine racing this one is how the 2026-07-12 vault corruption happened (stale engine committed over the live one's blocks → seal verification failed at every subsequent open).
                match crate::storage::FlatStorage::open_shared(
                    crate::storage::APP,
                    remembered.vault_seed,
                    device_secret,
                ) {
                    Ok(s) => {
                        self.contacts = crate::storage::contacts::load_all_contacts(&s);
                        // Fleet siblings load from their own index (they never enter the contacts index).
                        {
                            let siblings = crate::storage::contacts::load_all_siblings(
                                remembered.handle_proof,
                                &s,
                            );
                            if !siblings.is_empty() {
                                crate::log(&format!(
                                    "SIBLING: loaded {} sibling(s) from local vault on resume",
                                    siblings.len()
                                ));
                            }
                            self.contacts.extend(siblings);
                        }
                        // Load each contact's conversation history too — load_all_contacts only loads per-peer contact STATE from the vault, not the messages (those live in the rārangi DB, loaded separately). Without this the resume frame paints contacts with empty message lists, and the later query_resume result can't fix it: on_query_result merges by handle_proof and SKIPS already-loaded contacts as duplicates, so the message-bearing copy is discarded → history looks wiped until the next app launch. Loading here makes resume show full history at once.
                        for contact in &mut self.contacts {
                            if let Err(e) = crate::storage::contacts::load_messages(contact, &s) {
                                crate::log(&format!(
                                    "UI: resume failed to load messages for {}: {}",
                                    crate::fp(&contact.handle_proof).as_str(),
                                    e
                                ));
                            }
                        }
                        crate::log(&format!(
                            "UI: loaded {} contact(s) from local vault on resume",
                            self.contacts.len()
                        ));
                        // Load friendship chains NOW too, not just contacts. Resume paints Ready and the status checker starts answering immediately, but chains used to arrive only later via query_resume — so any chat that landed in that window hit "No friendship found for conversation_token" and was DROPPED (no chain = no decrypt, no buffer). Loading chains here closes that gap so a peer messaging us the instant we come back online doesn't lose messages. query_resume still merges (and won't clobber these — it only adds ids we don't already hold).
                        let friendship_ids: Vec<crate::types::FriendshipId> =
                            self.contacts.iter().filter_map(|c| c.friendship_id).collect();
                        let loaded_chains =
                            crate::storage::friendship::load_all_friendships(&friendship_ids, &s);
                        for (fid, chains) in loaded_chains {
                            if !self.friendship_chains.iter().any(|(id, _)| *id == fid) {
                                self.friendship_chains.push((fid, chains));
                            }
                        }
                        self.update_sync_records();
                        // Seed the checker's answerable-pubkey set with every loaded contact's FULL fleet so pongs/offers from any of their devices are honoured.
                        self.reseed_contact_pubkeys();
                        // Wake-up catch-up: re-fold each contact's fleet so a friend's device added while we were off is honoured now, not next launch. Our OWN hp is included explicitly — the drain routes it to sibling reconcile (fleet weave), so a freshly-joined device discovers its siblings on first resume even with an empty contact list.
                        let mut hps: Vec<[u8; 32]> = self
                            .contacts
                            .iter()
                            .filter(|c| !c.is_sibling)
                            .map(|c| c.handle_proof)
                            .collect();
                        hps.push(remembered.handle_proof);
                        hps.sort_unstable();
                        hps.dedup();
                        self.spawn_contact_fleet_refresh(hps);
                        // Rehydrate each contact's saved ephemeral keypairs from disk (~588KB each). load_contact_state deliberately doesn't pull these (they're huge and live in a separate vault key), so without this every resume re-runs the McEliece-heavy keygen below — which is what froze the UI on launch. Loading the persisted keypairs makes the re-key filter a no-op for contacts that already have them, so keygen only fires for genuinely keyless Pending ones.
                        for contact in self.contacts.iter_mut() {
                            if contact.clutch_our_keypairs.is_none() {
                                match crate::storage::contacts::load_clutch_keypairs(
                                    &contact.handle_hash,
                                    &s,
                                ) {
                                    Ok(Some(keypairs)) => {
                                        contact.clutch_our_keypairs = Some(keypairs);
                                    }
                                    Ok(None) => {}
                                    Err(e) => crate::log(&format!(
                                        "CLUTCH: failed to rehydrate keypairs for {}: {}",
                                        crate::fp(&contact.handle_proof), e
                                    )),
                                }
                            }
                        }
                        self.storage = Some(s);
                        // Load this device's avatar from the vault now that storage exists, and colour-convert it for the Ready screen. The vault read needs the just-built storage handle, so this can't run before storage init like the old filesystem path did.
                        if let Some(storage) = self.storage.as_ref() {
                            self.device_avatar_pixels = crate::ui::avatar::load_avatar_from_seed(
                                &remembered.identity_seed,
                                storage,
                            )
                            .map(|(_, vsf_rgb)| {
                                crate::ui::colour_convert::vsf_rgb_to_bt2020(&vsf_rgb)
                            });
                        }
                        // Local vault had no avatar (e.g. this device was cleared) — recover our own from FGTW, where it was published. Off-thread; installs via the avatar drain.
                        if self.device_avatar_pixels.is_none() {
                            self.spawn_self_avatar_recover(remembered.identity_seed);
                        }
                        // Force any self-contact Complete before re-keying so it's excluded (a self-contact has no peer to key with).
                        self.settle_self_contacts();
                        // Re-key Pending contacts that still lack keypairs after the rehydrate — but ONE AT A TIME (spawn_next_pending_keygen, repeated each tick), never all at once: parallel McEliece keygens on launch starved the UI thread.
                        self.spawn_next_pending_keygen();
                    }
                    Err(e) => {
                        crate::log(&format!("STORAGE: init failed on resume: {}", e));
                        // A hard vault-open failure (e.g. seal verification failed) is the WORST storage state — no contacts load and nothing persists — yet it previously showed no warning, while a mere recoverable mirror-divergence (`degraded()`) did. Flag it so the red "storage degraded" banner surfaces a fully-broken vault too.
                        self.vault_degraded = true;
                    }
                }
            }
            self.state = AppState::Ready;
            if let Some(hq) = self.handle_query.as_ref() {
                crate::log("UI: resumed to Ready from local session roots (tohu) — FGTW announce + presence run in background");
                hq.query_resume(remembered);
            }
            // Kick presence immediately for the just-loaded contacts so their online rings reflect reality without waiting for the FGTW round-trip.
            self.ping_contacts();
        }
    }

    fn on_resize(&mut self, _width: u32, _height: u32, ctx: &mut Context) {
        if let Some(chrome) = self.chrome.as_mut() {
            // Use `ctx.viewport` directly — it carries the current `ru` (zoom factor) that fluor's host has already updated from Ctrl/Cmd +/-/0/scroll. Building a fresh `Viewport::new(w, h)` here would reset ru to 1.0 every resize/zoom event and silently strip the user's zoom state. Width/height are redundant with `ctx.viewport.{width_px, height_px}` for the same reason.
            chrome.resize(ctx.viewport);
            // Maximize toggles always change size between user-sized and screen-sized, so on_resize is the natural sync point for full_edge mode (no perimeter hairline / corner cutout / shadow when the window fills the screen). On Android the surface is always fullscreen — soft-keyboard show/hide triggers an on_resize too, and ctx.is_maximized is hard-coded false there, so without this override the perimeter + corner cutout would re-appear every time the IME opens.
            #[cfg(target_os = "android")]
            chrome.set_full_edge(true);
            #[cfg(not(target_os = "android"))]
            chrome.set_full_edge(ctx.is_maximized);
        }
        self.update_widget_layout(ctx);
    }

    // A clickable element was ACTIVATED — pointer went DOWN on `hit_id` and released over the SAME `hit_id`, no drag-off (press-hold-release, arbitrated by fluor's PointerArbiter). Every ACTION lives here so a mis-touch dragged off before release fires NOTHING. Press-time concerns (focus, textbox cursor, drag-select, window drag) stay in `on_event`'s Pressed arm; the raw press/release still arrive there.
    fn on_activate(
        &mut self,
        hit_id: HitId,
        x: Coord,
        y: Coord,
        mods: fluor::event::ModifiersState,
        ctx: &mut Context,
    ) -> EventResponse {
        // Avatar tap on Ready dispatches to the image picker — not a Widget, just a hit-stamp in chrome.hit_test_map. Drops focus first because the picker overlays the whole UI.
        if hit_id == self.avatar_hit_id
            && matches!(self.state, AppState::Ready)
            && self.avatar_hit_id != HIT_NONE
        {
            self.change_focus(None);
            // Android: a tap opens the system image picker directly (the picker IS the update mechanism — tapping the grey circle is self-evident, so no on-screen prompt). Desktop: no picker — the avatar updates by drag/drop — the tap is swallowed here.
            #[cfg(target_os = "android")]
            {
                self.pending_picker_request = true;
            }
            ctx.window.request_redraw();
            return EventResponse::Handled;
        }

        // "Start fresh (wipe this device)" on the JOIN words screen — a removed device's self-clean path. Two-tap confirm → full clean (nuke vault + clear session), leaving a blank slate ready to attest fresh or join another fleet.
        if hit_id == self.join_startfresh_hit_id && self.join_startfresh_hit_id != HIT_NONE {
            if self.join_startfresh_armed {
                self.join_startfresh_armed = false;
                self.end_add_device_flow(); // leave JOIN mode before wiping
                self.clean_device_for_reuse();
            } else {
                self.join_startfresh_armed = true;
            }
            ctx.window.request_redraw();
            return EventResponse::Handled;
        }

        // Green-confirm on the AddDevice screen: the two-phase press that releases the fleet-key rotation (only live while a bind awaits it).
        if hit_id == self.add_confirm_hit_id
            && self.add_confirm_hit_id != HIT_NONE
            && matches!(self.state, AppState::AddDevice)
        {
            self.spawn_confirm_add();
            ctx.window.request_redraw();
            return EventResponse::Handled;
        }

        // Candidate-row tap on the AddDevice screen (BLE / list select): bind the tapped device by its registry request (consent), then wait for the human's "did it turn green?" confirm (two-phase — a list pick isn't a typed-key match, so the key waits on visual confirmation).
        if self.add_candidate_hit_base != HIT_NONE
            && matches!(self.state, AppState::AddDevice)
            && self.add_device_bound.is_none()
            && !self.add_device_checking
            && hit_id >= self.add_candidate_hit_base
            && hit_id < self.add_candidate_hit_base.wrapping_add(7)
        {
            let idx = (hit_id - self.add_candidate_hit_base) as usize;
            if let Some(cand) = self.add_device_candidates.get(idx) {
                let req = cand.req.clone();
                self.add_device_bind_ble = true;
                self.spawn_bind_device(req);
                ctx.window.request_redraw();
                return EventResponse::Handled;
            }
        }

        // Back button — Conversation and Add-device both return to the contact list. Navigation is a dedicated control; the orb is settings-only.
        if hit_id == self.back_btn_hit_id && self.back_btn_hit_id != HIT_NONE {
            if matches!(self.state, AppState::Conversation) {
                self.state = AppState::Ready;
                self.active_contact = None;
                ctx.window.request_redraw();
                return EventResponse::Handled;
            }
            if matches!(self.state, AppState::AddDevice) {
                // Cancel returns to the Fleet page the flow came from.
                self.end_add_device_flow();
                self.state = AppState::Settings(SettingsPage::Fleet);
                ctx.window.request_redraw();
                return EventResponse::Handled;
            }
            if matches!(self.state, AppState::Settings(_)) {
                self.change_focus(None);
                self.state = AppState::Ready;
                ctx.window.request_redraw();
                return EventResponse::Handled;
            }
        }

        // Settings nav rail + stub action pills — hit-id ranges owned by the panel. Rail rows switch the page; pills are inert stubs (log only), except Fleet's "Add device" pill which opens the pairing-words flow.
        if let AppState::Settings(page) = self.state {
            if self.settings_nav_base != HIT_NONE
                && hit_id >= self.settings_nav_base
                && hit_id < self.settings_nav_base.wrapping_add(9)
            {
                let idx = (hit_id - self.settings_nav_base) as usize;
                if let Some(p) = SettingsPage::ALL.get(idx) {
                    self.change_focus(None);
                    // Leaving a page clears its selection/destructive-action arms (interaction-cleared).
                    if *p != SettingsPage::Fleet {
                        self.settings_fleet_selected = None;
                    }
                    if *p != SettingsPage::Security {
                        self.settings_shred_armed = false;
                    }
                    // Fresh page starts at the top — a leftover scroll from a longer page would strand a short one mid-air.
                    self.settings_content_scroll = 0.0;
                    self.state = AppState::Settings(*p);
                    ctx.window.request_redraw();
                }
                return EventResponse::Handled;
            }
            if self.settings_btn_base != HIT_NONE
                && hit_id >= self.settings_btn_base
                && hit_id < self.settings_btn_base.wrapping_add(32)
            {
                let slot = hit_id - self.settings_btn_base;
                if page == SettingsPage::Fleet {
                    if slot == 0 {
                        // "Add device" pill → the pairing-words flow.
                        self.open_add_device_flow();
                    } else if slot >= 16 {
                        // Device-row tap → select that device (non-self only; self rows aren't stamped).
                        let idx = (slot - 16) as usize;
                        let devices = self.fleet_device_rows();
                        if let Some((pk, is_self, ..)) = devices.get(idx) {
                            if !is_self {
                                self.settings_fleet_selected = Some(*pk);
                            }
                        }
                    } else {
                        // "Rename" (slot 1) is still a stub — no device-label chain-op yet. Remove-other retired 2026-07-13 with the sovereign-records rule (self-signed departure only; eviction = withholding at the key layer, arriving with the device-trust bundle).
                        crate::log("settings-stub: Rename (no label op yet)");
                    }
                } else if page == SettingsPage::Security {
                    if slot == 0 {
                        // "Lock" → clear session only (de-attest); vault kept, re-unlock by re-typing your handle. Works on Android (the -1 broadcast drops Kotlin's sticky session).
                        self.settings_shred_armed = false;
                        tohu::clear_session();
                        self.session = None;
                        self.private_s = crate::crypto::blind::PrivateS::None;
                        self.pending_broadcast_signal = -1;
                        self.state = AppState::Launch(LaunchState::Fresh);
                        self.refocus_handle_select_all();
                        crate::log("SECURITY: locked — session cleared, vault kept; re-type handle to unlock");
                    } else if slot == 2 {
                        // "Shred (crypto-wipe)" → full clean (nuke vault + clear session). Two-tap confirm (destructive + irreversible).
                        if self.settings_shred_armed {
                            self.settings_shred_armed = false;
                            self.clean_device_for_reuse();
                        } else {
                            self.settings_shred_armed = true;
                        }
                    } else {
                        // Slot 1 "Remove this device from fleet" (self-removal) is deferred.
                        self.settings_shred_armed = false;
                        crate::log("settings-stub: self-fleet-removal deferred");
                    }
                } else if page == SettingsPage::Diagnostics {
                    if slot == 0 {
                        // "Clear" → wipe the on-device log; the next line reopens a fresh, empty file.
                        crate::clear_log();
                        self.ready_toast = Some("Log cleared".to_string());
                    } else if slot == 1 {
                        // "Snapshot" → a peek at the current log size (a cheap "there's something to send" confirmation; the durable copy now lives on FGTW after Submit, not a local freeze).
                        match crate::snapshot_log_bytes() {
                            Some(b) => {
                                self.ready_toast =
                                    Some(format!("Log: {} KiB", (b.len() + 1023) / 1024))
                            }
                            None => self.ready_toast = Some("Log is empty".to_string()),
                        }
                    } else if slot == 2 {
                        // "Submit" → upload the log + optional note to FGTW (outbound HTTPS, NAT-immune — works where P2P is failing, no USB pull needed).
                        // Greyed guard — the disabled pill stamps no hit id, but the hit map is a frame stale right after a success, so a fast second tap could still dispatch here. Same predicate as the render.
                        let submit_disabled = self.log_submit_inflight
                            || self.log_submitted_len == Some(crate::log_size_bytes());
                        if !submit_disabled {
                            let note: String = self
                                .settings_note_textbox
                                .as_ref()
                                .map(|tb| tb.chars.iter().collect())
                                .unwrap_or_default();
                            self.spawn_log_submit(note);
                        }
                    }
                } else {
                    crate::log(&format!(
                        "settings-stub: pill {slot} on {:?} (no behaviour wired)",
                        page
                    ));
                }
                ctx.window.request_redraw();
                return EventResponse::Handled;
            }
        }

        // Orb tap (chrome app-icon) — a no-op widget, so intercept here. Destined for the settings/about/help panel; until that exists it carries the INTERIM add-device entry on Ready (AddDevice cancel is now the dedicated back button, not the orb). Routed by `on_orb_click`.
        let orb_id = self.chrome.as_ref().map(|c| c.app_icon_btn.id());
        if Some(hit_id) == orb_id && hit_id != HIT_NONE && self.on_orb_click() {
            ctx.window.request_redraw();
            return EventResponse::Handled;
        }

        // Contact row tap — hit IDs in [contact_hit_base, contact_hit_base + 255].
        if matches!(self.state, AppState::Ready)
            && self.contact_hit_base != HIT_NONE
            && hit_id >= self.contact_hit_base
            && hit_id < self.contact_hit_base.wrapping_add(256)
        {
            let ci = (hit_id - self.contact_hit_base) as usize;
            if ci < self.contacts.len() {
                crate::log(&format!(
                    "contact-tap: opening conversation with '{}'",
                    self.contacts[ci].display_name()
                ));
                self.active_contact = Some(ci);
                self.state = AppState::Conversation;
                self.change_focus(None);
                // Refresh this contact's presence on conversation-enter so the header reflects reality promptly.
                self.ping_contact(ci);
                // Fetch the peer's avatar (once/session) so the conversation header shows it instead of the grey placeholder. Cache-first, network on miss; off-thread. Keyed by the pin-set (hp + party id + avatar key) — no handle.
                self.spawn_avatar_download(ci);
                ctx.window.request_redraw();
                return EventResponse::Handled;
            }
        }

        // Focus ONLY a textbox on activation — it's the keyboard target, so a release over it focuses it + raises the soft IME. Buttons are deliberately NOT focused by a pointer tap: focusing a button made it stick in the dark `BUTTON_ACTIVE` tint (and swallow hover) after a drag-off. Keyboard users still Tab to a button to focus it for Enter/Space. Done before `dispatch_release` so focus is set before the textbox places its cursor.
        let is_textbox = [
            self.textbox.as_ref(),
            self.contacts_textbox.as_ref(),
            self.message_textbox.as_ref(),
            self.settings_note_textbox.as_ref(),
        ]
        .into_iter()
        .flatten()
        .any(|tb| tb.hit_id() == hit_id);
        if is_textbox && self.change_focus(Some(hit_id)) {
            ctx.window.request_redraw();
        }

        // Release-activated widgets (textbox cursor placement + attest / + / send Buttons): `dispatch_release` fires only `activate_on_release()` widgets — now including the textbox — so a release over the field places its cursor, and a Button's `Click::on_click` (→ `fire`) runs here; the Released arm's `take_click` polls then submit. A drag-off yields no activation → no fire, so nothing commits on a mis-touch.
        let response = widget::dispatch_release(self, hit_id, x, y, mods);
        if matches!(response, EventResponse::Handled) {
            ctx.window.request_redraw();
        }
        response
    }

    fn on_event(&mut self, event: &Event, ctx: &mut Context) -> EventResponse {
        // Any event is user engagement — reset the presence-sweep idle clock so the cadence returns to the active (5s) tier. Cheap (just a timestamp); the immediate-sweep-on-focus is handled in the Focused arm below.
        self.last_interaction = Some(Instant::now());
        // Every event except cursor movement may move immediate-mode content, so it claims a full-viewport frame. CursorMoved's effects are all narrow-tracked: hover tints live in the host overlay pass, drag-select is the textbox's own damage, and the one content-flavoured hover (the Ready avatar hint) sets `scene_dirty` at its flip site.
        if !matches!(event, Event::CursorMoved { .. }) {
            self.scene_dirty = true;
        }
        match event {
            Event::CursorMoved { .. } => {
                // Hit-test against the shared hit_test_map (chrome stamps its buttons, widgets stamp their pill silhouettes — all into chrome's map). `hit_at` returns the id at the cursor regardless of which widget owns the stamp; we route hover updates to each kind separately. Chrome sets its own hover state; widgets get their `set_hovered` flipped if the hit matches.
                let new_hit = self
                    .chrome
                    .as_ref()
                    .map(|c| c.hit_at(ctx.cursor_x, ctx.cursor_y))
                    .unwrap_or(HIT_NONE);
                // Frozen (busy) widgets are inert under the pointer for free: `set_enabled(false)` clears their hover and `Textbox/Button::set_hovered` is a no-op while disabled, so a cursor passing over a busy field can't re-light it — no per-state gate needed here.
                let mut changed = false;
                if let Some(chrome) = self.chrome.as_mut() {
                    changed |= chrome.set_hover(new_hit);
                }
                if let Some(tb) = self.textbox.as_mut() {
                    let want = new_hit == tb.hit_id();
                    if tb.is_hovered() != want {
                        tb.set_hovered(want);
                        changed = true;
                    }
                }
                if let Some(btn) = self.attest_btn.as_mut() {
                    let want = new_hit == btn.hit_id();
                    if btn.is_hovered() != want {
                        btn.set_hovered(want);
                        changed = true;
                    }
                }
                // Ready-screen search box + plus button. Their hit IDs only land in the map while the contacts screen renders them, so matching `new_hit` is naturally screen-safe — no state gate needed.
                if let Some(tb) = self.contacts_textbox.as_mut() {
                    let want = new_hit == tb.hit_id();
                    if tb.is_hovered() != want {
                        tb.set_hovered(want);
                        changed = true;
                    }
                }
                if let Some(btn) = self.contacts_plus_btn.as_mut() {
                    let want = new_hit == btn.hit_id();
                    if btn.is_hovered() != want {
                        btn.set_hovered(want);
                        changed = true;
                    }
                }
                // Conversation compose box + send button — same screen-safe matching (their ids only land
                // in the map while the conversation renders them).
                if let Some(tb) = self.message_textbox.as_mut() {
                    let want = new_hit == tb.hit_id();
                    if tb.is_hovered() != want {
                        tb.set_hovered(want);
                        changed = true;
                    }
                }
                if let Some(btn) = self.message_send_btn.as_mut() {
                    let want = new_hit == btn.hit_id();
                    if btn.is_hovered() != want {
                        btn.set_hovered(want);
                        changed = true;
                    }
                }
                {
                    let want = self.avatar_hit_id != HIT_NONE && new_hit == self.avatar_hit_id;
                    if self.avatar_hovered != want {
                        self.avatar_hovered = want;
                        // The avatar hover hint is CONTENT (drawn text, not an overlay tint), so its flip needs the full-viewport frame CursorMoved otherwise avoids.
                        self.scene_dirty = true;
                        changed = true;
                    }
                }
                if changed {
                    ctx.window.request_redraw();
                }
                EventResponse::Pass
            }
            Event::CursorLeft { .. } => {
                let mut changed = false;
                if let Some(chrome) = self.chrome.as_mut() {
                    changed |= chrome.set_hover(HIT_NONE);
                }
                if let Some(tb) = self.textbox.as_mut() {
                    if tb.is_hovered() {
                        tb.set_hovered(false);
                        changed = true;
                    }
                }
                if let Some(btn) = self.attest_btn.as_mut() {
                    if btn.is_hovered() {
                        btn.set_hovered(false);
                        changed = true;
                    }
                }
                if let Some(tb) = self.contacts_textbox.as_mut() {
                    if tb.is_hovered() {
                        tb.set_hovered(false);
                        changed = true;
                    }
                }
                if let Some(btn) = self.contacts_plus_btn.as_mut() {
                    if btn.is_hovered() {
                        btn.set_hovered(false);
                        changed = true;
                    }
                }
                if let Some(tb) = self.message_textbox.as_mut() {
                    if tb.is_hovered() {
                        tb.set_hovered(false);
                        changed = true;
                    }
                }
                if let Some(btn) = self.message_send_btn.as_mut() {
                    if btn.is_hovered() {
                        btn.set_hovered(false);
                        changed = true;
                    }
                }
                if changed {
                    ctx.window.request_redraw();
                }
                EventResponse::Pass
            }
            Event::ModifiersChanged(mods) => {
                // Zoom hint persists only while a zoom modifier is held. The instant Ctrl/Cmd is released, drop the top-centre percentage watermark (render arms it when `ru` changes under a held modifier). Releasing focus mid-zoom also lands here via the WM clearing modifiers.
                if !(mods.control_key() || mods.super_key()) && self.zoom_hint {
                    self.zoom_hint = false;
                    // The watermark lives in the bg layer, which `rasterize_bg` only repaints when dirty — invalidate it so the clearing frame actually re-runs the closure without the hint, instead of leaving the stale glyphs painted.
                    if let Some(chrome) = self.chrome.as_mut() {
                        chrome.invalidate_bg();
                    }
                    ctx.window.request_redraw();
                }
                EventResponse::Pass
            }
            Event::Focused(focused) => {
                // On focus GAIN, force an immediate presence sweep so rings are fresh the instant the user looks — clearing last_presence_ping makes the next tick treat a sweep as due regardless of how far the idle cadence had backed off. (last_interaction was already stamped at the top of on_event, resetting the cadence to the active tier.)
                if *focused {
                    self.last_presence_ping = None;
                    ctx.window.request_redraw();
                }
                // Chrome's edges + title + orb dim when the window loses focus (palette swap to `WINDOW_*_UNFOCUSED` + `TEXT_COLOUR_UNFOCUSED` + `ORB_DARKEN_UNFOCUSED`). The host independently dims the drop shadow via its own `is_focused` tracker; this handler just propagates to chrome's internal flag so the chrome layer re-rasterizes with the dimmed palette.
                if let Some(chrome) = self.chrome.as_mut() {
                    if chrome.set_focused(*focused) {
                        ctx.window.request_redraw();
                    }
                }
                EventResponse::Pass
            }
            Event::MouseWheel { delta } => {
                // Bg-noise scroll. Vertical-only for now — horizontal trackpad gestures and shift-modified wheel both fold into the same `bg_scroll` axis. Discrete wheel notches (`Lines`) get multiplied to feel like a normal scroll step; continuous trackpad pixels (`Pixels`) are used directly. The scroll value feeds both `scroll_offset` (translates the noise pattern up/down on screens that want it) and `shimmer` (colour-bias cycle on every screen) in `render`.
                let dy = match delta {
                    MouseScrollDelta::Lines(_, y) => (*y as isize) * 8,
                    MouseScrollDelta::Pixels(_, y) => *y as isize,
                };
                if dy != 0 {
                    if matches!(self.state, AppState::Ready) {
                        // On the contacts screen the wheel scrolls the WHOLE user section + list as one block. Down-scroll (negative dy) moves the block up (reveals lower contacts), so subtract; the render pass clamps to the full-block extent and re-runs `update_widget_layout` after the clamp so the search box + plus button (whose rects are set off `contacts_scroll`) track the clamped offset.
                        self.contacts_scroll = (self.contacts_scroll - dy).max(0);
                    } else if matches!(self.state, AppState::Settings(_)) {
                        // Settings: the wheel scrolls the nav rail when the cursor is over it, else the content pane. Down-scroll (negative dy) reveals lower rows → add. The render pass clamps both to their natural-height extents.
                        let over_rail = {
                            let sl = SettingsLayout::compute(&ctx.viewport);
                            (ctx.cursor_x as f32) < sl.content.x
                        };
                        let step = -dy as f32;
                        if over_rail {
                            self.settings_rail_scroll = (self.settings_rail_scroll + step).max(0.0);
                        } else {
                            self.settings_content_scroll = (self.settings_content_scroll + step).max(0.0);
                        }
                    } else if matches!(self.state, AppState::Conversation) {
                        // In a conversation the wheel scrolls the message history. The list lays out bottom-up with newest at the bottom; a positive offset pushes messages down (reveals older ones above). Scroll-up (positive dy) shows older → add.
                        if let Some(ci) = self.active_contact {
                            if let Some(contact) = self.contacts.get_mut(ci) {
                                contact.message_scroll_offset =
                                    (contact.message_scroll_offset + dy as f32 * 8.0).max(0.0);
                                // Scrollback jumps the history-backfill queue: the user is heading toward the old edge, so the next page request fires on the next tick instead of waiting out the trickle interval.
                                if dy > 0 {
                                    if let Some(rec) = contact.history_recovery.as_mut() {
                                        if !rec.complete {
                                            rec.urgent = true;
                                        }
                                    }
                                }
                            }
                        }
                    } else {
                        self.bg_scroll = self.bg_scroll.wrapping_add(dy);
                    }
                    if let Some(chrome) = self.chrome.as_mut() {
                        chrome.invalidate_bg();
                        // Scrolling moves the content (and therefore every per-pixel hit zone) but doesn't dirty the chrome layer on its own, so `rasterize_chrome` would early-return and skip its `hit_test_map.fill(HIT_NONE)` — leaving STALE hit stamps at the pre-scroll row/widget positions. Those ghosts make `hit_at` return the wrong id under the cursor after a scroll, so the hover overlay tints the wrong pixels. Invalidate chrome so the map is cleared and re-stamped against this frame's scrolled positions.
                        chrome.invalidate_chrome();
                    }
                    ctx.window.request_redraw();
                }
                EventResponse::Pass
            }
            Event::MouseInput {
                state: ElementState::Pressed,
                button: MouseButton::Left,
                ..
            } => {
                // Any click dismisses the standing hints (event-driven — never hover or time).
                self.clear_hints();
                let hit_id = self
                    .chrome
                    .as_ref()
                    .map(|c| c.hit_at(ctx.cursor_x, ctx.cursor_y))
                    .unwrap_or(HIT_NONE);

                // Permanence interstitial ("Yes — forever"): a press ANYWHERE other than the attest button cancels back to the pre-proof Fresh state. Editing the handle already cancels; this makes a tap on empty space, the field, the orb — anything else — cancel too, so a stray tap can never corner the user into the forever-claim (on Android "click elsewhere" was otherwise swipe-up → home → long-press → switch away). The attest button press itself is the deliberate confirm, so it's excluded; we fall thru afterwards so the tap still does its normal thing (focus the field, start a drag, open settings, …).
                if matches!(self.state, AppState::Launch(LaunchState::Confirm)) {
                    let attest_hit = self.attest_btn.as_ref().map(|b| b.hit_id()).unwrap_or(HIT_NONE);
                    if hit_id != attest_hit {
                        self.clear_launch_error();
                        ctx.window.request_redraw();
                    }
                }

                if hit_id == HIT_NONE {
                    // No widget under the cursor — clear focus, then fall back to resize-edge / title-bar drag. Resize edge takes precedence; clicks anywhere else inside the visible window start a move-drag (which the host promotes to an actual drag once the cursor passes the dead-zone threshold).
                    if self.change_focus(None) {
                        ctx.window.request_redraw();
                    }
                    let edge = chrome::get_resize_edge(ctx.viewport, ctx.cursor_x, ctx.cursor_y);
                    if edge != ResizeEdge::None {
                        return EventResponse::StartResize(edge);
                    }
                    return EventResponse::StartWindowDrag;
                }

                // Every item — contacts, pills, nav, orb, back, avatar, start-fresh, the Buttons, AND the textboxes — now activates on RELEASE over the same element (fluor's PointerArbiter → `on_activate`); a drag-off before release cancels. So the press arm does NO activation and NO focus change: focusing on press was what left a button stuck in its dark focused tint after a drag-off (and swallowed hover). The host has already armed the element (held colour); we just consume the press so it doesn't fall through to a window drag.
                ctx.window.request_redraw();
                EventResponse::Handled
            }
            Event::MouseInput {
                state: ElementState::Released,
                button: MouseButton::Left,
                ..
            } => {
                // Attest button: poll `take_click` AFTER release — Button::on_click increments the counter at press; we observe the rising edge here so submit fires once per press/release pair regardless of how chrome dispatches subsequent events.
                let clicked = self
                    .attest_btn
                    .as_mut()
                    .map(|b| b.take_click())
                    .unwrap_or(false);
                if clicked {
                    self.submit_handle();
                    ctx.window.request_redraw();
                }
                // Contacts plus button — same release-edge polling pattern.
                let plus_clicked = self
                    .contacts_plus_btn
                    .as_mut()
                    .map(|b| b.take_click())
                    .unwrap_or(false);
                if plus_clicked {
                    self.submit_add_friend();
                    ctx.window.request_redraw();
                }
                // Conversation send button — same release-edge polling pattern as the plus button.
                let send_clicked = self
                    .message_send_btn
                    .as_mut()
                    .map(|b| b.take_click())
                    .unwrap_or(false);
                if send_clicked {
                    self.submit_message();
                    // Return focus to the compose box so the send button releases its focused/active
                    // (dark, pressed-in) tint — otherwise it sticks down — and the user keeps typing.
                    if let Some(id) = self.message_textbox.as_ref().map(|t| t.hit_id()) {
                        self.change_focus(Some(id));
                    }
                    ctx.window.request_redraw();
                }
                EventResponse::Pass
            }
            Event::KeyboardInput { event: kev, .. } => {
                // Any keystroke dismisses the standing hints (event-driven — never hover or time).
                self.clear_hints();
                // Bracket chord first — tracks Press/Release timestamps regardless of focus so the debug overlay arms as soon as both brackets are held, and the chord action runs before delivery to the focused widget (so an action letter like 'h' doesn't also type into the textbox).
                if let Key::Character(c) = &kev.logical_key {
                    let cs = c.as_str();
                    let now = Instant::now();
                    let mut action_char: Option<char> = None;
                    match (cs, kev.state) {
                        ("[", ElementState::Pressed) => self.chord_lb_press = Some(now),
                        ("[", ElementState::Released) => self.chord_lb_release = Some(now),
                        ("]", ElementState::Pressed) => self.chord_rb_press = Some(now),
                        ("]", ElementState::Released) => self.chord_rb_release = Some(now),
                        (_, ElementState::Pressed) if !kev.repeat => {
                            if self.brackets_held(now) {
                                action_char = c.to_ascii_lowercase().chars().next();
                            }
                        }
                        _ => {}
                    }
                    if cs == "[" || cs == "]" {
                        ctx.window.request_redraw();
                    }
                    if let Some(ac) = action_char {
                        if self.handle_chord_action(ac, ctx) {
                            return EventResponse::Handled;
                        }
                    }
                }

                // Press-only routing for Tab / Esc / Enter and delivery to the focused widget. Released arms (key-up) don't insert characters or trigger actions, so we no-op them. `repeat` keys DO insert characters (auto-repeat typing) so we don't filter on it here.
                if kev.state != ElementState::Pressed {
                    return EventResponse::Pass;
                }

                // Clipboard chords (Ctrl/Cmd + C / X / V) are intercepted HERE, before delivery to the focused widget — fluor's design keeps the OS clipboard (arboard) with the app, not on Textbox (the clipboard is a single global resource; threading it thru every widget would be premature). Ctrl+A stays on the widget (pure selection, no OS resource). Desktop only: Android paste arrives thru the IME commit path, and Redox has no arboard backend.
                #[cfg(not(any(target_os = "redox", target_os = "android")))]
                if ctx.modifiers.control_key() || ctx.modifiers.super_key() {
                    if let Key::Character(c) = &kev.logical_key {
                        let lc = c.to_lowercase();
                        if lc == "c" || lc == "x" || lc == "v" {
                            let resp = self.clipboard_chord(&lc, ctx.text);
                            if matches!(resp, EventResponse::Handled) {
                                ctx.window.request_redraw();
                                self.blink_timer.start(Instant::now());
                            }
                            return resp;
                        }
                    }
                }

                match &kev.logical_key {
                    // Tab cycles focus thru the widget tree in registration order (launch widgets first, then chrome). Intercepted BEFORE delivery so textbox can't swallow it as "\t" insertion.
                    Key::Named(NamedKey::Tab) => {
                        let dir = if ctx.modifiers.shift_key() {
                            TabDir::Backward
                        } else {
                            TabDir::Forward
                        };
                        let current_focus = self.focused;
                        let next = widget::linear_tab_next(self, current_focus, dir);
                        if self.change_focus(next) {
                            ctx.window.request_redraw();
                        }
                        EventResponse::Handled
                    }
                    // Esc clears focus. Also cancels an in-flight attestation back to Fresh — without this the user is stuck on the "Attesting…" indicator with no way out if the FGTW response never lands (offline FGTW, peer worker stall, etc.). Android back routes here via `nativeOnBackPressed` → Escape.
                    Key::Named(NamedKey::Escape) => {
                        if matches!(self.state, AppState::Conversation) {
                            self.state = AppState::Ready;
                            self.active_contact = None;
                            ctx.window.request_redraw();
                            return EventResponse::Handled;
                        }
                        if matches!(self.state, AppState::AddDevice) {
                            // Escape cancels back to the Fleet page the flow came from.
                            self.end_add_device_flow();
                            self.state = AppState::Settings(SettingsPage::Fleet);
                            ctx.window.request_redraw();
                            return EventResponse::Handled;
                        }
                        // Cancel launch JOIN mode (stops the join thread so it quits re-posting its request).
                        if self.launch_add_mode {
                            self.launch_add_mode = false;
                            self.end_join_flow();
                            self.add_join_status.clear();
                            if let Some(tb) = self.textbox.as_mut() {
                                tb.clear();
                            }
                            ctx.window.request_redraw();
                            return EventResponse::Handled;
                        }
                        if matches!(self.state, AppState::Launch(LaunchState::Attesting)) {
                            self.state = AppState::Launch(LaunchState::Fresh);
                            ctx.window.request_redraw();
                            return EventResponse::Handled;
                        }
                        if self.change_focus(None) {
                            ctx.window.request_redraw();
                        }
                        EventResponse::Handled
                    }
                    // Enter submits the handle when the textbox is focused — intercepted before delivery so the textbox doesn't insert a literal newline. When the attest button is focused, route to its on_key (Button activates on Enter / Space and we observe via take_click in tick / on_event Release path). Both Launch and Ready screens follow the same shape with their respective widgets.
                    Key::Named(NamedKey::Enter) => {
                        let focused_is_launch_textbox = self
                            .textbox
                            .as_ref()
                            .map(|t| Some(t.hit_id()) == self.focused)
                            .unwrap_or(false);
                        if focused_is_launch_textbox {
                            if matches!(self.state, AppState::AddDevice) {
                                // Words-entry screen: Enter nudges a re-match (the live matcher re-derives on every edit anyway; this covers "candidates arrived after I finished typing").
                                self.refresh_add_device_match();
                            } else {
                                self.submit_handle();
                            }
                            ctx.window.request_redraw();
                            return EventResponse::Handled;
                        }
                        let focused_is_contacts_textbox = self
                            .contacts_textbox
                            .as_ref()
                            .map(|t| Some(t.hit_id()) == self.focused)
                            .unwrap_or(false);
                        if focused_is_contacts_textbox {
                            self.submit_add_friend();
                            ctx.window.request_redraw();
                            return EventResponse::Handled;
                        }
                        let focused_is_compose = self
                            .message_textbox
                            .as_ref()
                            .map(|t| Some(t.hit_id()) == self.focused)
                            .unwrap_or(false);
                        if focused_is_compose {
                            // Shift+Enter inserts a newline (multi-line compose); plain Enter sends.
                            if ctx.modifiers.shift_key() {
                                if let Some(focus_id) = self.focused {
                                    let resp = widget::dispatch_key(
                                        self,
                                        focus_id,
                                        kev,
                                        ctx.modifiers,
                                        ctx.text,
                                    );
                                    if matches!(resp, EventResponse::Handled) {
                                        ctx.window.request_redraw();
                                    }
                                    return resp;
                                }
                            }
                            self.submit_message();
                            ctx.window.request_redraw();
                            return EventResponse::Handled;
                        }
                        if let Some(focus_id) = self.focused {
                            let resp =
                                widget::dispatch_key(self, focus_id, kev, ctx.modifiers, ctx.text);
                            // Either button can activate on Enter; poll both and route to the matching submit.
                            let attest_clicked = self
                                .attest_btn
                                .as_mut()
                                .map(|b| b.take_click())
                                .unwrap_or(false);
                            if attest_clicked {
                                self.submit_handle();
                            }
                            let plus_clicked = self
                                .contacts_plus_btn
                                .as_mut()
                                .map(|b| b.take_click())
                                .unwrap_or(false);
                            if plus_clicked {
                                self.submit_add_friend();
                            }
                            // Send button Space-activation (when focused).
                            let send_clicked = self
                                .message_send_btn
                                .as_mut()
                                .map(|b| b.take_click())
                                .unwrap_or(false);
                            if send_clicked {
                                self.submit_message();
                            }
                            if attest_clicked
                                || plus_clicked
                                || send_clicked
                                || matches!(resp, EventResponse::Handled)
                            {
                                ctx.window.request_redraw();
                            }
                            return resp;
                        }
                        EventResponse::Pass
                    }
                    // All other keys → focused widget via dispatch_key. The Textbox's on_key handles character insertion, backspace, arrows, selection, clipboard (Ctrl+A); Button's on_key handles Space activation. Unfocused → Pass so the host can ignore. Request redraw on Handled so character insertion paints immediately instead of waiting for the next tick.
                    _ => {
                        // Words-entry screen accepts ONLY letters and space — the 23 pairing words are ASCII-alphabetic voca words, so digits/punctuation/emoji are always typos. Named keys (backspace, arrows, Tab) aren't Character events and pass thru untouched.
                        if matches!(self.state, AppState::AddDevice) {
                            if let Key::Character(c) = &kev.logical_key {
                                if !c.chars().all(|ch| ch.is_ascii_alphabetic() || ch == ' ') {
                                    return EventResponse::Handled;
                                }
                            }
                        }
                        if let Some(focus_id) = self.focused {
                            // Snapshot the handle text so an EDIT (typing, backspace, delete-selection — any content change) tears down the Error/Confirm interstitial. The clipboard chords do this explicitly; this covers the plain-keystroke path, which previously didn't — so a user could arm Confirm on handle A, retype it to handle B, and the press fired A's probed roots (observed: attested as the fresh handle while the box showed the taken one).
                            let launch_text_before: Option<Vec<char>> =
                                if matches!(self.state, AppState::Launch(_)) {
                                    self.textbox.as_ref().map(|tb| tb.chars.clone())
                                } else {
                                    None
                                };
                            let resp =
                                widget::dispatch_key(self, focus_id, kev, ctx.modifiers, ctx.text);
                            if let Some(before) = launch_text_before {
                                if self.textbox.as_ref().map(|tb| &tb.chars) != Some(&before) {
                                    self.clear_launch_error();
                                }
                            }
                            if matches!(resp, EventResponse::Handled) {
                                ctx.window.request_redraw();
                                // Reset blink so the cursor stays solid thru fast typing instead of blinking mid-keystroke.
                                self.blink_timer.start(Instant::now());
                            }
                            return resp;
                        }
                        EventResponse::Pass
                    }
                }
            }
            Event::Ime(Ime::Commit(s)) => {
                // IME typing also dismisses the standing hints (event-driven — never hover or time).
                self.clear_hints();
                // Android: soft IME committed `s` (typing, swipe, autocomplete). Route it to whichever textbox holds focus — the attest handle field OR the contacts search box. (This used to be hardcoded to the attest box, so typing on the contacts screen was silently dropped on Android even though focus + the soft keyboard were correct; desktop never hit this because physical keys go thru the focus-generic `widget::dispatch_key`.) Backspace arrives as the literal "\b" character from PhotonSurfaceView's deleteSurroundingText / composing-text replacement path, so peel those off and route to `backspace`; everything else inserts verbatim. No-op when no textbox is focused (focus might sit on the attest button via Tab).
                let mut handled = false;
                let words_screen = matches!(self.state, AppState::AddDevice);
                if let Some(tb) = self.focused_textbox_mut() {
                    for c in s.chars() {
                        if c == '\u{0008}' {
                            tb.backspace(ctx.text);
                        } else if !words_screen || c.is_ascii_alphabetic() || c == ' ' {
                            // Words entry accepts only letters and space — swipe/autocomplete punctuation is silently dropped.
                            tb.insert_char(c, ctx.text);
                        }
                    }
                    handled = true;
                }
                if handled {
                    // Soft-IME edits are edits: tear down the Error/Confirm interstitial exactly like physical keystrokes, so Android can't re-arm stale probed roots either.
                    if matches!(self.state, AppState::Launch(_)) {
                        self.clear_launch_error();
                    }
                    self.blink_timer.start(Instant::now());
                    ctx.window.request_redraw();
                    return EventResponse::Handled;
                }
                EventResponse::Pass
            }
            Event::DroppedFile(path) => {
                // Desktop avatar update: a file dropped on the window (Ready screen) is read and run thru the same encode→save→load→install→upload pipeline as the Android picker. Ignored off the Ready screen and when no handle is attested yet (set_avatar_from_file no-ops without a handle). Android has no drop path — it uses the picker.
                if matches!(self.state, AppState::Ready) {
                    match std::fs::read(path) {
                        Ok(bytes) => {
                            self.set_avatar_from_file(bytes);
                            ctx.window.request_redraw();
                        }
                        Err(e) => crate::log(&format!("avatar drop: read failed: {e}")),
                    }
                }
                EventResponse::Handled
            }
            _ => EventResponse::Pass,
        }
    }

    fn wake_at(&self) -> Option<Instant> {
        // Schedule the next wakeup at the soonest of: * `blink_timer.next_tick()` — drives the focused-textbox cursor pulse (random 0-300ms intervals); `None` while no textbox is focused.
        // * `now` when an attestation is in flight — `tick()` advances `attest_anim_phase` at 1 cycle/sec for the "query in flight" wave shift; we need a wakeup every frame to keep it animating smoothly. Without this, the host blocks waiting for input and the animation stalls.
        let blink = self.blink_timer.next_tick();
        // An attestation OR an in-flight add-friend search both need a wakeup every frame to animate (the spectrum wave / the hourglass wobble).
        let animating = matches!(
            self.state,
            AppState::Launch(LaunchState::Attesting) | AppState::Searching
        ) || self.add_in_flight;
        let anim = animating.then(Instant::now);
        // Next background presence sweep — keeps online/offline rings refreshing while idle (no input/network). Only on Ready; first sweep is due immediately if never run. Interval tapers with idle time, so as the user stays away the scheduled wake naturally pushes further out.
        let presence = matches!(self.state, AppState::Ready).then(|| {
            let now = Instant::now();
            self.last_presence_ping
                .map_or(now, |last| last + self.presence_ping_interval(now))
        });
        // Pairing flows: join-words (new device) and add-device matcher/confirm (old device) results arrive on mpsc channels from worker threads, with nothing else guaranteed to drive a tick while the user's hands are off — so poll-drain at 2 Hz while either flow is live. This is channel plumbing, not time-based UI: nothing is shown or cleared on a clock.
        let pairing = (self.add_join_rx.is_some() || self.add_device_rx.is_some())
            .then(|| Instant::now() + std::time::Duration::from_millis(500));
        // Periodic own-chain re-fold (the fleet-membership doorbell) — scheduled on the screens where a stale fleet view matters, so it fires even while the desktop window sits idle on the Fleet page. 45s matches advance_protocol's cadence.
        let fleet_refold = matches!(self.state, AppState::Ready | AppState::Conversation | AppState::Settings(_))
            .then(|| self.last_fleet_refold.map_or_else(Instant::now, |last| last + std::time::Duration::from_secs(45)));
        // Soonest of all scheduled wakeups.
        [blink, anim, presence, pairing, fleet_refold].into_iter().flatten().min()
    }

    fn tick(&mut self, ctx: &mut Context) -> bool {
        let now = Instant::now();
        let mut needs_redraw = false;

        // Freeze / unfreeze the busy widgets (attest field+button while attesting, search box+plus while adding) before anything else this frame — disabled widgets drop out of dispatch via their fluor accessors.
        self.sync_busy_freeze();

        // Pairing v2 SHADOW-mode beacon scan (docs/pairing-v2.md milestone A): scan exactly while the AddDevice screen is up, diffed per tick so EVERY exit path (back, orb, bind, crash of the check thread) stops the radio without scattered call sites. Heard beacons only log + store; v1 words still carry the ceremony.
        let want_scan = matches!(self.state, AppState::AddDevice);
        if want_scan != self.beacon_scan_active {
            if want_scan {
                if let Some(hp) = self.session.as_ref().map(|s| s.handle_proof) {
                    crate::network::pairing_beacon::start_scan(fgtw::pair::hp_prefix(&hp));
                    self.beacon_scan_active = true;
                }
                // No session → no hp to filter on; stay inactive and re-try next tick (harmless — AddDevice without a session shouldn't exist anyway).
            } else {
                crate::network::pairing_beacon::stop_scan();
                self.beacon_scan_active = false;
            }
        }

        // Compute per-tick delta_time for the attest-animation accumulator. `last_tick` is None on the very first tick — bootstrap to "zero elapsed" so the accumulator doesn't take a huge jump on startup.
        let delta_time = match self.last_tick {
            Some(prev) => now.duration_since(prev).as_secs_f32(),
            None => 0.,
        };
        self.last_tick = Some(now);

        // Spectrum animation while attesting: wave phase advances at 2π rad/sec = 1 cycle/sec. Provides the visual "query in flight" cue the legacy build had — the bar slowly slides while we wait for FGTW to answer. Idle / Fresh / Error states leave the phase frozen so the screen stays calm.
        if matches!(self.state, AppState::Launch(LaunchState::Attesting))
            || matches!(self.state, AppState::Searching)
        {
            self.attest_anim_phase += delta_time * std::f32::consts::TAU;
            self.attest_anim_phase %= std::f32::consts::TAU;
            if let Some(chrome) = self.chrome.as_mut() {
                chrome.invalidate_bg();
            }
            needs_redraw = true;
        }

        // Add-friend hourglass: stochastic wobble (≈ −12..+13°/tick) while a search is in flight, so the icon "shakes" like sand. xorshift keeps it dependency-free; the icon lives in the foreground (not the bg layer), so a plain redraw repaints it.
        if self.add_in_flight {
            self.hourglass_rng ^= self.hourglass_rng << 13;
            self.hourglass_rng ^= self.hourglass_rng >> 7;
            self.hourglass_rng ^= self.hourglass_rng << 17;
            let wobble = (self.hourglass_rng % 26) as f32 - 12.0; // −12..+13
            self.hourglass_angle = (self.hourglass_angle + wobble).rem_euclid(360.0);
            needs_redraw = true;
        }

        // Drive the blinkey on the focused textbox. `BlinkTimer::poll(now)` returns `true` ONLY on the rising edge of each fire (then schedules the next random 0-300ms interval and returns false the rest of the time). On each fire, toggle the focused textbox's blinkey via `flip_blinkey` — which is a no-op on an unfocused textbox, so we can call it on every textbox without gating. Tracked SEPARATELY from `needs_redraw`: a blinkey flip is fully covered by the textbox's own `damage_rect`, so a pure-blink frame must not raise `scene_dirty` — that's what keeps the idle repaint a teeny cursor-sized rect instead of the whole window.
        let mut blink_redraw = false;
        if self.blink_timer.poll(now) {
            for (_, tb) in self.textboxes_mut() {
                if tb.flip_blinkey() {
                    blink_redraw = true;
                }
            }
        }

        // Everything network/protocol lives in advance_protocol(): presence sweep, channel drains, CLUTCH ceremony + chain advancement, retransmits. It touches NO surface, so it can also run headless from the Android foreground service while the app is backgrounded (screen off ⇒ the Choreographer stops calling tick, but the state is alive — see docs/background-tick.md). The frame-only work (animations above, render below) stays here in tick.
        needs_redraw |= self.advance_protocol(now);

        // Content-flavoured redraws dirty the scene (full-viewport frame); a pure blinkey flip stays out so its frame narrows to the textbox's own damage rect.
        self.scene_dirty |= needs_redraw;
        let redraw = needs_redraw || blink_redraw;
        if redraw {
            ctx.window.request_redraw();
        }
        redraw
    }

    fn damage_rect(&self, viewport: Viewport) -> Option<PixelRect> {
        let vw = viewport.width_px as usize;
        let vh = viewport.height_px as usize;
        // Full viewport whenever immediate-mode content may have moved (`scene_dirty`), and whenever the chord hint is up or just released (stale hint pixels need one covering frame to clear).
        let chord = self.last_chord_held || self.brackets_held(Instant::now());
        if self.scene_dirty || chord {
            let mut combined = PixelRect::new(0, 0, vw, vh);
            if chord {
                combined = combined.union(chord_hint_bbox(viewport, vw, vh));
            }
            return Some(combined);
        }
        // Pure widget frame (blinkey flip, drag-select growth): union each widget's self-reported damage. Gates MUST mirror `visit`'s render gates — claiming a rect for a widget that won't be rendered would clear its pixels to bare background. `None` = nothing changed, host skips the render entirely.
        let mut combined: Option<PixelRect> = None;
        let mut union_in = |r: Option<PixelRect>| {
            if let Some(r) = r {
                combined = Some(combined.map_or(r, |c| c.union(r)));
            }
        };
        if let Some(chrome) = self.chrome.as_ref() {
            union_in(chrome.damage_rect());
        }
        if matches!(self.state, AppState::Launch(_)) {
            let join_words_up = self.launch_add_mode && self.add_join_words.is_some();
            if !join_words_up {
                union_in(self.textbox.as_ref().and_then(|t| t.damage_rect(vw, vh)));
                let handle_entered =
                    self.textbox.as_ref().map(|tb| !tb.chars.is_empty()).unwrap_or(false);
                if handle_entered {
                    union_in(self.attest_btn.as_ref().and_then(|b| b.damage_rect(vw, vh)));
                }
            }
        }
        if matches!(self.state, AppState::AddDevice) {
            union_in(self.textbox.as_ref().and_then(|t| t.damage_rect(vw, vh)));
        }
        if matches!(self.state, AppState::Ready) {
            union_in(self.contacts_textbox.as_ref().and_then(|t| t.damage_rect(vw, vh)));
            union_in(self.contacts_plus_btn.as_ref().and_then(|b| b.damage_rect(vw, vh)));
        }
        if matches!(self.state, AppState::Conversation) {
            // Mirrors the render/focus gates: compose damage only counts once the box is actually shown (chain woven, or self-contact loopback).
            let our_handle_hash = self
                .session
                .as_ref()
                .map(|s| crate::crypto::clutch::identity_party_id(&s.identity_seed))
                .unwrap_or([0u8; 32]);
            let compose_ready = self
                .active_contact
                .and_then(|ci| self.contacts.get(ci))
                .map(|c| {
                    c.clutch_state == crate::types::ClutchState::Complete
                        && (c.chain_woven || c.handle_hash == our_handle_hash)
                })
                .unwrap_or(false);
            if compose_ready {
                union_in(self.message_textbox.as_ref().and_then(|t| t.damage_rect(vw, vh)));
                union_in(self.message_send_btn.as_ref().and_then(|b| b.damage_rect(vw, vh)));
            }
        }
        combined
    }

    fn render(&mut self, target: &mut [u32], ctx: &mut Context) {
        // Press-hold-release: sync the "held" visual on every clickable WIDGET (attest / + / send Buttons) to the pointer arbiter's currently-pressed hit id. On desktop the host's overlay pass then paints the held tint from each Button's `tint_delta`; the app's own hit-stamped elements (pills, contact rows, nav rows) read `ctx.pressed_hit` directly further down. Must run before the widget tree is walked for overlay deltas (post-render), so a press lights up the same frame.
        let pressed_hit = ctx.pressed_hit;
        widget::apply_pressed(self, pressed_hit);
        // Compute chord-held state BEFORE taking the mutable `chrome` borrow — `brackets_held` reads `&self` and the chrome borrow lives thru the entire render. Update `last_chord_held` here too so the next frame's `damage_rect` knows whether to include the hint bbox for the one-frame clear.
        let held_now = self.brackets_held(Instant::now());
        self.last_chord_held = held_now;
        let show_hitmask = self.show_hitmask;
        // Snapshot the colour table so the post-flatten hitmask overlay can read it after the chrome borrow ends.
        let buf_w = ctx.viewport.width_px as usize;
        let buf_h = ctx.viewport.height_px as usize;

        // Arm the zoom hint: the host swallows zoom events and mutates `ru` directly, so we detect a zoom by `ru` changing frame-to-frame. Arm only when a zoom modifier is held (so a programmatic/resize ru change wouldn't trigger it, and merely holding Ctrl with no scroll doesn't either — the change is what arms it). `ModifiersChanged` clears it on release.
        let zoom_mod_held = ctx.modifiers.control_key() || ctx.modifiers.super_key();
        if ctx.viewport.ru != self.last_ru {
            if zoom_mod_held {
                self.zoom_hint = true;
            }
            self.last_ru = ctx.viewport.ru;
        }
        // Dev-only: the zoom-% readout is a debugging aid, not a shipped affordance. Desktop shows it while a zoom modifier is held after a change (`zoom_hint`); Android pinch-zoom has NO keyboard modifier to arm/clear against, so there we show it whenever `ru` sits away from 100% — always accurate, no touch-release event needed (which fluor's multi-touch layer doesn't emit yet).
        let show_zoom = cfg!(feature = "development")
            && (self.zoom_hint || (cfg!(target_os = "android") && (ctx.viewport.ru - 1.0).abs() > 0.001));

        // Title-bar text by screen, computed BEFORE the chrome borrow (peer count reads `self.handle_query` / `self.session`). Launch/attest shows the "← Network" affordance; once attested (Ready) it shows the peer count — distinct identities in the store EXCLUDING our own: peers are PEOPLE, so the FGTW seed is not a peer (the old `+1` when online) and neither are our own fleet siblings (their records ride the same store for direct routing). `set_title` only re-rasterizes chrome when the string actually changes, so this is cheap to recompute each frame.
        let title_text: String = if matches!(self.state, AppState::Conversation) {
            self.active_contact
                .and_then(|ci| self.contacts.get(ci))
                .map(|c| c.display_name())
                .unwrap_or_else(|| "Conversation".to_string())
        } else if matches!(self.state, AppState::Ready) {
            let own_hp = self.session.as_ref().map(|s| s.handle_proof);
            let n = self
                .handle_query
                .as_ref()
                .and_then(|hq| hq.get_transport())
                // handle_count_excluding, not peer_count: the title counts PEOPLE (unique identities), and the store carries one row per device — a 3-phone friend must read as one peer, and we must not read as one at all.
                .map(|t| {
                    t.lock()
                        .map(|s| match &own_hp {
                            Some(hp) => s.handle_count_excluding(hp),
                            None => s.handle_count(),
                        })
                        .unwrap_or(0)
                })
                .unwrap_or(0);
            format!("{n} {}", if n == 1 { "peer" } else { "peers" })
        } else if matches!(self.state, AppState::Settings(_)) {
            // The settings screen draws its own "Settings" heading in the header band — a chrome title would double up behind it (portrait showed "‹ Network" bleeding thru the heading).
            String::new()
        } else {
            "\u{2039} Network".to_string()
        };

        // Clamp the contacts block scroll and refresh the contacts widget layout BEFORE taking the long-lived `chrome` borrow. The whole user section (avatar, hint, search box, separator) now scrolls with the contact rows as one block, and the search box / plus button rects are positioned in `update_widget_layout` off `contacts_scroll`; doing this here (rather than inside the borrowed render block, which can't call `&mut self`) keeps the box, the avatar, and the rows all reading the SAME clamped offset within a frame — no one-frame mismatch at the over-scroll boundary. The formula matches the in-block geometry exactly: `max_scroll = (rows.y0 + matching·row_h) − buf_h`, hard-stopped at 0. Scrolled Y for the Ready-screen version watermark (rides the scroll block); `None` on other screens, where the version uses its pinned `version_cy`.
        let mut ready_block_version_y: Option<f32> = None;
        if matches!(self.state, AppState::Ready) {
            let rl = ReadyLayout::compute(buf_w, buf_h, ctx.viewport.ru);
            let row_h = rl.row_height.max(1) as isize;
            let filter: String = self
                .contacts_textbox
                .as_ref()
                .map(|t| t.chars.iter().collect::<String>().to_lowercase())
                .unwrap_or_default();
            let n_matching = self
                .contacts
                .iter()
                .filter(|c| {
                    // Must mirror the render pass's `matching` filter exactly (siblings hidden) or the two clamps disagree within a frame.
                    !c.is_sibling
                        && (filter.is_empty()
                            || c.display_name().to_lowercase().contains(&filter))
                })
                .count();
            let block_bottom_at_zero = rl.rows.y0 as isize + n_matching as isize * row_h;
            // The version footer rides the block one row-height past the last row; extend the scroll extent past it (footer gap + a row-height of bottom margin) so the user can scroll the version fully into view instead of the bottom edge swallowing it.
            let block_end = block_bottom_at_zero + row_h * 2;
            let max_scroll = (block_end - buf_h as isize).max(0);
            self.contacts_scroll = self.contacts_scroll.clamp(0, max_scroll);
            self.update_widget_layout(ctx);
            // Contacts version watermark rides the scroll block: it sits just past the last contact row (one row-height of breathing room) and scrolls up with everything else, rather than being pinned to the bottom. Stash the scrolled Y for the bg-layer closure below; other screens keep the pinned `version_cy`.
            ready_block_version_y =
                Some((block_bottom_at_zero + row_h - self.contacts_scroll) as f32);
        }
        // Settings panel (STUB): reposition the active page's widgets each frame so zoom / resize track. Mirrors the Ready branch above; must run before the long-lived `chrome` borrow since it takes `&mut self`.
        if matches!(self.state, AppState::Settings(_)) {
            self.update_widget_layout(ctx);
        }
        // Settings scroll: clamp the rail + content offsets to their NATURAL-height extents (no clamp-to-fit in layout → content can overflow → this scroll reveals it, bounded so it can't scroll off the page). Done here, before the `chrome` borrow, so the render arms + update_widget_layout all read the same clamped offsets this frame. Captured into locals for use inside the borrowed render block.
        let (settings_rail_scroll, settings_content_scroll) = if let AppState::Settings(page) = self.state {
            let sl = SettingsLayout::compute(&ctx.viewport);
            let rail_extent = (sl.nav_row_h() * (SettingsPage::ALL.len() as Coord + 1.0) - sl.rail_inset().h).max(0.0);
            self.settings_rail_scroll = self.settings_rail_scroll.clamp(0.0, rail_extent);
            let content_extent = (sl.content_line_h() * settings_page_rows(page) as Coord - sl.content_inset().h).max(0.0);
            self.settings_content_scroll = self.settings_content_scroll.clamp(0.0, content_extent);
            (self.settings_rail_scroll, self.settings_content_scroll)
        } else {
            (0.0, 0.0)
        };
        // Fleet device inventory, gathered before the long-lived `chrome` borrow (the Fleet render arm can't call the `&self` helper while `chrome` is borrowed mutably). Empty off the Fleet page.
        let fleet_devices = if matches!(self.state, AppState::Settings(SettingsPage::Fleet)) {
            self.fleet_device_rows()
        } else {
            Vec::new()
        };
        // Conversation: lay out the compose textbox + send button each frame. Without this the send button kept stale placeholder geometry (mid-screen), rendered under the opaque message-list fill, and under()-blend discarded it — it never appeared. Same reason as the Ready/Settings branches above; must run before the long-lived `chrome` borrow (takes `&mut self`).
        if matches!(self.state, AppState::Conversation) {
            self.update_widget_layout(ctx);
        }

        let Some(chrome) = self.chrome.as_mut() else {
            return;
        };
        chrome.set_title(title_text);

        // Bg noise. `shimmer` is driven by `bg_scroll` and mixes into each row's starting colour — so the noise colour bias cycles as you scroll without changing the underlying pattern topology. `scroll_offset` is per-screen: Launch/Attest gets `0` (no vertical movement on the attest screen — shimmer only); future screens (Ready, Searching, Conversation) will pass `bg_scroll` so the noise pattern also translates with their page-scroll content. Phase 2+ branches on AppState to pick which.
        let bg_scroll = self.bg_scroll;
        let shimmer = bg_scroll as usize;
        let scroll_offset = 0; // Launch only for now.
        // Background texture origin + per-half scroll. On Settings the noise mirror-axis sits ON the rail|content divider (1/3 width), and each half scrolls with ITS pane — rail-scroll drives the left half, content-scroll the right — so the background tracks the scroll of whatever you're reading. Every other screen keeps the centred origin with both halves locked together (unified scroll).
        let (bg_split_x, bg_left_scroll, bg_right_scroll) = if matches!(self.state, AppState::Settings(_)) {
            let sl = SettingsLayout::compute(&ctx.viewport);
            (
                Some(sl.content.x as usize),
                Some(settings_rail_scroll as isize),
                settings_content_scroll as isize,
            )
        } else {
            (None, None, scroll_offset)
        };
                               // Launch layout: faithful proportional slicing port from legacy `Layout::new` — spectrum near the top, logo wordmark overlapping its bottom, attest block (textbox + hint + button) below. Compute every frame; cheap and lets resize flow thru without a separate cache.
        let layout = LaunchLayout::compute(buf_w, buf_h, ctx.viewport.ru);
        // Chromatic wave phase has two summands: * Scroll-driven base (`bg_scroll * 1/128 rad/scroll-unit`) — one wheel-notch ≈ 8 units → ~1/16 rad shift; user-tunable by changing the shift exponent.
        // * `attest_anim_phase` (advanced in `tick()` while `LaunchState::Attesting`) — the "query in flight" cue, 1 cycle/sec.
        // Summing them means the wave responds to BOTH inputs simultaneously: a user scrolling during an attestation still nudges the phase on top of the animation.
        let phase = bg_scroll as f32 * (1. / ((1 << 7) as f32)) + self.attest_anim_phase;
        let period_scale = 1.;
        let spectrum_rect = layout.spectrum;
        let logo_rect = layout.photon_text;
        // Faint dozenal version watermark, bottom-left on every screen it shows. Size = half the "handle" hint text (hint slot height × 0.7, halved); rendered at weight 400 so it resolves to the Oxanium `+glyphs` face carrying the dozenal control-block glyphs, in near-transparent white (VERSION_COLOUR) so it sits in the background like a watermark rather than competing with the foreground.
        let attest_for_version = AttestBlockLayout::compute(layout.attest_block);
        let version_size =
            (attest_for_version.hint.y1 - attest_for_version.hint.y0) as f32 * 0.7 * 0.5;
        let version_glyphs = dozenal_glyphs(deploy_version());
        // Bottom-LEFT watermark; the Security/Recovery posture meters sit bottom-right on the Ready strip. Left edge one font-size in from the screen edge, mirroring the posture group's right margin.
        let version_x = version_size;
        // `draw_text_left_u32`'s y is the text BOX CENTRE, not the baseline/bottom. Anchor by the glyph bottom instead: put the text's bottom edge one `version_size` up from the window bottom (mirroring the one-`version_size` left margin), so the version reads as bottom-left-aligned from the corner rather than centre-aligned. line_height = size × 1.2 (the renderer's Metrics::relative ratio), so the centre sits half that above the bottom edge.
        let version_line_h = version_size * 1.2;
        let version_cy = buf_h as f32 - version_size - version_line_h * 0.5;
        // Zoom watermark, top-centre: current `ru` zoom factor as a decimal percentage ("100%", "103%"), twice the version size, at 1/4 opacity. Mirrors the version's bottom-centre placement (one font-size in from the edge). Integer percent — the ~3%/step zoom granularity makes decimals noise.
        let zoom_size = version_size * 2.0;
        let zoom_text = format!("{}%", (ctx.viewport.ru * 100.0).round() as i64);
        let zoom_cx = buf_w as f32 * 0.5;
        let zoom_cy = zoom_size;
        // Split-borrow `ctx.damage` (consumed by rasterize_bg's first arg) and `ctx.text` (captured by the closure for the logo's text rendering). These are disjoint fields of `Context` so the borrow checker allows both reborrows simultaneously. The closure is non-`move` so the text reborrow ends when rasterize_bg returns, leaving `ctx.text` available for `rasterize_chrome` on the next line.
        let text = &mut *ctx.text;
        // Bg-first compose chain: noise paints opaque, the wave reads it for the `sqrt(c*scale + c_bg²)` blend, then the logo (glow / body / highlight) paints over both via legacy visible-RGB ops. Each step preserves α on the pixels it touches. The wave + logo are Launch-screen chrome — once attested the user shouldn't be staring at the wordmark every time they open the app, so Ready / Searching / Conversation get just the background noise and let their own widgets own the canvas.
        let on_launch = matches!(self.state, AppState::Launch(_));
        // Version watermark shows on the attest screen (Launch), the contacts screen (Ready), and the conversation screen — not other screens. (Settings, when it lands, spells the version out in words rather than glyphs; that's its own render path.) It's a faint bottom-left watermark painted in the bg pass, so on the conversation screen it sits behind the lifted compose box rather than competing with it.
        let show_version =
            on_launch || matches!(self.state, AppState::Ready | AppState::Conversation);
        // Swap the noise base colour to BG_BASE_WARNING when the dual-ring vault flagged degraded this session — the noise pass already runs every frame so this changes a colour, not the pass count. None on the happy path keeps fluor's default green-dark BG_BASE.
        let bg_base = if self.vault_degraded {
            Some(BG_BASE_WARNING)
        } else {
            None
        };
        // The 1-px noise inset exists ONLY to clear the window perimeter hairline / shadow band — so gate it on whether that perimeter is actually drawn, which is exactly `!chrome.full_edge`. A windowed desktop draws the perimeter → inset. A maximized/fullscreen desktop goes full_edge (no perimeter) and Android forces full_edge too → paint to the screen edge, else a 1-px unpainted border shows. (Earlier this was hardcoded per-OS, so desktop-maximized still inset for a perimeter that wasn't there.) `|| cfg!(android)` keeps the Android always-fullscreen guarantee even on a transient pre-resize frame where full_edge hasn't synced yet.
        let bg_fullscreen = chrome.full_edge || cfg!(target_os = "android");
        chrome.rasterize_bg(ctx.damage, |canvas| {
            // Chromatic wave FIRST, then the background noise — that is the paint order for the spectrum band.
            if on_launch {
                chromatic_wave(canvas, spectrum_rect, phase, period_scale);
                paint_photon_logo(canvas, text, logo_rect);
            }
            if show_version {
                // On the Ready screen the version rides the scroll block (positioned past the last contact row); elsewhere it stays pinned at `version_cy`.
                let vy = ready_block_version_y.unwrap_or(version_cy);
                text.draw_text_left_u32(
                    canvas,
                    &version_glyphs,
                    version_x,
                    vy,
                    version_size,
                    400,
                    VERSION_COLOUR,
                    "Oxanium",
                    None,
                    None,
                    None,
                );
            }
            // Zoom hint is independent of the version's screen gate — it shows on ANY screen, but only while actively zooming (a held zoom modifier after a `ru` change), per `show_zoom`.
            if show_zoom {
                text.draw_text_center_u32(
                    canvas,
                    &zoom_text,
                    zoom_cx,
                    zoom_cy,
                    zoom_size,
                    400,
                    ZOOM_COLOUR,
                    "Oxanium",
                    None,
                    None,
                    None,
                );
            }
            paint::background_noise_split(canvas, shimmer, bg_fullscreen, bg_right_scroll, bg_split_x, bg_left_scroll, None, bg_base);
            // Wave then logo — RMW ops that read the now-opaque noise beneath as their base. The chromatic wave quadrature-blends with the bg colour (sqrt-linear-light) so it MUST follow the noise; the logo composites over the wave/noise. (Watermarks above went before the noise so it composes under them.)
            if on_launch {
                chromatic_wave(canvas, spectrum_rect, phase, period_scale);
                paint_photon_logo(canvas, text, logo_rect);
            }
        });
        // Window-perimeter hairline FIRST — painted straight into `target` (not the chrome group) and carves the window-shape clip_mask. fluor is under-blend only, so whatever lands in `target` first wins at shared edge pixels; drawing the hairline before any content makes it survive over full-bleed screens (Ready/Conversation) whose content reaches the window edge. The chrome group (buttons / orb / strip / title) still composites UNDER content via `flatten_into` below. The clip_mask carve here is the SOLE source of the single window-shape alpha-trim done at the OS boundary in finalize.
        chrome.rasterize_perimeter(target, buf_w, buf_h, ctx.clip_mask);
        chrome.rasterize_chrome(ctx.damage, ctx.text, ctx.clip_mask);

        // Chord hint — painted INTO `target` BEFORE `flatten_into` so the hint glyphs sit at the TOP of the under-blend chain (chrome composes UNDER them).
        if held_now {
            let span = ctx.viewport.effective_span();
            let mut canvas = Canvas::new(target, buf_w, buf_h, ctx.damage);
            paint::draw_chord_hint(&mut canvas, ctx.text, CHORD_HINTS, span);
        }

        // Launch-screen widgets paint UNDER the chord hint (so the hint always wins over the textbox) and OVER chrome (so the pill sits on top of the spectrum strip / wordmark). Same target buffer as the chord hint; widgets stamp their hit IDs into chrome's shared `hit_test_map`. Only paint when the launch screen is the active state — Ready/Searching/Conversation get their own widgets later.
        if let AppState::Launch(launch_state) = &self.state {
            let layout = LaunchLayout::compute(buf_w, buf_h, ctx.viewport.ru);
            let attest = AttestBlockLayout::compute(layout.attest_block);
            let mut canvas = Canvas::new(target, buf_w, buf_h, ctx.damage);

            // Clear the attest block's footprint in the shared hit_test_map BEFORE re-stamping this frame's widgets. Chrome only wipes the map on its own dirty cycles (`rasterize_chrome` early-returns when chrome is clean), but the launch widgets re-stamp every frame — so when the Attest button stops rendering (handle cleared to empty) on a chrome-clean frame, its old hit-rect would otherwise linger and keep dispatching pointer + hitmask. The attest_block is the only Photon-owned region of the map on this screen, so clearing the whole block each frame is the cheap correct reset; the textbox/button/∞ below re-stamp whatever is actually present.
            restamp_hit_rect(
                &mut chrome.hit_test_map,
                buf_w,
                buf_h,
                layout.attest_block.x0 as isize,
                layout.attest_block.y0 as isize,
                layout.attest_block.x1 as isize,
                layout.attest_block.y1 as isize,
                HIT_NONE,
            );

            // Status slot — `attest.error` rect above the textbox. Carries either the red error message (`LaunchState::Error`) or the white "Attesting…" indicator (`LaunchState::Attesting`); empty in Fresh. Same geometry for both so they swap in place; colour differentiates "something's wrong" from "we're working". Wave's 1-cycle/sec phase animation pairs with the "Attesting…" line as the secondary cue.
            let status: Option<(&str, u32)> =
                if self.launch_add_mode && !self.add_join_status.is_empty() {
                    Some((self.add_join_status.as_str(), STATUS_TEXT_COLOUR))
                } else {
                    match launch_state {
                        LaunchState::Attesting => Some(("Attesting\u{2026}", STATUS_TEXT_COLOUR)),
                        LaunchState::Error(msg) if !msg.is_empty() => {
                            Some((msg.as_str(), ERROR_TEXT_COLOUR))
                        }
                        _ => None,
                    }
                };
            if let Some((text, colour)) = status {
                let error_rect = attest.error;
                if !error_rect.is_empty() {
                    let region_h = (error_rect.y1 - error_rect.y0) as f32;
                    let cx = (error_rect.x0 + error_rect.x1) as f32 * 0.5;
                    let cy = (error_rect.y0 + error_rect.y1) as f32 * 0.5;
                    // Half-height font: status messages are short by convention; full-rect-height is too loud for one-line text and overflows wide messages off the side.
                    ctx.text.draw_text_center_u32(
                        &mut canvas,
                        text,
                        cx,
                        cy,
                        region_h * 0.5,
                        500, // Medium weight — readable at small sizes; matches the Oxanium family already loaded in init().
                        colour,
                        "Oxanium",
                        None,
                        None,
                        None,
                    );
                }
            }

            // Permanence warning block (`LaunchState::Confirm`) — drawn in the empty 6-unit band BELOW the attest button, sized with the same ru-scaled math as the join-words rows. The headline takes the error colour for gravity; the detail lines stay in status grey. The button above now reads "Yes — forever"; editing the handle cancels back to Fresh.
            if matches!(launch_state, LaunchState::Confirm) && !self.launch_add_mode {
                let tb_h = (attest.textbox.y1 - attest.textbox.y0) as f32;
                let line_h = (tb_h * 0.45).min(buf_w as f32 / 22.0).max(10.0);
                let cx = buf_w as f32 * 0.5;
                let mut y = attest.attest.y1 as f32 + line_h * 1.6;
                // What's permanent is the IDENTITY, not the handle: a handle is a mutable label, but attesting mints crypto roots with no password / reset / recovery. Ownership binds to the HUMAN, not the hardware — the first person to attest owns that identity, while devices stay replaceable thru the fleet chain (remove the first device whenever, as long as another is added first). The warning must not mis-teach "this phone owns it" NOR "this name is a life sentence" — it's the identity behind it that can't be undone.
                let lines: [(&str, u32); 5] = [
                    ("This mints a permanent identity.", ERROR_TEXT_COLOUR),
                    ("No password. No reset. No recovery.", STATUS_TEXT_COLOUR),
                    ("The first human to attest owns it.", STATUS_TEXT_COLOUR),
                    ("Devices can be replaced. The identity can't.", STATUS_TEXT_COLOUR),
                    ("Press again if you mean it.", STATUS_TEXT_COLOUR),
                ];
                for (line, colour) in lines {
                    ctx.text.draw_text_center_u32(
                        &mut canvas, line, cx, y, line_h, 600, colour, "Oxanium", None, None, None,
                    );
                    y += line_h * 1.35;
                }
            }

            // Join words phase (new device): the screen becomes display-only — this device's pairing words, drawn in rows for reading onto the other device, flipping to the found-colour when a member matches them. No textbox, no attest button.
            let join_words_up = self.launch_add_mode && self.add_join_words.is_some();
            if join_words_up {
                if let Some(words) = self.add_join_words.as_ref() {
                    let tokens: Vec<String> = {
                        let mut v = Vec::new();
                        let mut cur = String::new();
                        for c in words.chars() {
                            if c.is_ascii_uppercase() && !cur.is_empty() {
                                v.push(std::mem::take(&mut cur));
                            }
                            cur.push(c);
                        }
                        if !cur.is_empty() {
                            v.push(cur);
                        }
                        v
                    };
                    // No intermediate ready-flip: red-until-green — the words stay neutral until membership folds, at which point this screen is LEFT (that departure is the green the far side confirms).
                    let colour = STATUS_TEXT_COLOUR;
                    let cx = buf_w as f32 * 0.5;
                    // Size + anchor from the attest-block layout so the words scale with ru/zoom like every other widget and sit BELOW the status slot instead of floating into the wordmark. Width-capped so 4-word lines fit a narrow window.
                    let tb_h = (attest.textbox.y1 - attest.textbox.y0) as f32;
                    let line_h = (tb_h * 0.45).min(buf_w as f32 / 18.0).max(10.0);
                    let lines: Vec<String> = tokens.chunks(4).map(|c| c.join(" ")).collect();
                    let mut y = attest.error.y1 as f32 + line_h * 1.2;
                    for line in &lines {
                        ctx.text.draw_text_center_u32(
                            &mut canvas, line, cx, y, line_h, 600, colour, "Oxanium", None, None, None,
                        );
                        y += line_h * 1.35;
                    }
                    // Name the device being enrolled, so a user pairing several devices can tell on both screens which one these words belong to. Deterministic two-word default from the device PUBLIC key + the fleet's identity seed, so the Fleet list on every device in this fleet shows this same name; the owner-edited override arrives with the devices page. Pre-attest the session isn't set yet, so derive the seed from the handle being joined (`add_join_handle`).
                    let join_seed = self
                        .session
                        .as_ref()
                        .map(|s| s.identity_seed)
                        .or_else(|| {
                            self.add_join_handle
                                .as_ref()
                                .map(|h| crate::storage::contacts::derive_identity_seed(h))
                        });
                    if let (Some(kp), Some(seed)) = (self.device_keypair.as_ref(), join_seed) {
                        let name = crate::network::fgtw::fleet::device_name_default(kp.public.as_bytes(), &seed);
                        y += line_h * 0.4;
                        ctx.text.draw_text_center_u32(
                            &mut canvas,
                            &format!("this device: {name}"),
                            cx,
                            y,
                            line_h * 0.8,
                            500,
                            fluor::theme::HINT_COLOUR,
                            "Oxanium",
                            None,
                            None,
                            None,
                        );
                    }
                    // How-to guidance: the two ways the OTHER (already-in-fleet) device adds this one, plus the confirm. Small + dim so it reads as instructions, not chrome.
                    {
                        y += line_h * 0.9;
                        let gsize = line_h * 0.62;
                        for line in [
                            "On your other device: Settings \u{2192} Fleet \u{2192} Add",
                            "Type these words there — or, if it's nearby,",
                            "just tap this device in the list.",
                            "You'll confirm the add on that device.",
                        ] {
                            ctx.text.draw_text_center_u32(
                                &mut canvas, line, cx, y, gsize, 400,
                                fluor::theme::HINT_COLOUR, "Oxanium", None, None, None,
                            );
                            y += gsize * 1.5;
                        }
                    }
                    // "Start fresh (wipe this device)" — the secondary escape: a device that was REMOVED from a fleet can't attest (can't reach the Security page), so this is its only self-clean path. Two-tap confirm. Hit-stamped so a tap on Android works (no chords there). Pushed well below the add guidance so it reads as the edge case, not the main action.
                    {
                        y += line_h * 1.4;
                        let sf_label = if self.join_startfresh_armed {
                            "Start fresh — tap again to wipe this device"
                        } else {
                            "Wrong device? Start fresh (wipe this device)"
                        };
                        let sf_size = line_h * 0.7;
                        let sf_colour = if self.join_startfresh_armed { ERROR_TEXT_COLOUR } else { fluor::theme::HINT_COLOUR };
                        ctx.text.draw_text_center_u32(
                            &mut canvas, sf_label, cx, y, sf_size, 500, sf_colour, "Oxanium", None, None, None,
                        );
                        let half_w = buf_w as f32 * 0.4;
                        restamp_hit_rect(
                            &mut chrome.hit_test_map,
                            buf_w,
                            buf_h,
                            (cx - half_w) as isize,
                            (y - sf_size * 0.8) as isize,
                            (cx + half_w) as isize,
                            (y + sf_size * 0.8) as isize,
                            self.join_startfresh_hit_id,
                        );
                    }
                }
            } else {
                // Hint slot — static "handle" label below the textbox. Tells the user what to type.
                let hint_rect = attest.hint;
                if !hint_rect.is_empty() {
                    let region_h = (hint_rect.y1 - hint_rect.y0) as f32;
                    let cx = (hint_rect.x0 + hint_rect.x1) as f32 * 0.5;
                    let cy = (hint_rect.y0 + hint_rect.y1) as f32 * 0.5;
                    let hint_label = if self.launch_add_mode {
                        "handle (join a fleet)"
                    } else {
                        "handle"
                    };
                    ctx.text.draw_text_center_u32(
                        &mut canvas,
                        hint_label,
                        cx,
                        cy,
                        region_h * 0.7,
                        500,
                        fluor::theme::HINT_COLOUR,
                        "Oxanium",
                        None,
                        None,
                        None,
                    );
                }

                // Resting-state gates for the attest slot. The handle textbox owns the empty/focused truth; the attest button and the infinity glyph are the two mutually-exclusive things that can occupy the slot below it.
                // - handle_entered: any typed character → show the Attest button (mirrors the contacts plus-button's `!chars.is_empty()` reveal).
                // - textbox_active: the textbox is focused (cursor in it) → the user is mid-entry even with no character yet, so the resting infinity steps aside.
                let handle_entered = self
                    .textbox
                    .as_ref()
                    .map(|tb| !tb.chars.is_empty())
                    .unwrap_or(false);
                let textbox_active = self
                    .textbox
                    .as_ref()
                    .map(|tb| Some(tb.hit_id()) == self.focused)
                    .unwrap_or(false);

                // Dormant infinity centred IN the handle textbox — it sits where the typed handle will appear, a half-brightness grey placeholder for the resting field, shown only while the field is empty AND unfocused. Painted BEFORE the textbox: fluor's under-blend is "topmost paints first; later opaque dst wins", so the glyph must precede the textbox's empty-pill fill to survive (same ordering the contacts plus-button uses). The instant the user focuses (cursor in) or a character lands, the gate goes false and the textbox owns the slot alone. Anchor and size come straight off the textbox (`center_x/center_y/font_size`), so the glyph lands pixel-identical to where a typed character would — the textbox draws its own glyphs via `draw_text_center_u32` at the same anchor, so matching it here keeps the ∞ from sitting high or scaling differently.
                if !handle_entered && !textbox_active {
                    if let Some(tb) = self.textbox.as_ref() {
                        // ∞ ink sits ~1-2px high because `draw_text_center_u32` centres on the line box (ascent+descent), and a math symbol's ink rides the math axis, slightly above where baseline-seated text reads as centred. Nudge the y anchor down by font_size/32 (≈1-2px here, scales with zoom) to seat the glyph at the pill's visual centre.
                        let baseline_nudge = tb.font_size * (1.0 / (1 << 5) as f32);
                        ctx.text.draw_text_center_u32(
                            &mut canvas,
                            "\u{221E}",
                            tb.center_x,
                            tb.center_y + baseline_nudge,
                            tb.font_size,
                            400, // Same weight the textbox renders its own glyphs at (see textbox `measure_text_widths_per_char` / draw calls).
                            fluor::theme::HINT_COLOUR,
                            "Oxanium",
                            None,
                            None,
                            None,
                        );
                    }
                }

                if let Some(tb) = self.textbox.as_mut() {
                    let id = tb.hit_id();
                    tb.render_content_into(
                        &mut canvas,
                        0.,
                        0.,
                        ctx.text,
                        None,
                        None,
                        Some(&mut chrome.hit_test_map),
                        id,
                    );
                }
                // The Attest button only exists once there's a handle to attest. An empty, untouched field shows the dormant infinity in its place instead; a focused-but-empty field shows neither (the user is typing). Hiding the button also keeps its hit-rect out of `hit_test_map`, so an empty field can't dispatch a no-op attest click.
                if handle_entered {
                    if let Some(btn) = self.attest_btn.as_mut() {
                        let id = btn.hit_id();
                        btn.render_content_into(
                            &mut canvas,
                            0.,
                            0.,
                            ctx.text,
                            None,
                            Some(&mut chrome.hit_test_map),
                            id,
                        );
                    }
                }
            }
        }

        // Ready screen — slice-based layout matching legacy ContactsUnifiedLayout. Today only the avatar circle is painted; the layout already carries rects for handle / hint / textbox / separator / contact rows so subsequent slices drop into named slots without re-computing geometry.
        if matches!(self.state, AppState::Ready) {
            let mut canvas = Canvas::new(target, buf_w, buf_h, ctx.damage);
            let ready_layout = ReadyLayout::compute(buf_w, buf_h, ctx.viewport.ru);

            // The whole user section (avatar, hint, search box + plus, separator) scrolls together with the contact rows as one block; `contacts_scroll` is the single block offset (0 = rest, avatar at its natural top). Subtract it from the Y of every scrolling element. The version watermark, Sec/Rec meters, and background do NOT scroll (rendered elsewhere / left unoffset here). The upper clamp lands below once `matching`/`rows` are known.
            let scroll = self.contacts_scroll as f32;

            // Clear the contacts textbox slot in the shared hit_test_map before re-stamping. Same reason as the launch screen: chrome only wipes the map on its own dirty cycles, but the textbox + overlaid plus-button re-stamp every frame, and the plus only renders when the field is non-empty. Without this, clearing the search field to empty on a chrome-clean frame would leave the plus-button's old hit-rect dispatching pointer + hitmask. The plus lives inside the textbox slot, so clearing that slot covers both. The slot scrolls with the block, so clear the SCROLLED rect (update_widget_layout offsets the textbox/button rects by the same `contacts_scroll`).
            restamp_hit_rect(
                &mut chrome.hit_test_map,
                buf_w,
                buf_h,
                ready_layout.textbox.x0 as isize,
                ready_layout.textbox.y0 as isize - self.contacts_scroll,
                ready_layout.textbox.x1 as isize,
                ready_layout.textbox.y1 as isize - self.contacts_scroll,
                HIT_NONE,
            );

            let (cx, cy_natural, radius) = ready_layout.avatar_center_radius();
            let cy = cy_natural - scroll;
            // 0xFFC5C5C5 in fluor's α+darkness format = α 0xFF, darkness 0xC5 each channel = visible RGB(0x3A, 0x3A, 0x3A) ≈ 22% brightness. Standalone constant (no theme.rs entry yet) — promote when Ready chrome gets a proper palette pass.
            if self.device_avatar_pixels.is_some() {
                let diameter = (radius * 2.0) as usize;
                if self.device_avatar_scaled.is_none()
                    || self.device_avatar_scaled_diameter != diameter
                {
                    let base = self.device_avatar_pixels.as_ref().unwrap();
                    self.device_avatar_scaled =
                        Some(crate::ui::avatar_render::update_avatar_scaled(
                            base,
                            crate::ui::avatar::AVATAR_SIZE,
                            diameter,
                        ));
                    self.device_avatar_scaled_diameter = diameter;
                }
                crate::ui::avatar_render::draw_avatar(
                    &mut canvas,
                    cx,
                    cy,
                    radius,
                    self.device_avatar_scaled.as_ref().unwrap(),
                    diameter,
                    None,
                );
            } else {
                paint::draw_circle(&mut canvas, cx, cy, radius, AVATAR_PLACEHOLDER, None);
            }
            // Stamp the avatar circle into the shared hit_test_map so a tap dispatches to the picker. Squared-distance test in the same row-major buffer the renderers use; bbox-clipped against the buffer extent so off-screen circles don't underflow.
            stamp_hit_circle(
                &mut chrome.hit_test_map,
                buf_w,
                buf_h,
                cx,
                cy,
                radius,
                self.avatar_hit_id,
            );

            // Avatar update hint below the circle — DESKTOP ONLY, shown on hover. On Android, tapping the grey circle to pick an image is self-evident.
            #[cfg(not(target_os = "android"))]
            if self.avatar_hovered {
                // Anchored directly below the avatar circle (not the hint slot), at half the hint slot's text size.
                let size = (ready_layout.hint.y1 - ready_layout.hint.y0) as f32 * 0.3;
                let hcy = cy + radius + size;
                ctx.text.draw_text_center_u32(
                    &mut canvas,
                    "drag/drop to update avatar",
                    cx,
                    hcy,
                    size,
                    500,
                    fluor::theme::HINT_COLOUR,
                    "Oxanium",
                    None,
                    None,
                    None,
                );
            }

            // Contacts-page textbox + plus button. The plus button is OVERLAID inside the textbox right edge and ONLY rendered when the textbox has content — empty textbox shows no button. While an add-friend search is in flight, a rotating hourglass replaces the button (and the button is not hit-stampable, so it can't be re-clicked mid-search).
            //
            // Under-blend is topmost-FIRST (first opaque writer wins colour AND its per-pixel hit stamp). Paint the button/hourglass BEFORE the textbox: the button claims its exact pill silhouette in the framebuffer and hit map, and the textbox drawn under it can't overwrite either (its own stamp is per-opaque-pixel too). No hit re-stamp — the draw yields the correct pill-shaped hit area on its own.
            let plus_visible = self
                .contacts_textbox
                .as_ref()
                .map(|tb| !tb.chars.is_empty())
                .unwrap_or(false);
            if self.add_in_flight {
                if let Some(btn) = self.contacts_plus_btn.as_ref() {
                    let sz = btn.width.min(btn.height);
                    draw_hourglass(
                        &mut canvas,
                        btn.center_x,
                        btn.center_y,
                        sz,
                        self.hourglass_angle,
                        HOURGLASS_COLOUR,
                    );
                }
            } else if plus_visible {
                if let Some(btn) = self.contacts_plus_btn.as_mut() {
                    let id = btn.hit_id();
                    btn.render_content_into(
                        &mut canvas,
                        0.,
                        0.,
                        ctx.text,
                        None,
                        Some(&mut chrome.hit_test_map),
                        id,
                    );
                }
            }
            // Search box placeholder — same treatment as the launch screen's ∞: a grey prompt centred in the empty, unfocused box, painted BEFORE the textbox so the under-blend keeps it behind the empty pill fill. Clears on focus or first character.
            let search_empty = self
                .contacts_textbox
                .as_ref()
                .map(|t| t.chars.is_empty())
                .unwrap_or(true);
            let search_focused = self
                .contacts_textbox
                .as_ref()
                .map(|t| Some(t.hit_id()) == self.focused)
                .unwrap_or(false);
            if search_empty && !search_focused {
                if let Some(tb) = self.contacts_textbox.as_ref() {
                    ctx.text.draw_text_center_u32(
                        &mut canvas,
                        "search | add",
                        tb.center_x,
                        tb.center_y,
                        tb.font_size * 0.6,
                        500,
                        fluor::theme::HINT_COLOUR,
                        "Oxanium",
                        None,
                        None,
                        None,
                    );
                }
            }
            if let Some(tb) = self.contacts_textbox.as_mut() {
                let id = tb.hit_id();
                tb.render_content_into(
                    &mut canvas,
                    0.,
                    0.,
                    ctx.text,
                    None,
                    None,
                    Some(&mut chrome.hit_test_map),
                    id,
                );
            }
            // Re-win the plus button's hit silhouette after the search textbox clobbered it (only when the button actually rendered — not during the in-flight hourglass, which isn't clickable).
            if !self.add_in_flight && plus_visible {
                if let Some(btn) = self.contacts_plus_btn.as_ref() {
                    btn.stamp_hit_into(&mut chrome.hit_test_map, buf_w, buf_h, btn.hit_id());
                }
            }

            // Add-friend result text in the hint slot above the search box: green "added {h}", red "not found" / "error: …". Stays until the next search starts (cleared in `submit_add_friend`).
            if let Some((text, colour)) = self.search_status.as_ref() {
                let hint = ready_layout.hint;
                if !hint.is_empty() {
                    let region_h = (hint.y1 - hint.y0) as f32;
                    let scx = (hint.x0 + hint.x1) as f32 * 0.5;
                    let scy = (hint.y0 + hint.y1) as f32 * 0.5 - scroll;
                    ctx.text.draw_text_center_u32(
                        &mut canvas,
                        text,
                        scx,
                        scy,
                        region_h * 0.6,
                        500,
                        *colour,
                        "Oxanium",
                        None,
                        None,
                        None,
                    );
                }
            }

            // "Device added √" confirmation — in the hint slot ABOVE the search box (not the bottom band). Green; sits until the next click/keystroke clears it via clear_hints (never time-based). Lifts one line when the add-friend result already occupies the hint slot so the two don't overlap.
            if let Some(msg) = &self.ready_toast {
                let hint = ready_layout.hint;
                if !hint.is_empty() {
                    let region_h = (hint.y1 - hint.y0) as f32;
                    let tcx = (hint.x0 + hint.x1) as f32 * 0.5;
                    let lift = if self.search_status.is_some() { region_h * 1.15 } else { 0.0 };
                    let tcy = (hint.y0 + hint.y1) as f32 * 0.5 - scroll - lift;
                    ctx.text.draw_text_center_u32(
                        &mut canvas,
                        msg,
                        tcx,
                        tcy,
                        region_h * 0.6,
                        600,
                        SEARCH_FOUND_COLOUR,
                        "Oxanium",
                        None,
                        None,
                        None,
                    );
                }
            }

            // ───────── Separator + scrollable contact list ───────── 1-pixel hairline centred in the separator slot (height 0 = hairline; the slot itself is just reserved breathing room around the line).
            let sep = ready_layout.separator;
            paint::fill_rect(
                &mut canvas,
                sep.x0 as isize,
                ((sep.y0 + sep.y1) / 2) as isize - self.contacts_scroll,
                (sep.x1 - sep.x0) as isize,
                0,
                SEPARATOR_COLOUR,
                None,
                None,
            );

            let rows = ready_layout.rows;
            let row_h = ready_layout.row_height.max(1) as isize;
            let diam = ready_layout.contact_avatar_diameter;
            let avatar_r = diam as f32 * 0.5;
            // Rows now scroll up into (and past) where the user section sat, so the clip can no longer stop at `rows.y0`. Clip top = the top of the content area (0); the chrome title bar composites on top afterwards via `chrome.flatten_into`, exactly as it does for the unclipped avatar that already draws high. Keep the x extent at the rows' columns.
            let rows_clip = fluor::paint::Clip::new(rows.x0, 0, rows.x1, buf_h);

            // Filter by the search text (case-insensitive substring on the handle); empty filter = all.
            let filter: String = self
                .contacts_textbox
                .as_ref()
                .map(|t| t.chars.iter().collect::<String>().to_lowercase())
                .unwrap_or_default();
            let matching: Vec<usize> = self
                .contacts
                .iter()
                .enumerate()
                .filter(|(_, c)| {
                    // Fleet siblings are infrastructure, not conversations — never listed (device management gets its own page later).
                    !c.is_sibling
                        && (filter.is_empty()
                            || c.display_name().to_lowercase().contains(&filter))
                })
                .map(|(i, _)| i)
                .collect();

            // Clamp scroll over the FULL block (user section + rows + version footer), hard-stop at both ends. Down-scroll stops when the version footer (one row past the last row) plus a row of bottom margin reaches the screen bottom; up-scroll stops at rest (0), with the avatar at its natural top. MUST match the pre-chrome clamp above (`block_end = block_bottom_at_zero + row_h*2`) so both passes agree within a frame.
            let block_bottom_at_zero = rows.y0 as isize + matching.len() as isize * row_h;
            let block_end = block_bottom_at_zero + row_h * 2;
            let max_scroll = (block_end - buf_h as isize).max(0);
            if self.contacts_scroll > max_scroll {
                self.contacts_scroll = max_scroll;
            }

            // Row geometry: avatar on the left with a half-radius margin, name to its right.
            let avatar_cx = rows.x0 as f32 + avatar_r * 1.5;
            let text_x = avatar_cx + avatar_r * 1.5;
            let text_size = row_h as f32 * 0.5;
            let ring_thickness = (avatar_r * 0.0375).max(1.0);
            // Handle names render in each contact's relationship colour (spaghettify per visible row is microseconds; revisit with a cache if contact lists ever get huge).
            let our_handle_hash = self
                .session
                .as_ref()
                .map(|s| crate::crypto::clutch::identity_party_id(&s.identity_seed))
                .unwrap_or([0u8; 32]);
            for (vis, &ci) in matching.iter().enumerate() {
                let row_top = rows.y0 as isize + vis as isize * row_h - self.contacts_scroll;
                if row_top + row_h <= 0 || row_top >= buf_h as isize {
                    continue; // fully outside the visible content area (rows now scroll up to the top, not just `rows.y0`)
                }
                // Held: a finger/pointer is DOWN on this row and a release here opens the conversation (press-hold-release). Paint the held tint FIRST so the avatar + name land on top of it; a drag-off clears `ctx.pressed_hit` and the tint vanishes next frame.
                if ci < 256 && ctx.pressed_hit != HIT_NONE
                    && ctx.pressed_hit == self.contact_hit_base.wrapping_add(ci as HitId)
                {
                    paint::fill_rect(
                        &mut canvas,
                        rows.x0 as isize,
                        row_top.max(0),
                        (rows.x1 - rows.x0) as isize,
                        (row_top + row_h).min(buf_h as isize) - row_top.max(0),
                        fluor::theme::BUTTON_HELD,
                        Some(rows_clip),
                        None,
                    );
                }
                let cy = (row_top + row_h / 2) as f32;
                let online = self.contacts[ci].is_online;

                // Build/refresh the contact's scaled-avatar cache at the row diameter.
                let has_avatar = self.contacts[ci].avatar_pixels.is_some();
                if has_avatar
                    && (self.contacts[ci].avatar_scaled.is_none()
                        || self.contacts[ci].avatar_scaled_diameter != diam)
                {
                    let base = self.contacts[ci].avatar_pixels.as_ref().unwrap();
                    let scaled = crate::ui::avatar_render::update_avatar_scaled(
                        base,
                        crate::ui::avatar::AVATAR_SIZE,
                        diam,
                    );
                    self.contacts[ci].avatar_scaled = Some(scaled);
                    self.contacts[ci].avatar_scaled_diameter = diam;
                }

                // Avatar (or placeholder) is topmost; the presence ring paints UNDER it so only the rim shows.
                if let Some(scaled) = self.contacts[ci].avatar_scaled.as_ref() {
                    crate::ui::avatar_render::draw_avatar(
                        &mut canvas,
                        avatar_cx,
                        cy,
                        avatar_r,
                        scaled,
                        diam,
                        Some(rows_clip),
                    );
                } else {
                    paint::draw_circle(
                        &mut canvas,
                        avatar_cx,
                        cy,
                        avatar_r,
                        AVATAR_PLACEHOLDER,
                        Some(rows_clip),
                    );
                }
                let ring = if online {
                    RING_ONLINE_COLOUR
                } else {
                    RING_OFFLINE_COLOUR
                };
                paint::draw_circle(
                    &mut canvas,
                    avatar_cx,
                    cy,
                    avatar_r + ring_thickness,
                    ring,
                    Some(rows_clip),
                );

                // Handle name, vertically centred in the row, clipped to the list region — in this contact's relationship colour. Self-as-contact rows get the neutral anchor (no other party, no relationship).
                let row_colour = if self.contacts[ci].handle_hash == our_handle_hash {
                    self_colour()
                } else {
                    party_colour(&relationship_digest(
                        &self.contacts[ci].handle_hash,
                        &our_handle_hash,
                    ))
                };
                ctx.text.draw_text_left_u32(
                    &mut canvas,
                    &self.contacts[ci].display_name(),
                    text_x,
                    cy,
                    text_size,
                    500,
                    row_colour,
                    "Oxanium",
                    Some(rows_clip),
                    None,
                    None,
                );

                // Stamp the row into the hit map so clicks dispatch to this contact.
                if ci < 256 {
                    let row_hit = self.contact_hit_base.wrapping_add(ci as HitId);
                    restamp_hit_rect(
                        &mut chrome.hit_test_map,
                        buf_w,
                        buf_h,
                        rows.x0 as isize,
                        row_top.max(0),
                        rows.x1 as isize,
                        (row_top + row_h).min(buf_h as isize),
                        row_hit,
                    );
                }
            }

            // Persistent degraded-vault indicator: amber text at the bottom. The matching warm background tint already lives in the noise pass above (we swap BG_BASE → BG_BASE_WARNING) so we add no extra render pass here, just the text glyph. Full details live in the README.
            if self.vault_degraded {
                // Visible RGB(255, 140, 0) amber. Packed: α=0xFF | darkness = (0x00, 0x73, 0xFF).
                const DEGRADED_TEXT: u32 = 0xFF_00_73_FF;
                // Band height off the span-based layout unit (zoom-aware, aspect-ratio-robust, no pixel floor) — same scaling family as the rest of the screen.
                let band_h = ready_layout.unit_height * 1.5;
                let cx = buf_w as f32 * 0.5;
                let cy = buf_h as f32 - band_h * 0.5;
                let font_size = band_h * 0.6;
                ctx.text.draw_text_center_u32(
                    &mut canvas,
                    "storage degraded",
                    cx,
                    cy,
                    font_size,
                    600,
                    DEGRADED_TEXT,
                    "Oxanium",
                    None,
                    None,
                    None,
                );
            }

            // Clock-off indicator: same amber as the degraded banner (nunc-time consensus says the system clock is grossly wrong). Warn only — Photon never corrects the clock. Stacks one band above "storage degraded" when both are showing so they don't overlap.
            if let Some(offset_secs) = self.clock_off {
                const CLOCK_TEXT: u32 = 0xFF_00_73_FF; // visible RGB(255, 140, 0) amber, as above
                let band_h = ready_layout.unit_height * 1.5;
                let cx = buf_w as f32 * 0.5;
                // Sit at the bottom; if the degraded banner is also up, lift this one band higher.
                let rows_below = if self.vault_degraded { 1.0 } else { 0.0 };
                let cy = buf_h as f32 - band_h * (0.5 + rows_below);
                let font_size = band_h * 0.6;
                // Human-readable magnitude + direction. ahead = system clock reads later than truth.
                let mag = offset_secs.unsigned_abs();
                let pretty = if mag >= 3600 {
                    format!("{}h", mag / 3600)
                } else if mag >= 60 {
                    format!("{}m", mag / 60)
                } else {
                    format!("{}s", mag)
                };
                let dir = if offset_secs < 0 { "ahead" } else { "behind" };
                let label = format!("clock off — {} {}", pretty, dir);
                ctx.text.draw_text_center_u32(
                    &mut canvas,
                    &label,
                    cx,
                    cy,
                    font_size,
                    600,
                    CLOCK_TEXT,
                    "Oxanium",
                    None,
                    None,
                    None,
                );
            }

            // Security & Recovery posture meters, bottom-right of the Ready strip (the dozenal version sits bottom-left). Two orthogonal axes — see `identity_posture`. Drawn into `target` at full opacity (unlike the watermark version) so they read as a real, glanceable status affordance, aligned to the version's baseline band. Read-only for now; the tap-to-device-sheet lands with the first modal primitive.
            {
                let (sec, rec) = identity_posture();
                let label_size = version_size;
                let pip_r = version_size * 0.30;
                let pip_pitch = pip_r * 2.6;
                let pips_span = pip_pitch * (POSTURE_PIPS as f32 - 1.0) + pip_r * 2.0;
                let lp_gap = version_size * 0.5; // label → first pip
                let group_gap = version_size * 1.2; // Sec group → Rec group
                let w_sec = ctx
                    .text
                    .measure_text_width("Sec", label_size, 500, "Oxanium");
                let w_rec = ctx
                    .text
                    .measure_text_width("Rec", label_size, 500, "Oxanium");
                let total = w_sec + lp_gap + pips_span + group_gap + w_rec + lp_gap + pips_span;
                // Inset by 2× the version's margin (right + bottom) to clear the now-2×-larger bottom-right squircle corner — the same move the top-left orb made for its enlarged corner. The bottom-left version stays put (it sits by the small BL corner).
                let mut x = buf_w as f32 - version_size * 2.0 - total;
                // Centre sits a clean 2·version_size up from the bottom — matching the 2·version_size right inset. Independent of `version_cy` (which carries the version's bottom-edge anchor offset); the pip rows + labels here are centre-anchored, so this is a direct centre inset.
                let strip_cy = buf_h as f32 - version_size * 2.0;
                for (label, w_label, filled) in [("Sec", w_sec, sec), ("Rec", w_rec, rec)] {
                    ctx.text.draw_text_left_u32(
                        &mut canvas,
                        label,
                        x,
                        strip_cy,
                        label_size,
                        500,
                        fluor::theme::HINT_COLOUR,
                        "Oxanium",
                        None,
                        None,
                        None,
                    );
                    x += w_label + lp_gap;
                    let on = posture_colour(filled);
                    for i in 0..POSTURE_PIPS {
                        let pcx = x + pip_r + i as f32 * pip_pitch;
                        let colour = if i < filled { on } else { POSTURE_OFF_COLOUR };
                        paint::draw_circle(&mut canvas, pcx, strip_cy, pip_r, colour, None);
                    }
                    x += pips_span + group_gap;
                }
            }
        }

        // Conversation screen — shows the selected contact's name, clutch state, and (eventually) messages.
        if matches!(self.state, AppState::Conversation) {
            let mut canvas = Canvas::new(target, buf_w, buf_h, ctx.damage);
            if let Some(ci) = self.active_contact {
                if ci < self.contacts.len() {
                    let ru = ctx.viewport.ru;
                    // Build/refresh the contact's scaled-avatar cache at the CONVERSATION-HEADER diameter BEFORE the immutable borrow below. The header renders the avatar bigger than the contact-list rows, but it has no rebuild of its own — it used to draw whatever `avatar_scaled` happened to hold (built at the small row diameter) while telling draw_avatar the buffer was header-sized → it sampled past the smaller buffer → "index out of bounds: len 2028 (26²·3) but index 2307" panic on conversation-open. Rebuilding here at the header diameter keeps the cache and the claimed scaled_diameter in lockstep.
                    {
                        let (_, _, header_r) =
                            ReadyLayout::compute(buf_w, buf_h, ru).avatar_center_radius();
                        let header_diam = (header_r * 2.0) as usize;
                        if self.contacts[ci].avatar_pixels.is_some()
                            && (self.contacts[ci].avatar_scaled.is_none()
                                || self.contacts[ci].avatar_scaled_diameter != header_diam)
                        {
                            let base = self.contacts[ci].avatar_pixels.as_ref().unwrap();
                            let scaled = crate::ui::avatar_render::update_avatar_scaled(
                                base,
                                crate::ui::avatar::AVATAR_SIZE,
                                header_diam,
                            );
                            self.contacts[ci].avatar_scaled = Some(scaled);
                            self.contacts[ci].avatar_scaled_diameter = header_diam;
                        }
                    }
                    let contact = &self.contacts[ci];
                    // Scale off the SAME span-based harmonic unit the contacts screen uses, so the conversation screen scales identically (aspect-ratio-robust, zoom-aware, no hardcoded pixels) instead of the old crude height-only `buf_h·0.04` with a magic 12px floor.
                    let conv_layout = ReadyLayout::compute(buf_w, buf_h, ru);
                    let unit = conv_layout.unit_height;

                    // Back arrow (top-left) — below the chrome title bar area.
                    let back_y = buf_h as f32 * 0.06 + unit;
                    let back_size = unit * 1.15;
                    let back_text = "\u{2039} Contacts";
                    ctx.text.draw_text_left_u32(
                        &mut canvas,
                        back_text,
                        unit,
                        back_y,
                        back_size,
                        500,
                        CONTACT_NAME_COLOUR,
                        "Oxanium",
                        None,
                        None,
                        None,
                    );
                    // Stamp the back button hit rect.
                    let back_w = ctx
                        .text
                        .measure_text_width(back_text, back_size, 500, "Oxanium");
                    restamp_hit_rect(
                        &mut chrome.hit_test_map,
                        buf_w,
                        buf_h,
                        0,
                        (back_y - back_size) as isize,
                        (unit + back_w + unit) as isize,
                        (back_y + back_size) as isize,
                        self.back_btn_hit_id,
                    );

                    // Avatar FIRST, then the handle below it. Sized to MATCH our own avatar on the Ready/contacts screen, so the friend's avatar here reads at the same scale. Both derive their radius from `ReadyLayout::avatar_center_radius` (a pure fn of viewport + zoom), so they stay identical across resize/zoom. Only the centre placement differs (centred on this screen vs. the Ready slot).
                    let (_, _, avatar_r) = conv_layout.avatar_center_radius();
                    let avatar_diam = (avatar_r * 2.0) as usize;
                    let avatar_cx = buf_w as f32 * 0.5;
                    let avatar_y = back_y + unit * 1.5 + avatar_r;
                    if let Some(scaled) = contact.avatar_scaled.as_ref() {
                        crate::ui::avatar_render::draw_avatar(
                            &mut canvas,
                            avatar_cx,
                            avatar_y,
                            avatar_r,
                            scaled,
                            avatar_diam,
                            None,
                        );
                    } else {
                        paint::draw_circle(
                            &mut canvas,
                            avatar_cx,
                            avatar_y,
                            avatar_r,
                            AVATAR_PLACEHOLDER,
                            None,
                        );
                    }
                    let ring = if contact.is_online {
                        RING_ONLINE_COLOUR
                    } else {
                        RING_OFFLINE_COLOUR
                    };
                    let ring_thick = (avatar_r * 0.0375).max(1.0);
                    paint::draw_circle(
                        &mut canvas,
                        avatar_cx,
                        avatar_y,
                        avatar_r + ring_thick,
                        ring,
                        None,
                    );

                    // Relationship colour for this contact: everything handle-specific on this screen (name, their message text) renders in it. Self is the neutral-grey anchor.
                    let our_handle_hash = self
                        .session
                        .as_ref()
                        .map(|s| crate::crypto::clutch::identity_party_id(&s.identity_seed))
                        .unwrap_or([0u8; 32]);
                    // Self-as-contact (notes-to-self): there is no other party, so no relationship colour — everything is the neutral anchor.
                    let is_self_contact = contact.handle_hash == our_handle_hash;
                    let their_colour = if is_self_contact {
                        self_colour()
                    } else {
                        party_colour(&relationship_digest(&contact.handle_hash, &our_handle_hash))
                    };

                    // Contact name, centred BELOW the avatar, in their relationship colour.
                    let name_size = unit * 1.2;
                    let name_y = avatar_y + avatar_r + unit * 1.2;
                    ctx.text.draw_text_center_u32(
                        &mut canvas,
                        &contact.display_name(),
                        buf_w as f32 * 0.5,
                        name_y,
                        name_size,
                        600,
                        their_colour,
                        "Oxanium",
                        None,
                        None,
                        None,
                    );

                    // CLUTCH state (compact, under the name). Show the base state PLUS a behind-the-scenes detail (slot fill, keygen / KEM / proof stage) so a stuck handshake reads as "what's it waiting on" instead of a flat "pending" — see Contact::clutch_status_detail. Self-contact (notes-to-self) has no peer + no ceremony: the weave probe is skipped, so chain_woven never seals and clutch_status_detail would read "testing · weaving the chain" forever — show a plain reachability line instead.
                    let clutch_y = name_y + unit * 1.5;
                    let clutch_label = if is_self_contact {
                        "notes to self".to_string()
                    } else {
                        format!("CLUTCH: {}", contact.clutch_status_detail())
                    };
                    let clutch_colour = if is_self_contact
                        || contact.clutch_state == crate::types::ClutchState::Complete
                    {
                        SEARCH_FOUND_COLOUR
                    } else {
                        HOURGLASS_COLOUR
                    };
                    ctx.text.draw_text_center_u32(
                        &mut canvas,
                        &clutch_label,
                        buf_w as f32 * 0.5,
                        clutch_y,
                        unit * 0.6,
                        500,
                        clutch_colour,
                        "Oxanium",
                        None,
                        None,
                        None,
                    );

                    // Message history + compose box only exist once CLUTCH is Complete — before that there's no chain to encrypt on, and sending no-ops. Until then the screen shows just the avatar + "CLUTCH: …" status (above), so the user isn't presented a dead input box for a contact they can't message yet.
                    if contact.clutch_state == crate::types::ClutchState::Complete {
                        // ── Message list ─────────────────────────────────────────── Text-only, right-aligned (outgoing) / left-aligned (incoming), one thin white divider after every message. Newest at the bottom, just above the compose bar; older scroll up off-screen.
                        // Our text is the neutral-grey anchor (same Y = 0.5, zero chroma); theirs is the relationship colour computed above.
                        let our_colour = self_colour();

                        let msg_size = unit * 0.62;
                        let line_h = msg_size * 1.6; // text + breathing room per message
                        let pad_x = unit; // left/right inset
                        let list_top = clutch_y + unit * 1.2;
                        // Compose bar reserves the bottom strip, lifted off the bottom edge by `compose_margin`. The list lives between list_top and list_bottom. Must match the layout pass's `compose_h`/`compose_margin` below.
                        let compose_h = unit * 1.8;
                        let compose_margin = unit * 0.8;
                        let list_bottom = buf_h as f32 - compose_h - compose_margin - unit * 0.5;
                        // Clamp so a short window (tall header) can never invert the clip (list_top > list_bottom) — that's what made every message vanish on resize. When there's no room, list_bottom collapses to list_top and the list is simply empty rather than drawing with a negative-height (inverted) clip.
                        let list_bottom = list_bottom.max(list_top);
                        let list_clip = fluor::paint::Clip::new(
                            0,
                            list_top as usize,
                            buf_w,
                            list_bottom as usize,
                        );

                        // Lay messages out bottom-up so the newest sits at list_bottom. Clamp scroll offset to the actual overscroll range so a stale offset from a previous (larger) window size can't push every message above list_top on resize.
                        let n = contact.messages.len();
                        let content_h = n as f32 * line_h;
                        let view_h = (list_bottom - list_top).max(0.0);
                        let max_scroll = (content_h - view_h).max(0.0);
                        let scroll = contact.message_scroll_offset.clamp(0.0, max_scroll);
                        let mut y = list_bottom - msg_size + scroll;
                        for msg in contact.messages.iter().rev() {
                            if y < list_top - line_h {
                                break; // scrolled above the visible region
                            }
                            // Divider under this message (between it and the next-newer one).
                            paint::fill_rect(
                                &mut canvas,
                                pad_x as isize,
                                (y + msg_size * 0.5) as isize,
                                (buf_w as f32 - pad_x * 2.0) as isize,
                                (ru.max(1.0)) as isize,
                                DIVIDER_COLOUR,
                                Some(list_clip),
                                None,
                            );
                            // Dim outgoing until delivered; incoming always full. Self-as-contact: every message is ours (there is no other party), so everything sits on the right in the neutral grey — their_colour is already the anchor in that case, and the loopback "incoming" copy renders like a delivered outgoing.
                            let colour = if msg.is_outgoing {
                                if msg.delivered {
                                    our_colour
                                } else {
                                    dim_colour(our_colour)
                                }
                            } else {
                                their_colour
                            };
                            if msg.is_outgoing || is_self_contact {
                                ctx.text.draw_text_right_u32(
                                    &mut canvas,
                                    &msg.content,
                                    buf_w as f32 - pad_x,
                                    y,
                                    msg_size,
                                    500,
                                    colour,
                                    "Open Sans",
                                    Some(list_clip),
                                    None,
                                    None,
                                );
                            } else {
                                ctx.text.draw_text_left_u32(
                                    &mut canvas,
                                    &msg.content,
                                    pad_x,
                                    y,
                                    msg_size,
                                    500,
                                    colour,
                                    "Open Sans",
                                    Some(list_clip),
                                    None,
                                    None,
                                );
                            }
                            y -= line_h;
                        }
                        let _ = n;

                        // ── Compose box (pinned bottom) ────────────────────────────
                        // Hidden until the chain-weave probe seals BOTH directions (chain_woven: their probe seen + our ACK-advanced) — Complete alone only proves the ceremony, not the ratchet, and a message typed into an unproven chain can desync it. The status line above reads "testing · weaving the chain" for exactly this window. Self-contacts are exempt (loopback, no peer to weave with, probe deliberately skipped).
                        if is_self_contact || contact.chain_woven {
                            let compose_empty = self
                                .message_textbox
                                .as_ref()
                                .map(|t| t.chars.is_empty())
                                .unwrap_or(true);
                            let compose_focused = self
                                .message_textbox
                                .as_ref()
                                .map(|t| Some(t.hit_id()) == self.focused)
                                .unwrap_or(false);
                            let compose_cy = buf_h as f32 - compose_margin - compose_h * 0.5;
                            if compose_empty && !compose_focused {
                                ctx.text.draw_text_left_u32(
                                    &mut canvas,
                                    "message",
                                    pad_x * 1.2,
                                    compose_cy,
                                    msg_size,
                                    400,
                                    LABEL_COLOUR,
                                    "Open Sans",
                                    None,
                                    None,
                                    None,
                                );
                            }
                            // Send button COLOUR first (its under() blit lands on the noise), then the arrowhead over the pill (source-over). The textbox draws after — it sits over the button and clobbers the button's hit stamp with its own id — so we re-stamp the button's TRUE pill silhouette (fill + stroke, which also covers the arrowhead) AFTER the textbox, as the last writer. That's the whole click + hover region: shape-accurate, not a bbox rectangle.
                            if let Some(btn) = self.message_send_btn.as_mut() {
                                let id = btn.hit_id();
                                btn.render_content_into(
                                    &mut canvas,
                                    0.,
                                    0.,
                                    ctx.text,
                                    None,
                                    Some(&mut chrome.hit_test_map),
                                    id,
                                );
                                draw_up_arrowhead(
                                    &mut canvas,
                                    btn.center_x,
                                    btn.center_y,
                                    btn.height * 0.5,
                                    SEND_ARROW_COLOUR,
                                );
                            }
                            if let Some(tb) = self.message_textbox.as_mut() {
                                let id = tb.hit_id();
                                tb.render_content_into(
                                    &mut canvas,
                                    0.,
                                    0.,
                                    ctx.text,
                                    None,
                                    None,
                                    Some(&mut chrome.hit_test_map),
                                    id,
                                );
                            }
                            // Re-win the send button's hit silhouette after the textbox clobbered it.
                            if let Some(btn) = self.message_send_btn.as_ref() {
                                btn.stamp_hit_into(&mut chrome.hit_test_map, buf_w, buf_h, btn.hit_id());
                            }
                        } // end chain-woven compose gate
                    } // end CLUTCH-Complete gate (message list + compose box)
                }
            }
        }

        // ── Add-device screen: this (existing) device shows the pairing secret words to type into the new device. ──
        if matches!(self.state, AppState::AddDevice) {
            let mut canvas = Canvas::new(target, buf_w, buf_h, ctx.damage);
            let cx = buf_w as f32 * 0.5;

            // Back affordance (top-left) — same "‹ Contacts" idiom + hit-id as the Conversation screen. Navigation is a dedicated control; the orb is reserved for settings and never carries context actions.
            {
                let unit = ReadyLayout::compute(buf_w, buf_h, ctx.viewport.ru).unit_height;
                let back_y = buf_h as f32 * 0.06 + unit;
                let back_size = unit * 1.15;
                let back_text = "‹ Contacts";
                ctx.text.draw_text_left_u32(
                    &mut canvas, back_text, unit, back_y, back_size, 500,
                    CONTACT_NAME_COLOUR, "Oxanium", None, None, None,
                );
                let back_w = ctx.text.measure_text_width(back_text, back_size, 500, "Oxanium");
                restamp_hit_rect(
                    &mut chrome.hit_test_map, buf_w, buf_h,
                    0, (back_y - back_size) as isize,
                    (unit + back_w + unit) as isize, (back_y + back_size) as isize,
                    self.back_btn_hit_id,
                );
            }

            // All geometry hangs off the textbox rect (laid out by update_widget_layout from the ru-scaled attest slot), so the whole screen scales with zoom and nothing collides with the pill.
            let (tb_cy, tb_h) = self
                .textbox
                .as_ref()
                .map(|tb| (tb.center_y, tb.font_size / 0.75))
                .unwrap_or((buf_h as f32 * 0.45, 40.0));
            let step = tb_h * 1.1;
            ctx.text.draw_text_center_u32(
                &mut canvas, "Add a device", cx, tb_cy - step * 2.4,
                tb_h * 0.9, 600, STATUS_TEXT_COLOUR, "Oxanium", None, None, None,
            );
            let subtitle = if self.add_device_bound.is_none() {
                "Type the words shown on the new device"
            } else if self.add_device_checking {
                // Words path: bound + auto-rotating; the status line below carries "Adding…".
                ""
            } else {
                // BLE path only: the line above the confirm is load-bearing (the press releases the fleet key) — the human must check the FAR screen, not this one.
                "Confirm only once the new device shows it's in"
            };
            ctx.text.draw_text_center_u32(
                &mut canvas, subtitle, cx, tb_cy - step * 1.3,
                tb_h * 0.45, 400, STATUS_TEXT_COLOUR, "Oxanium", None, None, None,
            );
            if self.add_device_bound.is_none() {
                // Words-entry field: the launch textbox instance does double duty (same rect as the attest slot); it stamps its hit id so click-to-focus works.
                if let Some(tb) = self.textbox.as_mut() {
                    let id = tb.hit_id();
                    tb.render_content_into(
                        &mut canvas,
                        0.,
                        0.,
                        ctx.text,
                        None,
                        None,
                        Some(&mut chrome.hit_test_map),
                        id,
                    );
                }
                // Live word counter under the field — fixed-width entry means completeness is knowable, so show progress and flip emphasis when full.
                let typed: String = self.textbox.as_ref().map(|tb| tb.chars.iter().collect()).unwrap_or_default();
                let count = crate::network::fgtw::fleet::pair_word_tokens(&typed);
                let full = count == crate::network::fgtw::fleet::PAIR_WORD_COUNT;
                let counter = format!("{count} / {}", crate::network::fgtw::fleet::PAIR_WORD_COUNT);
                let counter_colour = if full { SEARCH_FOUND_COLOUR } else { fluor::theme::HINT_COLOUR };
                ctx.text.draw_text_center_u32(
                    &mut canvas, &counter, cx, tb_cy + step * 1.2,
                    tb_h * 0.5, 500, counter_colour, "Oxanium", None, None, None,
                );
                // Tappable candidate list (BLE / list select): every device asking to join our fleet, by keyed name, with a "nearby" mark for the ones whose announce beacon we hear. Tap one to bind it (instead of typing its 23 words) — the far device's shown name is what you match against. Up to 7 rows.
                if !self.add_device_candidates.is_empty() {
                    let list_top = tb_cy + step * 1.9;
                    let row_h = tb_h * 0.85;
                    ctx.text.draw_text_center_u32(
                        &mut canvas, "or tap the device asking to join:", cx, list_top,
                        tb_h * 0.4, 400, fluor::theme::HINT_COLOUR, "Oxanium", None, None, None,
                    );
                    for (i, cand) in self.add_device_candidates.iter().take(7).enumerate() {
                        let ry = list_top + row_h * (i as f32 + 1.0);
                        let label = if cand.heard_ble {
                            format!("{}   · nearby", cand.name)
                        } else {
                            cand.name.clone()
                        };
                        let held = ctx.pressed_hit != HIT_NONE
                            && ctx.pressed_hit == self.add_candidate_hit_base.wrapping_add(i as HitId);
                        let colour = if cand.heard_ble { SEARCH_FOUND_COLOUR } else { STATUS_TEXT_COLOUR };
                        ctx.text.draw_text_center_u32(
                            &mut canvas, &label, cx, ry,
                            tb_h * 0.55, if held { 700 } else { 500 }, colour, "Oxanium", None, None, None,
                        );
                        let half_w = buf_w as f32 * 0.42;
                        restamp_hit_rect(
                            &mut chrome.hit_test_map, buf_w, buf_h,
                            (cx - half_w) as isize, (ry - row_h * 0.5) as isize,
                            (cx + half_w) as isize, (ry + row_h * 0.5) as isize,
                            self.add_candidate_hit_base.wrapping_add(i as HitId),
                        );
                    }
                }
            } else if !self.add_device_checking {
                // Green-confirm affordance (two-phase): the press fires the fleet-key rotation. Hit-stamped like the join screen's start-fresh tappable so Android taps land. On the WORDS path the Bound handler auto-fires the rotation (checking = true), so this affordance never renders — it's the BLE transport's gate, kept live for that path. When checking, only the "Adding…" status shows.
                let confirm_y = tb_cy + step * 1.2;
                ctx.text.draw_text_center_u32(
                    &mut canvas, "Yes, it's green \u{2014} finish", cx, confirm_y,
                    tb_h * 0.7, 600, SEARCH_FOUND_COLOUR, "Oxanium", None, None, None,
                );
                let half_w = buf_w as f32 * 0.4;
                restamp_hit_rect(
                    &mut chrome.hit_test_map, buf_w, buf_h,
                    (cx - half_w) as isize, (confirm_y - tb_h * 0.7) as isize,
                    (cx + half_w) as isize, (confirm_y + tb_h * 0.7) as isize,
                    self.add_confirm_hit_id,
                );
            }
            if !self.add_device_status.is_empty() {
                let status_colour = if self.add_device_bound.is_some() {
                    SEARCH_FOUND_COLOUR
                } else if self.add_device_typo.is_some() {
                    // Live matcher hit: the status line names the diverging word in red.
                    ERROR_TEXT_COLOUR
                } else {
                    STATUS_TEXT_COLOUR
                };
                ctx.text.draw_text_center_u32(
                    &mut canvas, &self.add_device_status, cx, tb_cy + step * 2.2,
                    tb_h * 0.5, 400, status_colour, "Oxanium", None, None, None,
                );
            }
            // Matching words bind automatically, so the orb's only job here is cancel.
            let hint = "tap the orb to cancel";
            ctx.text.draw_text_center_u32(
                &mut canvas, hint, cx, tb_cy + step * 3.2,
                tb_h * 0.4, 400, STATUS_TEXT_COLOUR, "Oxanium", None, None, None,
            );
        }

        // Settings panel (STUB) — nav rail + selected page body. Controls render but wire nothing (a checkbox may flip its own visual state; every button / dropdown / slider is inert).
        if let AppState::Settings(page) = self.state {
            let layout = SettingsLayout::compute(&ctx.viewport);
            let mut canvas = Canvas::new(target, buf_w, buf_h, ctx.damage);

            // Clear the whole settings region in the shared hit_test_map before re-stamping this frame's rail rows + pills — same reason as the launch block: immediate-mode stamps must not linger across page switches.
            restamp_hit_rect(
                &mut chrome.hit_test_map, buf_w, buf_h,
                0, layout.rail.y as isize, buf_w as isize, buf_h as isize,
                HIT_NONE,
            );

            // Open dropdown popup FIRST (under-blend: topmost content paints first) so it composites over everything painted after it.
            if page == SettingsPage::Appearance {
                if let Some(dd) = self.settings_theme_dropdown.as_mut() {
                    dd.render_popup_into(&mut canvas, ctx.text, None, Some(&mut chrome.hit_test_map));
                }
            }

            // Status toast ("Sending log (N KiB)…", "Log sent √", "Device removed √", ...) — the Ready screen draws `ready_toast` in its hint slot, but settings is a different AppState, so without this the toasts fired FROM settings pages (log submit, device remove) were invisible. Bottom of the content pane, painted early so under-blend keeps it above the page body; event-shown, cleared on the next interaction via clear_hints, never time-based.
            if let Some(msg) = &self.ready_toast {
                let ts = (layout.unit * 0.72).max(9.0);
                ctx.text.draw_text_center_u32(
                    &mut canvas, msg,
                    layout.content.x + layout.content.w * 0.5,
                    layout.content.bottom() - ts,
                    ts, 600, SEARCH_FOUND_COLOUR, "Oxanium",
                    None, None, None,
                );
            }

            // --- Header: title + back affordance --- (unit-scaled so the heading zooms with everything else)
            let hspan = (layout.unit * 1.05).min(layout.header.h * 0.72);
            ctx.text.draw_text_left_u32(
                &mut canvas, "Settings", layout.header.x + hspan * 0.6,
                layout.header.center_y(), hspan, 600, CONTACT_NAME_COLOUR, "Oxanium",
                None, None, None,
            );
            // --- Nav rail: Back (row 0) + nine page labels, stacked at natural height from the top, scrolled by settings_rail_scroll (no clamp-to-fit; overflows scroll). Back is a list row now, not a floating header button: bolder green ‹ arrow over a faint 0x40-black fill so it reads distinct from the page rows. ---
            let rail_inset = layout.rail_inset();
            let nav_h = layout.nav_row_h();
            let rspan = (layout.unit * 0.58).max(9.0);
            let rail_clip = layout.rail.to_clip();
            // Row 0: Back. Rows 1..=9: the pages.
            let nav_row = |i: usize| -> fluor::region::Region {
                fluor::region::Region::new(rail_inset.x, rail_inset.y - settings_rail_scroll + i as Coord * nav_h, rail_inset.w, nav_h)
            };
            {
                let r = nav_row(0);
                let back_held = ctx.pressed_hit != HIT_NONE && ctx.pressed_hit == self.back_btn_hit_id;
                // Faint solid-black fill at 0x20 opacity so the Back row reads distinct from the page rows. The buffer is DARKNESS space (stored = 255 − visible), so visible black is 0xFFFFFF in the RGB bytes; α = 0x20 in the top byte. (A raw 0x40_00_00_00 was visible WHITE — the "bright fill" bug.) Brighter when held.
                let fill = if back_held { fluor::theme::BUTTON_HELD } else { 0x20_FF_FF_FF };
                paint::fill_rect(&mut canvas, r.x as isize, r.y as isize, r.w as isize, r.h as isize, fill, Some(rail_clip), None);
                ctx.text.draw_text_left_u32(
                    &mut canvas, "‹ Back", r.x + rspan * 0.6, r.center_y(),
                    rspan, 600, SEARCH_FOUND_COLOUR, "Oxanium", Some(rail_clip), None, None,
                );
                restamp_hit_rect(
                    &mut chrome.hit_test_map, buf_w, buf_h,
                    r.x as isize, r.y.max(layout.rail.y) as isize,
                    r.right() as isize, r.bottom().min(layout.rail.bottom()) as isize,
                    self.back_btn_hit_id,
                );
            }
            for (i, p) in SettingsPage::ALL.iter().enumerate() {
                let r = nav_row(i + 1);
                // Skip rows scrolled fully out of the rail (also keeps their hit stamps from claiming off-rail pixels).
                if r.bottom() <= layout.rail.y || r.y >= layout.rail.bottom() {
                    continue;
                }
                let active = *p == page;
                let held = ctx.pressed_hit != HIT_NONE
                    && ctx.pressed_hit == self.settings_nav_base.wrapping_add(i as HitId);
                // Held (pointer down, release switches to this page) reads brightest; else the active page gets a faint backing bar. Held paints over active so the finger-down row is unmistakable.
                if held {
                    paint::fill_rect(
                        &mut canvas, r.x as isize, r.y as isize,
                        r.w as isize, r.h as isize, fluor::theme::BUTTON_HELD, Some(rail_clip), None,
                    );
                } else if active {
                    // Active-row backing bar (faint) so the selected page reads at a glance.
                    paint::fill_rect(
                        &mut canvas, r.x as isize, r.y as isize,
                        r.w as isize, r.h as isize, SEPARATOR_COLOUR, Some(rail_clip), None,
                    );
                }
                let colour = if active { CONTACT_NAME_COLOUR } else { LABEL_COLOUR };
                ctx.text.draw_text_left_u32(
                    &mut canvas, p.label(), r.x + rspan * 0.6, r.center_y(),
                    rspan, if active { 600 } else { 400 }, colour, "Oxanium",
                    Some(rail_clip), None, None,
                );
                restamp_hit_rect(
                    &mut chrome.hit_test_map, buf_w, buf_h,
                    r.x as isize, r.y.max(layout.rail.y) as isize,
                    r.right() as isize, r.bottom().min(layout.rail.bottom()) as isize,
                    self.settings_nav_base.wrapping_add(i as HitId),
                );
            }

            // Hairline between rail and content.
            paint::fill_rect(
                &mut canvas, layout.content.x as isize, layout.content.y as isize,
                1, layout.content.h as isize, SEPARATOR_COLOUR, None, None,
            );

            // --- Selected page body ---
            // (page body is computed per-arm as a scrolled, natural-height region — see `layout.content_scrolled`)
            // Everything sizes off layout.unit — the ONE span·ru harmonic unit — so text, pills, rows, and controls all scale together with window shape AND zoom. (The old mix — text × ru inside fixed rows, controls off bare region fractions — is what made zoom hit-or-miss.)
            let tspan = (layout.unit * 0.72).max(8.0);
            let hspan2 = tspan * 0.75;
            // Stub-pill height as a fraction of its row — the row is already unit-scaled, so no extra ru factor (that would double-scale).
            let pillf = |base: Coord| base.min(1.0);
            // Draw a labelled action pill; stamps `settings_btn_base + slot` and returns nothing (stub). `n` rows must match update_widget_layout's split where widgets coexist.
            let btn_base = self.settings_btn_base;
            // Immediate-mode stub pill helper — captured as a closure over the canvas/text/hit-map isn't possible (multiple &mut borrows), so pills are drawn inline per page below via `draw_stub_pill`.
            match page {
                SettingsPage::You => {
                    let rows = layout.content_scrolled(7, settings_content_scroll).split_v([1.0; 7]);
                    settings_line(&mut canvas, ctx.text, rows[0], "Handle", tspan, CONTACT_NAME_COLOUR, 600);
                    settings_line(&mut canvas, ctx.text, rows[1], "zesty-otter-4383  (double-click to copy)", hspan2, LABEL_COLOUR, 400);
                    settings_line(&mut canvas, ctx.text, rows[2], "Avatar", tspan, CONTACT_NAME_COLOUR, 600);
                    draw_stub_pill(&mut canvas, ctx.text, &mut chrome.hit_test_map, buf_w, buf_h, rows[3].center_h(pillf(0.5)), "Change avatar…", btn_base.wrapping_add(0), ctx.pressed_hit);
                    settings_line(&mut canvas, ctx.text, rows[4], "Pubkey / handle_proof", tspan, CONTACT_NAME_COLOUR, 600);
                    settings_line(&mut canvas, ctx.text, rows[5], "b3:9f2a…c701  (double-click to copy)", hspan2, LABEL_COLOUR, 400);
                }
                SettingsPage::Fleet => {
                    // Live device inventory (gathered above the chrome borrow): this device + our siblings. Rows 1..=6 hold up to 6 devices (fleets are usually ≤5; a scroll follows if this grows past the row budget). Non-self rows are tap-selectable (hit-stamped btn_base+16+index); the Remove pill acts on the selection with a two-tap confirm.
                    let devices = &fleet_devices;
                    let rows = layout.content_scrolled(8, settings_content_scroll).split_v([1.0; 8]);
                    settings_line(&mut canvas, ctx.text, rows[0], "Your devices", tspan, CONTACT_NAME_COLOUR, 600);
                    for (i, (pk, is_self, online, name)) in devices.iter().take(6).enumerate() {
                        let row = rows[1 + i];
                        let selected = self.settings_fleet_selected == Some(*pk);
                        let (label, colour) = if *is_self {
                            (format!("{name}    (this device)"), LABEL_COLOUR)
                        } else if *online {
                            (format!("{name}    online"), SEARCH_FOUND_COLOUR)
                        } else {
                            (format!("{name}    offline"), LABEL_COLOUR)
                        };
                        // Selection cue: a leading marker + bold weight (no filled-rect machinery needed).
                        let (label, weight) = if selected {
                            (format!("\u{25b8} {label}"), 600)
                        } else {
                            (label, 400)
                        };
                        settings_line(&mut canvas, ctx.text, row, &label, hspan2, colour, weight);
                        // Only OUR OTHER devices are selectable (self-remove is deferred).
                        if !is_self {
                            restamp_hit_rect(
                                &mut chrome.hit_test_map,
                                buf_w,
                                buf_h,
                                row.x as isize,
                                row.y as isize,
                                (row.x + row.w) as isize,
                                (row.y + row.h) as isize,
                                btn_base.wrapping_add(16 + i as HitId),
                            );
                        }
                    }
                    // No Remove pill: expulsion is not a verb (sovereign records, 2026-07-13) — a device leaves by its own signed departure, and a LOST device is evicted by withholding (re-key) when the device-trust bundle lands.
                    settings_line(&mut canvas, ctx.text, rows[6], "A device can only remove itself — a lost one gets keyed out, not erased.", hspan2, LABEL_COLOUR, 400);
                    let pr = rows[7].split_h([1.0, 1.0]);
                    draw_stub_pill(&mut canvas, ctx.text, &mut chrome.hit_test_map, buf_w, buf_h, pr[0].center_h(0.85), "Add device", btn_base.wrapping_add(0), ctx.pressed_hit);
                    draw_stub_pill(&mut canvas, ctx.text, &mut chrome.hit_test_map, buf_w, buf_h, pr[1].center_h(0.85), "Rename", btn_base.wrapping_add(1), ctx.pressed_hit);
                }
                SettingsPage::Security => {
                    let rows = layout.content_scrolled(8, settings_content_scroll).split_v([1.0; 8]);
                    settings_line(&mut canvas, ctx.text, rows[0], "Security", tspan, CONTACT_NAME_COLOUR, 600);
                    settings_line(&mut canvas, ctx.text, rows[1], "Named by destructiveness.", hspan2, LABEL_COLOUR, 400);
                    // Lock (slot 0): clear the session only — de-attest, vault kept, re-unlock by re-typing your handle. Shred (slot 2): full clean — nuke vault + clear session, a blank slate for a new owner (two-tap confirm). Slot 1 ("Sign out & remove") is self-fleet-removal, still deferred.
                    draw_stub_pill(&mut canvas, ctx.text, &mut chrome.hit_test_map, buf_w, buf_h, rows[2].center_h(pillf(0.55)), "Lock (re-unlock with your handle)", btn_base.wrapping_add(0), ctx.pressed_hit);
                    draw_stub_pill(&mut canvas, ctx.text, &mut chrome.hit_test_map, buf_w, buf_h, rows[3].center_h(pillf(0.55)), "Remove this device from fleet", btn_base.wrapping_add(1), ctx.pressed_hit);
                    let shred_label = if self.settings_shred_armed { "Shred — tap again to confirm" } else { "Shred (crypto-wipe)" };
                    draw_stub_pill(&mut canvas, ctx.text, &mut chrome.hit_test_map, buf_w, buf_h, rows[4].center_h(pillf(0.55)), shred_label, btn_base.wrapping_add(2), ctx.pressed_hit);
                    if self.settings_shred_armed {
                        settings_line(&mut canvas, ctx.text, rows[5], "Wipes the vault AND identity on this device — irreversible.", hspan2, ERROR_TEXT_COLOUR, 500);
                    }
                    settings_line(&mut canvas, ctx.text, rows[6], "Security: strong   ·   Recovery: not set up", hspan2, LABEL_COLOUR, 400);
                }
                SettingsPage::Recovery => {
                    let rows = layout.content_scrolled(8, settings_content_scroll).split_v([1.0; 8]);
                    settings_line(&mut canvas, ctx.text, rows[0], "Recovery", tspan, CONTACT_NAME_COLOUR, 600);
                    settings_line(&mut canvas, ctx.text, rows[1], "Custodians (v1)", hspan2, CONTACT_NAME_COLOUR, 600);
                    if let Some(cb) = self.settings_custodian_check.as_mut() {
                        cb.render_content_into(&mut canvas, ctx.text, None, Some(&mut chrome.hit_test_map));
                    }
                    settings_line(&mut canvas, ctx.text, rows[4], "Identity backup", hspan2, CONTACT_NAME_COLOUR, 600);
                    settings_line(&mut canvas, ctx.text, rows[5], "Reinstalling won't ask for your handle.", hspan2, LABEL_COLOUR, 400);
                    draw_stub_pill(&mut canvas, ctx.text, &mut chrome.hit_test_map, buf_w, buf_h, rows[6].center_h(pillf(0.5)), "Back up identity…", btn_base.wrapping_add(0), ctx.pressed_hit);
                }
                SettingsPage::Appearance => {
                    let rows = layout.content_scrolled(8, settings_content_scroll).split_v([1.0; 8]);
                    settings_line(&mut canvas, ctx.text, rows[0], "Appearance", tspan, CONTACT_NAME_COLOUR, 600);
                    settings_line(&mut canvas, ctx.text, rows[1], "Theme", hspan2, LABEL_COLOUR, 400);
                    if let Some(dd) = self.settings_theme_dropdown.as_mut() {
                        dd.render_content_into(&mut canvas, 0., 0., ctx.text, None, Some(&mut chrome.hit_test_map));
                    }
                    settings_line(&mut canvas, ctx.text, rows[3], "Party colours (placeholder → perceptual L≈50%)", hspan2, LABEL_COLOUR, 400);
                    settings_line(&mut canvas, ctx.text, rows[4], "Zoom / text size", hspan2, LABEL_COLOUR, 400);
                    if let Some(sl) = self.settings_zoom_slider.as_mut() {
                        sl.render_content_into(&mut canvas, Some(&mut chrome.hit_test_map), sl.hit_id());
                    }
                    settings_line(&mut canvas, ctx.text, rows[6], "Colour calibration (Android panel)", hspan2, LABEL_COLOUR, 400);
                }
                SettingsPage::Notifications => {
                    let rows = layout.content_scrolled(8, settings_content_scroll).split_v([1.0; 8]);
                    settings_line(&mut canvas, ctx.text, rows[0], "Notifications", tspan, CONTACT_NAME_COLOUR, 600);
                    if let Some(cb) = self.settings_chime_check.as_mut() {
                        cb.render_content_into(&mut canvas, ctx.text, None, Some(&mut chrome.hit_test_map));
                    }
                    settings_line(&mut canvas, ctx.text, rows[2], "Per-contact override lives in each conversation.", hspan2, LABEL_COLOUR, 400);
                    if let Some(cb) = self.settings_presence_check.as_mut() {
                        cb.render_content_into(&mut canvas, ctx.text, None, Some(&mut chrome.hit_test_map));
                    }
                }
                SettingsPage::Updates => {
                    let rows = layout.content_scrolled(8, settings_content_scroll).split_v([1.0; 8]);
                    settings_line(&mut canvas, ctx.text, rows[0], "Updates", tspan, CONTACT_NAME_COLOUR, 600);
                    settings_line(&mut canvas, ctx.text, rows[1], "Photon 0.0.25 (dozenal 21)", hspan2, LABEL_COLOUR, 400);
                    if let Some(cb) = self.settings_autoupdate_check.as_mut() {
                        cb.render_content_into(&mut canvas, ctx.text, None, Some(&mut chrome.hit_test_map));
                    }
                }
                SettingsPage::Diagnostics => {
                    let rows = layout.content_scrolled(10, settings_content_scroll).split_v([1.0; 10]);
                    settings_line(&mut canvas, ctx.text, rows[0], "Diagnostics", tspan, CONTACT_NAME_COLOUR, 600);
                    settings_line(&mut canvas, ctx.text, rows[1], "On-device log · 16 MiB · self-expires 24–48h", hspan2, LABEL_COLOUR, 400);
                    let pr = rows[3].split_h([1.0, 1.0, 1.0]);
                    draw_stub_pill(&mut canvas, ctx.text, &mut chrome.hit_test_map, buf_w, buf_h, pr[0].center_h(0.85), "Clear", btn_base.wrapping_add(0), ctx.pressed_hit);
                    draw_stub_pill(&mut canvas, ctx.text, &mut chrome.hit_test_map, buf_w, buf_h, pr[1].center_h(0.85), "Snapshot", btn_base.wrapping_add(1), ctx.pressed_hit);
                    // Submit greys while an upload is in flight or the log hasn't grown past the last successful submit — a resend then would be a byte-identical duplicate. Any new record (or Clear) moves the size and re-arms it.
                    let submit_disabled = self.log_submit_inflight
                        || self.log_submitted_len == Some(crate::log_size_bytes());
                    if submit_disabled {
                        draw_stub_pill_disabled(&mut canvas, ctx.text, &mut chrome.hit_test_map, buf_w, buf_h, pr[2].center_h(0.85), "Submit", btn_base.wrapping_add(2), ctx.pressed_hit);
                    } else {
                        draw_stub_pill(&mut canvas, ctx.text, &mut chrome.hit_test_map, buf_w, buf_h, pr[2].center_h(0.85), "Submit", btn_base.wrapping_add(2), ctx.pressed_hit);
                    }
                    settings_line(&mut canvas, ctx.text, rows[6], "Optional note", hspan2, LABEL_COLOUR, 400);
                    if let Some(tb) = self.settings_note_textbox.as_mut() {
                        let id = tb.hit_id();
                        tb.render_content_into(&mut canvas, 0., 0., ctx.text, None, None, Some(&mut chrome.hit_test_map), id);
                    }
                }
                SettingsPage::About => {
                    let rows = layout.content_scrolled(8, settings_content_scroll).split_v([1.0; 8]);
                    settings_line(&mut canvas, ctx.text, rows[0], "About Photon", tspan, CONTACT_NAME_COLOUR, 600);
                    settings_line(&mut canvas, ctx.text, rows[1], "No password. Your device is your key.", hspan2, CONTACT_NAME_COLOUR, 400);
                    settings_line(&mut canvas, ctx.text, rows[2], "Stay signed in until power-off; reboot → re-enter your handle.", hspan2, LABEL_COLOUR, 400);
                    settings_line(&mut canvas, ctx.text, rows[3], "No servers. No tracking. Your data is yours.", hspan2, LABEL_COLOUR, 400);
                    settings_line(&mut canvas, ctx.text, rows[5], "Version 0.0.25 (dozenal 21)", hspan2, LABEL_COLOUR, 400);
                    settings_line(&mut canvas, ctx.text, rows[6], "Feedback: fractaldecoder@proton.me", hspan2, LABEL_COLOUR, 400);
                    settings_line(&mut canvas, ctx.text, rows[7], "Built on the TOKEN stack · licences under the hood.", hspan2, LABEL_COLOUR, 400);
                }
            }
        }

        chrome.flatten_into(target, buf_w, buf_h, None);

        // Development builds get the amber debug theme (orange bg tint / window hairline / title) via fluor's `amber` feature — pure theme-CONSTANT swaps, zero extra drawing steps. The old post-composite amber wash is gone: it wrote straight-RGB into fluor's α+darkness buffer, which inverted to blue.

        // Hit-mask overlay (`[]h`): replace every pixel with the opaque random colour for its hit_test_map ID. Drawn LAST over everything (including chrome + chord hint) — hit testing is per-final-pixel anyway, so the overlay shows exactly what `hit_at` would return. `.get` keeps the index lookup safe for any stale stamp at an unregistered high ID.
        if show_hitmask && !self.debug_hit_colours.is_empty() {
            let map = chrome.hit_test_map();
            let n = map.len().min(target.len());
            for i in 0..n {
                target[i] = self
                    .debug_hit_colours
                    .get(map[i] as usize)
                    .copied()
                    .unwrap_or(0);
            }
        }

        // Everything content-flavoured is now freshly painted — the next frame can narrow to pure widget damage unless something re-dirties the scene.
        self.scene_dirty = false;
    }

    fn hit_test_map(&self) -> Option<(&[HitId], usize, usize)> {
        let chrome = self.chrome.as_ref()?;
        let (w, h) = chrome.dims();
        Some((chrome.hit_test_map(), w, h))
    }

    fn overlay_deltas(&mut self) -> Vec<u32> {
        // Walk the container once; every Hover-capable widget contributes its tint to the slot indexed by its HitId. Slot 0 is HIT_NONE (= 0 tint). Chrome's four buttons emit their per-action hover colours via the impl in chrome_widget; future Photon widgets get the same treatment for free as soon as they impl Hover::tint_delta.
        let count = self.hit_counter as usize + 1;
        widget::build_overlay_deltas(self, count)
    }

    fn overlay_bboxes(
        &mut self,
        viewport_w: usize,
        viewport_h: usize,
    ) -> Vec<Option<fluor::canvas::PixelRect>> {
        // Parallel to overlay_deltas: each Hover widget's pill bbox by HitId, so the host bounds the tint
        // scan to the hovered widget's rect instead of the whole window. Widgets without a bbox (e.g.
        // chrome buttons that don't impl hover_bbox yet) get None → full-window fallback for that id.
        let count = self.hit_counter as usize + 1;
        widget::build_overlay_bboxes(self, count, viewport_w, viewport_h)
    }

    fn cursor_for(&self, x: Coord, y: Coord, ctx: &Context) -> CursorIcon {
        let hit = self
            .chrome
            .as_ref()
            .map(|c| c.hit_at(x, y))
            .unwrap_or(HIT_NONE);
        if let Some(chrome) = self.chrome.as_ref() {
            // Every chrome button is pressable — including the orb (settings/about/help panel; interim add-device wiring) — so all get the pointer cue, matching the orb's hover brighten.
            if chrome.owns_hit(hit) {
                return CursorIcon::Pointer;
            }
        }
        if let Some(btn) = self.attest_btn.as_ref() {
            if btn.hit_id() == hit {
                return CursorIcon::Pointer;
            }
        }
        if let Some(btn) = self.contacts_plus_btn.as_ref() {
            if btn.hit_id() == hit {
                return CursorIcon::Pointer;
            }
        }
        if let Some(btn) = self.message_send_btn.as_ref() {
            if btn.hit_id() == hit {
                return CursorIcon::Pointer;
            }
        }
        if let Some(tb) = self.textbox.as_ref() {
            if tb.hit_id() == hit {
                return CursorIcon::Text;
            }
        }
        if let Some(tb) = self.contacts_textbox.as_ref() {
            if tb.hit_id() == hit {
                return CursorIcon::Text;
            }
        }
        if let Some(tb) = self.message_textbox.as_ref() {
            if tb.hit_id() == hit {
                return CursorIcon::Text;
            }
        }
        // Contact rows and conversation back button — pointer cursor.
        if self.contact_hit_base != HIT_NONE
            && hit >= self.contact_hit_base
            && hit < self.contact_hit_base.wrapping_add(256)
        {
            return CursorIcon::Pointer;
        }
        if hit == self.back_btn_hit_id && self.back_btn_hit_id != HIT_NONE {
            return CursorIcon::Pointer;
        }
        match chrome::get_resize_edge(ctx.viewport, x, y) {
            ResizeEdge::Top | ResizeEdge::Bottom => CursorIcon::NsResize,
            ResizeEdge::Left | ResizeEdge::Right => CursorIcon::EwResize,
            ResizeEdge::TopLeft | ResizeEdge::BottomRight => CursorIcon::NwseResize,
            ResizeEdge::TopRight | ResizeEdge::BottomLeft => CursorIcon::NeswResize,
            ResizeEdge::None => CursorIcon::Default,
        }
    }
}

impl PhotonApp {
    /// The surface-free half of `tick`: presence pinging, draining every network/background channel, and advancing the CLUTCH ceremony + message chains. Returns `true` if anything changed (the caller turns that into a redraw request). Split out of `tick` so the Android foreground service can drive it headlessly while backgrounded — the paused Activity's Choreographer has stopped calling `tick`, but `PhotonApp` is alive and its inbound CLUTCH/chat still needs to advance so ceremonies complete and messages get ACKed without the screen being on. See docs/background-tick.md. MUST touch no `Context`/surface state — everything here is pure `self`.
    pub fn advance_protocol(&mut self, now: Instant) -> bool {
        let mut needs_redraw = false;

        // Recurring background presence sweep — re-ping every contact so online/offline rings stay live. The interval tapers with idle time (5s active → 1min idle → 15min deep-idle) so an untouched window isn't hammering the network. Runs on Ready AND in a Conversation — CRITICAL: presence is symmetric only if both sides keep pinging, and the person you most need a live status for is the one you're actively chatting with. Gating this to Ready meant opening a conversation stopped your pings, so your view of that contact went stale — and if both people opened the chat with each other, NEITHER pinged and both showed offline (observed: peer-B on Ready saw a peer online, a peer in the conversation saw peer-B offline). `wake_at()` schedules the next sweep so this fires even while otherwise idle.
        if matches!(self.state, AppState::Ready | AppState::Conversation) {
            let interval = self.presence_ping_interval(now);
            let due = self
                .last_presence_ping
                .is_none_or(|last| now.duration_since(last) >= interval);
            if due {
                self.last_presence_ping = Some(now);
                self.ping_contacts();
            }
        }

        // Periodic OWN-chain re-fold — the reliable doorbell for fleet membership changes (docs/pairing-v2.md). The hub `fleet` event is the instant path but best-effort; this catches a device add/remove that arrived while our WebSocket was down. Reconciling siblings re-seeds the answerable-pubkey set, so a newly-added device starts getting pong answers (stops showing offline) and appears in the Fleet list without a relaunch. 45s: brisk enough that a just-added device goes live within a sweep, slow enough to be a negligible one-fetch background poll.
        const FLEET_REFOLD_INTERVAL: std::time::Duration = std::time::Duration::from_secs(45);
        if matches!(self.state, AppState::Ready | AppState::Conversation | AppState::Settings(_)) {
            let due = self
                .last_fleet_refold
                .is_none_or(|last| now.duration_since(last) >= FLEET_REFOLD_INTERVAL);
            if due {
                if let Some(our_hp) = self.handle_query.as_ref().and_then(|hq| hq.get_handle_proof()) {
                    self.last_fleet_refold = Some(now);
                    self.spawn_contact_fleet_refresh(vec![our_hp]);
                }
            }
        }

        // Drain per-contact presence + CLUTCH ceremony updates (pongs → is_online/ip; offers/KEM/complete → ceremony progress), plus the three background-job result channels (keygen / KEM-encap / ceremony-expand). TEMP instrumentation: log any tick phase that blocks the UI thread > 50ms so the launch hang is pinpointed in the trace rather than guessed at. Remove once the hang source is fixed.
        macro_rules! timed {
            ($label:literal, $body:expr) => {{
                let __t = Instant::now();
                let __r = $body;
                let __ms = __t.elapsed().as_millis();
                if __ms > 50 {
                    crate::log(&format!("PERF: {} took {}ms (UI thread)", $label, __ms));
                }
                __r
            }};
        }
        if timed!("check_status_updates", self.check_status_updates()) {
            needs_redraw = true;
        }
        if timed!("check_clutch_keygens", self.check_clutch_keygens()) {
            needs_redraw = true;
        }
        // Serialized keygen queue: once the in-flight keygen (if any) has completed and cleared its flag above, start the next Pending-keyless contact's keygen. One McEliece at a time keeps the UI responsive on a multi-contact launch instead of spawning them all at once.
        timed!(
            "spawn_next_pending_keygen",
            self.spawn_next_pending_keygen()
        );
        if timed!("check_clutch_kem_encaps", self.check_clutch_kem_encaps()) {
            needs_redraw = true;
        }
        if timed!("check_clutch_ceremonies", self.check_clutch_ceremonies()) {
            needs_redraw = true;
        }

        // Drain handle_query results. `try_recv` is non-blocking; we collect into local Vecs so the immutable borrow on `handle_query` ends before the `&mut self` handlers run. Three channels feed in: attestation results, connectivity changes, handle searches.
        let mut drained: Vec<QueryResult> = Vec::new();
        let mut drained_searches: Vec<crate::ui::state::SearchResult> = Vec::new();
        if let Some(hq) = self.handle_query.as_ref() {
            while let Some(result) = hq.try_recv() {
                drained.push(result);
            }
            while let Some(online) = hq.try_recv_online() {
                self.online = online;
                if let Some(chrome) = self.chrome.as_mut() {
                    chrome.set_orb_tint(orb_tint_for(online));
                }
                needs_redraw = true;
            }
            while let Some(search) = hq.try_recv_search() {
                drained_searches.push(search);
            }
        }
        for result in drained {
            timed!("on_query_result", self.on_query_result(result));
            needs_redraw = true;
        }
        for search in drained_searches {
            self.on_search_result(search);
            needs_redraw = true;
        }

        // AddDevice flow: apply off-thread match-check/bind results (drain first so the rx borrow ends before we mutate self).
        let add_updates: Vec<AddDeviceUpdate> = self
            .add_device_rx
            .as_ref()
            .map(|rx| rx.try_iter().collect())
            .unwrap_or_default();
        for update in add_updates {
            match update {
                AddDeviceUpdate::Candidates(reqs) => {
                    // Precompute each candidate's expected word tokens + keyed name once per refresh, so the per-keystroke matcher is a plain string walk. Requests were already signature-verified in bindreq_list; the seed is in-session by definition on this screen. `heard_ble` marks candidates whose announce beacon we're hearing right now (proximity).
                    if let Some(seed) = self.session.as_ref().map(|s| s.identity_seed) {
                        use crate::network::fgtw::fleet;
                        let heard: Vec<[u8; 32]> = crate::network::pairing_beacon::heard()
                            .into_iter()
                            .map(|c| c.device_pubkey)
                            .collect();
                        self.add_device_candidates = reqs
                            .into_iter()
                            .map(|req| {
                                let words = fleet::masked_device_words(&req.device_pubkey, &seed);
                                AddCandidate {
                                    name: fleet::device_name_default(&req.device_pubkey, &seed),
                                    tokens: fleet::pair_word_list(&words),
                                    heard_ble: heard.contains(&req.device_pubkey),
                                    req,
                                }
                            })
                            .collect();
                        self.refresh_add_device_match();
                    }
                }
                AddDeviceUpdate::Bound(pk) => {
                    self.add_device_checking = false;
                    self.add_device_bound = Some(pk);
                    let name = self
                        .session
                        .as_ref()
                        .map(|s| crate::network::fgtw::fleet::device_name_default(&pk, &s.identity_seed))
                        .unwrap_or_default();
                    if self.add_device_bind_ble {
                        // BLE / list-tap select: the candidate was picked by proximity + name, NOT by typing its full 256-bit key — so a wrong pick is possible. Hold the fleet-key rotation behind the human's "did it turn green?" confirm (two-phase); a wrong bind stays a keyless ledger entry.
                        self.add_device_status = format!("Bound {name} — did it turn green?");
                    } else {
                        // WORDS path: the typed 256-bit match already IS the confirmation (you can only type the words shown on the one device in your hand — no wrong candidate), so release the fleet key immediately.
                        self.add_device_status = format!("Adding {name}\u{2026}");
                        self.spawn_confirm_add();
                    }
                }
                AddDeviceUpdate::Rotated => {
                    self.add_device_checking = false;
                    // Ceremony complete — back to the Fleet page it was launched from (the new device's row is the confirmation), instead of stranding the user on a finished words screen.
                    self.end_add_device_flow();
                    self.state = AppState::Settings(SettingsPage::Fleet);
                    self.ready_toast = Some("Device added \u{221a}".to_string());
                    // The confirm rotated the fleet key — recover the new epoch AND re-seal the roster under it in one ordered pass, so the just-joined device's roster pull decrypts instead of failing aead::Error until a relaunch. (Was a bare key-sync that left the roster stale-sealed forever, since the periodic re-push only fires on a non-in-app attest.)
                    self.spawn_roster_republish();
                    // And re-fold our own chain immediately so the freshly-bound device gets its sibling contact (fleet weave kickoff) without waiting for the next fleet event.
                    if let Some(our_hp) =
                        self.handle_query.as_ref().and_then(|hq| hq.get_handle_proof())
                    {
                        self.spawn_contact_fleet_refresh(vec![our_hp]);
                    }
                }
                AddDeviceUpdate::Failed(e) => {
                    self.add_device_checking = false;
                    self.add_device_status = format!("Error: {e}");
                }
            }
            needs_redraw = true;
        }

        // Diagnostics log-submit results (off-thread FGTW upload).
        let log_submit_updates: Vec<Result<(), String>> = self
            .log_submit_rx
            .as_ref()
            .map(|rx| rx.try_iter().collect())
            .unwrap_or_default();
        for update in log_submit_updates {
            self.log_submit_inflight = false;
            match update {
                Ok(()) => {
                    self.ready_toast = Some("Log sent \u{221a}".to_string());
                    // Clear the note once it's been submitted so the next submit starts blank. MUST be the widget's clear() — wiping `chars` directly leaves cursor/widths stale, and the next cursor paint slices widths[..cursor] out of range → panic → abort (this was the "submit a log → app dies" crash).
                    if let Some(tb) = self.settings_note_textbox.as_mut() {
                        tb.clear();
                    }
                    crate::log("DIAG: log submitted to FGTW");
                    // Baseline AFTER the success line above so the submit flow's own records don't instantly re-arm the pill — Submit greys until something genuinely new lands in the log.
                    self.log_submitted_len = Some(crate::log_size_bytes());
                }
                Err(e) => {
                    self.ready_toast = Some(format!("Send failed: {e}"));
                    crate::log(&format!("DIAG: log submit failed: {e}"));
                }
            }
            needs_redraw = true;
        }

        // The auto-update checkbox is the first linked-settings consumer: a user toggle writes updates.auto (born linked, so the whole fleet follows; unlink comes with the per-setting link affordance). Poll-then-set keeps the borrow simple.
        let autoupdate_toggle = self
            .settings_autoupdate_check
            .as_mut()
            .map(|cb| (cb.take_toggle(), cb.is_checked()));
        if let Some((true, checked)) = autoupdate_toggle {
            if self.settings_set("updates.auto", vec![checked as u8]) {
                crate::log(&format!("SETTINGS: updates.auto = {checked} (linked write)"));
            }
            needs_redraw = true;
        }

        // AddDevice flow: the status line is EVENT-driven, re-derived on every edit by the LIVE MATCHER — the typed entry prefix-matches against the candidate word strings from the binding-request registry (docs/pairing-v2.md), so a typo flags at the exact word it happens and a full 23-word match auto-binds.
        if matches!(self.state, AppState::AddDevice) {
            let text: String = self.textbox.as_ref().map(|tb| tb.chars.iter().collect()).unwrap_or_default();
            if text != self.add_device_wordcheck_text {
                self.add_device_wordcheck_text = text;
                self.refresh_add_device_match();
                needs_redraw = true;
            }
        }

        // New-device JOIN flow: words display + matched flag + membership results.
        let join_updates: Vec<JoinUpdate> = self
            .add_join_rx
            .as_ref()
            .map(|rx| rx.try_iter().collect())
            .unwrap_or_default();
        for update in join_updates {
            match update {
                JoinUpdate::ShowWords(words) => {
                    self.add_join_words = Some(words);
                    self.add_join_status = "Add this device from one that's already signed in:".to_string();
                }
                JoinUpdate::Joined(fleet_key, session) => {
                    // We're in the fleet now — leaving this screen IS the green the far side confirms. Drop add-mode and run the normal attest (it now passes the fleet gate). Stash any received fleet key to persist once attest sets the vault up.
                    self.add_join_rx = None;
                    self.launch_add_mode = false;
                    self.add_join_words = None;
                    self.add_join_status.clear();
                    self.pending_fleet_key = fleet_key;
                    self.add_join_handle = None;
                    // Attest with the roots the join thread already derived — no handle re-entry, no second ~1s proof, and no route thru submit_handle's permanence interstitial (this claims nothing new; the fleet exists and we were just bound into it).
                    if let Some(hq) = self.handle_query.as_ref() {
                        hq.query_first_attest_with_roots(session);
                        self.state = AppState::Launch(LaunchState::Attesting);
                        self.change_focus(None);
                    }
                }
                JoinUpdate::Failed(e) => {
                    // The ceremony is dead — take the words DOWN with it. Leaving them up strands the screen on a corpse: the user keeps waiting on words no thread is polling for. Back to handle entry with the error visible; re-submitting starts a fresh ceremony.
                    self.add_join_rx = None;
                    self.add_join_words = None;
                    self.add_join_status = format!("Join failed: {e}");
                }
            }
            needs_redraw = true;
        }

        // Deferred initial roster pull: fire the moment the (async-synced) fleet key lands, so wake-up catch-up brings sibling-added friends onto this device. One-shot per attest/resume.
        if self.needs_initial_roster_pull
            && self.roster_pull_rx.is_none()
            && self.fleet_key_cached().is_some()
        {
            self.needs_initial_roster_pull = false;
            crate::log("FLEET: initial roster pull (wake-up catch-up)");
            self.spawn_roster_pull();
        }

        // Fleet roster pull result: merge into the contact list (re-CLUTCH happens via the serialized keygen kick inside merge_roster_entries). Fleet-event push: a sibling device changed the shared roster (fstate) or the membership chain (fleet) — pull the change NOW instead of at our next attest. This is what makes a friend added on one device appear on the rest of the fleet in about a second.
        let fleet_evts: Vec<(&'static str, [u8; 32])> = self
            .fleet_evt_rx
            .as_ref()
            .map(|rx| rx.try_iter().collect())
            .unwrap_or_default();
        if !fleet_evts.is_empty() {
            let our_hp = self.handle_query.as_ref().and_then(|hq| hq.get_handle_proof());
            let mut refresh_contacts: Vec<[u8; 32]> = Vec::new();
            for (kind, evt_hp) in &fleet_evts {
                if Some(*evt_hp) == our_hp {
                    // OUR fleet: shared-state or membership change — pull it now.
                    match *kind {
                        "fstate" | "friendship" if self.roster_pull_rx.is_none() => self.spawn_roster_pull(),
                        "fleet" => {
                            self.spawn_fleet_key_sync();
                            // Membership changed: re-fold our own chain so sibling contacts reconcile (fleet weave) — this is how existing members learn about a freshly-added device within ~a second.
                            if !refresh_contacts.contains(evt_hp) {
                                refresh_contacts.push(*evt_hp);
                            }
                        }
                        _ => {}
                    }
                } else if *kind == "fleet"
                    && self.contacts.iter().any(|c| c.handle_proof == *evt_hp)
                    && !refresh_contacts.contains(evt_hp)
                {
                    // A CONTACT's fleet chain extended (they added/removed a device) — re-fold so we honour their current device set.
                    refresh_contacts.push(*evt_hp);
                }
            }
            if !refresh_contacts.is_empty() {
                self.spawn_contact_fleet_refresh(refresh_contacts);
            }
            needs_redraw = true;
        }

        // Contact-fleet refresh results: fold-and-honour a friend's current device set, and ARM the fold-respecting trust rule. OUR OWN hp routes to sibling reconcile FIRST and never into any contact's fleet_members — the self-contact and every sibling contact carry our hp, and folding our own fleet into one of them would make it swallow sibling pongs/paths via first-match `knows_device` routing.
        let member_updates: Vec<([u8; 32], Vec<[u8; 32]>, i64)> = self
            .contact_members_rx
            .as_ref()
            .map(|rx| rx.try_iter().collect())
            .unwrap_or_default();
        if !member_updates.is_empty() {
            let our_hp = self.handle_query.as_ref().and_then(|hq| hq.get_handle_proof());
            let mut changed = false;
            let mut to_persist: Vec<usize> = Vec::new();
            for (hp, members, tip_ts) in member_updates {
                if Some(hp) == our_hp {
                    self.reconcile_fleet_siblings(&members);
                    needs_redraw = true;
                    continue;
                }
                let Some((idx, c)) = self
                    .contacts
                    .iter_mut()
                    .enumerate()
                    .find(|(_, c)| c.handle_proof == hp)
                else {
                    continue;
                };
                // Monotonic freshness gate FIRST, before any mutation: never adopt a fold whose tip is older than the last one we adopted (an R2 eventual-consistency read serving a stale pre-removal set must not overwrite a fresh post-removal one). A first fold (fleet_members_ts == 0) always passes since real eagle times are positive.
                if c.fleet_folded_once && tip_ts < c.fleet_members_ts {
                    crate::log(&format!("FLEET: ignoring stale fold for {} (tip {} < adopted {})", crate::fp(&hp), tip_ts, c.fleet_members_ts));
                    continue;
                }
                let shrank = c.fleet_folded_once && c.fleet_members.iter().any(|m| !members.contains(m));
                let set_changed = c.fleet_members != members;
                let arming = !c.fleet_folded_once;
                if set_changed || arming || tip_ts != c.fleet_members_ts {
                    c.fleet_members = members;
                    c.fleet_members_ts = tip_ts;
                    c.fleet_folded_once = true; // armed ONLY here, on an adopted fold — never on Err/stale
                    changed = changed || set_changed || arming;
                    to_persist.push(idx);
                    if shrank {
                        crate::log(&format!("FLEET: device revoked from {}'s fleet — dropping it from the answerable set", crate::fp(&hp)));
                    }
                }
            }
            if changed {
                self.reseed_contact_pubkeys(); // rebuild answerable set BEFORE persist: an in-flight pong this tick already sees the revoked device gone
                needs_redraw = true;
            }
            // Persist the adopted folded set + arm flag + tip ts so a restart resumes fold-respecting trust immediately (no bootstrap regression, no trust-nobody window).
            if !to_persist.is_empty() {
                if let Some(storage) = self.storage.as_ref().cloned() {
                    to_persist.sort_unstable();
                    to_persist.dedup();
                    for idx in to_persist {
                        if let Err(e) = crate::storage::contacts::save_contact(&self.contacts[idx], &storage) {
                            crate::log(&format!("FLEET: persist folded set failed: {e}"));
                        }
                    }
                }
            }
        }

        match self.roster_pull_rx.as_ref().map(|rx| rx.try_recv()) {
            Some(Ok(Ok(state))) => {
                self.roster_pull_rx = None;
                self.roster_pull_retries_left = 0;
                // Settings layers fold in first (global LWW + device newest-copy-wins); a change persists and takes effect on the next read of each key — a sibling's toggle lands here.
                if self.ensure_fleet_settings() {
                    let changed = self
                        .fleet_settings
                        .as_mut()
                        .unwrap()
                        .merge_from(state.global_settings, state.device_settings);
                    if changed {
                        if let (Some(fs), Some(storage)) = (self.fleet_settings.as_ref(), self.storage.as_ref()) {
                            if let Err(e) = crate::storage::fleet_settings::save_fleet_settings(fs, storage) {
                                crate::log(&format!("SETTINGS: persist after merge failed: {e}"));
                            }
                        }
                        self.apply_settings_to_ui();
                        crate::log("SETTINGS: adopted fleet changes");
                    }
                }
                self.merge_roster_entries(state.roster);
                needs_redraw = true;
            }
            Some(Ok(Err(_e))) => {
                // Pull failed to fetch/decrypt. On a fresh join this is the pairing key still being a pre-rotation generation; the in-flight fan-out key sync writes the current key within ~150ms, so re-arm and retry until the budget runs out (the pull's own round-trip spaces the attempts).
                self.roster_pull_rx = None;
                if self.roster_pull_retries_left > 0 {
                    self.roster_pull_retries_left -= 1;
                    self.needs_initial_roster_pull = true;
                    crate::log(&format!(
                        "FLEET: roster pull failed — retrying once the current fleet key lands ({} attempt(s) left)",
                        self.roster_pull_retries_left
                    ));
                } else {
                    crate::log("FLEET: roster pull retries exhausted — will re-try on the next fleet event or relaunch");
                }
            }
            Some(Err(std::sync::mpsc::TryRecvError::Disconnected)) => {
                self.roster_pull_rx = None; // thread died without sending; drop the dead channel
            }
            _ => {} // still pending, or no pull in flight
        }

        needs_redraw
    }
    /// Send a [`PhotonEvent`] thru the event-loop proxy. Returns `false` if the proxy hasn't been set yet (host hasn't called `set_event_proxy`) or if the event loop has closed. Background tasks clone the proxy once at startup and call this; UI-thread code should mutate state directly + return `true` from `tick` or `on_event` instead of going thru the proxy.
    #[allow(dead_code)] // Wired for background tasks to push events onto the UI thread; no caller yet.
    pub fn send_event(&self, event: PhotonEvent) -> bool {
        match &self.event_proxy {
            Some(proxy) => proxy.send(event).is_ok(),
            None => false,
        }
    }

    /// Recompute the launch-screen widget geometry from the current viewport. Called from `init` once after construction and from `on_resize` on every viewport/zoom change. Font size and stroke ride `effective_span()` (= span × ru) so widgets grow/shrink with Ctrl+/Ctrl-/Ctrl+scroll zoom in lockstep with chrome.
    fn update_widget_layout(&mut self, ctx: &mut Context) {
        let buf_w = ctx.viewport.width_px as usize;
        let buf_h = ctx.viewport.height_px as usize;
        let layout = LaunchLayout::compute(buf_w, buf_h, ctx.viewport.ru);
        let attest = AttestBlockLayout::compute(layout.attest_block);
        // Font size = textbox-slot height × 0.75. Derived from the pill so the text-to-pill ratio stays constant at any viewport — span/24 sized text via the harmonic-mean span which scales differently from pill_h (pill_h is linear in viewport_h, span is biased toward the narrower dim), so on a tall narrow phone the pill grew faster than the text and a soft-keyboard show/hide jumped the ratio. Pill-derived sizing keeps padding around the text proportional, so descenders + ascenders never crowd the squircle edge. Same scalar drives the attest button and the resting ∞ so they read as a matched set.
        let textbox_h = (attest.textbox.y1 as f32) - (attest.textbox.y0 as f32);
        let font_size = textbox_h * 0.75;

        if let Some(tb) = self.textbox.as_mut() {
            let (cx, cy, w, h) = rect_center_dims(attest.textbox);
            tb.set_rect(cx, cy, w, h);
            tb.set_font_size(font_size, ctx.text);
        }
        if let Some(btn) = self.attest_btn.as_mut() {
            let (cx, cy, w, h) = rect_center_dims(attest.attest);
            btn.set_rect(cx, cy, w, h);
            btn.set_font_size(font_size);
        }

        // Contacts-page widgets: textbox takes the full ReadyLayout textbox slot; the plus button is OVERLAID inside the textbox's right edge. Button size = 7/8 textbox height, inset from the right by 1/16 of the textbox height. Same font_size as the launch widgets so zoom feels consistent across screens.
        let ready_layout = ReadyLayout::compute(buf_w, buf_h, ctx.viewport.ru);
        let slot = ready_layout.textbox;
        let slot_x0 = slot.x0 as f32;
        let slot_y0 = slot.y0 as f32;
        let slot_w = (slot.x1 - slot.x0) as f32;
        let slot_h = (slot.y1 - slot.y0) as f32;
        // The search box + overlaid plus button scroll with the user section, so subtract the SAME `contacts_scroll` the render pass uses. Both passes read it from `self`, so the rendered content (drawn at this rect) and the hit-stamp (which follows the rect) move together; offsetting the rect moves visual + hit area as one.
        let tb_cx = slot_x0 + slot_w * 0.5;
        let tb_cy = slot_y0 + slot_h * 0.5 - self.contacts_scroll as f32;
        let plus_size = slot_h * 7.0 / 8.0;
        let plus_inset = slot_h / 16.0;
        let plus_cx = slot_x0 + slot_w - plus_inset - plus_size * 0.5;
        let plus_cy = tb_cy;
        if let Some(tb) = self.contacts_textbox.as_mut() {
            tb.set_rect(tb_cx, tb_cy, slot_w, slot_h);
            tb.set_font_size(font_size, ctx.text);
        }
        if let Some(btn) = self.contacts_plus_btn.as_mut() {
            btn.set_rect(plus_cx, plus_cy, plus_size, plus_size);
            btn.set_font_size(font_size);
        }

        // Conversation compose box: a full-width strip lifted off the bottom edge by `compose_margin`. Geometry must match the render block's `compose_h`/`compose_margin`/`compose_cy`, where `unit` is ReadyLayout's span-based harmonic unit (same as the contacts screen — no hardcoded pixels). The send button is OVERLAID inside the box's right edge, exactly like the contacts-screen `+` search button (7/8 of the box height, inset 1/16 from the right).
        let unit = ReadyLayout::compute(buf_w, buf_h, ctx.viewport.ru).unit_height;
        let compose_h = unit * 1.8;
        let compose_margin = unit * 0.8;
        let compose_w = buf_w as f32 - unit * 2.0;
        let compose_cx = buf_w as f32 * 0.5;
        let compose_cy = buf_h as f32 - compose_margin - compose_h * 0.5;
        if let Some(tb) = self.message_textbox.as_mut() {
            tb.set_rect(compose_cx, compose_cy, compose_w, compose_h);
            tb.set_font_size(font_size, ctx.text);
        }
        if let Some(btn) = self.message_send_btn.as_mut() {
            let send_size = compose_h * 7.0 / 8.0;
            let send_inset = compose_h / 16.0;
            let box_right = compose_cx + compose_w * 0.5;
            let send_cx = box_right - send_inset - send_size * 0.5;
            btn.set_rect(send_cx, compose_cy, send_size, send_size);
            btn.set_font_size(font_size);
        }

        // Settings panel (STUB): position the stateful widgets on the selected page. Content-body rows give each control a slot; a control's rect is a portion of its row so the label can sit beside / above it. Only the active page's widgets are repositioned — the others keep their placeholder geometry off-screen, and `visit` gates them out anyway.
        if let AppState::Settings(page) = self.state {
            let layout = SettingsLayout::compute(&ctx.viewport);
            let settings_content_scroll = self.settings_content_scroll;
            // Controls ride the same unit as the page text (zoom + shape aware) — these were the "don't change scale" elements.
            let ctrl_font = (layout.unit * 0.58).max(8.0);
            let ctrl_h = (layout.unit * 1.00).max(14.0);
            match page {
                SettingsPage::Appearance => {
                    // Rows: [0]=title [1]=Theme label [2]=Theme dropdown [3]=Party colours [4]=Zoom label [5]=Zoom slider [6]=Calibration.
                    let rows = layout.content_scrolled(8, settings_content_scroll).split_v([1.0; 8]);
                    if let Some(dd) = self.settings_theme_dropdown.as_mut() {
                        let r = rows[2].center_h(0.7);
                        dd.set_rect(r.center_x(), r.center_y(), r.w, ctrl_h);
                        dd.set_font_size(ctrl_font);
                    }
                    if let Some(sl) = self.settings_zoom_slider.as_mut() {
                        let r = rows[5].center_h(0.8);
                        sl.set_rect(r.center_x(), r.center_y(), r.w, ctrl_h);
                    }
                }
                SettingsPage::Recovery => {
                    let rows = layout.content_scrolled(8, settings_content_scroll).split_v([1.0; 8]);
                    if let Some(cb) = self.settings_custodian_check.as_mut() {
                        let r = rows[2];
                        cb.set_rect(r.x + r.w * 0.45, r.center_y(), r.w * 0.9, ctrl_h);
                        cb.set_font_size(ctrl_font);
                    }
                }
                SettingsPage::Notifications => {
                    let rows = layout.content_scrolled(8, settings_content_scroll).split_v([1.0; 8]);
                    if let Some(cb) = self.settings_chime_check.as_mut() {
                        let r = rows[1];
                        cb.set_rect(r.x + r.w * 0.45, r.center_y(), r.w * 0.9, ctrl_h);
                        cb.set_font_size(ctrl_font);
                    }
                    if let Some(cb) = self.settings_presence_check.as_mut() {
                        let r = rows[3];
                        cb.set_rect(r.x + r.w * 0.45, r.center_y(), r.w * 0.9, ctrl_h);
                        cb.set_font_size(ctrl_font);
                    }
                }
                SettingsPage::Updates => {
                    let rows = layout.content_scrolled(8, settings_content_scroll).split_v([1.0; 8]);
                    if let Some(cb) = self.settings_autoupdate_check.as_mut() {
                        let r = rows[2];
                        cb.set_rect(r.x + r.w * 0.45, r.center_y(), r.w * 0.9, ctrl_h);
                        cb.set_font_size(ctrl_font);
                    }
                }
                SettingsPage::Diagnostics => {
                    let rows = layout.content_scrolled(10, settings_content_scroll).split_v([1.0; 10]);
                    if let Some(tb) = self.settings_note_textbox.as_mut() {
                        let r = rows[7].center_h(0.95);
                        tb.set_rect(r.center_x(), r.center_y(), r.w, ctrl_h * 1.2);
                        tb.set_font_size(ctrl_font, ctx.text);
                    }
                }
                _ => {}
            }
        }
    }

    /// Submit the contacts-page textbox contents as an FGTW handle search. Called from Enter in `contacts_textbox` and from clicking `contacts_plus_btn`. Bails on empty input, on no `HandleQuery` available (init failure path), and on a search for the user's own attested handle (would just find their own device — no point). Successful Found results land in `tick()`'s drain loop and append to `self.contacts`. Persistence + UI transition into a search-in-flight visual state (the rotating-hourglass plus button) ride in subsequent slices.
    fn submit_add_friend(&mut self) {
        let handle: String = match self.contacts_textbox.as_ref() {
            Some(tb) => tb.chars.iter().collect(),
            None => return,
        };
        if handle.is_empty() {
            return;
        }

        let typed_pid = crate::crypto::clutch::identity_party_id(&crate::types::Handle::to_identity_seed(&handle));
        if self.contacts.iter().any(|c| c.handle_hash == typed_pid) {
            crate::log("add-friend: handle already in contacts");
            return;
        }
        // Self-contact: if the handle matches our own identity, create the contact directly — FGTW won't return our own record as a search result.
        let is_self = self.session.as_ref().map_or(false, |s| {
            crate::storage::contacts::derive_identity_seed(&handle) == s.identity_seed
        });
        if is_self {
            if let Some(session) = &self.session {
                let handle_text = crate::types::HandleText::new(&handle);
                let device_pubkey = self
                    .device_keypair
                    .as_ref()
                    .map(|kp| crate::types::DevicePubkey::from_bytes(*kp.public.as_bytes()))
                    .unwrap_or_else(|| crate::types::DevicePubkey::from_bytes([0u8; 32]));
                let mut contact =
                    crate::types::Contact::new(handle_text, session.handle_proof, device_pubkey);
                contact.clutch_state = crate::types::ClutchState::Complete;
                crate::log("add-friend: self-contact — CLUTCH auto-completed");
                self.contacts.push(contact);
                if let Some(storage) = self.storage.as_ref() {
                    if let Some(c) = self.contacts.last() {
                        if let Err(e) = crate::storage::contacts::save_contact(c, storage) {
                            crate::log(&format!("Failed to save contact: {}", e));
                        }
                    }
                }
                self.search_status = Some((format!("added {handle}"), SEARCH_FOUND_COLOUR));
                if let Some(tb) = self.contacts_textbox.as_mut() {
                    tb.clear();
                }
            }
            return;
        }

        if let Some(hq) = self.handle_query.as_ref() {
            crate::log("add-friend: searching FGTW");
            hq.search(handle);
            // Enter the search-in-flight visual state: rotating hourglass over the plus button, last result cleared. Defocus the textbox so further typing doesn't mutate the handle being searched.
            self.add_in_flight = true;
            self.search_status = None;
            self.change_focus(None);
        }
    }

    /// Clipboard chord handler (desktop only). `op` is the lowercased character — "c" copy, "x" cut, "v" paste — acting on whichever textbox holds focus (launch handle or contacts search). Returns `Handled` when a textbox owned the focus, `Pass` otherwise (so the chord doesn't get eaten on a non-text screen). Copy/cut read `selected_text`; cut only deletes after the OS `set_text` succeeds, so a clipboard failure never silently destroys the selection. Paste inserts the clipboard string at the cursor (replacing any selection via `insert_str`). A launch-textbox edit clears a stale `Error` back to `Fresh`; the cursor blink reset is the caller's job.
    #[cfg(not(any(target_os = "redox", target_os = "android")))]
    fn clipboard_chord(&mut self, op: &str, text: &mut fluor::text::TextRenderer) -> EventResponse {
        // Resolve focus to exactly one editable textbox; bail to Pass if focus is elsewhere (button, avatar, nothing).
        let on_launch = self
            .textbox
            .as_ref()
            .map(|t| Some(t.hit_id()) == self.focused)
            .unwrap_or(false);
        let on_contacts = self
            .contacts_textbox
            .as_ref()
            .map(|t| Some(t.hit_id()) == self.focused)
            .unwrap_or(false);
        if !on_launch && !on_contacts {
            return EventResponse::Pass;
        }
        // A busy field can't be the clipboard target: `sync_busy_freeze` releases focus before disabling it, so `on_launch`/`on_contacts` (which key off `self.focused`) are already false above. No separate attesting/add-in-flight gate needed.
        let tb = if on_launch {
            self.textbox.as_mut()
        } else {
            self.contacts_textbox.as_mut()
        };
        let Some(tb) = tb else {
            return EventResponse::Pass;
        };

        match op {
            "c" => {
                if let Some(sel) = tb.selected_text() {
                    if let Ok(mut clip) = arboard::Clipboard::new() {
                        let _ = clip.set_text(sel);
                    }
                }
            }
            "x" => {
                if let Some(sel) = tb.selected_text() {
                    // Only delete after the clipboard accepts the text — a failed copy must not destroy the selection.
                    let copied = arboard::Clipboard::new()
                        .and_then(|mut clip| clip.set_text(sel))
                        .is_ok();
                    if copied {
                        tb.delete_selection(text);
                        if on_launch {
                            self.clear_launch_error();
                        }
                    } else {
                        crate::log("clipboard: copy failed, not cutting");
                    }
                }
            }
            "v" => {
                if let Ok(mut clip) = arboard::Clipboard::new() {
                    if let Ok(s) = clip.get_text() {
                        // Words entry accepts only letters and space — strip everything else from the paste (newlines/tabs become nothing; the camelCase/space tokenizer handles the rest).
                        let s = if matches!(self.state, AppState::AddDevice) {
                            s.chars().filter(|c| c.is_ascii_alphabetic() || *c == ' ').collect()
                        } else {
                            s
                        };
                        if !s.is_empty() {
                            tb.insert_str(&s, text);
                            if on_launch {
                                self.clear_launch_error();
                            }
                        }
                    }
                }
            }
            _ => {}
        }
        EventResponse::Handled
    }

    /// A launch-handle edit invalidates any prior attestation error OR an armed permanence confirmation — drop `Error`/`Confirm` back to `Fresh` so the message clears and the user can resubmit. Editing the handle IS the cancel gesture for the Confirm interstitial. No-op off the launch screen or in other states.
    fn clear_launch_error(&mut self) {
        if matches!(
            self.state,
            AppState::Launch(LaunchState::Error(_)) | AppState::Launch(LaunchState::Confirm)
        ) {
            self.state = AppState::Launch(LaunchState::Fresh);
            self.probed_session = None;
            self.probed_handle = None;
            if let Some(btn) = self.attest_btn.as_mut() {
                btn.set_label("Attest");
            }
        }
    }

    /// Encrypt + send the compose-box contents to the open contact, append it as an outgoing bubble, and persist. No-op unless a CLUTCH-Complete contact is open with a friendship chain and the box is non-empty. The crypto/wire/persist layers already exist (`FriendshipChains::prepare_send`, `StatusChecker::send_message`, `save_messages`); this is the UI→chain→network glue. Orb (chrome app-icon) tap. Returns true if it acted (caller redraws). Routed by screen: Ready → open the settings / about / help panel (its own screen with a nine-page nav rail); Settings → no-op (the dedicated back affordance exits). Launch / AddDevice / Conversation ignore the orb. The interim Ready → AddDevice entry moved onto the Fleet page's "Add device" pill.
    fn on_orb_click(&mut self) -> bool {
        match self.state {
            AppState::Ready => {
                self.change_focus(None);
                self.state = AppState::Settings(SettingsPage::You);
                true
            }
            // Settings / AddDevice / Launch / Conversation fall thru: the orb is settings-only, and navigation off those screens is a dedicated control (back button), never the orb.
            _ => false,
        }
    }

    /// Enter the add-device (pairing-words) flow. Was the interim Ready-orb action; now reached from the Fleet page's "Add device" pill. Spawns the bindreq watch so the candidate set is live before the first keystroke.
    fn open_add_device_flow(&mut self) {
        self.add_device_candidates.clear();
        self.add_device_bound = None;
        self.add_device_bind_ble = false;
        self.add_device_typo = None;
        self.add_device_wordcheck_text.clear();
        self.add_device_checking = false;
        self.add_device_status = "Type the words shown on the new device".to_string();
        let (tx, rx) = std::sync::mpsc::channel();
        self.add_device_rx = Some(rx);
        self.add_device_tx = Some(tx);
        self.state = AppState::AddDevice;
        if let Some(tb) = self.textbox.as_mut() {
            tb.clear();
        }
        let tb_id = self.textbox.as_ref().map(|t| t.hit_id());
        self.change_focus(tb_id);
        self.spawn_bindreq_watch();
    }

    /// Off-thread candidate watch for the AddDevice screen: list the registry (member-gated, signature-verified in the client), push the set up, then wait on a hub poke (`pair_evt` kind "request") or an ~8s cadence — whichever first — until the flow's stop flag drops it. Push-driven with a poll floor, the join loop's shape.
    fn spawn_bindreq_watch(&mut self) {
        use crate::network::fgtw::fleet;
        use std::sync::atomic::{AtomicBool, Ordering};
        let (Some(hp), Some(kp), Some(seed), Some(tx)) = (
            self.handle_query.as_ref().and_then(|hq| hq.get_handle_proof()),
            self.device_keypair.clone(),
            self.session.as_ref().map(|s| s.identity_seed),
            self.add_device_tx.clone(),
        ) else {
            return;
        };
        let stop = std::sync::Arc::new(AtomicBool::new(false));
        self.add_device_stop = Some(stop.clone());
        std::thread::spawn(move || {
            // Hub subscription: a "request" event for OUR identity pokes the refetch the moment the new device posts/withdraws; the poll cadence carries when the socket is down. Best-effort accelerator, never load-bearing.
            let (wake_tx, wake_rx) = std::sync::mpsc::channel::<()>();
            {
                let stop = stop.clone();
                crate::network::http::runtime().spawn(async move {
                    use futures::StreamExt;
                    let Ok((mut ws, _)) = tokio_tungstenite::connect_async("wss://fgtw.org/ws").await else {
                        return;
                    };
                    loop {
                        tokio::select! {
                            frame = ws.next() => {
                                match frame {
                                    Some(Ok(m)) => {
                                        if stop.load(Ordering::Relaxed) {
                                            return;
                                        }
                                        if let Some((kind, evt_hp)) = fleet::parse_pair_event(&m.into_data()) {
                                            if evt_hp == hp && kind == "request" {
                                                let _ = wake_tx.send(());
                                            }
                                        }
                                    }
                                    _ => return, // closed or errored — poll cadence takes over
                                }
                            }
                            _ = tokio::time::sleep(std::time::Duration::from_secs(5)) => {
                                if stop.load(Ordering::Relaxed) {
                                    return;
                                }
                            }
                        }
                    }
                });
            }
            loop {
                if stop.load(Ordering::Relaxed) {
                    return;
                }
                match fleet::bindreq_list(&kp, &hp, &seed) {
                    Ok(reqs) => {
                        let _ = tx.send(AddDeviceUpdate::Candidates(reqs));
                    }
                    Err(e) => crate::log(&format!("FLEET: bindreq list failed: {e}")),
                }
                match wake_rx.recv_timeout(crate::jitter_dur(std::time::Duration::from_secs(8))) {
                    Ok(()) | Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                    Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                        std::thread::sleep(crate::jitter_dur(std::time::Duration::from_secs(8)));
                    }
                }
            }
        });
    }

    /// Re-derive the AddDevice status from the current entry + candidate set — the live matcher (docs/pairing-v2.md). Per candidate: every completed typed token must equal the expected token exactly; the still-being-typed last token passes while it's a prefix of the candidate's next word. Divergence flags the exact word; a full 23-token exact match IS the selection and auto-binds (correct words are the confirmation — the fleet key still waits behind the green confirm).
    fn refresh_add_device_match(&mut self) {
        use crate::network::fgtw::fleet;
        if !matches!(self.state, AppState::AddDevice) || self.add_device_bound.is_some() || self.add_device_checking {
            return;
        }
        let text = self.add_device_wordcheck_text.clone();
        let tokens = fleet::pair_word_list(&text);
        self.add_device_typo = None;
        if self.add_device_candidates.is_empty() {
            // Nothing to match against yet — the voca spell-check still catches finger trouble while the registry answers.
            self.add_device_typo = fleet::first_bad_pair_word(&text);
            self.add_device_status = match &self.add_device_typo {
                Some(w) => format!("'{w}' isn't one of the words"),
                None => "Waiting for the new device\u{2026} it should be showing its words".to_string(),
            };
            return;
        }
        if tokens.is_empty() {
            let n = self.add_device_candidates.len();
            self.add_device_status = if n == 1 {
                "Type the words shown on the new device".to_string()
            } else {
                format!("{n} devices asking to join \u{2014} type the words on the one in your hand")
            };
            return;
        }
        // The last typed token is COMPLETE (must match exactly) only once a separator follows it — same rule as first_bad_pair_word, so nothing flashes red mid-word.
        let last_complete = text != text.trim_end();
        let n = tokens.len();
        let mut full_match: Option<usize> = None;
        let mut alive = 0usize;
        let mut alive_idx = 0usize;
        let mut deepest = 0usize;
        for (ci, cand) in self.add_device_candidates.iter().enumerate() {
            let mut depth = 0usize;
            let mut ok = n <= cand.tokens.len();
            if ok {
                for (i, t) in tokens.iter().enumerate() {
                    let is_last = i + 1 == n;
                    let hit = if !is_last || last_complete {
                        &cand.tokens[i] == t
                    } else {
                        cand.tokens[i].starts_with(t.as_str())
                    };
                    if hit {
                        depth += 1;
                    } else {
                        ok = false;
                        break;
                    }
                }
            }
            deepest = deepest.max(depth);
            if ok {
                alive += 1;
                alive_idx = ci;
                if n == fleet::PAIR_WORD_COUNT && tokens[n - 1] == cand.tokens[n - 1] {
                    full_match = Some(ci);
                }
            }
        }
        if let Some(ci) = full_match {
            // Exact 23-word match against a verified request — the selection is made. Bind (phase one); the fold re-verifies the consent this request carries. WORDS path → auto-rotate (clear any stale BLE-tap flag so the Bound handler doesn't park on the confirm).
            let req = self.add_device_candidates[ci].req.clone();
            self.add_device_bind_ble = false;
            self.spawn_bind_device(req);
            return;
        }
        if alive == 0 {
            // The entry left every candidate at token index `deepest` — name that exact word in red.
            let bad = tokens.get(deepest.min(n - 1)).cloned().unwrap_or_default();
            self.add_device_status = format!("'{bad}' doesn't match any device asking to join");
            self.add_device_typo = Some(bad);
        } else if alive == 1 {
            let name = &self.add_device_candidates[alive_idx].name;
            self.add_device_status = format!("matching {name}\u{2026}");
        } else {
            self.add_device_status = format!("matching\u{2026} ({alive} devices)");
        }
    }

    /// Stop the NEW-device join thread and clear its display state (words, ready light, handle step).
    fn end_join_flow(&mut self) {
        if let Some(stop) = self.add_stop.take() {
            stop.store(true, std::sync::atomic::Ordering::Relaxed);
        }
        self.add_join_handle = None;
        self.add_join_rx = None;
        self.add_join_words = None;
    }

    /// The fleet key from the local vault cache (fast, no network, no mint). `None` until a fan-out recover/establish has populated it (`spawn_fleet_key_sync`). Callers that seal/open fleet state read this; the background sync keeps it fresh so a rotation propagates.
    fn fleet_key_cached(&self) -> Option<[u8; 32]> {
        let storage = self.storage.as_ref()?;
        let session = self.session.as_ref()?;
        let addr = crate::storage::vault_key("fleet_key", &session.vault_seed);
        if let Ok(Some(bytes)) = storage.read_addr(&addr) {
            if let Ok(k) = <[u8; 32]>::try_from(bytes.as_slice()) {
                return Some(k);
            }
        }
        None
    }

    /// Recover the current fleet key from the fan-out (or establish it at genesis), off-thread, and cache it in the vault. This is how a device gets the fleet key now — sealed to its own device key, recoverable with just its `ihi` — superseding the pairing-secret hand-off. Triggered on attest and after a membership change, so a rotation propagates to every device. Session-long subscription to the FGTW hub's typed events for OUR identity. "fstate" (roster changed by a sibling device) and "fleet" (membership chain extended) land in `fleet_evt_rx`; tick drains them into a roster pull / key sync, and the wake proxy pokes the loop so it reacts immediately. Reconnects with jittered backoff — unlike the join ceremony's throwaway socket, this one is the LIVE propagation path (friend added on one device appears on the others), so it survives network blips. Idempotent: repeat calls while a subscription is live are no-ops.
    fn spawn_fleet_event_sub(&mut self) {
        use std::sync::atomic::Ordering;
        if self.fleet_evt_rx.is_some() {
            return;
        }
        let Some(hp) = self.handle_query.as_ref().and_then(|hq| hq.get_handle_proof()) else {
            return;
        };
        let (tx, rx) = std::sync::mpsc::channel::<(&'static str, [u8; 32])>();
        let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        self.fleet_evt_rx = Some(rx);
        self.fleet_evt_stop = Some(stop.clone());
        let wake = self.event_proxy.clone();
        let _ = hp; // attest-gate only; we no longer filter by our own hp — tick routes by hp (ours vs a contact's)
        crate::network::http::runtime().spawn(async move {
            use futures::StreamExt;
            loop {
                if stop.load(Ordering::Relaxed) {
                    return;
                }
                if let Ok((mut ws, _)) = tokio_tungstenite::connect_async("wss://fgtw.org/ws").await {
                    loop {
                        tokio::select! {
                            frame = ws.next() => {
                                match frame {
                                    Some(Ok(m)) => {
                                        if stop.load(Ordering::Relaxed) {
                                            return;
                                        }
                                        // Forward EVERY recognized event with its hp — no our-hp filter. Registry dings (fleet) go photon-wide, so a contact's chain change reaches us here too; tick decides whether an hp is ours (roster/key sync) or a contact's (member refresh). Bumps are tiny and the hub is low-traffic, so receive-all + filter-app-side is cheaper than re-subscribing every time contacts change.
                                        if let Some((kind, evt_hp)) = crate::network::fgtw::fleet::parse_pair_event(&m.into_data()) {
                                            let k: &'static str = match kind.as_str() {
                                                "fstate" => "fstate",
                                                "fleet" => "fleet",
                                                "friendship" => "friendship",
                                                _ => continue,
                                            };
                                            if tx.send((k, evt_hp)).is_err() {
                                                return; // app side dropped the receiver — subscription retired
                                            }
                                            if let Some(w) = wake.as_ref() {
                                                let _ = w.send(crate::ui::PhotonEvent::NetworkUpdate);
                                            }
                                        }
                                    }
                                    _ => break, // closed / errored — fall out to the reconnect sleep
                                }
                            }
                            _ = tokio::time::sleep(std::time::Duration::from_secs(5)) => {
                                if stop.load(Ordering::Relaxed) {
                                    return;
                                }
                            }
                        }
                    }
                }
                tokio::time::sleep(crate::jitter_dur(std::time::Duration::from_secs(20))).await;
            }
        });
        crate::log("FLEET: event subscription started (fstate/fleet/friendship push)");
    }

    /// Off-thread: fold each given contact's public membership chain (`fleet::current_members_with_ts` by handle_proof, blocking HTTP) and post back the current device set plus the chain-tip eagle time for tick to adopt into `fleet_members` (monotonically — see the drain). Idempotent and best-effort: a fetch failure sends nothing, so the drain never runs and the last-known folded set + arm flag stay untouched (never trust-nobody on a network blip). Results land via `contact_members_rx`. Started on contact load/merge (wake-up catch-up) and on a `fleet` bump for a contact's identity (live).
    fn spawn_contact_fleet_refresh(&mut self, handle_proofs: Vec<[u8; 32]>) {
        if handle_proofs.is_empty() {
            return;
        }
        if self.contact_members_tx.is_none() {
            let (tx, rx) = std::sync::mpsc::channel::<([u8; 32], Vec<[u8; 32]>, i64)>();
            self.contact_members_rx = Some(rx);
            self.contact_members_tx = Some(tx);
        }
        let tx = self.contact_members_tx.as_ref().unwrap().clone();
        let wake = self.event_proxy.clone();
        std::thread::spawn(move || {
            for hp in handle_proofs {
                match crate::network::fgtw::fleet::current_members_with_ts(&hp) {
                    Ok((members, tip_ts)) => {
                        if tx.send((hp, members, tip_ts)).is_err() {
                            return; // app dropped the receiver
                        }
                    }
                    Err(e) => crate::log(&format!(
                        "FLEET: contact fleet refresh failed for {}: {e}",
                        crate::fp(&hp)
                    )),
                }
            }
            if let Some(w) = wake.as_ref() {
                let _ = w.send(crate::ui::PhotonEvent::NetworkUpdate);
            }
        });
    }

    /// Reconcile OUR OWN folded fleet membership into sibling contacts (the fleet weave). For each member device that isn't us and has no sibling contact yet: create one as `ClutchState::Pending` — the serialized keygen queue picks it up and the full CLUTCH ceremony + weave runs against it exactly like a friend. For each sibling contact whose device fell out of the fold: remove it and delete its state + chains (revocation hygiene — an ex-member must not stay ceremony-eligible). Idempotent; triggered from attest/resume, our-hp `fleet` events, and the binder's `AddDeviceUpdate::Bound`.
    fn reconcile_fleet_siblings(&mut self, members: &[[u8; 32]]) {
        let Some(our_device) = self.device_keypair.as_ref().map(|kp| *kp.public.as_bytes())
        else {
            return;
        };
        let Some(our_hp) = self.handle_query.as_ref().and_then(|hq| hq.get_handle_proof()) else {
            return;
        };
        let mut changed = false;

        // Add newly-folded members.
        for device in members {
            if *device == our_device {
                continue;
            }
            if self
                .contacts
                .iter()
                .any(|c| c.is_sibling && c.public_identity.key == *device)
            {
                continue;
            }
            let sib = crate::types::Contact::new_sibling(
                our_hp,
                crate::types::DevicePubkey::from_bytes(*device),
            );
            crate::log(&format!(
                "SIBLING: reconciled +1 (device {}) — fleet weave pending",
                hex::encode(&device[..4])
            ));
            if let Some(storage) = self.storage.as_ref() {
                if let Err(e) = crate::storage::contacts::save_contact(&sib, storage) {
                    crate::log(&format!("SIBLING: failed to persist sibling: {}", e));
                }
            }
            self.contacts.push(sib);
            changed = true;
        }

        // Drop de-folded members (device removed from OUR chain).
        let mut removed: Vec<crate::types::Contact> = Vec::new();
        self.contacts.retain(|c| {
            if c.is_sibling && !members.contains(&c.public_identity.key) {
                removed.push(c.clone());
                false
            } else {
                true
            }
        });
        for c in &removed {
            crate::log(&format!(
                "SIBLING: reconciled -1 (device {} left the fold)",
                hex::encode(&c.public_identity.key[..4])
            ));
            changed = true;
            if let Some(storage) = self.storage.as_ref() {
                if let Err(e) =
                    crate::storage::contacts::delete_sibling(&c.public_identity.key, storage)
                {
                    crate::log(&format!("SIBLING: failed to delete sibling state: {}", e));
                }
                if let Some(fid) = c.friendship_id {
                    self.friendship_chains.retain(|(id, _)| *id != fid);
                    if let Err(e) =
                        crate::storage::friendship::delete_friendship_chains(&fid, storage)
                    {
                        crate::log(&format!("SIBLING: failed to delete sibling chains: {}", e));
                    }
                }
            }
        }

        if changed {
            self.reseed_contact_pubkeys();
            // Address the freshly-created siblings from the retained attest echo (device-keyed rows). Without this the sibling has no `ip`, so its CLUTCH offer — built the moment keygen finishes — silently no-ops on the missing address and never retries (no address ⇒ never pinged ⇒ never Online ⇒ Online-handler re-send never fires). The echo carried the sibling's row all along; it was just consumed before the sibling existed.
            if !self.last_peers.is_empty() {
                let peers = std::mem::take(&mut self.last_peers);
                self.refresh_contact_addrs_from_peers(&peers);
                self.last_peers = peers;
            }
        }
    }

    fn spawn_fleet_key_sync(&self) {
        use crate::network::fgtw::fleet;
        let (Some(hp), Some(device_key), Some(storage), Some(session)) = (
            self.handle_query.as_ref().and_then(|hq| hq.get_handle_proof()),
            self.device_keypair.clone(),
            self.storage.as_ref().cloned(),
            self.session.as_ref(),
        ) else {
            return;
        };
        let addr = crate::storage::vault_key("fleet_key", &session.vault_seed);
        std::thread::spawn(
            move || match fleet::recover_or_establish_fleet_key(&hp, &device_key) {
                Ok(Some(k)) => {
                    if let Err(e) = storage.write_addr(&addr, &k) {
                        crate::log(&format!("FLEET: fleet key cache failed: {e}"));
                    } else {
                        crate::log("FLEET: fleet key synced from fan-out");
                    }
                }
                Ok(None) => {}
                Err(e) => crate::log(&format!("FLEET: fleet key sync failed: {e}")),
            },
        );
    }

    /// After binding a new device (which rotated the fan-out epoch), recover the CURRENT fleet key and re-seal our contact roster under it — in ONE ordered thread so the push can't race the async cache write and seal under the stale epoch. Without this the roster slot stays sealed under the pre-rotation key, and the freshly-joined device's pulls fail `aead::Error` forever (it correctly holds the new key). The roster entries are snapshotted on the UI thread (needs `&self`); the recover+cache+push run off-thread.
    fn spawn_roster_republish(&self) {
        use crate::network::fgtw::fleet;
        let entries = self.current_roster();
        let (Some(hp), Some(kp), Some(storage), Some(session)) = (
            self.handle_query.as_ref().and_then(|hq| hq.get_handle_proof()),
            self.device_keypair.clone(),
            self.storage.as_ref().cloned(),
            self.session.as_ref(),
        ) else {
            return;
        };
        let addr = crate::storage::vault_key("fleet_key", &session.vault_seed);
        std::thread::spawn(move || match fleet::recover_or_establish_fleet_key(&hp, &kp) {
            Ok(Some(k)) => {
                if let Err(e) = storage.write_addr(&addr, &k) {
                    crate::log(&format!("FLEET: fleet key cache failed: {e}"));
                }
                crate::log("FLEET: fleet key synced from fan-out (post-bind)");
                if entries.is_empty() {
                    return; // no contacts to share, but the key is now current for the roster PULL
                }
                match fleet::push_roster(&hp, &kp, &k, &entries) {
                    Ok(()) => crate::log(&format!(
                        "FLEET: roster re-pushed under rotated epoch ({} entr(ies))",
                        entries.len()
                    )),
                    Err(e) => crate::log(&format!("FLEET: roster re-push failed: {e}")),
                }
            }
            Ok(None) => {}
            Err(e) => crate::log(&format!("FLEET: post-bind key sync failed: {e}")),
        });
    }

    /// Persist a fleet key received over pairing (new device), overwriting any local placeholder so this device converges on the founder's key.
    fn fleet_key_store(&self, key: &[u8; 32]) {
        if let (Some(storage), Some(session)) = (self.storage.as_ref(), self.session.as_ref()) {
            let addr = crate::storage::vault_key("fleet_key", &session.vault_seed);
            if let Err(e) = storage.write_addr(&addr, key) {
                crate::log(&format!("FLEET: fleet key store failed: {e}"));
            }
        }
    }

    /// Build the fleet roster from the live contact list — the syncable subset, minus self-contacts (notes-to-self are device-local, not a friend to share) and minus fleet siblings (infrastructure, not friends — a sibling pid leaking into the roster would merge as a bogus contact on every device).
    fn current_roster(&self) -> Vec<crate::network::fgtw::fleet::RosterEntry> {
        use crate::network::fgtw::fleet::RosterEntry;
        let our_pid = self.session.as_ref().map(|s| crate::crypto::clutch::identity_party_id(&s.identity_seed));
        self.contacts
            .iter()
            .filter(|c| !c.is_sibling && our_pid != Some(c.handle_hash))
            .map(|c| RosterEntry {
                handle_proof: c.handle_proof,
                handle_hash: c.handle_hash,
                public_identity: *c.public_identity.as_bytes(),
                name: c.petname.clone(),
                avatar_pin: c.avatar_pin,
                added: c.added,
                updated: c.added,
                tombstone: false,
            })
            .collect()
    }

    /// Merge pulled roster entries into the live contact list — same path as a cloud-merge: add the ones we don't have as Pending stubs, register their pubkeys, then kick ONE serialized keygen (the tick loop re-CLUTCHes the rest one McEliece at a time so a multi-contact join doesn't starve the UI). v0 ignores tombstones (removal-propagation deferred).
    fn merge_roster_entries(&mut self, entries: Vec<crate::network::fgtw::fleet::RosterEntry>) {
        let mut added = 0usize;
        for e in entries {
            if e.tombstone {
                continue;
            }
            // Dedup against FRIEND contacts only — sibling contacts carry OUR handle_proof and must not suppress a roster entry (nor be treated as its holder).
            if self
                .contacts
                .iter()
                .any(|c| !c.is_sibling && c.handle_proof == e.handle_proof)
            {
                continue;
            }
            let device_pubkey = crate::types::DevicePubkey::from_bytes(e.public_identity);
            let contact = crate::types::Contact::from_pin(e.name.clone(), e.avatar_pin, e.handle_proof, e.handle_hash, device_pubkey);
            self.contacts.push(contact);
            added += 1;
        }
        if added == 0 {
            return;
        }
        crate::log(&format!(
            "FLEET: merged {added} contact(s) from fleet roster (total: {})",
            self.contacts.len()
        ));
        self.reseed_contact_pubkeys();
        // Re-fold every contact's fleet after a roster merge (newly-merged contacts have no members yet).
        let mut hps: Vec<[u8; 32]> = self
            .contacts
            .iter()
            .filter(|c| !c.is_sibling)
            .map(|c| c.handle_proof)
            .collect();
        hps.sort_unstable();
        hps.dedup();
        self.spawn_contact_fleet_refresh(hps);
        // Force any merged self-contact Complete so it's skipped by the keygen filter, then persist the newly-added tail (post-settle so a self→Complete flip is saved).
        self.settle_self_contacts();
        let start = self.contacts.len() - added;
        if let Some(storage) = self.storage.as_ref().cloned() {
            for c in &self.contacts[start..] {
                if let Err(e) = crate::storage::contacts::save_contact(c, &storage) {
                    crate::log(&format!("FLEET: save merged contact failed: {e}"));
                }
            }
        }
        self.spawn_next_pending_keygen();
    }

    /// Spawn a background pull of the fleet roster (debounced: one in flight at a time). The result is drained in `tick` and merged. No-op without a handle_proof + fleet key.
    fn spawn_roster_pull(&mut self) {
        use crate::network::fgtw::fleet;
        if self.roster_pull_rx.is_some() {
            return;
        }
        let (Some(hp), Some(fleet_key)) = (
            self.handle_query.as_ref().and_then(|hq| hq.get_handle_proof()),
            self.fleet_key_cached(),
        ) else {
            return;
        };
        let (tx, rx) = std::sync::mpsc::channel();
        self.roster_pull_rx = Some(rx);
        std::thread::spawn(move || {
            let result = match fleet::pull_fstate(&hp, &fleet_key) {
                Ok(Some(s)) => {
                    crate::log(&format!(
                        "FLEET: state pulled — {} roster entr(ies), {} global setting(s), {} device map(s)",
                        s.roster.len(),
                        s.global_settings.len(),
                        s.device_settings.len()
                    ));
                    Ok(s)
                }
                Ok(None) => {
                    crate::log("FLEET: state pull — slot empty");
                    Ok(fgtw::fstate::FleetState::default())
                }
                Err(e) => {
                    crate::log(&format!("FLEET: state pull failed: {e}"));
                    Err(e)
                }
            };
            let _ = tx.send(result);
        });
    }

    /// Lazily load the linked-settings cache (needs storage + our device pubkey). Returns whether it's available.
    fn ensure_fleet_settings(&mut self) -> bool {
        if self.fleet_settings.is_some() {
            return true;
        }
        let (Some(storage), Some(kp)) = (self.storage.as_ref(), self.device_keypair.as_ref()) else {
            return false;
        };
        self.fleet_settings = Some(crate::storage::fleet_settings::load_fleet_settings(storage, kp.public.to_bytes()));
        self.apply_settings_to_ui();
        true
    }

    /// Mirror the settings layer into the widgets that display it (after a load or an adopted fleet merge). updates.auto defaults ON until a value exists (the compiled default per docs/updates.md).
    fn apply_settings_to_ui(&mut self) {
        let auto = self
            .fleet_settings
            .as_ref()
            .and_then(|fs| fs.effective("updates.auto").map(|v| v != [0]))
            .unwrap_or(true);
        if let Some(cb) = self.settings_autoupdate_check.as_mut() {
            cb.set_checked(auto);
        }
    }

    /// Set a setting from UI: writes the global (linked, the default) or our device map (unlinked), persists, and pushes to the fleet slot. Returns true if the value actually changed.
    fn settings_set(&mut self, key: &str, value: Vec<u8>) -> bool {
        if !self.ensure_fleet_settings() {
            return false;
        }
        let fs = self.fleet_settings.as_mut().unwrap();
        if !fs.set(key, value, vsf::eagle_time_oscillations()) {
            return false;
        }
        self.persist_and_push_settings();
        true
    }

    /// Flip a key's link on this device (unlink = set locally from now on; relink = follow the fleet). Persists + pushes on change.
    fn settings_set_link(&mut self, key: &str, linked: bool) -> bool {
        if !self.ensure_fleet_settings() {
            return false;
        }
        let fs = self.fleet_settings.as_mut().unwrap();
        if !fs.set_link(key, linked, vsf::eagle_time_oscillations()) {
            return false;
        }
        self.persist_and_push_settings();
        true
    }

    fn persist_and_push_settings(&mut self) {
        if let (Some(fs), Some(storage)) = (self.fleet_settings.as_ref(), self.storage.as_ref()) {
            if let Err(e) = crate::storage::fleet_settings::save_fleet_settings(fs, storage) {
                crate::log(&format!("SETTINGS: persist failed: {e}"));
            }
        }
        self.spawn_settings_push();
    }

    /// Push our settings layers to the fleet slot (off-thread, best-effort). Pull-merge-push: the slot's current state folds in first, so a concurrent sibling write converges by CRDT instead of being clobbered — same doctrine as push_roster's roster-preserving pull.
    fn spawn_settings_push(&self) {
        use crate::network::fgtw::fleet;
        let Some(fs) = self.fleet_settings.as_ref() else {
            return;
        };
        let ours = fgtw::fstate::FleetState {
            roster: Vec::new(),
            global_settings: fs.global.clone(),
            device_settings: fs.devices.clone(),
        };
        let (Some(hp), Some(kp), Some(fleet_key)) = (
            self.handle_query.as_ref().and_then(|hq| hq.get_handle_proof()),
            self.device_keypair.clone(),
            self.fleet_key_cached(),
        ) else {
            return;
        };
        std::thread::spawn(move || {
            let slot = match fleet::pull_fstate(&hp, &fleet_key) {
                Ok(Some(s)) => s,
                _ => fgtw::fstate::FleetState::default(),
            };
            // Empty ours.roster merges to the slot's roster untouched (union) — settings pushes never disturb the roster.
            let merged = fgtw::fstate::merge_fstate(slot, ours);
            match fleet::push_fstate(&hp, &kp, &fleet_key, &merged) {
                Ok(()) => crate::log("SETTINGS: pushed to the fleet slot"),
                Err(e) => crate::log(&format!("SETTINGS: push failed: {e}")),
            }
        });
    }

    /// Publish this device's contact roster to the fleet slot (off-thread, best-effort). No-op if we have no contacts to share or lack the key/membership.
    fn spawn_roster_push(&self) {
        use crate::network::fgtw::fleet;
        let entries = self.current_roster();
        if entries.is_empty() {
            return;
        }
        let (Some(hp), Some(kp), Some(fleet_key)) = (
            self.handle_query.as_ref().and_then(|hq| hq.get_handle_proof()),
            self.device_keypair.clone(),
            self.fleet_key_cached(),
        ) else {
            return;
        };
        std::thread::spawn(move || {
            if let Err(e) = fleet::push_roster(&hp, &kp, &fleet_key, &entries) {
                crate::log(&format!("FLEET: roster push failed: {e}"));
            }
        });
    }

    /// Clear the AddDevice (existing-device) words-entry state. Sign the matched device into the fleet + rotate the fan-out, off-thread (both block on HTTP). Fires automatically when the typed words match the posted request — the deliberate act of typing 23 words on the already-trusted device IS the consent, and the new device's ready light has already flipped on the matched flag, so waiting for another tap only leaves the two screens out of step.
    /// Phase ONE of the two-phase ceremony: publish the consent-carrying Add for a fully-matched candidate. The chain entry is KEYLESS until the human confirms green — `spawn_confirm_add` holds the rotation.
    fn spawn_bind_device(&mut self, req: crate::network::fgtw::fleet::BindRequest) {
        use crate::network::fgtw::fleet;
        self.add_device_status = "Words match \u{2014} adding\u{2026}".to_string();
        self.add_device_checking = true;
        if let (Some(hp), Some(kp), Some(tx)) = (
            self.handle_query.as_ref().and_then(|hq| hq.get_handle_proof()),
            self.device_keypair.clone(),
            self.add_device_tx.clone(),
        ) {
            std::thread::spawn(move || {
                let _ = tx.send(match fleet::bind_device(&kp, &hp, &req) {
                    Ok(()) => AddDeviceUpdate::Bound(req.device_pubkey),
                    Err(e) => AddDeviceUpdate::Failed(e),
                });
            });
        }
    }

    /// Phase TWO — the human pressed "It's in" after seeing the new device leave its words screen: rotate the fleet key to the member set including the newcomer, releasing the key to it. Held behind the press so a wrong bind stays a keyless ledger entry.
    fn spawn_confirm_add(&mut self) {
        use crate::network::fgtw::fleet;
        if self.add_device_checking || self.add_device_bound.is_none() {
            return;
        }
        self.add_device_status = "Finishing\u{2026}".to_string();
        self.add_device_checking = true;
        if let (Some(hp), Some(kp), Some(tx)) = (
            self.handle_query.as_ref().and_then(|hq| hq.get_handle_proof()),
            self.device_keypair.clone(),
            self.add_device_tx.clone(),
        ) {
            std::thread::spawn(move || {
                let r = fleet::current_members(&hp)
                    .and_then(|members| fleet::rotate_fleet_key(&hp, &kp, &members).map(|_| ()));
                let _ = tx.send(match r {
                    Ok(()) => AddDeviceUpdate::Rotated,
                    Err(e) => AddDeviceUpdate::Failed(e),
                });
            });
        }
    }

    /// The fleet device inventory for the Fleet settings page: this device first, then our other devices (sibling contacts). Each entry is `(device_pubkey, is_self, is_online, name)`. Reuses phase-1's sibling reconcile as the live inventory (`current_members(own_hp)` is the authority; reconcile keeps the sibling set == fleet-minus-this-device) — no synchronous network fetch on render. Name = the canonical `device_name_default` (two voca words from the device PUBLIC key + our identity seed), the SAME function the pairing screen uses, so a device shows the same name here, on its own pairing screen, and on every other device in THIS fleet (a handed-off device gets a fresh name in the new owner's fleet).
    fn fleet_device_rows(&self) -> Vec<([u8; 32], bool, bool, String)> {
        use crate::network::fgtw::fleet::device_name_default;
        let Some(seed) = self.session.as_ref().map(|s| s.identity_seed) else {
            return Vec::new();
        };
        let mut rows = Vec::new();
        if let Some(kp) = self.device_keypair.as_ref() {
            let me = *kp.public.as_bytes();
            rows.push((me, true, true, device_name_default(&me, &seed)));
        }
        for c in self.contacts.iter().filter(|c| c.is_sibling) {
            let pk = c.public_identity.key;
            rows.push((pk, false, c.is_online, device_name_default(&pk, &seed)));
        }
        rows
    }

    /// Off-thread submit of this device's diagnostic log to FGTW (the Diagnostics "Submit" pill).
    /// The log can be up to 16 MiB and the POST blocks, so it runs on a thread and reports thru `log_submit_rx`. `note` is the user's optional-note textbox text. Snapshots the log bytes on the caller thread first (a plain file read) so a submit captures the log AT press time.
    fn spawn_log_submit(&mut self, note: String) {
        use crate::network::fgtw::put_log_blocking;
        let Some(bytes) = crate::snapshot_log_bytes() else {
            self.ready_toast = Some("No log to send yet".to_string());
            return;
        };
        if self.log_submit_tx.is_none() {
            let (tx, rx) = std::sync::mpsc::channel();
            self.log_submit_rx = Some(rx);
            self.log_submit_tx = Some(tx);
        }
        if let (Some(hp), Some(kp), Some(seed), Some(tx)) = (
            self.handle_query.as_ref().and_then(|hq| hq.get_handle_proof()),
            self.device_keypair.clone(),
            self.session.as_ref().map(|s| s.identity_seed),
            self.log_submit_tx.clone(),
        ) {
            let n = bytes.len();
            // Immediate press feedback — the upload runs seconds on a big log, and silence here read as "the button did nothing". Replaced by "Log sent √" / "Send failed" when the worker thread reports.
            self.ready_toast = Some(format!("Sending log ({} KiB)\u{2026}", (n + 1023) / 1024));
            crate::log(&format!("DIAG: submitting log ({n} bytes) to FGTW (sealed)"));
            std::thread::spawn(move || {
                let r = put_log_blocking(&bytes, &note, &kp, &hp, &seed).map_err(|e| format!("{e}"));
                let _ = tx.send(r);
            });
            self.log_submit_inflight = true;
        } else {
            self.ready_toast = Some("Can't send: not signed in".to_string());
        }
    }

    /// Rebuild the status checker's answerable-pubkey set from every contact's FULL fleet (`answerable_pubkeys` = first-met device union folded members). Idempotent — clears and refills, so it's safe after any change to contacts or their fleet_members. This is the single seam that makes presence + CLUTCH honour a friend's every device: seed here, and the offer/KEM/complete/SPEC gates (all of which read this one set) open for the whole fleet at once.
    fn reseed_contact_pubkeys(&self) {
        if let Ok(mut pks) = self.contact_pubkeys.lock() {
            pks.clear();
            for c in &self.contacts {
                for k in c.answerable_pubkeys() {
                    let dk = crate::types::DevicePubkey::from_bytes(k);
                    if !pks.contains(&dk) {
                        pks.push(dk);
                    }
                }
            }
        }
    }

    fn end_add_device_flow(&mut self) {
        // Stop the bindreq watch first — every exit path (back, orb, rotate-complete) kills the registry polling with the screen.
        if let Some(stop) = self.add_device_stop.take() {
            stop.store(true, std::sync::atomic::Ordering::Relaxed);
        }
        self.add_device_candidates.clear();
        self.add_device_bound = None;
        self.add_device_bind_ble = false;
        self.add_device_wordcheck_text.clear();
        self.add_device_typo = None;
        self.add_device_checking = false;
        self.add_device_status.clear();
        self.add_device_rx = None;
        self.add_device_tx = None;
        if let Some(tb) = self.textbox.as_mut() {
            tb.clear();
        }
        // Drop focus off the words textbox — clearing chars doesn't unfocus it, so without this the (now hidden) widget keeps its blinkey firing, keystrokes still route to it, and Android's soft keyboard stays up over the contact list.
        self.change_focus(None);
    }

    /// New-device JOIN: the handle was entered — generate this device's pairing identity, display its words, post the signed request, and poll for the matched flag + membership off-thread.
    fn submit_join_step(&mut self, precomputed_proof: Option<[u8; 32]>) {
        use crate::network::fgtw::fleet;
        use std::sync::atomic::{AtomicBool, Ordering};
        if self.add_join_rx.is_some() {
            return; // already joining
        }
        let handle: String = match self.textbox.as_ref() {
            Some(tb) => tb.chars.iter().collect(),
            None => return,
        };
        let handle = handle.trim().to_string();
        if handle.is_empty() {
            return;
        }
        let Some(device_key) = self.device_keypair.clone() else {
            self.add_join_status = "no device key".to_string();
            return;
        };
        self.add_join_handle = Some(handle.clone());
        self.add_join_status = "Preparing\u{2026}".to_string();
        self.change_focus(None);
        if let Some(tb) = self.textbox.as_mut() {
            tb.clear();
        }
        let (tx, rx) = std::sync::mpsc::channel();
        self.add_join_rx = Some(rx);
        let stop = std::sync::Arc::new(AtomicBool::new(false));
        self.add_stop = Some(stop.clone());
        std::thread::spawn(move || {
            // Derive the COMPLETE session roots once. identity_seed is microseconds; the ~1s memory-hard proof is reused from the probe when the caller passed it (the add-this-device branch already paid it), else computed here. Joined hands them to the attest worker so it never re-derives.
            let identity_seed = crate::storage::contacts::derive_identity_seed(&handle);
            let me = device_key.public.to_bytes();
            // SHOW THE WORDS FIRST — they only need identity_seed (microseconds) + the device pubkey, NOT the ~1s memory-hard handle_proof or the radio. Deferring this behind either left the screen on "Preparing…" for the whole proof (and, on Android, behind a blocking BLE-advertise JNI call) — the "stuck on Preparing" report. The words are this device's OWN pubkey masked to the fleet: shoulder-surfing them is inert (nothing binds without the request signature below; the mask makes them noise outside this fleet).
            let _ = tx.send(JoinUpdate::ShowWords(fleet::masked_device_words(&me, &identity_seed)));
            // NOW the expensive derivation (reused from the probe when the caller passed it; else the ~1s proof here) — the words are already up, so this cost is invisible.
            let handle_proof = precomputed_proof.unwrap_or_else(|| crate::types::Handle::username_to_handle_proof(&handle));
            let session = tohu::SessionIdentity {
                identity_seed,
                vault_seed: identity_seed,
                handle_proof,
            };
            let hp = session.handle_proof;
            // Pairing v2 SHADOW-mode announce beacon (docs/pairing-v2.md milestone A): advertise {hp4, device_pk} for the whole ceremony — the guard drops on every exit path below, stopping the radio. The words carry the real ceremony until the BLE transport lands.
            let _beacon = crate::network::pairing_beacon::announce_guard(&hp, &me);
            // The binding request: "I consent to join fleet hp", signed by the device key + co-signed by the identity key. THE registry entry the old device's matcher screens candidates from, and the consent egg the Add op will carry.
            if let Err(e) = fleet::bindreq_put(&device_key, &identity_seed, &hp) {
                let _ = tx.send(JoinUpdate::Failed(format!("request failed: {e}")));
                return;
            }

            // Push subscription: the FGTW hub broadcasts `pair_evt` frames on registry changes ("request") and when the chain extends ("fleet"). Each one for OUR identity pokes `wake_tx`, and the poll loop's sleep below is a `recv_timeout` — so the ceremony reacts the moment the other device acts, with the poll cadence as the guarantee when the socket drops (best-effort accelerator, never load-bearing). The task dies with the socket or the stop flag; no reconnect — the poll still covers everything.
            let (wake_tx, wake_rx) = std::sync::mpsc::channel::<()>();
            {
                let stop = stop.clone();
                crate::network::http::runtime().spawn(async move {
                    use futures::StreamExt;
                    let Ok((mut ws, _)) = tokio_tungstenite::connect_async("wss://fgtw.org/ws").await else {
                        return;
                    };
                    loop {
                        tokio::select! {
                            frame = ws.next() => {
                                match frame {
                                    Some(Ok(m)) => {
                                        if stop.load(Ordering::Relaxed) {
                                            return;
                                        }
                                        if let Some((_kind, evt_hp)) = fleet::parse_pair_event(&m.into_data()) {
                                            if evt_hp == hp {
                                                let _ = wake_tx.send(());
                                            }
                                        }
                                    }
                                    _ => return, // closed or errored — poll cadence takes over
                                }
                            }
                            _ = tokio::time::sleep(std::time::Duration::from_secs(5)) => {
                                if stop.load(Ordering::Relaxed) {
                                    return;
                                }
                            }
                        }
                    }
                });
            }
            // PUSH-DRIVEN, no deadline, no poll cadence. The hub events (request / fleet) wake each check instantly; the ONLY timers are the ones the protocol/transport demand — the binding request's 5-minute freshness (re-post at ~3.5min when no event arrives sooner) and a degraded-transport fallback cadence when the socket is dead. The user standing at the screen is the timeout: the ceremony ends when the bind lands, when they tap the orb (the stop flag), or on a hard network error. The request is WITHDRAWN by this device (its author) on every exit — green, cancel, or wrong-fleet — and lapses by stamp if the thread dies unclean.
            let withdraw = |why: &str| {
                if let Err(e) = fleet::bindreq_withdraw(&device_key, &hp) {
                    crate::log(&format!("JOIN: request withdraw ({why}) failed (lapses anyway): {e}"));
                }
            };
            let mut last_repost = std::time::Instant::now();
            loop {
                if stop.load(Ordering::Relaxed) {
                    withdraw("cancel");
                    return;
                }
                // Genesis re-checked on EVERY fetch, not just the probe: a relay that served the real chain once must not be able to swap in a structurally-valid foreign chain mid-ceremony and have this loop adopt its members (the probe-time-only TOCTOU — docs/pairing-v2.md).
                match fleet::current_members_verified(&hp, &identity_seed) {
                    Ok(m) if m.contains(&me) => {
                        // In the fleet — this is the green, and LEAVING THIS SCREEN is what the far-side human confirms, so it must happen NOW, not after the key. Withdraw our request (the author's exit act), try the fan-out once in case the sponsor already confirmed, and hand off — the key otherwise arrives via the post-attest fleet-event sync the moment the green-confirm rotation lands (the worker broadcasts "fleet" on fanout_put). Waiting here deadlocks the ceremony: the sponsor waits for our green before releasing the key this wait was for.
                        crate::log("JOIN: bound — this device is in the fleet chain");
                        withdraw("green");
                        let fleet_key = fleet::recover_fleet_key(&hp, &device_key).ok().flatten();
                        crate::log(&format!(
                            "JOIN: fleet key {} — attesting",
                            if fleet_key.is_some() { "recovered from fan-out" } else { "follows the sponsor's confirm (event-synced)" }
                        ));
                        let _ = tx.send(JoinUpdate::Joined(fleet_key, session));
                        return;
                    }
                    Ok(_) => {} // not bound yet — keep the request fresh and wait
                    Err(e) if e.contains("not rooted in this identity") => {
                        // Wrong-fleet is a VERDICT, not a blip: the chain now folds to a genesis that isn't ours, mid-ceremony. Scream and stop — retrying would just poll an imposter chain forever.
                        crate::log("JOIN: chain genesis no longer matches this identity — aborting");
                        withdraw("wrong-fleet");
                        let _ = tx.send(JoinUpdate::Failed("this handle's fleet is not ours — the chain changed hands mid-join".into()));
                        return;
                    }
                    Err(e) => {
                        // A transient fetch failure (laptop sleep/wake, dropped wifi, a server blip) must NOT kill the ceremony — the user is still standing at the words screen, and a dead thread strands it there forever (observed: macbook stuck on its words after one failed fetch). Log, back off, retry; the stop flag (orb cancel) is the only exit.
                        crate::log(&format!("JOIN: membership fetch failed ({e}) — retrying"));
                        std::thread::sleep(crate::jitter_dur(std::time::Duration::from_secs(15)));
                        continue;
                    }
                }
                // Wait for a push, but re-poll membership on a SHORT cadence so the ceremony completes fast even when the hub WebSocket push never arrives (observed on macOS: the bind landed but the new device sat ~3.5 min on the old 210s timeout before its next `current_members` check). The request re-post stays on its protocol cadence (~3.5 min — the registry stamp lapses at 5), tracked separately so the frequent membership polls don't spam re-posts. A hub event still short-circuits instantly.
                const MEMBERSHIP_POLL: std::time::Duration = std::time::Duration::from_secs(8);
                const REPOST_EVERY: std::time::Duration = std::time::Duration::from_secs(210);
                match wake_rx.recv_timeout(crate::jitter_dur(MEMBERSHIP_POLL)) {
                    Ok(()) => {} // hub push — loop re-checks membership immediately
                    Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                        if last_repost.elapsed() >= REPOST_EVERY {
                            let _ = fleet::bindreq_put(&device_key, &identity_seed, &hp);
                            last_repost = std::time::Instant::now();
                        }
                        // else: just loop and re-poll current_members (the fast path)
                    }
                    Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                        std::thread::sleep(crate::jitter_dur(std::time::Duration::from_secs(8)));
                        if last_repost.elapsed() >= REPOST_EVERY {
                            let _ = fleet::bindreq_put(&device_key, &identity_seed, &hp);
                            last_repost = std::time::Instant::now();
                        }
                    }
                }
            }
        });
    }

    /// Textbox front-end for the open conversation: pull + trim the compose text, hand it to [`Self::send_chain_message`] for the active contact (bubble shown), then clear the box.
    fn submit_message(&mut self) {
        let Some(ci) = self.active_contact else {
            return;
        };
        // Pull the compose text and send it VERBATIM. Any non-empty content sends, whitespace included (a lone space, all spaces, a newline are all valid messages). No trim, no whitespace judgment.
        let text: String = match self.message_textbox.as_ref() {
            Some(tb) => tb.chars.iter().collect(),
            None => return,
        };
        if text.is_empty() {
            // Empty send = liveness probe. Optimistically mark the peer offline and ping them; a returning pong flips is_online back true (check_status_updates), so an empty send confirms whether they're actually reachable right now instead of doing nothing.
            self.contacts[ci].is_online = false;
            self.ping_contact(ci);
            return;
        }
        self.send_chain_message(ci, &text, false);
        if let Some(tb) = self.message_textbox.as_mut() {
            tb.clear();
        }
        // Tell the Android host to restart IME input — a predictive keyboard still holds the just-sent text as a composing buffer and would re-materialise it on the next keystroke without this.
        self.pending_input_reset = true;
    }

    /// Encrypt + send + persist one chat message to `contact_idx` over the friendship chain, appending an outgoing bubble only when `!suppress_bubble`. Returns `true` if the message was dispatched to the network (so callers like the chain-weave probe only latch `probe_sent` on an actual send, and retry next cycle if the contact had no address yet). This is the reusable core factored out of the old open-contact send: it works for ANY contact index (not just `active_contact`), so the hidden chain-weave probe can ride the exact same ratchet path with its UI suppressed. Chain math (`prepare_send`, salt/advance) is untouched — the probe is a normal message whose only difference is a reserved marker content and a hidden bubble.
    fn send_chain_message(&mut self, contact_idx: usize, text: &str, suppress_bubble: bool) -> bool {
        use vsf::schema::section::FieldValue;

        let ci = contact_idx;
        let text = text.to_string();

        // Notes-to-self: no peer, no chains, no network — the message is delivered by definition (we already hold it). Insert the bubble + persist to the self conversation table (keyed off handle_hash like every conversation, so fleet history sync later carries it across our devices). Without this branch the send dead-ended at the missing friendship chain while submit_message cleared the box anyway — typed notes vanished. Probes never target self (maybe_send_chain_probe guard), so suppress_bubble can't arrive true here.
        let is_self = self.session.as_ref().map(|s| crate::crypto::clutch::identity_party_id(&s.identity_seed))
            == self.contacts.get(ci).map(|c| c.handle_hash);
        if is_self {
            let Some(contact) = self.contacts.get_mut(ci) else {
                return false;
            };
            let mut msg =
                ChatMessage::new_with_timestamp(text, true, vsf::eagle_time_oscillations());
            msg.delivered = true;
            contact.insert_message_sorted(msg);
            contact.message_scroll_offset = 0.0;
            if let Some(storage) = self.storage.as_ref() {
                if let Err(e) = crate::storage::contacts::save_messages(contact, storage) {
                    crate::log(&format!("STORAGE: failed to save self-note: {}", e));
                }
            }
            return true;
        }

        // Contact must be CLUTCH-Complete with a friendship chain.
        let (friendship_id, recipient_pubkey, addr_pair, our_handle_hash) = {
            let Some(contact) = self.contacts.get(ci) else {
                return false;
            };
            if contact.clutch_state != crate::types::ClutchState::Complete {
                crate::log("CHAT: cannot send — CLUTCH not complete");
                return false;
            }
            let Some(fid) = contact.friendship_id else {
                crate::log("CHAT: cannot send — no friendship chain");
                return false;
            };
            // Party id per contact: identity seed for friends, device-derived pid for fleet siblings — the chain index in prepare_send must match what from_clutch was keyed with.
            let Some(our_pid) = self.our_party_id(contact) else {
                return false;
            };
            (fid, contact.public_identity.key, contact.race_addrs(), our_pid)
        };
        let Some((peer_addr, alt_addr)) = addr_pair else {
            crate::log("CHAT: cannot send — no known address for contact");
            return false;
        };

        let eagle_time = vsf::eagle_time_oscillations();

        // The braid: choose up to TWO distinct prior PEER messages to weave into this chain step. Eligible = incoming messages (is_outgoing == false) in the last ≤256 of this conversation — any stored incoming row is one the receive path already ACKed, so the sender knows the peer holds it (both-held → identical strands → lockstep). The weave ingredient is the message's x-text (`content`), recoverable identically on both sides from the message DB. Each chosen message's eagle_time goes on the wire so the receiver resolves the SAME content. 0 eligible → weave nothing (anchor). 1 → single strand. ≥2 → two distinct (a true braid). Pick with gen_range (bounded, bias-free) — NEVER modulo. Strands are sorted by eagle_time so both peers frame derive_fresh_link identically regardless of pick order.
        let (woven_strands, woven_times): (Vec<Vec<u8>>, Vec<i64>) = {
            let mut chosen: Vec<(i64, Vec<u8>)> = Vec::new();
            if let Some(contact) = self.contacts.get(ci) {
                let window: Vec<&crate::types::ChatMessage> = contact
                    .messages
                    .iter()
                    .rev()
                    .filter(|m| !m.is_outgoing)
                    .take(256)
                    .collect();
                use rand::Rng;
                let mut rng = rand::thread_rng();
                if window.len() == 1 {
                    let m = window[0];
                    chosen.push((m.timestamp, m.content.as_bytes().to_vec()));
                } else if window.len() >= 2 {
                    let i = rng.gen_range(0..window.len());
                    let mut j = rng.gen_range(0..window.len() - 1);
                    if j >= i {
                        j += 1; // map [0, len-1) → [0, len)\{i} so j is distinct from i, uniformly
                    }
                    for &idx in &[i, j] {
                        let m = window[idx];
                        chosen.push((m.timestamp, m.content.as_bytes().to_vec()));
                    }
                }
            }
            chosen.sort_by_key(|(t, _)| *t);
            let times = chosen.iter().map(|(t, _)| *t).collect();
            let strands = chosen.into_iter().map(|(_, c)| c).collect();
            (strands, times)
        };

        // Build the message VSF the receiver parses: (message: x{text}, hp{incorporated_hp}, e6{woven_time}…, hR{pad}), field order shuffled to enforce type-marker (not positional) parsing. The e6 values name the woven peer messages (0, 1, or 2). The receive path reads them back via VsfField::parse.
        let (ciphertext, prev_msg_hp, conversation_token) = {
            let Some((_, chains)) = self
                .friendship_chains
                .iter_mut()
                .find(|(id, _)| *id == friendship_id)
            else {
                crate::log("CHAT: friendship chains missing for open contact");
                return false;
            };
            let incorporated_hp = chains
                .last_incorporated_hp()
                .map(|h| *h)
                .unwrap_or([0u8; 32]);
            let mut values = vec![
                vsf::VsfType::x(text.clone()),
                vsf::VsfType::hp(incorporated_hp.to_vec()),
            ];
            // The braid: name each woven peer message by its eagle_time (e6). 0, 1, or 2 of these.
            for &t in &woven_times {
                values.push(vsf::VsfType::e(vsf::EtType::e6(t)));
            }
            // Short random pad (median ~53B) for traffic-analysis resistance.
            let pad_len = rand::random::<u8>()
                .min(rand::random::<u8>())
                .min(rand::random::<u8>()) as usize;
            if pad_len > 0 {
                let pad: Vec<u8> = (0..pad_len).map(|_| rand::random()).collect();
                values.push(vsf::VsfType::hR(pad));
            }
            use rand::seq::SliceRandom;
            values.shuffle(&mut rand::thread_rng());
            let payload = FieldValue::new("message", values).flatten();

            // Chain ingredient = the bare x-text only (the hp/hR pad are siblings of x in the field, not part of it, and are never chain-key material). The full `payload` is what's encrypted onto the wire; `text` is what salts/advances the chain.
            let salt_text = text.clone().into_bytes();

            let conv_token = chains.conversation_token;
            match chains.prepare_send(&our_handle_hash, payload, salt_text, eagle_time, woven_strands) {
                Some((ct, prev, _msg_hp, _ph)) => (ct, prev, conv_token),
                None => {
                    crate::log("CHAT: prepare_send failed (not a participant)");
                    return false;
                }
            }
        };

        // CRASH SAFETY: persist chains (pending message + last_sent_hash) BEFORE the network send — disk is the commit point, the network is just notification.
        if let Some(storage) = self.storage.as_ref() {
            if let Some((_, chains)) = self
                .friendship_chains
                .iter()
                .find(|(id, _)| *id == friendship_id)
            {
                if let Err(e) = crate::storage::friendship::save_friendship_chains(chains, storage)
                {
                    crate::log(&format!("STORAGE CRITICAL: save chains before send: {}", e));
                }
            }
        }

        // Send over PT (UDP-preferred, TCP/relay fallback already wired).
        if let Some(ref checker) = self.status_checker {
            checker.send_message(crate::network::status::MessageRequest {
                peer_addr,
                alt_addr,
                recipient_pubkey,
                conversation_token,
                prev_msg_hp,
                ciphertext,
                eagle_time,
            });
            crate::log(&format!(
                "CHAT: sent message ({} chars) to contact",
                text.len()
            ));
        }

        // Append the outgoing bubble (delivered=false until the ACK lands) and persist — unless this is a suppressed send (the hidden chain-weave probe: it must ride the chain but show no UI).
        if let Some(contact) = self.contacts.get_mut(ci) {
            if !suppress_bubble {
                contact
                    .insert_message_sorted(ChatMessage::new_with_timestamp(text, true, eagle_time));
                contact.message_scroll_offset = 0.0;
                if let Some(storage) = self.storage.as_ref() {
                    if let Err(e) = crate::storage::contacts::save_messages(contact, storage) {
                        crate::log(&format!("STORAGE: failed to save messages: {}", e));
                    }
                }
            }
        }
        true
    }

    /// Just after a contact's CLUTCH reaches `Complete`, fire the one hidden chain-weave probe: a normal chat message with the reserved [`CHAIN_PROBE_MARKER`] content, sent once (guarded by `probe_sent`) with its UI bubble suppressed. When it lands the peer advances+ACKs the chain like any message, which is what proves the ratchet works end-to-end without the user seeing a decoy message. No-op if the contact isn't Complete, has no friendship chain yet, or already probed. Skips self-contacts (no peer to answer). Consolidates the transition-site logic so every `= ClutchState::Complete` path only needs one call.
    fn maybe_send_chain_probe(&mut self, contact_idx: usize) {
        let should_send = match self.contacts.get(contact_idx) {
            Some(c) => {
                c.clutch_state == crate::types::ClutchState::Complete
                    && c.friendship_id.is_some()
                    && !c.probe_sent
                    // Self-contact has no peer device to answer the probe.
                    && self.session.as_ref().map(|s| crate::crypto::clutch::identity_party_id(&s.identity_seed)) != Some(c.handle_hash)
            }
            None => false,
        };
        if !should_send {
            return;
        }
        crate::log("CHAIN-PROBE: sending hidden chain-weave probe");
        // Latch `probe_sent` only on an actual dispatch — if the contact had no address yet the send is a no-op and we retry on the next Complete transition / re-arm cycle rather than stalling.
        if self.send_chain_message(contact_idx, crate::types::CHAIN_PROBE_MARKER, true) {
            if let Some(c) = self.contacts.get_mut(contact_idx) {
                c.probe_sent = true;
            }
        }
    }

    /// Mark the chain end-to-end proven (`chain_woven = true`) once BOTH directions are validated: we've seen the peer's probe (their TX / our RX proven, `their_probe_seen`) AND our own chain has advanced via an ACK at least once (`chain_advanced_by_ack`, our TX / their RX proven). On seal, kill the ceremony proof rebroadcast (`clutch_proof_resends_left = 0`) so the completed CLUTCH stops re-announcing, flip the status line from "weaving the chain" to "secured", and persist. Idempotent — safe to call from either the probe-receive path or the ACK path. The chain math itself is never touched here.
    fn seal_chain_if_ready(&mut self, contact_idx: usize) {
        let Some(c) = self.contacts.get_mut(contact_idx) else {
            return;
        };
        if c.chain_woven {
            return;
        }
        if !(c.their_probe_seen && c.chain_advanced_by_ack) {
            return;
        }
        c.chain_woven = true;
        c.clutch_completed_at = Some(std::time::Instant::now()); // refresh the re-key cooldown thru the weave (armed at completion; this extends it)
        c.clutch_proof_resends_left = 0;
        crate::log("CHAIN-PROBE: chain woven — end-to-end verified, ceremony rebroadcast cancelled");
        // A fresh weave re-opens the blind conversation with this friend: re-probe for a deposit (reset side) and allow a fresh put (their reset wiped nothing of ours, but a re-key on OUR side after []n starts from scratch).
        c.blind_probe_missed = false;
        c.blind_in_flight = None;
        // Kick off friend-history recovery on the woven-chain EDGE (this fn fires exactly once per seal; vault loads latch chain_woven without passing here, so restarts resume via the persisted cursor instead of re-kicking). Always request the head page — if we already hold the history, merging dedups and the early-stop rule completes after one page. Siblings are excluded: friend-history recovery resolves "the other participant ≠ our seed", which is ambiguous on a sibling chain — fleet history sync is its own later phase.
        if !c.is_sibling {
            let was_complete_before = c
                .history_recovery
                .as_ref()
                .map(|r| r.complete)
                .unwrap_or(false);
            c.history_recovery = Some(crate::types::HistoryRecovery {
                oldest_recovered_osc: i64::MAX,
                complete: false,
                in_flight: None,
                next_request_osc: 0,
                urgent: true, // head page jumps the trickle interval — conversation usable ASAP
                was_complete_before,
            });
            crate::log("HISTORY: recovery kicked off (head page next tick)");
        }
        if let Some(storage) = self.storage.as_ref() {
            if let Err(e) = crate::storage::contacts::save_contact(c, storage) {
                crate::log(&format!("CHAIN-PROBE: failed to persist woven contact: {}", e));
            }
        }
    }

    /// Send the current textbox contents as an attestation query and transition Launch → Attesting. Called from Enter in the textbox path and from clicking the Attest button — same submit path. No-op if the textbox is empty, HandleQuery wasn't constructed (init failure path), or the launch sub-state forbids submission (`LaunchState::Attesting` — query already in flight; second submit would double-spend the ~5s memory-hard proof).
    fn submit_handle(&mut self) {
        if let AppState::Launch(s) = &self.state {
            if !s.can_edit_handle() {
                return;
            }
        }
        // In JOIN mode the launch textbox feeds the pairing flow (handle then words), not a fresh attestation.
        if self.launch_add_mode {
            self.submit_join_step(None);
            return;
        }
        let handle: String = match self.textbox.as_ref() {
            Some(tb) => tb.chars.iter().collect(),
            None => return,
        };
        if handle.is_empty() {
            return;
        }
        // A press FROM the Confirm interstitial is the deliberate second act: claim the (probed-Fresh) handle with the roots the probe already derived — no second proof, no permanence warning re-shown. GUARD: fire the stashed roots ONLY if the box still holds the handle they were derived from. Every edit path tears Confirm down, but this is the invariant that survives a missed one — firing stale roots attests a DIFFERENT identity than the box shows (observed: probe handle A, retype to taken handle B, press → attested as A, user believes they claimed B). On mismatch the press falls thru to a fresh probe of the current text.
        if matches!(self.state, AppState::Launch(LaunchState::Confirm)) {
            if let Some(btn) = self.attest_btn.as_mut() {
                btn.set_label("Attest");
            }
            let matches_probe = self.probed_handle.as_deref()
                == Some(crate::types::Handle::canonical(&handle).as_str());
            let session = self.probed_session.take();
            self.probed_handle = None;
            if let (Some(session), true) = (session, matches_probe) {
                self.fire_attest_query_with_roots(session);
                return;
            }
            crate::log("attest: confirm-press text no longer matches the probed handle — re-probing");
        }
        // First press: PROBE the handle against the network before deciding anything. The ~1s proof runs here (once); the branch (permanence warning / add-this-device / resume / taken) is chosen in `on_query_result` from the probe outcome, so "forever" is never shown for a handle that already has a fleet.
        if let Some(hq) = self.handle_query.as_ref() {
            hq.probe(handle);
            self.state = AppState::Launch(LaunchState::Attesting);
            self.change_focus(None);
        }
    }

    /// Fire an attest with caller-supplied roots (the probe already derived them), skipping the permanence interstitial and the second proof. First-attest persistence semantics.
    fn fire_attest_query_with_roots(&mut self, session: tohu::SessionIdentity) {
        if let Some(hq) = self.handle_query.as_ref() {
            hq.query_first_attest_with_roots(session);
            self.state = AppState::Launch(LaunchState::Attesting);
            self.change_focus(None);
        }
    }

    // (fire_attest_query — the string-based attest that BYPASSED the permanence interstitial — is deliberately gone: every launch-screen claim now flows probe → Confirm → roots-verified fire, so no path can attest a string the user didn't just confirm.)

    /// Handle a [`QueryResult`] arriving from HandleQuery's background worker. On success, stashes the proof, loads the device avatar + contacts, and transitions to the Ready screen; on rejection/error, drops to `LaunchState::Error` and refocuses the handle field.
    fn on_query_result(&mut self, result: QueryResult) {
        use num_bigint::BigUint;
        // Resume painted Ready optimistically from local state, so a result arriving while we're already past Launch is a background refresh (presence / contacts / cloud-merge), NOT a first attest. This gates the bailouts below: a transient network error must not knock a valid local session off Ready.
        let in_app = !matches!(self.state, AppState::Launch(_));
        match result {
            QueryResult::Probe { outcome, session } => {
                use crate::network::handle_query::ProbeOutcome;
                // The attest three-way branch, chosen from the network probe (see submit_handle). A probe result arriving while already in-app is stale (we navigated on) — ignore it.
                if in_app {
                    return;
                }
                match outcome {
                    ProbeOutcome::Fresh => {
                        // Genuine fresh claim — NOW show the permanence warning, stashing the probed roots (and the canonical handle they belong to) so the confirm press claims without re-deriving the proof.
                        self.probed_session = Some(session);
                        self.probed_handle = self
                            .textbox
                            .as_ref()
                            .map(|tb| crate::types::Handle::canonical(&tb.chars.iter().collect::<String>()));
                        self.state = AppState::Launch(LaunchState::Confirm);
                        if let Some(btn) = self.attest_btn.as_mut() {
                            btn.set_label("Yes — forever");
                        }
                    }
                    ProbeOutcome::Member => {
                        // Already in the fleet — just attest (resume). No warning; the announce passes the membership gate.
                        self.fire_attest_query_with_roots(session);
                    }
                    ProbeOutcome::JoinOurs => {
                        // Our identity, this device unenrolled — route straight to add-this-device (JOIN). The handle is still in the textbox; submit_join_step reads it and shows the pairing words.
                        crate::log("attest: handle has our fleet, this device unenrolled → add-this-device");
                        self.launch_add_mode = true;
                        self.state = AppState::Launch(LaunchState::Fresh);
                        self.add_join_handle = None;
                        self.submit_join_step(Some(session.handle_proof));
                    }
                    ProbeOutcome::Taken => {
                        self.state = AppState::Launch(LaunchState::Error(
                            "this handle is taken".to_string(),
                        ));
                        self.refocus_handle_select_all();
                    }
                }
            }
            QueryResult::Success(data) => {
                if let Some(hq) = self.handle_query.as_ref() {
                    hq.set_handle_proof(data.handle_proof);
                }
                // Sync our own avatar with FGTW now that the handle_proof is set — newest-wins, so a copy this identity set on another device propagates here, and ours propagates out, without either clobbering a fresher one. (Was a blind one-way upload, which could overwrite a newer server copy with a stale local one.)
                self.spawn_avatar_sync();
                // Live fleet propagation: subscribe to hub events for this identity (idempotent across resumes/re-attests in one run).
                self.spawn_fleet_event_sub();
                // Pubkey emitted as voca-encoded camelCase so a user reading the log can double-click + paste the value as a single word (matches `Development:` key lines from handle_query.rs). The handle is deliberately NOT logged — Photon never surfaces the plaintext handle.
                crate::log(&format!(
                    "attestation success: pubkey = {}",
                    voca::encode(BigUint::from_bytes_be(&data.handle_proof))
                ));
                // Adopt the session roots the worker just derived + persisted (register-shaped, no handle string). Shared across the user's TOKEN apps, gone at logout; a close/reopen resumes from these without re-typing or recomputing the proof. Fall back to the roots carried in the attest result if the tohu READ-BACK comes up empty (a persist failure must not leave THIS RUN sessionless — that made the avatar picker report "not attested" seconds after a successful attest). vault_seed == identity_seed mirrors the worker's derivation (handle_query FirstAttest).
                self.session = tohu::session().or(Some(tohu::SessionIdentity {
                    identity_seed: data.identity_seed,
                    vault_seed: data.identity_seed,
                    handle_proof: data.handle_proof,
                }));
                self.pending_broadcast_signal = 1;
                self.vault_degraded = data.vault_degraded;
                // The worker already loaded this device's avatar (keyed on identity_seed) into `data.avatar_pixels`; colour-convert it to BT.2020 γ=2.0 for the Ready screen. `None` = storage-miss → grey placeholder.
                if let Some(vsf_rgb) = &data.avatar_pixels {
                    self.device_avatar_pixels =
                        Some(crate::ui::colour_convert::vsf_rgb_to_bt2020(vsf_rgb));
                    self.device_avatar_scaled = None;
                    self.device_avatar_scaled_diameter = 0;
                }
                // Initialize local encrypted storage from the session's vault_seed + device secret. open_shared: on a resume this returns the SAME engine the resume path already opened (and the attest worker holds) — a second independent engine on the live vault is the corruption class, not a refresh.
                if let Some(session) = &self.session {
                    if let Some(kp) = &self.device_keypair {
                        let device_secret = *kp.secret.as_bytes();
                        match crate::storage::FlatStorage::open_shared(
                            crate::storage::APP,
                            session.vault_seed,
                            device_secret,
                        ) {
                            Ok(s) => self.storage = Some(s),
                            Err(e) => {
                                crate::log(&format!("STORAGE: init failed: {}", e));
                                // Hard vault-open failure → surface the red banner (overrides any `false` from `data.vault_degraded` set just above — a local open failure is worse).
                                self.vault_degraded = true;
                            }
                        }
                    }
                }
                // If we just joined a fleet, the vault is now open — persist the fleet key we recovered from the fan-out during pairing so this device shares the fleet's private state.
                if let Some(k) = self.pending_fleet_key.take() {
                    self.fleet_key_store(&k);
                    crate::log("FLEET: stored fleet key recovered during pairing");
                }
                // Establish (genesis founder) or refresh (existing device, picks up a rotation) the fleet key from the fan-out and cache it, so the roster/state seal uses the current key.
                self.spawn_fleet_key_sync();
                // Sync the fleet's shared contact roster. The pull is DEFERRED to tick (via the flag) because the fleet key is written by the async sync above — pulling here would race it and read an empty/stale key. On the INITIAL attest also push our existing set so a fleet formed before roster-sync existed seeds FGTW for newly-joined devices. Pulled contacts merge in as Pending stubs and re-CLUTCH on this device's own key (drained in tick).
                self.needs_initial_roster_pull = true;
                // ~8 attempts × the pull's ~150ms round-trip ≈ 1.2s — enough to outlast the fan-out key sync writing the current (post-rotation) key, after which the retry decrypts the roster cleanly.
                self.roster_pull_retries_left = 8;
                if !in_app {
                    self.spawn_roster_push();
                }
                // Merge incoming contacts with any already loaded locally — union by handle_proof so contacts added on another device (via FGTW/cloud) appear without losing locally-added ones. Siblings are excluded from domination: they carry OUR handle_proof and must not suppress a merged self-contact.
                let mut added = 0usize;
                let mut merged_ids: Vec<(ContactId, [u8; 32])> = Vec::new();
                for incoming in &data.contacts {
                    let dominated = self
                        .contacts
                        .iter()
                        .any(|c| !c.is_sibling && c.handle_proof == incoming.handle_proof);
                    if !dominated {
                        merged_ids.push((incoming.id.clone(), incoming.handle_hash));
                        self.contacts.push(incoming.clone());
                        added += 1;
                    }
                }
                if added > 0 {
                    crate::log(&format!(
                        "UI: merged {} new contact(s) from FGTW (total: {})",
                        added,
                        self.contacts.len()
                    ));
                    // Register the merged contacts' pubkeys so the checker answers their pings, and kick CLUTCH keygen for any that arrived Pending without keypairs. The resume path (load_all_contacts) already does this for locally-stored contacts, but cloud/FGTW-merged contacts land here AFTER that ran — without this they sit Pending forever with no keypairs, no offer, no connection (exactly what broke after a []n nuke wiped the local vault and contacts came back only via cloud).
                    self.reseed_contact_pubkeys();
                    // A merged self-contact (notes-to-self) needs no key exchange — force it Complete so it's skipped by the keygen filter below.
                    self.settle_self_contacts();
                    // Kick at most ONE keygen now; the rest are serialized by spawn_next_pending_keygen (called each tick) so we never run two McEliece keygens at once — two in parallel on launch starved the UI thread (the "first launch hangs" symptom). The Pending + keyless contacts are picked up one at a time as each keygen completes.
                    self.spawn_next_pending_keygen();
                }
                // Merge the friendship chains the worker loaded from disk into the live map. Without this, resumed contacts have no chains in self.friendship_chains and sending fails with "friendship chains missing" — even though storage loaded them fine. Only add ids we don't already hold; an in-session chain (built at ceremony completion) is fresher than a disk copy, so never clobber it.
                let mut merged_chains = 0usize;
                for (fid, chains) in data.friendships {
                    if !self.friendship_chains.iter().any(|(id, _)| *id == fid) {
                        self.friendship_chains.push((fid, chains));
                        merged_chains += 1;
                    }
                }
                if merged_chains > 0 {
                    crate::log(&format!(
                        "UI: merged {} friendship chain(s) from disk (total: {})",
                        merged_chains,
                        self.friendship_chains.len()
                    ));
                    self.update_sync_records();
                }
                // Refresh existing contacts' WAN + LAN addresses from the FGTW peer list. FGTW reports both a public and a same-LAN address per device; pulling the LAN address in lets the offer/KEM send race the LAN path against the WAN path right away, instead of waiting for LAN multicast (which routers often drop) or a pong. This is what unblocks a same-router peer whose stored WAN IPv6 says "No route to host" — the case where m never received an offer. Retain the echo so a sibling contact created LATER (by the async fleet fold below) can be addressed from the same rows.
                self.last_peers = data.peers.clone();
                self.refresh_contact_addrs_from_peers(&data.peers);
                // Fleet weave: load persisted siblings if this attest path didn't come thru the resume loader (e.g. JOIN-flow first attest, or []u→re-attest), then re-fold OUR OWN chain — the members drain routes our hp to reconcile_fleet_siblings, which creates Pending sibling contacts for any member device we don't hold yet. Fires on every background refresh too, giving a ~30s catch-up cadence for fleet changes we missed.
                if let Some(storage) = self.storage.as_ref().cloned() {
                    if !self.contacts.iter().any(|c| c.is_sibling) {
                        let siblings = crate::storage::contacts::load_all_siblings(
                            data.handle_proof,
                            &storage,
                        );
                        if !siblings.is_empty() {
                            crate::log(&format!(
                                "SIBLING: loaded {} sibling(s) from local vault on attest",
                                siblings.len()
                            ));
                            let fids: Vec<crate::types::FriendshipId> =
                                siblings.iter().filter_map(|c| c.friendship_id).collect();
                            for (fid, chains) in
                                crate::storage::friendship::load_all_friendships(&fids, &storage)
                            {
                                if !self.friendship_chains.iter().any(|(id, _)| *id == fid) {
                                    self.friendship_chains.push((fid, chains));
                                }
                            }
                            self.contacts.extend(siblings);
                            self.reseed_contact_pubkeys();
                        }
                    }
                    self.spawn_contact_fleet_refresh(vec![data.handle_proof]);
                }
                self.hints_dismissed = false;
                // Only flip to Ready on the INITIAL attest (we were still on the Launch screen). This Success branch also fires on every recurring background resume refresh — if the user has already navigated in-app (Ready, or inside a Conversation), forcing Ready here would yank them out of an open chat back to the contact list each sweep.
                if !in_app {
                    self.state = AppState::Ready;
                }
            }
            QueryResult::AlreadyAttested(peer) => {
                let msg = format!(
                    "handle already attested by another device (pubkey {})",
                    voca::encode(BigUint::from_bytes_be(peer.device_pubkey.as_bytes()))
                );
                crate::log_at(crate::LogLevel::Error, &format!("attestation rejected: {msg}"));
                // AlreadyAttested is now sent ONLY on a CHAIN-PROVEN takeover: the worker fold-verified a fleet chain whose genesis identity is not ours (handle_query.rs verdict). This is the genuine takeover case, so clearing the contested roots is correct — an indeterminate result (fold/parse/transport error) arrives as QueryResult::Error below, which does NOT clear the session. Clear so the next launch can't auto-resume into the same rejection, and bail to the attest screen (even from an optimistic Ready).
                tohu::clear_session();
                self.session = None;
                self.state = AppState::Launch(LaunchState::Error(msg));
                self.refocus_handle_select_all();
            }
            QueryResult::Error(e) => {
                crate::log_at(crate::LogLevel::Error, &format!("attestation error: {e}"));
                if in_app {
                    // Transient network failure on a resume refresh — the local session is still valid. Stay on Ready; the next presence cycle retries. Do NOT drop the user back to the attest screen.
                    crate::log("UI: background refresh failed (network); staying on local session");
                } else if e.contains("not in the fleet") && !self.launch_add_mode {
                    // Safety net for a probe→announce race (device removed from the fleet in the gap): the announce says "not in the fleet". The probe normally routes this to add-this-device UP FRONT, but if it slips thru, catch it here too.
                    self.launch_add_mode = true;
                    self.state = AppState::Launch(LaunchState::Fresh);
                    self.add_join_handle = None;
                    self.submit_join_step(None);
                } else {
                    self.state = AppState::Launch(LaunchState::Error(e));
                    self.refocus_handle_select_all();
                }
            }
        }
    }

    /// On an attestation error, return the user to an editable handle field with the whole handle selected. The submit path dropped focus into the frozen Attesting state; coming back to `Error` (which `can_edit_handle()` allows) we refocus the textbox and select-all so the most common fix — the handle is claimed, retype a different one — is one keystroke: the first character typed replaces the selection. On Android, `change_focus` into a textbox also re-raises the soft keyboard via the pending-keyboard signal.
    fn refocus_handle_select_all(&mut self) {
        let Some(id) = self.textbox.as_ref().map(|t| t.hit_id()) else {
            return;
        };
        self.change_focus(Some(id));
        if let Some(tb) = self.textbox.as_mut() {
            tb.select_all();
        }
    }

    /// Refocus the contacts textbox and select all text — used on search failure so the user can immediately retype.
    fn refocus_contacts_select_all(&mut self) {
        let Some(id) = self.contacts_textbox.as_ref().map(|t| t.hit_id()) else {
            return;
        };
        self.change_focus(Some(id));
        if let Some(tb) = self.contacts_textbox.as_mut() {
            tb.select_all();
        }
    }

    /// Handle a [`SearchResult`] from `HandleQuery::search`. On `Found`, build a `Contact` from the peer and append to `self.contacts` (skip if a contact with the same handle already exists; should be rare given `submit_add_friend` pre-checks, but the search races against attestation worker's contact load). Ends the in-flight hourglass and sets the result text shown below the search box: green "added {h}", red "not found" / "error: …".
    fn on_search_result(&mut self, result: crate::ui::state::SearchResult) {
        use crate::ui::state::SearchResult;
        // Search resolved — drop the hourglass regardless of which branch we take below.
        self.add_in_flight = false;
        match result {
            SearchResult::Found(peer) => {
                // peer.handle is the user's TYPED search input riding along locally — the first-met seam. Dedup by the party id it derives, never by a stored string.
                let handle = peer.handle.as_str().to_string();
                let typed_pid = crate::crypto::clutch::identity_party_id(&crate::types::Handle::to_identity_seed(&handle));
                let already = self.contacts.iter().any(|c| c.handle_hash == typed_pid);
                if already {
                    crate::log(&format!(
                        "search-result: '{}' already in contacts — skipping add",
                        handle
                    ));
                    self.search_status =
                        Some((format!("{handle} already added"), SEARCH_FOUND_COLOUR));
                    return;
                }
                let mut contact = crate::types::Contact::new(
                    peer.handle.clone(),
                    peer.handle_proof,
                    peer.device_pubkey.clone(),
                )
                .with_ip(peer.ip)
                .with_local_ip(peer.local_ip, peer.ip.port());
                // Self-contact: same identity, no key exchange needed.
                let is_self =
                    self.session.as_ref().map(|s| crate::crypto::clutch::identity_party_id(&s.identity_seed)) == Some(contact.handle_hash);
                if is_self {
                    contact.clutch_state = crate::types::ClutchState::Complete;
                }
                let contact_id = contact.id.clone();
                let their_handle_hash = contact.handle_hash;
                let their_handle_proof = contact.handle_proof;
                // Mark keygen in flight BEFORE spawning (race guard) for non-self contacts.
                if !is_self {
                    contact.clutch_keygen_in_progress = true;
                }
                crate::log(&format!(
                    "search-result: added contact '{}' (total: {})",
                    crate::fp(&contact.handle_proof).as_str(),
                    self.contacts.len() + 1
                ));
                self.contacts.push(contact);
                // Register the new contact (and its fleet, once refreshed) so the checker answers pings/offers from any of its devices, and kick CLUTCH keypair generation so the contact becomes offer-ready when it comes online.
                self.reseed_contact_pubkeys();
                self.spawn_contact_fleet_refresh(vec![their_handle_proof]);
                if !is_self {
                    let our_handle_hash = self
                        .session
                        .as_ref()
                        .map(|s| crate::crypto::clutch::identity_party_id(&s.identity_seed))
                        .unwrap_or([0u8; 32]);
                    self.spawn_clutch_keygen(contact_id, our_handle_hash, their_handle_hash);
                }
                if let Some(storage) = self.storage.as_ref() {
                    if let Some(c) = self.contacts.last() {
                        if let Err(e) = crate::storage::contacts::save_contact(c, storage) {
                            crate::log(&format!("Failed to save contact: {}", e));
                        }
                    }
                }
                self.search_status = Some((format!("added {handle}"), SEARCH_FOUND_COLOUR));
                if let Some(tb) = self.contacts_textbox.as_mut() {
                    tb.clear();
                }
                // Propagate the new friend to the rest of the fleet's devices.
                self.spawn_roster_push();
            }
            SearchResult::NotFound => {
                crate::log("search-result: handle not found on FGTW");
                self.search_status = Some(("not found".to_string(), SEARCH_FAIL_COLOUR));
                self.refocus_contacts_select_all();
            }
            SearchResult::Error(e) => {
                crate::log(&format!("search-result: error '{}'", e));
                self.search_status = Some((format!("error: {e}"), SEARCH_FAIL_COLOUR));
                self.refocus_contacts_select_all();
            }
        }
    }

    /// Apply a focus change: update `self.focused`, then walk the widget tree via `apply_focus_change` so the old + new widgets fire `set_focused(false/true)` and mark their caches dirty. Returns `true` if anything changed (caller decides whether to request a redraw — most callers do). Also drops a one-shot Android keyboard-show/hide request when focus enters or leaves a textbox; the Activity reads it via `FluorApp::wants_keyboard` after each touch and raises / dismisses the soft IME accordingly. Dismiss the standing hints (the desktop avatar prompt) and clear the transient search status. Called on any click or keystroke: hints are event-shown and interaction-cleared — never hover- or time-driven. The avatar prompt's dismissal is reset on each `Ready` entry.
    fn clear_hints(&mut self) {
        self.hints_dismissed = true;
        self.search_status = None;
        // The "Device added" confirmation follows the house rule for every transient banner: event-shown, INTERACTION-cleared — never time-based. It sits until the user's next click or keystroke acknowledges it.
        self.ready_toast = None;
    }

    fn change_focus(&mut self, new: Option<HitId>) -> bool {
        if new == self.focused {
            return false;
        }
        let old = self.focused;
        let was_textbox = self.is_textbox(old);
        let is_textbox = self.is_textbox(new);
        #[cfg(feature = "development")]
        crate::log(&format!("FOCUS: {old:?} -> {new:?} (textbox {was_textbox} -> {is_textbox})"));
        if was_textbox != is_textbox {
            self.pending_keyboard_request = Some(is_textbox);
        }
        self.focused = new;
        widget::apply_focus_change(self, old, new);
        // Restart blink so the cursor lands solid on the newly-focused textbox instead of mid-cycle dark. `start` resets the phase to the start of the visible half whether the timer was already running or not.
        self.blink_timer.start(Instant::now());
        true
    }

    // ───────── CLUTCH ceremony machinery (extracted verbatim from the retired src/ui/app.rs; only field-access seams adapted: device_keypair/event_proxy are Option here, user_identity_seed → session.identity_seed, window_dirty → the returned changed bool) ─────────

    /// OUR party id when this device participates in a SIBLING ceremony (fleet weave): device-derived, since all our devices share one handle_hash. `None` only pre-init (device_keypair unset).
    fn our_sibling_pid(&self) -> Option<[u8; 32]> {
        self.device_keypair
            .as_ref()
            .map(|kp| crate::crypto::clutch::sibling_party_id(kp.public.as_bytes()))
    }

    /// OUR party id in a ceremony with `contact`: the identity PUBKEY for friends (the value they pin at first-met — never the seed, which must not travel or be stored anywhere but our own registers), the device-derived sibling pid for fleet siblings. Every slot lookup, conversation token, ceremony id, and chain index in a ceremony must use THIS, not `session.identity_seed` directly.
    fn our_party_id(&self, contact: &crate::types::Contact) -> Option<[u8; 32]> {
        if contact.is_sibling {
            self.our_sibling_pid()
        } else {
            self.session.as_ref().map(|s| crate::crypto::clutch::identity_party_id(&s.identity_seed))
        }
    }

    // (Chains-first paths — chat/ACK receive — resolve our party id INLINE: whichever of (identity seed, sibling pid) is a participant. A &self helper can't be called there while the chains are mutably borrowed.)

    /// Recompute the shared sync-records (last-received-time per conversation) from `friendship_chains` and publish them to the checker, for message retransmit.
    pub fn update_sync_records(&mut self) {
        use crate::network::fgtw::protocol::SyncRecord;

        let mut records = Vec::new();
        for (_fid, chains) in &self.friendship_chains {
            // Get the max last_received_time across all participants This is when we last received ANY message in this conversation
            let max_time = chains
                .last_received_times()
                .iter()
                .filter_map(|t| *t)
                .fold(None, |acc: Option<i64>, t| {
                    Some(acc.map_or(t, |a| if t > a { t } else { a }))
                });

            if let Some(last_received_osc) = max_time {
                records.push(SyncRecord {
                    conversation_token: chains.conversation_token,
                    last_received_osc,
                });
            }
        }

        // Update the shared provider
        let mut provider = self.sync_records.lock().unwrap();
        *provider = records;
    }

    /// Spawn at most ONE CLUTCH keygen, for the first Pending contact that needs keypairs, but only if no keygen is already running. McEliece keygen is heavy; running several in parallel (e.g. after a multi-contact cloud merge on launch) starves the UI thread. Serializing to one-at-a-time keeps the app responsive — each completion frees the slot and `tick()` calls this again to start the next. Returns true if a keygen was spawned.
    fn spawn_next_pending_keygen(&mut self) -> bool {
        let Some(our_seed) = self.session.as_ref().map(|s| s.identity_seed) else {
            return false;
        };
        // One keygen at a time.
        if self.contacts.iter().any(|c| c.clutch_keygen_in_progress) {
            return false;
        }
        let next_idx = self.contacts.iter().position(|c| {
            c.handle_hash != our_seed
                && c.clutch_state == crate::types::ClutchState::Pending
                && c.clutch_our_keypairs.is_none()
                && !c.clutch_keygen_in_progress
        });
        if let Some(i) = next_idx {
            // Party id per contact: identity seed for friends, device-derived pid for fleet siblings.
            let Some(our_pid) = self.our_party_id(&self.contacts[i]) else {
                return false;
            };
            let c = &mut self.contacts[i];
            c.clutch_keygen_in_progress = true;
            let (cid, their_hh) = (c.id.clone(), c.handle_hash);
            crate::log("CLUTCH: spawning keygen for Pending contact (serialized, one at a time)");
            self.spawn_clutch_keygen(cid, our_pid, their_hh);
            true
        } else {
            false
        }
    }

    /// Sync this device's own avatar with FGTW, newest-wins (off-thread). Call on attest success (handle_proof fresh). Replaces the old one-way "always upload": a blind upload would clobber a NEWER FGTW copy (e.g. one this same identity set on another device) with our stale local one. `sync_avatar_bidirectional_from_seed` compares the local cache's eagle-time creation stamp to the server copy's and uploads only if we're newer, downloads + re-caches if the server is. When the server wins, the freshly-cached avatar is delivered back over `avatar_dl_tx` with an EMPTY handle so the drain installs it as `device_avatar_pixels`. No-op without keypair / proof / session / storage.
    fn spawn_avatar_sync(&self) {
        let (Some(kp), Some(session), Some(storage)) = (
            self.device_keypair.as_ref(),
            self.session.as_ref(),
            self.storage.as_ref().map(Arc::clone),
        ) else {
            return;
        };
        let Some(handle_proof) = self.handle_query.as_ref().and_then(|hq| hq.get_handle_proof())
        else {
            return;
        };
        let secret = kp.secret.clone();
        let identity_seed = session.identity_seed;
        let tx = self.avatar_dl_tx.clone();
        #[cfg(not(target_os = "android"))]
        let proxy = self.event_proxy.clone();
        std::thread::spawn(move || {
            use crate::ui::avatar::AvatarSyncResult;
            let result = crate::ui::avatar::sync_avatar_bidirectional_from_seed(
                &secret,
                &identity_seed,
                Some(&handle_proof),
                &storage,
            );
            match result {
                AvatarSyncResult::ServerNewer => {
                    // FGTW had a newer copy — it's now re-cached; load it and push to the UI.
                    crate::log("Avatar: FGTW copy newer — adopted it (startup sync)");
                    let pixels = crate::ui::avatar::load_cached_avatar_from_seed(&identity_seed, &storage)
                        .map(|(_, p)| p);
                    if pixels.is_some() {
                        let _ = tx.send(crate::ui::avatar::AvatarDownloadResult {
                            owner: None, // self
                            pixels,
                        });
                        #[cfg(not(target_os = "android"))]
                        if let Some(p) = proxy.as_ref() {
                            let _ = p.send(crate::ui::PhotonEvent::NetworkUpdate);
                        }
                    }
                }
                AvatarSyncResult::LocalNewer => {
                    crate::log("Avatar: local newer — published to FGTW (startup sync)")
                }
                AvatarSyncResult::InSync => crate::log("Avatar: already in sync with FGTW"),
                AvatarSyncResult::ServerEmpty | AvatarSyncResult::NoLocalAvatar => {
                    crate::log("Avatar: nothing to sync (startup)")
                }
                AvatarSyncResult::Error(e) => {
                    crate::log(&format!("Avatar: startup FGTW sync skipped/failed: {e}"))
                }
            }
        });
    }

    /// Kick a background download of `handle`'s avatar from FGTW (once per session per handle). The fetch + decode runs off the UI thread (FGTW round-trip + dav1d decode); the result is delivered over `avatar_dl_tx` and installed by the drain in `check_status_updates`. No-op if storage isn't ready yet or we've already started a download for this handle this session. This is the peer Send a direct P2P AvatarRequest to a MUTUAL (CLUTCH Complete) peer, once per session per peer. The peer's `AvatarResponse` arrives via the status drain and installs on the matching contact. This is the "a friend's avatar comes from the friend" path; if no response lands within the fallback window the sweep escalates to FGTW. `sent_at` (eagle-time) is recorded so the sweep can time the fallback. No-op without a status checker (the pending marker is only set once the request is actually handed off, so a checker that arrives later still triggers the request).
    fn spawn_avatar_request_p2p(
        &mut self,
        peer_addr: std::net::SocketAddr,
        recipient_pubkey: [u8; 32],
        sent_at: i64,
    ) {
        let Some(checker) = self.status_checker.as_ref() else {
            return;
        };
        self.avatar_req_pending.insert(recipient_pubkey, sent_at);
        checker.send_avatar_request(crate::network::status::AvatarRequestSend {
            peer_addr,
            recipient_pubkey,
        });
    }

    /// half of the avatar feature — the self avatar loads from the local vault; peers fetch by handle.
    fn spawn_avatar_download(&mut self, ci: usize) {
        let Some(c) = self.contacts.get(ci) else { return };
        let (hp, party_id, avatar_pin) = (c.handle_proof, c.handle_hash, c.avatar_pin);
        if avatar_pin == [0u8; 64] {
            return; // unpinned (old row / sibling) — nothing to decrypt with
        }
        if self.avatar_dl_started.contains(&hp) {
            return;
        }
        let Some(storage) = self.storage.as_ref().map(Arc::clone) else {
            return;
        };
        self.avatar_dl_started.insert(hp);
        let tx = self.avatar_dl_tx.clone();
        #[cfg(not(target_os = "android"))]
        let proxy = self.event_proxy.clone();
        std::thread::spawn(move || {
            // Cache-first, FGTW on a miss — everything keyed off the pin (docs/identity-profile.md).
            let pixels = crate::ui::avatar::download_avatar_pinned(&party_id, &avatar_pin, &storage).map(|(_, p)| p);
            let _ = tx.send(crate::ui::avatar::AvatarDownloadResult { owner: Some(hp), pixels });
            #[cfg(not(target_os = "android"))]
            if let Some(p) = proxy.as_ref() {
                let _ = p.send(crate::ui::PhotonEvent::NetworkUpdate);
            }
        });
    }

    /// Drain completed peer-avatar downloads: colour-convert the VSF-RGB pixels to the display buffer (same path as the self avatar) and install them on the matching contact, invalidating its scaled cache so the next render rebuilds + shows it. A `None` result (no avatar / fetch failed) just leaves the placeholder.
    fn drain_avatar_downloads(&mut self) {
        while let Ok(result) = self.avatar_dl_rx.try_recv() {
            let Some(vsf_rgb) = result.pixels else {
                continue;
            };
            let display = crate::ui::colour_convert::vsf_rgb_to_bt2020(&vsf_rgb);
            // `owner: None` = our OWN avatar recovered from FGTW (the local vault was cleared). Install it as the device avatar and invalidate the scaled cache so the Ready screen repaints it.
            let Some(owner_hp) = result.owner else {
                self.device_avatar_pixels = Some(display);
                self.device_avatar_scaled = None;
                self.device_avatar_scaled_diameter = 0;
                crate::log("Avatar: recovered own avatar from FGTW after local clear");
                continue;
            };
            if let Some(contact) = self
                .contacts
                .iter_mut()
                .find(|c| !c.is_sibling && c.handle_proof == owner_hp)
            {
                contact.avatar_pixels = Some(display);
                contact.avatar_scaled = None; // force rebuild at the current diameter on next render
                contact.avatar_scaled_diameter = 0;
                crate::log(&format!("Avatar: installed peer avatar for {}", crate::fp(&contact.handle_proof)));
            }
        }
    }

    /// Drain the nunc-time clock verdict. A consensus offset beyond ±`CLOCK_OFF_THRESHOLD_SECS` raises the amber "clock off" banner (`clock_off`); within threshold clears it. An `Unavailable` result (we couldn't reach consensus) is NOT an anomaly — we leave the banner as-is rather than claiming the clock is fine. This is warn-only: the system clock is never corrected.
    fn drain_clock_check(&mut self) {
        /// How far off (seconds) the system clock must be before we warn. 30s — well past ordinary NTP jitter and nunc's own confidence half-width, so the banner means a real problem.
        const CLOCK_OFF_THRESHOLD_SECS: i64 = 30;

        while let Ok(result) = self.clock_check_rx.try_recv() {
            match result {
                crate::network::ClockCheckResult::Ok {
                    offset_secs,
                    confidence_secs,
                    sources_used,
                    sources_queried,
                } => {
                    crate::log(&format!(
                        "Clock: nunc consensus offset = {}s (±{}s, {}/{} sources)",
                        offset_secs, confidence_secs, sources_used, sources_queried
                    ));
                    self.clock_off = if offset_secs.abs() > CLOCK_OFF_THRESHOLD_SECS {
                        crate::log(&format!(
                            "Clock: system clock off by {}s — raising banner (warn only, not corrected)",
                            offset_secs
                        ));
                        Some(offset_secs)
                    } else {
                        None
                    };
                }
                crate::network::ClockCheckResult::Unavailable(why) => {
                    crate::log(&format!("Clock: consensus unavailable ({why}) — banner unchanged"));
                }
            }
        }
    }

    /// Kick a one-shot fleet-inbox drain off-thread (blocking HTTPS). Pulls this identity's pending worker-observed events (bind-attempt alerts) and posts them over `inbox_check_tx`; `drain_fleet_inbox` surfaces them on a later tick. No-op without a handle_proof + device key (not yet attested).
    fn spawn_inbox_drain(&self) {
        if let (Some(hp), Some(kp), tx) = (
            self.handle_query.as_ref().and_then(|hq| hq.get_handle_proof()),
            self.device_keypair.clone(),
            self.inbox_check_tx.clone(),
        ) {
            std::thread::spawn(move || {
                match crate::network::fgtw::inbox_drain_blocking(&kp, &hp) {
                    Ok(events) if !events.is_empty() => {
                        let _ = tx.send(events);
                    }
                    Ok(_) => {}
                    Err(e) => crate::log(&format!("INBOX: drain failed: {e}")),
                }
            });
        }
    }

    /// Drain any pulled fleet-inbox events and surface them as an event-shown notice (interaction-cleared, never timed). A `bind_attempt` renders "someone tried to enrol one of your devices"; if the attempted-into handle_proof matches a known contact, name it — that's the case that distinguishes an insider or your own fumble from an anonymous thief (docs/fleet-inbox.md).
    fn drain_fleet_inbox(&mut self) {
        // Collect first so the rx borrow is released before we touch self.contacts / self.ready_toast.
        let batches: Vec<Vec<crate::network::fgtw::FleetInboxEvent>> =
            self.inbox_check_rx.try_iter().collect();
        for events in batches {
            let mut bind_attempts = 0usize;
            let mut named: Option<String> = None;
            for ev in &events {
                crate::log(&format!(
                    "INBOX: {} — device {} attempted-by {}",
                    ev.kind,
                    crate::fp(&ev.device),
                    crate::fp(&ev.attempted_by),
                ));
                if ev.kind == "bind_attempt" {
                    bind_attempts += 1;
                    if named.is_none() {
                        named = self
                            .contacts
                            .iter()
                            .find(|c| c.handle_proof == ev.attempted_by)
                            .map(|c| c.display_name());
                    }
                }
            }
            if bind_attempts > 0 {
                let who = match &named {
                    Some(name) => format!(" into {name}'s fleet"),
                    None => String::new(),
                };
                let plural = if bind_attempts == 1 { "" } else { "s" };
                self.ready_toast = Some(format!(
                    "\u{26a0} {bind_attempts} attempt{plural} to enrol your device{who}"
                ));
            }
        }
    }

    /// Recover the device's OWN avatar from FGTW after a local clear (the vault load returned nothing). Off-thread (blocking FGTW round-trip); the result comes back over avatar_dl_tx with an EMPTY handle, which drain_avatar_downloads routes into device_avatar_pixels. No-op without storage.
    fn spawn_self_avatar_recover(&self, identity_seed: [u8; 32]) {
        let Some(storage) = self.storage.as_ref().map(Arc::clone) else {
            return;
        };
        let tx = self.avatar_dl_tx.clone();
        #[cfg(not(target_os = "android"))]
        let proxy = self.event_proxy.clone();
        std::thread::spawn(move || {
            let pixels =
                crate::ui::avatar::download_avatar_from_seed(&identity_seed, &storage).map(|(_, p)| p);
            if pixels.is_some() {
                let _ = tx.send(crate::ui::avatar::AvatarDownloadResult {
                    owner: None, // self
                    pixels,
                });
                #[cfg(not(target_os = "android"))]
                if let Some(p) = proxy.as_ref() {
                    let _ = p.send(crate::ui::PhotonEvent::NetworkUpdate);
                }
            }
        });
    }

    pub fn spawn_clutch_keygen(
        &self,
        contact_id: ContactId,
        _our_handle_hash: [u8; 32],
        _their_handle_hash: [u8; 32],
    ) {
        use crate::crypto::clutch::generate_all_ephemeral_keypairs;

        let tx = self.clutch_keygen_tx.clone();
        #[cfg(not(target_os = "android"))]
        let proxy = self.event_proxy.clone();

        // Keypair generation includes McEliece460896 — very CPU-heavy (large matrix build). On resume every Pending contact re-keys at once (two contacts = two McEliece keygens in parallel), so this MUST run at Min priority or it starves the UI render thread and the window freezes until keygen finishes — the "GUI loads but you can't do anything until it syncs" symptom. Matches the Min-priority KEM-encap and ceremony-expand threads.
        let thread_body = move || {
            #[cfg(feature = "development")]
            crate::log("CLUTCH: Background keypair generation started...");
            let keypairs = generate_all_ephemeral_keypairs();
            crate::log(
                "CLUTCH: Keypairs ready (ceremony_id computed when ping provenances available)",
            );

            let _ = tx.send(ClutchKeygenResult {
                contact_id,
                keypairs,
            });

            // Wake the event loop so it processes the result
            #[cfg(not(target_os = "android"))]
            if let Some(p) = proxy.as_ref() {
                let _ = p.send(crate::ui::PhotonEvent::ClutchKeygenComplete);
            }
        };

        #[cfg(not(target_os = "redox"))]
        {
            use thread_priority::{ThreadBuilderExt, ThreadPriority};
            std::thread::Builder::new()
                .name("clutch-keygen".to_string())
                .spawn_with_priority(ThreadPriority::Min, move |_| thread_body())
                .expect("Failed to spawn CLUTCH keygen thread");
        }
        #[cfg(target_os = "redox")]
        {
            std::thread::Builder::new()
                .name("clutch-keygen".to_string())
                .spawn(thread_body)
                .expect("Failed to spawn CLUTCH keygen thread");
        }
    }

    /// Spawn background thread to perform CLUTCH KEM encapsulation. The PQ KEMs (~800ms total) are slow, so we do them off the main thread. Results are received via clutch_kem_encap_rx and processed in check_clutch_kem_encaps().
    #[allow(clippy::too_many_arguments)]
    pub fn spawn_clutch_kem_encap(
        &self,
        contact_id: ContactId,
        their_offer: crate::crypto::clutch::ClutchOfferPayload,
        ceremony_id: [u8; 32],
        conversation_token: [u8; 32],
        peer_addr: std::net::SocketAddr,
    ) {
        use crate::crypto::clutch::ClutchKemResponsePayload;

        let tx = self.clutch_kem_encap_tx.clone();
        #[cfg(not(target_os = "android"))]
        let proxy = self.event_proxy.clone();

        let thread_body = move || {
            #[cfg(feature = "development")]
            #[cfg(feature = "development")]
            crate::log("CLUTCH: Background KEM encapsulation started (low priority)...");
            let (kem_response, local_secrets) =
                ClutchKemResponsePayload::encapsulate_to_peer(&their_offer);
            #[cfg(feature = "development")]
            #[cfg(feature = "development")]
            crate::log("CLUTCH: KEM encapsulation complete");

            let _ = tx.send(ClutchKemEncapResult {
                contact_id,
                kem_response,
                local_secrets,
                ceremony_id,
                conversation_token,
                peer_addr,
            });

            // Wake the event loop so it processes the result
            #[cfg(not(target_os = "android"))]
            if let Some(p) = proxy.as_ref() {
                let _ = p.send(crate::ui::PhotonEvent::ClutchKemEncapComplete);
            }
        };

        #[cfg(not(target_os = "redox"))]
        {
            use thread_priority::{ThreadBuilderExt, ThreadPriority};
            std::thread::Builder::new()
                .name("clutch-kem-encap".to_string())
                .spawn_with_priority(ThreadPriority::Min, move |_| thread_body())
                .expect("Failed to spawn KEM encap thread");
        }
        #[cfg(target_os = "redox")]
        {
            std::thread::Builder::new()
                .name("clutch-kem-encap".to_string())
                .spawn(thread_body)
                .expect("Failed to spawn KEM encap thread");
        }
    }

    /// Spawn background thread to complete CLUTCH ceremony (avalanche_expand). The 2MB memory-hard expansion (~850ms) is slow, so we do it off the main thread. Results are received via clutch_ceremony_rx and processed in check_clutch_ceremonies().
    #[allow(clippy::too_many_arguments)]
    pub fn spawn_clutch_ceremony(
        &self,
        contact_id: ContactId,
        our_handle_hash: [u8; 32],
        their_handle_hash: [u8; 32],
        our_device_pub: [u8; 32],
        their_device_pub: [u8; 32],
        friendship_secret: [u8; 32],
        secrets: crate::crypto::clutch::ClutchSharedSecrets,
        ceremony_id: [u8; 32],
        conversation_token: [u8; 32],
        peer_addr: std::net::SocketAddr,
        their_hqc_prefix: [u8; 8],
    ) {
        use crate::crypto::clutch::clutch_complete_full;

        let tx = self.clutch_ceremony_tx.clone();
        #[cfg(not(target_os = "android"))]
        let proxy = self.event_proxy.clone();

        let thread_body = move || {
            #[cfg(feature = "development")]
            #[cfg(feature = "development")]
            crate::log("CLUTCH: Background ceremony completion started (low priority)...");

            // Phase 1: Compute eggs (moderately fast)
            let result = clutch_complete_full(
                &our_device_pub,
                &their_device_pub,
                &our_handle_hash,
                &their_handle_hash,
                &friendship_secret,
                &secrets,
            );

            // Phase 2: Expand to 2MB and derive chains (slow - avalanche_expand)
            let friendship_chains = FriendshipChains::from_clutch(
                &[our_handle_hash, their_handle_hash],
                result.eggs.as_slice(),
            );

            #[cfg(feature = "development")]
            #[cfg(feature = "development")]
            crate::log("CLUTCH: Ceremony completion finished");

            let _ = tx.send(ClutchCeremonyResult {
                contact_id,
                friendship_chains,
                eggs_proof: result.proof,
                their_handle_hash,
                ceremony_id,
                conversation_token,
                peer_addr,
                their_hqc_prefix,
            });

            // Wake the event loop so it processes the result
            #[cfg(not(target_os = "android"))]
            if let Some(p) = proxy.as_ref() {
                let _ = p.send(crate::ui::PhotonEvent::ClutchCeremonyComplete);
            }
        };

        #[cfg(not(target_os = "redox"))]
        {
            use thread_priority::{ThreadBuilderExt, ThreadPriority};
            std::thread::Builder::new()
                .name("clutch-ceremony".to_string())
                .spawn_with_priority(ThreadPriority::Min, move |_| thread_body())
                .expect("Failed to spawn ceremony thread");
        }
        #[cfg(target_os = "redox")]
        {
            std::thread::Builder::new()
                .name("clutch-ceremony".to_string())
                .spawn(thread_body)
                .expect("Failed to spawn ceremony thread");
        }
    }

    /// Process background CLUTCH key generation results.
    ///
    /// Slot-based design: keypairs stored once, slots filled as messages arrive. Ceremony completes when all slots have offer + both KEM secret directions.
    pub fn check_clutch_keygens(&mut self) -> bool {
        use crate::crypto::clutch::{
            derive_conversation_token, ClutchKemSharedSecrets, ClutchOfferPayload,
        };
        use crate::network::status::ClutchOfferRequest;
        use crate::types::CeremonyId;

        let mut changed = false;
        let mut ceremony_completions: Vec<usize> = Vec::new();
        // Deferred KEM encapsulation spawn (to avoid borrow conflict)
        let mut kem_encap_spawn: Option<(
            ContactId,
            ClutchOfferPayload,
            [u8; 32],
            [u8; 32],
            std::net::SocketAddr,
        )> = None;

        // Our party id for CLUTCH: the identity pubkey (public; contacts pin it — never the seed).
        let our_handle_hash = match self.session.as_ref().map(|s| crate::crypto::clutch::identity_party_id(&s.identity_seed)) {
            Some(h) => h,
            None => return changed,
        };
        let device_pubkey = *self
            .device_keypair
            .as_ref()
            .expect("device_keypair set in init")
            .public
            .as_bytes();
        let device_secret = *self
            .device_keypair
            .as_ref()
            .expect("device_keypair set in init")
            .secret
            .as_bytes();

        while let Ok(result) = self.clutch_keygen_rx.try_recv() {
            let result_id_hex = hex::encode(&result.contact_id.as_bytes()[..4]);
            crate::log(&format!(
                "CLUTCH: Processing keygen result for contact_id {}...",
                result_id_hex,
            ));

            let mut found = false;
            for (idx, contact) in self.contacts.iter_mut().enumerate() {
                if contact.id == result.contact_id {
                    found = true;

                    // Party-id seam: shadow the hoisted seed with THIS contact's "our" id — the device-derived sibling pid for fleet siblings (same handle ⇒ the seed would collide), the identity seed for friends. Every slot lookup / token / ceremony-id below then keys correctly with no further edits.
                    let our_handle_hash = if contact.is_sibling {
                        crate::crypto::clutch::sibling_party_id(&device_pubkey)
                    } else {
                        our_handle_hash
                    };

                    // Clear the in-progress flag now that keygen is complete
                    contact.clutch_keygen_in_progress = false;

                    // Store keypairs (ceremony_id computed on-demand when provenances available)
                    contact.clutch_our_keypairs = Some(result.keypairs);
                    changed = true;

                    // Persist keypairs to disk immediately (crash recovery)
                    if let (Some(ref keypairs), Some(storage)) =
                        (&contact.clutch_our_keypairs, self.storage.as_ref())
                    {
                        if let Err(e) = crate::storage::contacts::save_clutch_keypairs(
                            keypairs,
                            &contact.handle_hash,
                            storage,
                        ) {
                            crate::log(&format!(
                                "CLUTCH: Failed to save keypairs for {}: {}",
                                crate::fp(&contact.handle_proof), e
                            ));
                        }
                    }

                    // Initialize slots if not done yet (sorted by handle_hash)
                    if contact.clutch_slots.is_empty() {
                        contact.init_clutch_slots(our_handle_hash);
                    }

                    // Check if their slot has an offer (received before keygen completed)
                    let their_slot_has_offer = contact
                        .get_slot(&contact.handle_hash)
                        .map(|s| s.offer.is_some())
                        .unwrap_or(false);

                    // Store local offer in local slot
                    if let Some(ref keypairs) = contact.clutch_our_keypairs {
                        let our_offer = ClutchOfferPayload::from_keypairs(keypairs);
                        if let Some(local_slot) = contact.get_slot_mut(&our_handle_hash) {
                            local_slot.offer = Some(our_offer);
                            crate::log(&format!(
                                "CLUTCH: Stored local offer in local slot for {}",
                                crate::fp(&contact.handle_proof)
                            ));
                        } else {
                            crate::log(&format!(
                                "CLUTCH: Could not find local slot for {} - handle_hash mismatch?",
                                crate::fp(&contact.handle_proof)
                            ));
                        }
                    }

                    // Send our offer if not already sent (don't wait for ceremony_id - that comes later)
                    if !contact.clutch_offer_sent {
                        if let Some(ip) = contact.ip {
                            if let Some(ref keypairs) = contact.clutch_our_keypairs {
                                use crate::network::fgtw::protocol::build_clutch_offer_vsf;

                                let offer = ClutchOfferPayload::from_keypairs(keypairs);
                                let conv_token = derive_conversation_token(&[
                                    our_handle_hash,
                                    contact.handle_hash,
                                ]);

                                // Build VSF and capture our offer_provenance
                                match build_clutch_offer_vsf(
                                    &conv_token,
                                    &offer,
                                    &device_pubkey,
                                    &device_secret,
                                ) {
                                    Ok((vsf_bytes, our_offer_provenance)) => {
                                        // Store our offer provenance (for ceremony_id derivation)
                                        if !contact
                                            .offer_provenances
                                            .contains(&our_offer_provenance)
                                        {
                                            contact.offer_provenances.push(our_offer_provenance);
                                        }

                                        // Persist provenance immediately
                                        if let Some(storage) = self.storage.as_ref() {
                                            if let Err(e) =
                                                crate::storage::contacts::save_clutch_slots(
                                                    &contact.clutch_slots,
                                                    &contact.offer_provenances,
                                                    contact.ceremony_id,
                                                    &contact.handle_hash,
                                                    storage,
                                                )
                                            {
                                                crate::log(&format!(
                                                    "Failed to persist CLUTCH provenance: {}",
                                                    e
                                                ));
                                            }
                                        }

                                        if let Some(ref checker) = self.status_checker {
                                            let (primary, alt) =
                                                contact.race_addrs().unwrap_or((ip, None));
                                            checker.send_offer(ClutchOfferRequest {
                                                peer_addr: primary,
                                                alt_addr: alt,
                                                vsf_bytes,
                                            });
                                            contact.clutch_offer_sent = true;
                                            crate::log(&format!(
                                                "CLUTCH: Sent offer to {} (prov={}...)",
                                                crate::fp(&contact.handle_proof),
                                                hex::encode(&our_offer_provenance[..4])
                                            ));
                                        }
                                    }
                                    Err(e) => {
                                        crate::log(&format!(
                                            "CLUTCH: Failed to build offer VSF for {}: {}",
                                            crate::fp(&contact.handle_proof), e
                                        ));
                                    }
                                }
                            }
                        }
                    }

                    // Compute ceremony_id if we have enough offer provenances (2 for DM)
                    let required_provenances = 2;
                    if contact.ceremony_id.is_none()
                        && contact.offer_provenances.len() >= required_provenances
                    {
                        let ceremony_id = *CeremonyId::derive(
                            &[our_handle_hash, contact.handle_hash],
                            &contact.offer_provenances,
                        )
                        .as_bytes();
                        contact.ceremony_id = Some(ceremony_id);
                        crate::log(&format!(
                            "CLUTCH: Computed ceremony_id for {} from {} offer provenances",
                            crate::fp(&contact.handle_proof),
                            contact.offer_provenances.len()
                        ));
                    }

                    // Send KEM response if we have ceremony_id and their offer
                    if their_slot_has_offer {
                        let already_sent_kem = contact
                            .get_slot(&our_handle_hash)
                            .map(|s| s.kem_secrets_to_them.is_some())
                            .unwrap_or(false);

                        if !already_sent_kem && !contact.clutch_kem_encap_in_progress {
                            if let Some(ceremony_id) = contact.ceremony_id {
                                if let Some(ip) = contact.ip {
                                    let conv_token = derive_conversation_token(&[
                                        our_handle_hash,
                                        contact.handle_hash,
                                    ]);
                                    let remote_offer = contact
                                        .get_slot(&contact.handle_hash)
                                        .and_then(|s| s.offer.clone());

                                    if let Some(remote_offer) = remote_offer {
                                        // Defer spawn for KEM encapsulation (to avoid borrow conflict) (PQ crypto is slow ~800ms, would block UI/network)
                                        contact.clutch_kem_encap_in_progress = true;
                                        kem_encap_spawn = Some((
                                            contact.id.clone(),
                                            remote_offer,
                                            ceremony_id,
                                            conv_token,
                                            ip,
                                        ));
                                        crate::log(&format!(
                                            "CLUTCH: Will spawn KEM encapsulation for {} (post-keygen)",
                                            crate::fp(&contact.handle_proof)
                                        ));
                                    }
                                }
                            } else {
                                crate::log(&format!(
                                    "CLUTCH: Keypairs ready for {} - need ceremony_id for KEM response (have {} offer provenances)",
                                    crate::fp(&contact.handle_proof),
                                    contact.offer_provenances.len()
                                ));
                            }
                        }
                    }

                    // Process any pending KEM response that arrived before keygen completed. Also compute ceremony_id here if provenances are ready — the KEM may have arrived in the network thread between when we added our provenance and when the main loop got here to run the ceremony_id derivation above.
                    if contact.clutch_pending_kem.is_some() {
                        if contact.ceremony_id.is_none() && contact.offer_provenances.len() >= 2 {
                            let ceremony_id = *CeremonyId::derive(
                                &[our_handle_hash, contact.handle_hash],
                                &contact.offer_provenances,
                            )
                            .as_bytes();
                            contact.ceremony_id = Some(ceremony_id);
                            crate::log(&format!(
                                "CLUTCH: Computed ceremony_id for {} while draining queued KEM",
                                crate::fp(&contact.handle_proof)
                            ));
                        }
                    }

                    if let Some(pending_kem) = contact.clutch_pending_kem.take() {
                        crate::log(&format!(
                            "CLUTCH: Processing queued KEM response from {}",
                            crate::fp(&contact.handle_proof)
                        ));
                        // Decapsulate remote KEM (remote encapsulated to local pubkeys)
                        if let Some(ref local_keys) = contact.clutch_our_keypairs {
                            let remote_secrets = ClutchKemSharedSecrets::decapsulate_from_peer(
                                &pending_kem,
                                local_keys,
                            );
                            // Store remote secrets (from decapsulating FROM remote) in remote slot
                            let remote_hash = contact.handle_hash;
                            if let Some(remote_slot) = contact.get_slot_mut(&remote_hash) {
                                remote_slot.kem_secrets_from_them = Some(remote_secrets);
                                crate::log(&format!(
                                    "CLUTCH: Decapsulated queued KEM from {} - stored in slot",
                                    crate::fp(&contact.handle_proof)
                                ));
                            }

                            // If we haven't sent our own KEM encap yet, do it now. This covers the case where their KEM arrived before we had ceremony_id, so the normal encap-trigger was skipped.
                            let already_sent_kem = contact
                                .get_slot(&our_handle_hash)
                                .map(|s| s.kem_secrets_to_them.is_some())
                                .unwrap_or(false);
                            if !already_sent_kem
                                && !contact.clutch_kem_encap_in_progress
                                && kem_encap_spawn.is_none()
                            {
                                if let Some(ceremony_id) = contact.ceremony_id {
                                    if let Some(ip) = contact.ip {
                                        let conv_token = derive_conversation_token(&[
                                            our_handle_hash,
                                            contact.handle_hash,
                                        ]);
                                        let remote_offer = contact
                                            .get_slot(&contact.handle_hash)
                                            .and_then(|s| s.offer.clone());
                                        if let Some(remote_offer) = remote_offer {
                                            contact.clutch_kem_encap_in_progress = true;
                                            kem_encap_spawn = Some((
                                                contact.id.clone(),
                                                remote_offer,
                                                ceremony_id,
                                                conv_token,
                                                ip,
                                            ));
                                            crate::log(&format!(
                                                "CLUTCH: Spawning KEM encap for {} after draining queued KEM",
                                                crate::fp(&contact.handle_proof)
                                            ));
                                        }
                                    }
                                }
                            }

                            // Persist slot state after processing pending KEM
                            if let Some(storage) = self.storage.as_ref() {
                                if let Err(e) = crate::storage::contacts::save_clutch_slots(
                                    &contact.clutch_slots,
                                    &contact.offer_provenances,
                                    contact.ceremony_id,
                                    &contact.handle_hash,
                                    storage,
                                ) {
                                    crate::log(&format!(
                                        "CLUTCH: Failed to save slots for {}: {}",
                                        crate::fp(&contact.handle_proof), e
                                    ));
                                }
                            }
                        }
                    }

                    // Check if ceremony can complete
                    if contact.all_slots_complete() {
                        crate::log(&format!(
                            "CLUTCH: All slots complete for {} after keygen - triggering ceremony completion",
                            crate::fp(&contact.handle_proof)
                        ));
                        ceremony_completions.push(idx);
                    }

                    break;
                }
            }

            if !found {
                crate::log(&format!(
                    "CLUTCH: Keygen result contact_id {}... not found in contacts!",
                    result_id_hex
                ));
            }
        }

        // Spawn deferred KEM encapsulation after releasing contacts borrow
        if let Some((contact_id, offer, ceremony_id, conv_token, peer_addr)) = kem_encap_spawn {
            self.spawn_clutch_kem_encap(contact_id, offer, ceremony_id, conv_token, peer_addr);
        }

        // Process deferred ceremony completions (after releasing contacts borrow)
        for idx in ceremony_completions {
            self.complete_clutch_ceremony_by_idx(idx);
            changed = true;
        }

        if changed {}
        changed
    }

    /// Process background CLUTCH KEM encapsulation results. When KEM encap completes, store the secrets and send the KEM response.
    pub fn check_clutch_kem_encaps(&mut self) -> bool {
        use crate::network::status::ClutchKemResponseRequest;

        let mut changed = false;
        let mut ceremony_completions: Vec<usize> = Vec::new();
        let our_handle_hash = match self.session.as_ref().map(|s| crate::crypto::clutch::identity_party_id(&s.identity_seed)) {
            Some(h) => h,
            None => return changed,
        };
        let device_pubkey = *self
            .device_keypair
            .as_ref()
            .expect("device_keypair set in init")
            .public
            .as_bytes();
        let device_secret = *self
            .device_keypair
            .as_ref()
            .expect("device_keypair set in init")
            .secret
            .as_bytes();

        while let Ok(result) = self.clutch_kem_encap_rx.try_recv() {
            let result_id_hex = hex::encode(&result.contact_id.as_bytes()[..4]);
            crate::log(&format!(
                "CLUTCH: Processing KEM encap result for contact_id {}...",
                result_id_hex,
            ));

            // Find the contact and update state
            let mut found_idx = None;
            for (idx, contact) in self.contacts.iter_mut().enumerate() {
                if contact.id == result.contact_id {
                    found_idx = Some(idx);
                    contact.clutch_kem_encap_in_progress = false;

                    // Party-id seam: sibling ceremonies key our slot on the device-derived pid, not the (shared) identity seed.
                    let our_handle_hash = if contact.is_sibling {
                        crate::crypto::clutch::sibling_party_id(&device_pubkey)
                    } else {
                        our_handle_hash
                    };

                    // Store local encapsulation secrets in local slot (local contribution) Also store the KEM response payload for re-send
                    if let Some(slot) = contact.get_slot_mut(&our_handle_hash) {
                        slot.kem_secrets_to_them = Some(result.local_secrets);
                        slot.kem_response_for_resend = Some(result.kem_response.clone());
                    }

                    // Persist slot state before sending KEM
                    if let Some(storage) = self.storage.as_ref() {
                        if let Err(e) = crate::storage::contacts::save_clutch_slots(
                            &contact.clutch_slots,
                            &contact.offer_provenances,
                            contact.ceremony_id,
                            &contact.handle_hash,
                            storage,
                        ) {
                            crate::log(&format!(
                                "CLUTCH: Failed to save slots for {}: {}",
                                crate::fp(&contact.handle_proof), e
                            ));
                        }
                    }

                    // Send the KEM response
                    if let Some(ref checker) = self.status_checker {
                        let (primary, alt) =
                            contact.race_addrs().unwrap_or((result.peer_addr, None));
                        checker.send_kem_response(ClutchKemResponseRequest {
                            peer_addr: primary,
                            alt_addr: alt,
                            conversation_token: result.conversation_token,
                            ceremony_id: result.ceremony_id,
                            payload: result.kem_response,
                            device_pubkey,
                            device_secret,
                        });
                        crate::log(&format!("CLUTCH: Sent KEM response to {}", crate::fp(&contact.handle_proof)));
                    }

                    // Check if all slots are complete after storing our KEM encap secrets
                    if contact.all_slots_complete() {
                        crate::log(&format!(
                            "CLUTCH: All slots complete for {} after KEM encap - triggering ceremony",
                            crate::fp(&contact.handle_proof)
                        ));
                        ceremony_completions.push(idx);
                    }

                    changed = true;
                    break;
                }
            }

            if found_idx.is_none() {
                crate::log(&format!(
                    "CLUTCH: KEM encap result contact_id {}... not found in contacts!",
                    result_id_hex
                ));
            }
        }

        // Process deferred ceremony completions (after releasing contacts borrow)
        for idx in ceremony_completions {
            self.complete_clutch_ceremony_by_idx(idx);
            changed = true;
        }

        if changed {}
        changed
    }

    /// Process background CLUTCH ceremony completion results. When ceremony completes, store the friendship chains and send proof.
    pub fn check_clutch_ceremonies(&mut self) -> bool {
        use crate::crypto::clutch::ClutchCompletePayload;
        use crate::network::status::ClutchCompleteRequest;
        use crate::types::ClutchState;

        let mut changed = false;
        let _our_handle_hash = match self.session.as_ref().map(|s| crate::crypto::clutch::identity_party_id(&s.identity_seed)) {
            Some(h) => h,
            None => return changed,
        };
        let device_pubkey = *self
            .device_keypair
            .as_ref()
            .expect("device_keypair set in init")
            .public
            .as_bytes();
        let device_secret = *self
            .device_keypair
            .as_ref()
            .expect("device_keypair set in init")
            .secret
            .as_bytes();

        while let Ok(result) = self.clutch_ceremony_rx.try_recv() {
            let result_id_hex = hex::encode(&result.contact_id.as_bytes()[..4]);
            crate::log(&format!(
                "CLUTCH: Processing ceremony result for contact_id {}...",
                result_id_hex,
            ));

            let friendship_id = *result.friendship_chains.id();

            // Save chains to disk first
            if let Some(storage) = self.storage.as_ref() {
                crate::log(&format!(
                    "CLUTCH: Saving friendship chains to disk (fid={}...)",
                    hex::encode(&friendship_id.as_bytes()[..8])
                ));
                if let Err(e) = crate::storage::friendship::save_friendship_chains(
                    &result.friendship_chains,
                    storage,
                ) {
                    crate::log(&format!("Failed to save friendship chains: {}", e));
                } else {
                    #[cfg(feature = "development")]
                    #[cfg(feature = "development")]
                    crate::log("CLUTCH: Friendship chains saved successfully");
                }
            } else {
                #[cfg(feature = "development")]
                #[cfg(feature = "development")]
                crate::log("CLUTCH: Cannot save chains - no storage!");
            }

            // Cache chains in memory
            if let Some(entry) = self
                .friendship_chains
                .iter_mut()
                .find(|(id, _)| *id == friendship_id)
            {
                // Supersede: scrub the OLD chains' history key before the re-keyed chains replace them (the fresh chains carry their own newly derived key).
                entry.1.zeroize_history_key();
                entry.1 = result.friendship_chains;
            } else {
                self.friendship_chains
                    .push((friendship_id, result.friendship_chains));
            }

            // Update sync records for new friendship
            self.update_sync_records();

            // Find the contact and update state
            if let Some(contact) = self.contacts.iter_mut().find(|c| c.id == result.contact_id) {
                let contact_handle = contact.display_name();
                contact.clutch_ceremony_in_progress = false;
                contact.friendship_id = Some(friendship_id);

                crate::log(&format!(
                    "CLUTCH: Eggs computed with {}! (proof: {}...)",
                    contact_handle,
                    hex::encode(&result.eggs_proof[..8])
                ));

                // Store our proof for later verification
                contact.clutch_our_eggs_proof = Some(result.eggs_proof);
                // Budget a handful of proof retransmits — the proof is a single unreliable UDP packet, so ping_contacts re-sends it until this drains, guaranteeing the peer gets it even on a lossy or freshly-changed path.
                contact.clutch_proof_resends_left = 5;

                // Check if we already received their proof (fast party case)
                let their_early_proof = contact.clutch_their_eggs_proof;

                // Send ClutchComplete proof to peer
                if let Some(ref checker) = self.status_checker {
                    let payload = ClutchCompletePayload {
                        eggs_proof: result.eggs_proof,
                    };

                    let (primary, alt) = contact.race_addrs().unwrap_or((result.peer_addr, None));
                    checker.send_complete_proof(ClutchCompleteRequest {
                        peer_addr: primary,
                        alt_addr: alt,
                        conversation_token: result.conversation_token,
                        ceremony_id: result.ceremony_id,
                        payload,
                        device_pubkey,
                        device_secret,
                    });

                    crate::log(&format!(
                        "CLUTCH: Sent proof to {} via status checker",
                        contact_handle
                    ));
                }

                // Check if they already sent us their proof
                if let Some(their_proof) = their_early_proof {
                    if their_proof == result.eggs_proof {
                        // SUCCESS! Both parties computed same eggs
                        crate::log(&format!(
                            "CLUTCH: Early proof verified with {}! ✓ proof={}...",
                            contact_handle,
                            hex::encode(&result.eggs_proof[..8])
                        ));
                        contact.clutch_state = ClutchState::Complete;
                        contact.clutch_completed_at = Some(std::time::Instant::now()); // arm the post-completion re-key cooldown (before the ~1s-later weave)
                        // A FRESH ceremony just completed = a brand-new chain — any prior weave seal is void. Reset the double-toggle state so the hidden probe REFIRES for this chain. Without this, a peer that client-reset and re-CLUTCHed hits a deadlock: our persisted chain_woven=true (load latches all probe flags true) suppresses our probe, the reset peer waits forever for it ("weaving the chain"), and we dismiss their re-sent proofs as woven-duplicates. First-ceremony case: flags already false, no-op.
                        contact.chain_woven = false;
                        contact.probe_sent = false;
                        contact.their_probe_seen = false;
                        contact.chain_advanced_by_ack = false;
                        // Store their HQC pub prefix to detect stale offers after restart
                        contact.completed_their_hqc_prefix = Some(result.their_hqc_prefix);
                        // We're Complete, but the peer may not have our proof yet — we got theirs first, and our single send (just above) might have dropped. Keep the proof and the resend budget so ping_contacts keeps delivering it for a few more cycles; that's exactly what stops the peer from hanging in AwaitingProof.
                        contact.clutch_their_eggs_proof = None;
                    } else {
                        // CRYPTOGRAPHIC FAILURE!
                        let our_hex = hex::encode(&result.eggs_proof);
                        let their_hex = hex::encode(&their_proof);
                        crate::log(&format!(
                            "CLUTCH: ⚠ PROOF MISMATCH with {}! ours={}... theirs={}...",
                            contact_handle,
                            &our_hex[..16],
                            &their_hex[..16]
                        ));
                        // Reset to Pending to allow re-keying
                        contact.clutch_state = ClutchState::Pending;
                        contact.clutch_our_eggs_proof = None;
                        contact.clutch_their_eggs_proof = None;
                    }
                } else {
                    // Set state to AwaitingProof - wait for their proof
                    contact.clutch_state = ClutchState::AwaitingProof;
                    crate::log(&format!(
                        "CLUTCH: Awaiting proof from {} (we sent ours)",
                        contact_handle
                    ));
                }

                // Save contact to persist friendship_id and clutch_state
                if let Some(storage) = self.storage.as_ref() {
                    if let Err(e) = crate::storage::contacts::save_contact(contact, storage) {
                        crate::log(&format!("Failed to save contact after CLUTCH: {}", e));
                    } else {
                        #[cfg(feature = "development")]
                        #[cfg(feature = "development")]
                        crate::log(&format!("CLUTCH: Saved {} state to disk", contact_handle));
                    }

                    // Delete slots file - ceremony is complete, slots no longer needed
                    if let Err(e) = crate::storage::contacts::delete_clutch_slots(
                        &contact.handle_hash,
                        storage,
                    ) {
                        crate::log(&format!("Failed to delete CLUTCH slots: {}", e));
                    }
                }
                changed = true;
            } else {
                crate::log(&format!(
                    "CLUTCH: Ceremony result contact_id {}... not found in contacts!",
                    result_id_hex
                ));
            }

            // If the early-proof branch just took this contact to Complete, fire the hidden chain-weave probe (once). Done after the mutable-borrow block above releases.
            if let Some(idx) = self
                .contacts
                .iter()
                .position(|c| c.id == result.contact_id)
            {
                self.maybe_send_chain_probe(idx);
            }
        }

        if changed {}
        changed
    }

    /// Spawn background CLUTCH ceremony completion when all slots are filled. Extracts data from contact and spawns background thread for heavy crypto.
    ///
    /// Takes contact index to avoid borrow conflicts in the event loop. Derives OUR party id internally (identity seed for friends, device-derived pid for fleet siblings) — callers used to pass a hoisted seed, which was wrong for sibling ceremonies.
    fn complete_clutch_ceremony_by_idx(&mut self, contact_idx: usize) {
        use crate::crypto::clutch::{derive_conversation_token, ClutchSharedSecrets};

        let our_handle_hash = match self
            .contacts
            .get(contact_idx)
            .and_then(|c| self.our_party_id(c))
        {
            Some(pid) => pid,
            None => {
                crate::log("CLUTCH: No party id available for ceremony completion");
                return;
            }
        };

        // Extract data from contact to avoid borrow issues
        let contact = match self.contacts.get_mut(contact_idx) {
            Some(c) => c,
            None => {
                #[cfg(feature = "development")]
                #[cfg(feature = "development")]
                crate::log("CLUTCH: Invalid contact index");
                return;
            }
        };

        // Check if ceremony already in progress
        if contact.clutch_ceremony_in_progress {
            crate::log(&format!(
                "CLUTCH: Ceremony already in progress for {}",
                crate::fp(&contact.handle_proof)
            ));
            return;
        }

        // Get their slot (the other party)
        let their_handle_hash = contact.handle_hash;
        let contact_is_sibling = contact.is_sibling;
        let contact_hp = contact.handle_proof;
        let contact_id = contact.id.clone();
        let contact_handle = contact.display_name();
        let their_device_pub = *contact.public_identity.as_bytes();

        // Extract all needed data from slots (cloning to release borrow)
        let our_slot = match contact.get_slot(&our_handle_hash) {
            Some(s) => s,
            None => {
                #[cfg(feature = "development")]
                #[cfg(feature = "development")]
                crate::log("CLUTCH: No slot for local party");
                return;
            }
        };
        let their_slot = match contact.get_slot(&their_handle_hash) {
            Some(s) => s,
            None => {
                #[cfg(feature = "development")]
                #[cfg(feature = "development")]
                crate::log("CLUTCH: No slot for remote party");
                return;
            }
        };

        // Local encapsulation secrets from local slot
        let our_kem_secrets = match &our_slot.kem_secrets_to_them {
            Some(s) => s.clone(),
            None => {
                #[cfg(feature = "development")]
                #[cfg(feature = "development")]
                crate::log("CLUTCH: No kem_secrets_to_them in local slot");
                return;
            }
        };
        // Remote encapsulation secrets from remote slot
        let their_kem_secrets = match &their_slot.kem_secrets_from_them {
            Some(s) => s.clone(),
            None => {
                #[cfg(feature = "development")]
                #[cfg(feature = "development")]
                crate::log("CLUTCH: No kem_secrets_from_them in remote slot");
                return;
            }
        };

        // Get their HQC prefix for stale detection
        let their_hqc_prefix: [u8; 8] = their_slot
            .offer
            .as_ref()
            .map(|o| o.hqc256_public[..8].try_into().unwrap_or_default())
            .unwrap_or_default();

        // Get peer address and ceremony_id
        let peer_addr = match contact.ip {
            Some(ip) => ip,
            None => {
                #[cfg(feature = "development")]
                #[cfg(feature = "development")]
                crate::log(&format!("CLUTCH: No IP for {}", crate::fp(&contact.handle_proof)));
                return;
            }
        };
        let ceremony_id = match contact.ceremony_id {
            Some(c) => c,
            None => {
                #[cfg(feature = "development")]
                #[cfg(feature = "development")]
                crate::log(&format!("CLUTCH: No ceremony_id for {}", crate::fp(&contact.handle_proof)));
                return;
            }
        };

        let conversation_token = derive_conversation_token(&[our_handle_hash, their_handle_hash]);

        crate::log(&format!(
            "CLUTCH: Spawning ceremony completion for {}",
            contact_handle
        ));

        // Determine low/high ordering by handle hash
        let we_are_low = our_handle_hash < their_handle_hash;

        // Build shared secrets struct with proper ordering
        let secrets = if we_are_low {
            ClutchSharedSecrets {
                low_x25519: our_kem_secrets.x25519,
                high_x25519: their_kem_secrets.x25519,
                low_p384: our_kem_secrets.p384.clone(),
                high_p384: their_kem_secrets.p384.clone(),
                low_secp256k1: our_kem_secrets.secp256k1.clone(),
                high_secp256k1: their_kem_secrets.secp256k1.clone(),
                low_p256: our_kem_secrets.p256.clone(),
                high_p256: their_kem_secrets.p256.clone(),
                low_frodo: our_kem_secrets.frodo.clone(),
                high_frodo: their_kem_secrets.frodo.clone(),
                low_ntru: our_kem_secrets.ntru.clone(),
                high_ntru: their_kem_secrets.ntru.clone(),
                low_mceliece: our_kem_secrets.mceliece.clone(),
                high_mceliece: their_kem_secrets.mceliece.clone(),
                low_hqc: our_kem_secrets.hqc.clone(),
                high_hqc: their_kem_secrets.hqc.clone(),
            }
        } else {
            ClutchSharedSecrets {
                low_x25519: their_kem_secrets.x25519,
                high_x25519: our_kem_secrets.x25519,
                low_p384: their_kem_secrets.p384.clone(),
                high_p384: our_kem_secrets.p384.clone(),
                low_secp256k1: their_kem_secrets.secp256k1.clone(),
                high_secp256k1: our_kem_secrets.secp256k1.clone(),
                low_p256: their_kem_secrets.p256.clone(),
                high_p256: our_kem_secrets.p256.clone(),
                low_frodo: their_kem_secrets.frodo.clone(),
                high_frodo: our_kem_secrets.frodo.clone(),
                low_ntru: their_kem_secrets.ntru.clone(),
                high_ntru: our_kem_secrets.ntru.clone(),
                low_mceliece: their_kem_secrets.mceliece.clone(),
                high_mceliece: our_kem_secrets.mceliece.clone(),
                low_hqc: their_kem_secrets.hqc.clone(),
                high_hqc: our_kem_secrets.hqc.clone(),
            }
        };

        // Mark ceremony in progress and spawn background thread
        contact.clutch_ceremony_in_progress = true;

        let our_device_pub = *self
            .device_keypair
            .as_ref()
            .expect("device_keypair set in init")
            .public
            .as_bytes();
        // The SECRET identity binding for the eggs (docs/identity-profile.md): friends = static identity DH against their pinned identity pubkey (the party id); siblings share the identity seed itself (their party ids aren't curve points). A pin that isn't a valid point is an old-format contact row — flag-day: fail loudly, re-add the friend.
        let Some(our_seed) = self.session.as_ref().map(|s| s.identity_seed) else {
            crate::log("CLUTCH: no session — cannot derive friendship secret");
            return;
        };
        let friendship_secret = if contact_is_sibling {
            our_seed
        } else {
            match crate::crypto::clutch::identity_friendship_secret(&our_seed, &their_handle_hash) {
                Some(fs) => fs,
                None => {
                    crate::log(&format!(
                        "CLUTCH: pinned identity for {} is not a curve point (old-format contact row) — re-add this friend",
                        crate::fp(&contact_hp)
                    ));
                    return;
                }
            }
        };
        self.spawn_clutch_ceremony(
            contact_id,
            our_handle_hash,
            their_handle_hash,
            our_device_pub,
            their_device_pub,
            friendship_secret,
            secrets,
            ceremony_id,
            conversation_token,
            peer_addr,
            their_hqc_prefix,
        );
    }

    /// Cross-reference the FGTW peer list into existing contacts, updating each matched contact's public address (`ip`) and same-LAN address (`local_ip`/`local_port`). Matched by handle_proof + device_pubkey so the right device's record updates the right contact. Only IPv4 LAN addresses are stored (the hairpin case the `local_ip` field is typed for); a v6-only peer just refreshes the WAN address. The send path races both (see [`crate::types::Contact::race_addrs`]).
    fn refresh_contact_addrs_from_peers(&mut self, peers: &[crate::network::fgtw::PeerRecord]) {
        // Addresses whose transfers must be cancelled because they went stale (collected here so the checker borrow stays out of the contact-iter loop).
        let mut stale_addrs: Vec<std::net::SocketAddr> = Vec::new();
        for peer in peers {
            for contact in self.contacts.iter_mut() {
                if contact.handle_proof == peer.handle_proof
                    && contact.public_identity.as_bytes() == peer.device_pubkey.as_bytes()
                {
                    let old_ip = contact.ip;
                    let old_local = contact.local_ip;
                    contact.ip = Some(peer.ip);
                    if let Some(std::net::IpAddr::V4(v4)) = peer.local_ip {
                        contact.local_ip = Some(v4);
                        contact.local_port = Some(peer.ip.port());
                        crate::log(&format!(
                            "UI: refreshed {} addrs from FGTW — WAN {} / LAN {}:{}",
                            crate::fp(&contact.handle_proof),
                            peer.ip,
                            v4,
                            peer.ip.port()
                        ));
                    }
                    // If the address actually moved while a CLUTCH offer was already sent, that offer is in flight to a now-dead address (the "No route to host" retries we kept hammering). Cancel the stale transfer and reset clutch_offer_sent so the contact's next online pong re-sends the offer to the fresh address, with the LAN path now raced alongside. Without this the one-shot flag blocks re-send and the ceremony stalls forever on the dead path.
                    let addr_changed = old_ip != contact.ip || old_local != contact.local_ip;
                    if addr_changed
                        && contact.clutch_offer_sent
                        && contact.clutch_state == crate::types::ClutchState::Pending
                    {
                        if let Some(stale) = old_ip {
                            stale_addrs.push(stale);
                        }
                        contact.clutch_offer_sent = false;
                        crate::log(&format!(
                            "CLUTCH: {} address changed — cancelling stale offer transfer, will re-send to fresh address",
                            crate::fp(&contact.handle_proof)
                        ));
                    }
                    break;
                }
            }
        }
        if let Some(checker) = self.status_checker.as_ref() {
            for addr in stale_addrs {
                checker.clear_pt_sends(addr);
            }
        }
    }

    /// True if `handle_hash` (a party id) is our own identity — i.e. this contact is the user's self-contact (notes to self / future multi-device sync). A self-contact shares our single identity, so there is no peer to exchange keys with: CLUTCH must be forced Complete and keygen/offer/ceremony skipped entirely. Without this a self-contact runs a pointless CLUTCH loop against its own device and never settles. Party ids are identity PUBKEYS now, so the comparison derives ours.
    fn is_self_contact(&self, handle_hash: &[u8; 32]) -> bool {
        self.session
            .as_ref()
            .is_some_and(|s| crate::crypto::clutch::identity_party_id(&s.identity_seed) == *handle_hash)
    }

    /// Force every self-contact in the list to CLUTCH-Complete and clear any in-flight CLUTCH work. Applied after contacts load on resume and after cloud/FGTW merges, since those paths build contacts as Pending by default. Returns true if any contact changed.
    fn settle_self_contacts(&mut self) -> bool {
        let Some(our_pid) = self.session.as_ref().map(|s| crate::crypto::clutch::identity_party_id(&s.identity_seed)) else {
            return false;
        };
        let mut changed = false;
        for contact in self.contacts.iter_mut() {
            if contact.handle_hash == our_pid
                && contact.clutch_state != crate::types::ClutchState::Complete
            {
                contact.clutch_state = crate::types::ClutchState::Complete;
                contact.clutch_keygen_in_progress = false;
                changed = true;
                crate::log(&format!(
                    "CLUTCH: self-contact '{}' auto-completed (no key exchange with self)",
                    crate::fp(&contact.handle_proof)
                ));
                if let Some(storage) = self.storage.as_ref() {
                    let _ = crate::storage::contacts::save_contact(contact, storage);
                }
            }
        }
        changed
    }

    /// Current presence-sweep interval, chosen by how long since the user last interacted. Active (5s) while engaged → idle (1min) → deep-idle (15min). `now` is the tick's clock. Jittered to 50–100% of the tier so a roomful of devices doesn't ping their contacts in lockstep (a synchronised presence sweep is a self-inflicted DDoS). Presence timing is soft, so the fuzziness is free.
    fn presence_ping_interval(&self, now: Instant) -> std::time::Duration {
        let idle = self
            .last_interaction
            .map_or(std::time::Duration::ZERO, |last| now.duration_since(last));
        let tier = if idle < PRESENCE_IDLE_NEAR {
            PRESENCE_PING_ACTIVE
        } else if idle < PRESENCE_IDLE_FAR {
            PRESENCE_PING_IDLE
        } else {
            PRESENCE_PING_DEEP
        };
        crate::jitter_dur(tier)
    }

    /// Ping all contacts that have IP addresses (call periodically)
    fn ping_contacts(&mut self) {
        use crate::network::traverse::session::PATH_TTL;
        // Cycles an online contact may go punched-but-unvalidated before we treat it as direct-unreachable.
        const PUNCH_UNREACHABLE_THRESHOLD: u8 = 3;

        // Expire stale validated paths (no keepalive ack within TTL → the NAT mapping is likely dead): clear so `race_addrs` falls back to LAN/public and this cycle re-punches. Track the symmetric↔symmetric case: an online contact we keep punching but never validate is direct-unreachable — bump the graceful-failure counter (the hook M2's relay reads) and log the state once at the threshold.
        for c in self.contacts.iter_mut() {
            if let Some((_, at)) = c.validated_path {
                if at.elapsed() >= PATH_TTL {
                    c.validated_path = None;
                }
            }
            if c.is_online && c.validated_path.is_none() {
                c.punch_unvalidated_cycles = c.punch_unvalidated_cycles.saturating_add(1);
                if c.punch_unvalidated_cycles == PUNCH_UNREACHABLE_THRESHOLD {
                    crate::log(&format!(
                        "TRAVERSE: {} online but no direct path after {} cycles — pending relay (M2)",
                        crate::fp(&c.handle_proof).as_str(),
                        PUNCH_UNREACHABLE_THRESHOLD
                    ));
                }
            }
        }

        let Some(checker) = self.status_checker.as_ref() else {
            return;
        };
        let mut pinged = 0;
        for contact in &self.contacts {
            // Ping the LAN address AND the public address (when both are known) rather than preferring LAN and never falling back. Two devices that once shared a LAN have a stored `local_ip`; the moment one moves to a different network (e.g. phone → cellular) that LAN address is stale and unreachable, but the public address in the registry is correct — pinging only LAN strands them offline forever. Each ping is tracked by a unique provenance hash and a single pong clears the whole per-contact failure counter (see status.rs StatusPong handler), so the unreachable address simply times out harmlessly while the reachable one keeps the contact online. On-LAN the LAN ping wins (no router hairpin / AP isolation); off-LAN the public ping wins.
            let lan_addr = match (contact.local_ip, contact.local_port) {
                (Some(ip), Some(port)) => {
                    Some(std::net::SocketAddr::new(std::net::IpAddr::V4(ip), port))
                }
                _ => None,
            };
            // Punch candidates, fired alongside the first ping (stale paths were cleared above):
            // - validated → keepalive: probe just the validated remote to keep its NAT mapping warm; its ack refreshes liveness so the path never expires while the contact stays reachable.
            // - unvalidated → (re)punch: probe all the peer's addresses, best-first, so the first to round-trip wins.
            let mut punch: Vec<std::net::SocketAddr> = match contact.validated_path {
                Some((remote, _)) => vec![remote],
                None => crate::network::traverse::gather::gather_peer_candidates(contact)
                    .sorted()
                    .into_iter()
                    .map(|c| c.addr)
                    .collect(),
            };
            let mut sent = false;
            if let Some(addr) = lan_addr {
                checker.ping(
                    addr,
                    contact.public_identity.clone(),
                    std::mem::take(&mut punch),
                );
                sent = true;
            }
            // Public address — skip only if it's identical to the LAN address we already pinged.
            if let Some(public) = contact.ip {
                if Some(public) != lan_addr {
                    checker.ping(
                        public,
                        contact.public_identity.clone(),
                        std::mem::take(&mut punch),
                    );
                    sent = true;
                }
            }
            if sent {
                pinged += 1;
            }
        }
        if pinged > 0 {
            crate::log(&format!("Status: pinged {pinged} contact(s)"));
        }
        // LAN broadcast for same-network local-IP discovery (hairpin-NAT workaround).
        if let (Some(session), Some(hq)) = (self.session.as_ref(), self.handle_query.as_ref()) {
            checker.send_lan_broadcast(session.handle_proof, hq.port());
        }

        // Recovery for a side stranded in AwaitingProof: while the peer is ONLINE and we still hold our computed proof, keep the resend budget topped up so we keep re-sending our proof every few cycles. The peer — already Complete — now treats our repeated proof as an implicit re-request and re-sends its ClutchComplete (see the Complete-state duplicate handler). So a ClutchComplete dropped during the original ceremony (e.g. before the v4-mapped-v6 send fix, or any single UDP loss) self-heals once both sides are online, instead of leaving us AwaitingProof forever with the peer already Complete. Bounded per-cycle so an offline peer doesn't spin; it only tops up when we actually have the peer online with a proof to send.
        for contact in self.contacts.iter_mut() {
            if contact.is_online
                && contact.clutch_state == crate::types::ClutchState::AwaitingProof
                && contact.clutch_our_eggs_proof.is_some()
                && contact.ceremony_id.is_some()
                && contact.clutch_proof_resends_left == 0
            {
                contact.clutch_proof_resends_left = 1; // one re-send this cycle; re-armed next ping while still stuck
            }
        }

        // Retransmit the ClutchComplete proof for any contact with budget left. The proof is a lone unreliable UDP packet, so a single drop (or a send to a since-refreshed address) would strand the peer in AwaitingProof. Re-sending it for a few ping cycles converges both sides regardless of which completed first or which packet was lost. Self-terminates as the budget drains; a peer already Complete re-arms its own resend on the duplicate.
        self.retransmit_pending_clutch_proofs();
    }

    /// Re-send the ClutchComplete proof to every contact whose retransmit budget (`clutch_proof_resends_left`) is non-zero, decrementing each. See [`ping_contacts`] for why this exists. Clears our held proof once the budget reaches zero so it isn't kept forever.
    fn retransmit_pending_clutch_proofs(&mut self) {
        use crate::crypto::clutch::{derive_conversation_token, ClutchCompletePayload};
        use crate::network::status::ClutchCompleteRequest;

        let Some(our_handle_hash) = self.session.as_ref().map(|s| s.identity_seed) else {
            return;
        };
        let Some(kp) = self.device_keypair.as_ref() else {
            return;
        };
        let device_pubkey = *kp.public.as_bytes();
        let device_secret = *kp.secret.as_bytes();
        let Some(checker) = self.status_checker.as_ref() else {
            return;
        };

        for contact in self.contacts.iter_mut() {
            if contact.clutch_proof_resends_left == 0 {
                continue;
            }
            let (Some(eggs_proof), Some(ceremony_id)) =
                (contact.clutch_our_eggs_proof, contact.ceremony_id)
            else {
                // Nothing to resend (proof/ceremony cleared) — drop the budget.
                contact.clutch_proof_resends_left = 0;
                continue;
            };
            let Some((primary, alt)) = contact.race_addrs() else {
                continue;
            };
            // Party-id seam: sibling tokens derive from the device pids, not the shared seed.
            let our_pid = if contact.is_sibling {
                crate::crypto::clutch::sibling_party_id(&device_pubkey)
            } else {
                our_handle_hash
            };
            let conv_token = derive_conversation_token(&[our_pid, contact.handle_hash]);
            checker.send_complete_proof(ClutchCompleteRequest {
                peer_addr: primary,
                alt_addr: alt,
                conversation_token: conv_token,
                ceremony_id,
                payload: ClutchCompletePayload { eggs_proof },
                device_pubkey,
                device_secret,
            });
            contact.clutch_proof_resends_left -= 1;
            crate::log(&format!(
                "CLUTCH: Retransmitted proof to {} ({} resends left)",
                crate::fp(&contact.handle_proof), contact.clutch_proof_resends_left
            ));
            // Budget exhausted — stop holding the proof.
            if contact.clutch_proof_resends_left == 0
                && contact.clutch_state == crate::types::ClutchState::Complete
            {
                contact.clutch_our_eggs_proof = None;
            }
        }
    }

    /// Reliability sweep (every tick): resend any unacked outgoing message whose backoff deadline has passed, with exponential backoff, until an ACK clears it or it exhausts its attempts. This is the per-message retry the protocol was missing — without it, a single dropped message OR a single dropped ACK desyncs the chain permanently (the sender advances on ACK, so a lost ACK freezes its chain while the receiver's has moved on → every later message decrypts as garbage). Resending is safe: the receiver dedupes by eagle_time and its ACK is deterministic, so a redelivered message just yields a free re-ACK. Uses the same LAN-preferring `race_addrs()` as the live send.
    fn retransmit_due_messages(&mut self) {
        let now_osc = vsf::eagle_time_oscillations();

        // Snapshot (friendship_id → primary + alt addr + recipient pubkey) from contacts so we don't hold a contacts borrow across the mutable chains sweep. Only Complete contacts with a known address. Carry BOTH addresses — a retransmit that only re-hit the primary would keep blackholing an off-LAN peer for the whole retry budget (observed: 8 attempts all to a dead LAN IPv4).
        let routes: Vec<(crate::types::FriendshipId, std::net::SocketAddr, Option<std::net::SocketAddr>, [u8; 32])> = self
            .contacts
            .iter()
            .filter_map(|c| {
                let fid = c.friendship_id?;
                let (primary, alt) = c.race_addrs()?;
                Some((fid, primary, alt, *c.public_identity.as_bytes()))
            })
            .collect();
        if routes.is_empty() {
            return;
        }

        let Some(checker) = self.status_checker.as_ref() else {
            return;
        };

        for (fid, peer_addr, alt_addr, recipient_pubkey) in routes {
            let Some((_, chains)) = self.friendship_chains.iter_mut().find(|(id, _)| *id == fid)
            else {
                continue;
            };
            let conversation_token = chains.conversation_token;
            for (eagle_time, prev_msg_hp, ciphertext, attempts, exhausted) in
                chains.collect_due_retransmits(now_osc)
            {
                checker.send_message(crate::network::status::MessageRequest {
                    peer_addr,
                    alt_addr,
                    recipient_pubkey,
                    conversation_token,
                    prev_msg_hp,
                    ciphertext,
                    eagle_time,
                });
                if exhausted {
                    crate::log(&format!(
                        "CHAT: retransmit GAVE UP on msg eagle_time {} after {} attempts (undelivered)",
                        eagle_time, attempts
                    ));
                } else {
                    crate::log(&format!(
                        "CHAT: retransmit msg eagle_time {} (attempt {})",
                        eagle_time, attempts
                    ));
                }
            }
        }
    }

    /// History-recovery driver (every tick): for each contact mid-backfill, expire a lost in-flight request and fire the next page request when due. Newest-first cursor pagination — `urgent` (weave-seal kickoff / scrollback) jumps the trickle interval; otherwise pages are rate-limited to one per HIST_TRICKLE_OSC so a 10-year backfill hums along in the background without competing with live traffic. Requests are idempotent (rid-correlated, merge dedups), so an expiry + re-request after a lost page is always safe.
    fn drive_history_recovery(&mut self) {
        const HIST_TRICKLE_OSC: i64 = 2 * crate::OSC_PER_SEC; // one page per ~2s in background
        const HIST_INFLIGHT_TIMEOUT_OSC: i64 = 15 * crate::OSC_PER_SEC; // lost request/page

        let now_osc = vsf::eagle_time_oscillations();

        // Snapshot device keys once (frame building signs on this thread).
        let Some(kp) = self.device_keypair.as_ref() else {
            return;
        };
        let device_pubkey = *kp.public.as_bytes();
        let device_secret = *kp.secret.as_bytes();

        // Candidate pass (read-only): eligible contacts + their conversation token/history key route.
        let candidates: Vec<(usize, [u8; 32], std::net::SocketAddr, Option<std::net::SocketAddr>, [u8; 32])> = self
            .contacts
            .iter()
            .enumerate()
            .filter_map(|(idx, c)| {
                let rec = c.history_recovery.as_ref()?;
                if rec.complete || !c.is_online || !c.chain_woven {
                    return None;
                }
                let fid = c.friendship_id?;
                let (_, chains) = self.friendship_chains.iter().find(|(id, _)| *id == fid)?;
                chains.history_key()?; // no key (pre-feature chains) = recovery unavailable
                let (primary, alt) = c.race_addrs()?;
                Some((
                    idx,
                    chains.conversation_token,
                    primary,
                    alt,
                    *c.public_identity.as_bytes(),
                ))
            })
            .collect();
        if candidates.is_empty() {
            return;
        }
        let Some(checker) = self.status_checker.as_ref() else {
            return;
        };

        for (idx, token, primary, alt, recipient_pubkey) in candidates {
            let Some(rec) = self.contacts[idx].history_recovery.as_mut() else {
                continue;
            };
            // Expire a lost in-flight request so the walk resumes.
            if let Some((_, sent_osc, _)) = rec.in_flight {
                if now_osc.saturating_sub(sent_osc) > HIST_INFLIGHT_TIMEOUT_OSC {
                    crate::log("HISTORY: in-flight request expired — re-requesting");
                    rec.in_flight = None;
                } else {
                    continue; // one request at a time per conversation
                }
            }
            if !rec.urgent && now_osc < rec.next_request_osc {
                continue; // trickle interval not up yet
            }

            let rid: [u8; 32] = rand::random();
            let before = rec.oldest_recovered_osc;
            match crate::network::fgtw::protocol::build_history_request_vsf(
                &token,
                before,
                crate::network::history_pages::MAX_PAGE_ROWS as u32,
                &rid,
                &device_pubkey,
                &device_secret,
            ) {
                Ok(vsf_bytes) => {
                    rec.in_flight = Some((rid, now_osc, before));
                    rec.next_request_osc = now_osc + HIST_TRICKLE_OSC;
                    rec.urgent = false;
                    crate::log(&format!(
                        "HISTORY: requesting page before {} from {}",
                        if before == i64::MAX { "HEAD".to_string() } else { before.to_string() },
                        primary
                    ));
                    checker.send_history(crate::network::status::HistorySendRequest {
                        peer_addr: primary,
                        alt_addr: alt,
                        recipient_pubkey,
                        vsf_bytes,
                    });
                }
                Err(e) => crate::log(&format!("HISTORY: request build failed: {e}")),
            }
        }
    }

    /// Blind-ops driver (every tick, beside `drive_history_recovery`): keeps the friend-blinded private-identity-secret machinery converged. Per eligible friend (online, woven, mutual, not a sibling): expire a lost in-flight op (~15s), then fire the ONE op this contact needs — a `blind_get` while S is unknown and this friend hasn't answered `found=0` yet (probe and reconstitute are the SAME op), or a `blind_put` while S exists and this friend hasn't disk-confirmed our deposit. One op in flight per contact; responses land in the `BlindFrameReceived` arm. Steady state (S live, deposits confirmed everywhere) is a pure no-op.
    fn drive_blind_ops(&mut self) {
        use crate::crypto::blind::PrivateS;
        const BLIND_INFLIGHT_TIMEOUT_OSC: i64 = 15 * crate::OSC_PER_SEC;
        let now_osc = vsf::eagle_time_oscillations();

        let Some(our_seed) = self.session.as_ref().map(|s| s.identity_seed) else {
            return;
        };
        let Some(kp) = self.device_keypair.as_ref() else {
            return;
        };
        let device_pubkey = *kp.public.as_bytes();
        let device_secret = *kp.secret.as_bytes();
        let s_known = !matches!(self.private_s, PrivateS::None);
        // A stack copy for blob building inside the contacts borrow; lives only this call.
        let s_copy: Option<zeroize::Zeroizing<[u8; 32]>> =
            self.private_s.secret().map(|s| zeroize::Zeroizing::new(**s));
        let Some(checker) = self.status_checker.as_ref() else {
            return;
        };

        for contact in self.contacts.iter_mut() {
            if !contact.is_online || !contact.chain_woven || !contact.is_mutual() {
                continue;
            }
            // Expire a lost op so the machinery retries.
            if let Some((_, sent_osc, _)) = contact.blind_in_flight {
                if now_osc.saturating_sub(sent_osc) > BLIND_INFLIGHT_TIMEOUT_OSC {
                    crate::log("BLIND: in-flight op expired — retrying");
                    contact.blind_in_flight = None;
                } else {
                    continue; // one op at a time per contact
                }
            }
            // Which op does this contact need? Siblings are PROBE-only — an S-less device pulls S over the sealed sibling channel (blind_get → AEAD-sealed srv); deposits go to friends only (a sibling holding our OTP blind would be pointless — it serves S itself when it has one).
            let want_probe = !s_known && !contact.blind_probe_missed;
            let want_put = s_known && !contact.blind_deposited && !contact.is_sibling;
            if !want_probe && !want_put {
                continue;
            }
            let Some((primary, alt)) = contact.race_addrs() else {
                continue;
            };
            // Party-id seam: sibling tokens derive from the device pids (fleet weave), friend tokens from the seeds.
            let our_pid = if contact.is_sibling {
                crate::crypto::clutch::sibling_party_id(&device_pubkey)
            } else {
                our_seed
            };
            let token =
                crate::crypto::clutch::derive_conversation_token(&[our_pid, contact.handle_hash]);
            let rid: [u8; 32] = rand::random();
            let built = if want_probe {
                crate::network::fgtw::protocol::build_blind_get_vsf(
                    &token,
                    &rid,
                    &device_pubkey,
                    &device_secret,
                )
            } else {
                let Some(s) = s_copy.as_ref() else { continue };
                let pad = crate::crypto::blind::derive_blind_pad(&device_secret, &contact.handle_hash);
                let blob = crate::crypto::blind::make_blind_blob(s, &pad);
                crate::network::fgtw::protocol::build_blind_put_vsf(
                    &token,
                    &rid,
                    &blob,
                    &device_pubkey,
                    &device_secret,
                )
            };
            match built {
                Ok(vsf_bytes) => {
                    contact.blind_in_flight = Some((rid, now_osc, want_probe));
                    crate::log(&format!(
                        "BLIND: {} {}",
                        if want_probe {
                            "probing for our deposit at"
                        } else {
                            "depositing our blind with"
                        },
                        crate::fp(&contact.handle_proof)
                    ));
                    checker.send_history(crate::network::status::HistorySendRequest {
                        peer_addr: primary,
                        alt_addr: alt,
                        recipient_pubkey: *contact.public_identity.as_bytes(),
                        vsf_bytes,
                    });
                }
                Err(e) => crate::log(&format!("BLIND: frame build failed: {e}")),
            }
        }
    }

    /// Probe-before-generate verdict, called when a `blind_srv` miss lands while S is None. Generates a fresh S ONLY when no probe is still in flight and EVERY eligible online+woven friend has answered `found=0` — i.e. the network reachable right now provably holds no deposit for this device. A single hit anywhere reconstitutes instead (handled at the srv arrival). This asymmetry is the whole point: a `[]n`-reset device must RECOVER its S, never mint a second one while a deposit is reachable.
    fn maybe_generate_s(&mut self) {
        use crate::crypto::blind::PrivateS;
        if !matches!(self.private_s, PrivateS::None) {
            return;
        }
        let mut any_eligible = false;
        for c in &self.contacts {
            // Siblings count: a woven sibling holding S serves it (a hit); one without answers found=0 like a friend, so the all-missed rule still converges. (Two FRESH siblings with zero friends can both generate — the deterministic lower-s_id tie-break at srv-adoption converges them after.)
            if !c.is_online || !c.chain_woven || !c.is_mutual() {
                continue;
            }
            any_eligible = true;
            if c.blind_in_flight.map_or(false, |(_, _, is_get)| is_get) {
                return; // a probe is still out — its answer decides
            }
            if !c.blind_probe_missed {
                return; // not asked/answered yet — the driver will probe it
            }
        }
        if !any_eligible {
            return; // nobody reachable to attest a miss — keep waiting
        }
        let mut s = zeroize::Zeroizing::new([0u8; 32]);
        rand::RngCore::fill_bytes(&mut rand::thread_rng(), s.as_mut());
        crate::log("S: generated (provisional) — no deposit found at any reachable friend");
        self.private_s = PrivateS::Provisional(s);
        for c in self.contacts.iter_mut() {
            c.blind_deposited = false;
        }
        // Deposit immediately (the answering friend is online right now); Provisional→Live flips on the first blind_ack.
        self.drive_blind_ops();
    }

    /// Ping a single contact (on conversation-enter) so its presence refreshes promptly. Same LAN-IPv4-preferring address selection as `ping_contacts`.
    fn ping_contact(&mut self, idx: usize) {
        let Some(checker) = self.status_checker.as_ref() else {
            return;
        };
        let Some(contact) = self.contacts.get(idx) else {
            return;
        };
        let addr = match (contact.local_ip, contact.local_port) {
            (Some(ip), Some(port)) => {
                Some(std::net::SocketAddr::new(std::net::IpAddr::V4(ip), port))
            }
            _ => contact.ip,
        };
        if let Some(ip) = addr {
            let punch: Vec<std::net::SocketAddr> = match contact.validated_path {
                Some((remote, _)) => vec![remote], // keepalive the validated path
                None => crate::network::traverse::gather::gather_peer_candidates(contact)
                    .sorted()
                    .into_iter()
                    .map(|c| c.addr)
                    .collect(),
            };
            checker.ping(ip, contact.public_identity.clone(), punch);
        }
    }

    /// Drain `StatusUpdate`s from the checker and apply them to contacts. v1 (presence checkpoint) handles only `Online`: match the pong's pubkey to a contact, update its `ip` from the source address, and flip `is_online`. Returns true if any contact changed (→ redraw the list ring). The CLUTCH arms (offer/KEM/complete) land in the follow-up commit. Chat/ack/PT arms are intentionally ignored (messaging not yet ported).
    pub fn check_status_updates(&mut self) -> bool {
        use crate::crypto::clutch;
        use crate::network::status::StatusUpdate;
        // NOTE: ClutchRequest and ClutchRequestType imports removed - legacy v1 CLUTCH no longer used
        use crate::types::ClutchState;

        // Peer avatars: install any completed downloads, then kick a fetch (once/session/handle) for any contact still without one. Cache-first + dedup'd by avatar_dl_started, so this is cheap to run every tick — it spawns at most one thread per peer per session.
        self.drain_avatar_downloads();

        // Clock sanity: drain any completed nunc verdict, then (if the wall clock has grossly jumped since the last baseline) spawn a fresh re-check. Both are cheap — the jump check is two clock reads and a subtraction; a re-check only spawns on an actual jump.
        self.drain_clock_check();
        // Surface any fleet-inbox alerts pulled since the last tick (bind attempts on our devices).
        self.drain_fleet_inbox();
        if self.online && self.clock_jump.check_and_reset() {
            crate::log("Clock: wall clock jumped — re-verifying via nunc consensus");
            #[cfg(not(target_os = "android"))]
            if let Some(proxy) = self.event_proxy.clone() {
                crate::network::spawn_clock_check(self.clock_check_tx.clone(), Some(proxy));
            }
            #[cfg(target_os = "android")]
            crate::network::spawn_clock_check(self.clock_check_tx.clone(), None);
        }
        // Avatar acquisition policy (once/session/contact). A MUTUAL contact (CLUTCH Complete, which is impossible unless both added each other) gets a direct P2P AvatarRequest — a friend's avatar comes from the friend. We fall back to FGTW for that friend ONLY if no AvatarResponse has installed an avatar within AVATAR_P2P_FALLBACK_OSC (the friend is offline or avatar-less). A non-mutual contact never gets a direct request — it only ever pulls the public FGTW copy. Never blocks; each branch is dedup'd so the per-tick sweep is cheap.
        /// ~3 seconds (oscillations) before a mutual peer's silent P2P request falls back to FGTW.
        const AVATAR_P2P_FALLBACK_OSC: i64 = 3 * crate::OSC_PER_SEC;
        enum AvatarPlan {
            // Cached locally (the common launch case): just kick the local-first background load, which reads the vault and never touches the network. Keeps the P2P/FGTW escalation from firing a redundant request every launch when we already hold the avatar. `spawn_avatar_download`'s worker is cache-first, so this IS the "look local first" path — the caller states intent, the fetch layer serves it from the vault.
            LocalCached {
                ci: usize,
            },
            // Complete + addressable, NOT cached: try the peer directly; FGTW only after the timeout.
            P2pThenFgtw {
                peer_addr: std::net::SocketAddr,
                recipient_pubkey: [u8; 32],
                ci: usize,
            },
            // Non-mutual (or Complete-but-unaddressable) and not cached: public FGTW copy only.
            FgtwOnly {
                ci: usize,
            },
        }
        // Steady state: every contact already has an avatar → skip the sweep entirely (no timestamp read, no allocation) since this runs every tick. Only do the work when something's missing.
        if self.contacts.iter().any(|c| c.avatar_pixels.is_none()) {
        let now = vsf::eagle_time_oscillations();
        let plans: Vec<AvatarPlan> = self
            .contacts
            .iter()
            .enumerate()
            .filter(|(_, c)| c.avatar_pixels.is_none())
            .map(|(ci, c)| {
                // Local vault first — a cheap `read_addr` (encrypted blob, no decode). If we have it, the network never runs. This is what stops the every-launch redundant P2P request: the friend's avatar is already cached, so we don't re-ask them for it.
                let cached = self
                    .storage
                    .as_ref()
                    .is_some_and(|s| crate::ui::avatar::has_cached_avatar_from_seed(&c.handle_hash, s));
                if cached {
                    return AvatarPlan::LocalCached { ci };
                }
                if c.is_mutual() {
                    if let Some((addr, _alt)) = c.race_addrs() {
                        return AvatarPlan::P2pThenFgtw {
                            peer_addr: addr,
                            recipient_pubkey: *c.public_identity.as_bytes(),
                            ci,
                        };
                    }
                }
                AvatarPlan::FgtwOnly { ci }
            })
            .collect();
        for plan in plans {
            match plan {
                // Cache-first background load; never hits the network for an already-cached avatar.
                AvatarPlan::LocalCached { ci } => self.spawn_avatar_download(ci),
                AvatarPlan::FgtwOnly { ci } => self.spawn_avatar_download(ci),
                AvatarPlan::P2pThenFgtw {
                    peer_addr,
                    recipient_pubkey,
                    ci,
                } => match self.avatar_req_pending.get(&recipient_pubkey).copied() {
                    // Never asked this peer — send the P2P request now, record when.
                    None => {
                        self.spawn_avatar_request_p2p(peer_addr, recipient_pubkey, now);
                    }
                    // Asked, but the peer hasn't answered within the window — fall back to FGTW (dedup'd by avatar_dl_started, so this fires at most once per peer).
                    Some(sent_at) if now.saturating_sub(sent_at) > AVATAR_P2P_FALLBACK_OSC => {
                        self.spawn_avatar_download(ci);
                    }
                    // Asked recently — still waiting on the peer; do nothing this tick.
                    Some(_) => {}
                },
            }
        }
        } // end avatar sweep (skipped when every contact already has an avatar)

        let checker = match &self.status_checker {
            Some(c) => c,
            None => return false,
        };

        // Our party id for CLUTCH: the identity PUBKEY (the value contacts pin at first-met). It rides CLUTCH offers for contact matching — public by design; the secret identity binding is the friendship-secret egg, never this id. (Was the raw identity seed, which also parked our seed in every peer's contact row — docs/identity-profile.md.)
        let our_handle_hash = match self.session.as_ref().map(|s| crate::crypto::clutch::identity_party_id(&s.identity_seed)) {
            Some(h) => h,
            None => return false, // Can't do CLUTCH without our party id
        };

        // Alias kept for the keygen-spawn call below (same value — the party id).
        let our_identity_seed = our_handle_hash;

        // Our device pubkey, hoisted for the sibling party-id shadow inside the contacts loop (a &self method call there would conflict with the &mut contacts borrow).
        let our_device_pubkey = match self.device_keypair.as_ref() {
            Some(kp) => *kp.public.as_bytes(),
            None => return false,
        };

        let mut changed = false;
        let mut ceremony_completions: Vec<usize> = Vec::new(); // Contact indices to complete after loop
        let mut lan_ping_indices: Vec<usize> = Vec::new(); // Contact indices to ping immediately on new LAN discovery
                                                           // Collect pending message retransmit requests (friendship_id, ip, handle, device_pubkey, last_received_ef6) to process after loop last_received_ef6 from pong tells us what they already have - only retransmit newer
        let mut retransmit_requests: Vec<(
            crate::types::FriendshipId,
            std::net::SocketAddr,
            Option<std::net::SocketAddr>, // alt address to race (public/LAN counterpart)
            String,
            [u8; 32], // Recipient device pubkey for relay fallback
            Option<i64>,
        )> = Vec::new();
        // Flag to update sync records after the loop (when borrows are released)
        let mut need_sync_update = false;
        // Deferred probe-before-generate verdict (maybe_generate_s needs &mut self; the loop holds the checker borrow) — set when a blind_srv miss lands while S is None.
        let mut check_s_genesis = false;

        // Chain-weave probe deferrals — the loop holds an immutable `checker` borrow of `self`, so the `&mut self` seal/probe helpers can't run inline; collect contact indices and process them after the loop, like ceremony_completions / lan_ping_indices already do.
        let mut chain_seal_indices: Vec<usize> = Vec::new(); // seal_chain_if_ready after loop
        let mut chain_probe_indices: Vec<usize> = Vec::new(); // maybe_send_chain_probe after loop

        // The braid / strict-ordering replay queue: when a successful decrypt fills a hash-chain gap, the now-contiguous buffered messages are pushed here as synthetic ChatMessage updates and drained BEFORE the next channel item, so a buffered N+1 is reprocessed immediately after N (and can itself cascade to N+2). FIFO front-drain.
        let mut replay_queue: std::collections::VecDeque<StatusUpdate> =
            std::collections::VecDeque::new();

        loop {
            let update = match replay_queue.pop_front() {
                Some(u) => u,
                None => match checker.try_recv() {
                    Some(u) => u,
                    None => break,
                },
            };
            match update {
                StatusUpdate::Online {
                    peer_pubkey,
                    is_online,
                    peer_addr,
                    sync_records,
                } => {
                    // Stall recovery (runs EVERY ping that carries sync records, not just the offline→online edge): each record is the peer's contiguous tip (last_received_osc = "I have everything in order up to here"). Re-arm any pending message of ours that's newer than that tip AND has exhausted its retransmit attempts — so a gap-filler the sender already gave up on gets resent, and a receiver stuck behind a permanently-lost message un-sticks. collect_due_retransmits (the tick path) then actually sends the revived messages.
                    let now_osc = vsf::eagle_time_oscillations();
                    for record in &sync_records {
                        if let Some((_, chains)) = self
                            .friendship_chains
                            .iter_mut()
                            .find(|(_, c)| c.conversation_token == record.conversation_token)
                        {
                            let n = chains.rearm_pending_after(record.last_received_osc, now_osc);
                            if n > 0 {
                                crate::log(&format!(
                                    "CHAT: re-armed {} given-up pending msg(s) past peer tip {} (stall recovery)",
                                    n, record.last_received_osc
                                ));
                            }
                        }
                    }
                    // Find matching contact and update status
                    for contact in &mut self.contacts {
                        if contact.knows_device(&peer_pubkey.key) {
                            // Party-id seam: sibling offers/tokens key on the device-derived pid, not the (shared) identity seed.
                            let our_handle_hash = if contact.is_sibling {
                                crate::crypto::clutch::sibling_party_id(&our_device_pubkey)
                            } else {
                                our_handle_hash
                            };
                            // Note: ceremony_id is now computed from offer_provenances, not ping provenances. Offer provenances are collected when ClutchOfferReceived messages arrive.

                            // Update the address from the ping/pong source — but keep PUBLIC and LAN addresses in their own fields. We now ping both the LAN (`local_ip`) and the public (`ip`) address every cycle, so pongs arrive from BOTH; blindly writing every `src_addr` into `contact.ip` made it flap between the public IPv6 and the `192.168.x` LAN address each cycle, corrupting `race_addrs` (which reads `ip` as the public path). So: a pong from a PUBLIC source refreshes `contact.ip` (the peer's WAN address may have changed); a pong from a PRIVATE/LAN source refreshes `contact.local_ip`/`local_port` instead, never clobbering the public address.
                            if let Some(addr) = peer_addr {
                                if is_private_addr(&addr.ip()) {
                                    if let std::net::IpAddr::V4(v4) = addr.ip() {
                                        if contact.local_ip != Some(v4)
                                            || contact.local_port != Some(addr.port())
                                        {
                                            contact.local_ip = Some(v4);
                                            contact.local_port = Some(addr.port());
                                        }
                                    }
                                } else if contact.ip != Some(addr) {
                                    crate::log(&format!(
                                        "Status: Updated {} public IP from ping/pong: {:?} -> {}",
                                        crate::fp(&contact.handle_proof), contact.ip, addr
                                    ));
                                    contact.ip = Some(addr);
                                }
                            }

                            // True only on the offline→online EDGE, not every online ping/chat. Retransmit-of-pending (below) keys off this — without the edge gate it re-fired on every received chat (now that a chat marks the sender online), resending all pending messages in a storm.
                            let came_online = is_online && !contact.is_online;
                            if contact.is_online != is_online {
                                contact.is_online = is_online;
                                changed = true;
                                crate::log(&format!(
                                    "Status: {} is now {}",
                                    crate::fp(&contact.handle_proof),
                                    if is_online { "ONLINE" } else { "offline" }
                                ));
                            }

                            // Deadlock recovery: a queued KEM with no offer means their offer never arrived (lost in transit — their KEM landed but the larger offer transfer didn't). We can't derive ceremony_id or complete our slot without it, and there's no timeout on the queue, so this hangs Pending forever (only a restart, which forces a fresh offer exchange, recovers it). Self-heal: when we see a still-queued KEM on a pong AND we've already sent our offer (so we're genuinely stuck, not mid-initial-exchange), reset clutch_offer_sent so the offer-send block below re-fires this pong — our re-sent offer prompts them to re-send theirs (the same path a restart takes). Pong cadence rate-limits the re-request to one per pong. Only the "their offer was lost" case is recoverable here; if a peer genuinely never sends an offer, nothing we do fixes it.
                            if is_online
                                && contact.clutch_state == ClutchState::Pending
                                && contact.clutch_pending_kem.is_some()
                                && contact.clutch_offer_sent
                            {
                                crate::log(&format!(
                                    "CLUTCH: still waiting for offer from {} (their KEM is queued) — re-requesting by re-sending our offer",
                                    crate::fp(&contact.handle_proof)
                                ));
                                contact.clutch_offer_sent = false;
                            }

                            // Send full offer when contact comes online and keys are ready Keys are pre-generated in background when contact is added Slot-based: send if Pending, have keypairs, haven't sent yet Note: ceremony_id is now computed AFTER offers are exchanged
                            if is_online
                                && contact.clutch_state == ClutchState::Pending
                                && !contact.clutch_offer_sent
                            {
                                if let Some(ref keypairs) = contact.clutch_our_keypairs {
                                    use crate::network::fgtw::protocol::build_clutch_offer_vsf;
                                    use crate::network::status::ClutchOfferRequest;

                                    let payload =
                                        clutch::ClutchOfferPayload::from_keypairs(keypairs);

                                    if let Some(ip) = contact.ip {
                                        // Build VSF and capture our offer_provenance
                                        let conversation_token =
                                            clutch::derive_conversation_token(&[
                                                our_handle_hash,
                                                contact.handle_hash,
                                            ]);
                                        match build_clutch_offer_vsf(
                                            &conversation_token,
                                            &payload,
                                            self.device_keypair
                                                .as_ref()
                                                .expect("device_keypair set in init")
                                                .public
                                                .as_bytes(),
                                            self.device_keypair
                                                .as_ref()
                                                .expect("device_keypair set in init")
                                                .secret
                                                .as_bytes(),
                                        ) {
                                            Ok((vsf_bytes, our_offer_provenance)) => {
                                                crate::log(&format!(
                                                    "CLUTCH: Sending full offer to {} (prov={}...)",
                                                    crate::fp(&contact.handle_proof),
                                                    hex::encode(&our_offer_provenance[..4])
                                                ));

                                                // Store our offer provenance (for ceremony_id derivation)
                                                if !contact
                                                    .offer_provenances
                                                    .contains(&our_offer_provenance)
                                                {
                                                    contact
                                                        .offer_provenances
                                                        .push(our_offer_provenance);
                                                }

                                                // Persist provenance immediately
                                                if let Some(storage) = self.storage.as_ref() {
                                                    if let Err(e) =
                                                        crate::storage::contacts::save_clutch_slots(
                                                            &contact.clutch_slots,
                                                            &contact.offer_provenances,
                                                            contact.ceremony_id,
                                                            &contact.handle_hash,
                                                            storage,
                                                        )
                                                    {
                                                        crate::log(&format!(
                                                            "Failed to persist CLUTCH provenance: {}",
                                                            e
                                                        ));
                                                    }
                                                }

                                                let (primary, alt) =
                                                    contact.race_addrs().unwrap_or((ip, None));
                                                checker.send_offer(ClutchOfferRequest {
                                                    peer_addr: primary,
                                                    alt_addr: alt,
                                                    vsf_bytes,
                                                });
                                                contact.clutch_offer_sent = true;
                                                changed = true;
                                            }
                                            Err(e) => {
                                                crate::log(&format!(
                                                    "CLUTCH: Failed to build offer VSF: {}",
                                                    e
                                                ));
                                            }
                                        }
                                    }
                                }
                            }

                            // Queue retransmit of pending messages only on the offline→online EDGE (not every online update) — otherwise every received chat would re-trigger a full pending resend.
                            if came_online {
                                if let (Some(fid), Some((primary, alt))) =
                                    (contact.friendship_id, contact.race_addrs())
                                {
                                    // Look up sync record for this friendship's conversation_token
                                    let last_received = if let Some((_, chains)) =
                                        self.friendship_chains.iter().find(|(id, _)| *id == fid)
                                    {
                                        sync_records
                                            .iter()
                                            .find(|r| {
                                                r.conversation_token == chains.conversation_token
                                            })
                                            .map(|r| r.last_received_osc)
                                    } else {
                                        None
                                    };
                                    retransmit_requests.push((
                                        fid,
                                        primary,
                                        alt,
                                        contact.display_name(),
                                        *contact.public_identity.as_bytes(),
                                        last_received,
                                    ));
                                }
                            }

                            break;
                        }
                    }
                }
                // NOTE: ClutchOffer, ClutchInit, ClutchResponse, ClutchComplete handlers REMOVED Full 8-primitive CLUTCH uses ClutchOfferReceived and ClutchKemResponseReceived which are handled above (via TCP/PT transport).
                StatusUpdate::ChatMessage {
                    conversation_token,
                    prev_msg_hp,
                    ciphertext,
                    timestamp,
                    sender_addr,
                } => {
                    // Get our handle_hash for chain lookups
                    let our_handle_hash = match self.session.as_ref().map(|s| s.identity_seed) {
                        Some(h) => h,
                        None => {
                            crate::log("CHAT: No user_identity_seed - cannot decrypt");
                            continue;
                        }
                    };
                    // Sibling pid candidate, resolved BEFORE the chains borrow (a &self method call inside would conflict). Sibling chains carry the device-derived pid as our participant, not the identity seed.
                    let our_sibling_pid = self.our_sibling_pid();

                    // Find friendship by conversation_token
                    let chains_result = self
                        .friendship_chains
                        .iter_mut()
                        .find(|(_, c)| c.conversation_token == conversation_token);

                    let mut need_sync_records_update = false;
                    // Contact index to seal the chain-weave for AFTER the `chains` borrow ends.
                    let mut recv_seal_idx: Option<usize> = None;
                    if let Some((fid, chains)) = chains_result {
                        // Party-id seam: whichever of (identity seed, sibling pid) is actually a participant is "us" in these chains.
                        let our_handle_hash = if chains.participants().contains(&our_handle_hash) {
                            our_handle_hash
                        } else if let Some(pid) =
                            our_sibling_pid.filter(|p| chains.participants().contains(p))
                        {
                            pid
                        } else {
                            crate::log("CHAT: we are not a participant in these chains");
                            continue;
                        };
                        // For 2-party chats, infer sender as the "other" participant
                        let from_handle_hash = match chains.other_participant(&our_handle_hash) {
                            Some(h) => *h,
                            None => {
                                crate::log("CHAT: Could not determine sender (not a 2-party chat or we're not a participant)");
                                continue;
                            }
                        };

                        // Find contact by their handle_hash
                        let contact_info = self.contacts.iter().enumerate().find_map(|(idx, c)| {
                            if c.handle_hash == from_handle_hash {
                                Some((idx, c.display_name()))
                            } else {
                                None
                            }
                        });

                        let (contact_idx, handle) = match contact_info {
                            Some((idx, h)) => (idx, h),
                            None => {
                                crate::log(&format!(
                                    "CHAT: Contact not found for handle_hash {}...",
                                    hex::encode(&from_handle_hash[..8])
                                ));
                                continue;
                            }
                        };

                        // Deduplication: we've already processed this exact message (UDP duplicate, or — the important case — the sender RETRANSMITTED because our ACK was lost). Don't re-process (that would double-advance), but DO re-send the ACK if this is the most recently acked message, so the lost-ACK case heals instead of the sender retrying until it gives up and its chain stays frozen.
                        if chains.is_duplicate(&from_handle_hash, timestamp) {
                            // Re-ACK from the stored message, looked up by its eagle_time. Unlike the old single-slot last_acked (which only remembered the MOST RECENT ack and so dropped any earlier duplicate → permanent sender stall), every received message persists its own ack_hash, so ANY duplicate self-heals a lost ACK.
                            let stored = self.contacts.get(contact_idx).and_then(|c| {
                                let ack = c
                                    .messages
                                    .iter()
                                    .find(|m| !m.is_outgoing && m.timestamp == timestamp)
                                    .and_then(|m| m.ack_hash)?;
                                Some((ack, *c.public_identity.as_bytes()))
                            });
                            if let Some((ph, recipient_pubkey)) = stored {
                                if let Some(ref checker) = self.status_checker {
                                    checker.send_ack(AckRequest {
                                        peer_addr: sender_addr,
                                        recipient_pubkey,
                                        conversation_token,
                                        acked_eagle_time: timestamp,
                                        plaintext_hash: ph,
                                    });
                                    crate::log(&format!(
                                        "CHAT: Re-ACKed duplicate from {} (eagle_time {}) — our earlier ACK was likely lost",
                                        handle, timestamp
                                    ));
                                }
                            } else {
                                crate::log(&format!(
                                    "CHAT: Skipping duplicate from {} (eagle_time {}) — no stored ack_hash (pre-fix message or outgoing)",
                                    handle, timestamp
                                ));
                            }
                            continue;
                        }

                        // Strict in-order processing (Layer 1). The receiver decrypts at CURRENT_KEY_INDEX, which is only correct when this message is the immediate successor of the last one we processed. So verify_chain_link is now HARD: on a mismatch the message is "ahead" (its predecessor hasn't arrived yet) — buffer it on the `prev_msg_hp` it awaits and SKIP decrypt. It gets replayed when that predecessor lands (see the gap-buffer drain after a successful advance below). "Behind"/duplicate is already handled by is_duplicate above; an unrelated stale prev_msg_hp simply waits in the buffer (and the retransmit path re-sends).
                        if let Err(expected) =
                            chains.verify_chain_link(&from_handle_hash, &prev_msg_hp)
                        {
                            crate::log(&format!(
                                "CHAT: Hash chain gap from {} - expected prev {}..., got {}... — buffering (ahead of us)",
                                handle,
                                hex::encode(&expected[..8]),
                                hex::encode(&prev_msg_hp[..8])
                            ));
                            chains.buffer_for_gap(
                                prev_msg_hp,
                                from_handle_hash,
                                timestamp,
                                ciphertext.clone(),
                                sender_addr,
                            );
                            continue;
                        }

                        crate::log(&format!(
                            "CHAT: Received message from {} (eagle_time {}), {} bytes ciphertext",
                            handle,
                            timestamp,
                            ciphertext.len()
                        ));

                        use crate::crypto::chain::{
                            decrypt_layers, derive_salt, generate_scratch, CURRENT_KEY_INDEX,
                        };

                        // Get sender's chain for decryption
                        let sender_chain = match chains.chain(&from_handle_hash) {
                            Some(c) => c.clone(), // Clone to avoid borrow issues
                            None => {
                                crate::log("CHAT: Sender chain not found");
                                continue;
                            }
                        };

                        // Get sender's last plaintext for salt derivation
                        let their_last_plaintext =
                            chains.last_plaintext(&from_handle_hash).to_vec();

                        // Derive salt from their previous plaintext
                        let salt = derive_salt(&their_last_plaintext, &sender_chain);

                        // Generate scratch pad
                        let scratch = generate_scratch(&sender_chain, &salt);

                        // Convert eagle time for decryption
                        let eagle_time = vsf::EagleTime::from_oscillations(timestamp);

                        // DEBUG: Log decryption parameters
                        crate::log(&format!(
                            "CHAIN DECRYPT: sender_handle_hash={}..., key={}..., salt={}..., eagle_time={}, ciphertext_len={}",
                            hex::encode(&from_handle_hash[..4]),
                            hex::encode(&sender_chain.current_key()[..4]),
                            hex::encode(&salt[..4]),
                            timestamp,
                            ciphertext.len()
                        ));

                        // Decrypt using sender's chain
                        let plaintext = decrypt_layers(
                            &ciphertext,
                            &sender_chain,
                            CURRENT_KEY_INDEX,
                            &scratch,
                            &eagle_time,
                        );

                        // DEBUG: Log raw decrypted bytes
                        crate::log(&format!(
                            "CHAIN DECRYPT: raw plaintext bytes = {:?}",
                            &plaintext
                        ));

                        // Parse VSF field: (d{message}:x{text},hp{inc_hp},hR{pad}) Uses VsfField::parse() per AGENT.md
                        let mut ptr = 0usize;
                        let mut message_text = String::new();
                        let mut incorporated_hp = [0u8; 32];
                        // The braid: eagle_times naming the prior peer (=our outgoing) messages this step weaves. 0, 1, or 2.
                        let mut woven_times: Vec<i64> = Vec::new();

                        let field = match vsf::file_format::VsfField::parse(&plaintext, &mut ptr) {
                            Ok(f) => f,
                            Err(e) => {
                                crate::log(&format!("CHAT: VsfField parse error: {}", e));
                                continue;
                            }
                        };

                        if field.name != "message" {
                            crate::log(&format!(
                                "CHAT: Expected field name 'message', got '{}'",
                                field.name
                            ));
                            continue;
                        }

                        // Extract values by type marker (not position)
                        for value in &field.values {
                            match value {
                                vsf::VsfType::x(s) => message_text = s.clone(),
                                vsf::VsfType::hp(hash) if hash.len() == 32 => {
                                    incorporated_hp.copy_from_slice(hash);
                                }
                                vsf::VsfType::e(et) => match et {
                                    vsf::EtType::e5(t) => woven_times.push(*t as i64),
                                    vsf::EtType::e6(t) => woven_times.push(*t),
                                    vsf::EtType::e7(t) => woven_times.push(*t as i64),
                                    _ => {}
                                },
                                vsf::VsfType::hR(_) => {} // Random padding - ignore
                                other => {
                                    crate::log(&format!(
                                        "CHAT: Unexpected type in message: {:?}",
                                        other
                                    ));
                                }
                            }
                        }

                        if message_text.is_empty() {
                            crate::log("CHAT: No message text found in payload");
                            continue;
                        }

                        // Hidden chain-weave probe: a reserved-marker message that proves the ratchet works but must show NO chat bubble. Everything else on the receive path (chain advance, set_last_plaintext, mark_received, ACK send) still runs so the sender's chain advances and dedup works — only the UI is suppressed.
                        let is_chain_probe = message_text == crate::types::CHAIN_PROBE_MARKER;

                        crate::log(&format!(
                            "CHAT: Decrypted message from {}: \"{}\" (incorporated_hp={}...)",
                            handle,
                            if is_chain_probe { "<chain-weave probe>" } else { &message_text },
                            hex::encode(&incorporated_hp[..8])
                        ));

                        // Compute plaintext hash for ACK
                        let plaintext_hash = *blake3::hash(&plaintext).as_bytes();

                        // Derive this message's hash pointer (for bidirectional tracking)
                        use crate::types::friendship::derive_msg_hp;
                        let msg_hp = derive_msg_hp(&prev_msg_hp, &plaintext_hash, timestamp);

                        // Update their last_plaintext for next message's salt — the x-text ONLY (must match what the sender stored: salt source is text, never the full payload/pad).
                        chains.set_last_plaintext(&from_handle_hash, message_text.clone().into_bytes());

                        // Update bidirectional entropy state (derive weave hash from full message context)
                        chains.update_received_for_mixing(timestamp, msg_hp, &plaintext);

                        // The braid: resolve each woven eagle_time to its message content. The peer wove messages IT received — i.e. messages WE authored — so we resolve against our OUTGOING rows (is_outgoing == true). Both sides hold identical `content` for any such message → identical strands → the chains advance in lockstep. Sort by eagle_time so framing matches the sender's (which also sorted). A single device can't emit two messages at the same 704ps tick, so eagle_time is unique within our stream; the adversarial same-tick collision is not handled here (would need a content_hash tiebreak carried on the wire) — left as a known guard gap.
                        let woven_strands: Vec<Vec<u8>> = {
                            let mut times = woven_times.clone();
                            times.sort_unstable();
                            let mut strands = Vec::with_capacity(times.len());
                            for t in times {
                                if let Some(m) = self.contacts[contact_idx]
                                    .messages
                                    .iter()
                                    .find(|m| m.is_outgoing && m.timestamp == t)
                                {
                                    strands.push(m.content.as_bytes().to_vec());
                                } else {
                                    crate::log(&format!(
                                        "CHAT: braid strand miss — no outgoing message at eagle_time {}",
                                        t
                                    ));
                                }
                            }
                            strands
                        };
                        let strand_refs: Vec<&[u8]> =
                            woven_strands.iter().map(|s| s.as_slice()).collect();

                        // Advance their chain with the braid strands. our_plaintext = the decrypted x-text ONLY (must match the sender's process_ack, which advances with the stored salt-text — never the full payload/pad).
                        let message_text_bytes = message_text.clone().into_bytes();
                        let eagle_time_for_advance = vsf::EagleTime::from_oscillations(timestamp);
                        chains.advance(
                            &from_handle_hash,
                            &eagle_time_for_advance,
                            &message_text_bytes,
                            &strand_refs,
                        );

                        // Mark as received for deduplication (protects against UDP duplicates)
                        chains.mark_received(&from_handle_hash, timestamp);

                        // Update hash chain state for next message verification
                        chains.update_received_hash(&from_handle_hash, msg_hp);
                        crate::log(&format!(
                            "CHAT: Updated hash chain for {} - msg_hp={}...",
                            handle,
                            hex::encode(&msg_hp[..8])
                        ));

                        // Layer 1 gap-buffer drain: this message's msg_hp is now our last_received_hash, so any buffered message that was waiting on THIS as its predecessor is now contiguous. Replay them (front of the queue) so they're processed in order immediately — and each can cascade to fill the next gap when IT advances.
                        let ready = chains.take_buffered_for(&msg_hp);
                        if !ready.is_empty() {
                            crate::log(&format!(
                                "CHAT: gap filled — replaying {} buffered message(s) after msg_hp={}...",
                                ready.len(),
                                hex::encode(&msg_hp[..8])
                            ));
                            for buf in ready {
                                replay_queue.push_back(StatusUpdate::ChatMessage {
                                    conversation_token,
                                    prev_msg_hp: buf.prev_msg_hp,
                                    ciphertext: buf.ciphertext,
                                    timestamp: buf.eagle_time,
                                    sender_addr: buf.sender_addr,
                                    // (buf.sender_addr is SocketAddr; matches the variant field)
                                });
                            }
                        }

                        // CRASH SAFETY: Persist to disk BEFORE sending ACK If we crash after ACK but before disk, sender thinks we have it but we don't. Disk write is the commit point - ACK is just notification. If chain save fails, DO NOT send ACK. Sender will retransmit and we can try again, preventing permanent desync.
                        if let Some(storage) = self.storage.as_ref() {
                            if let Err(e) =
                                crate::storage::friendship::save_friendship_chains(chains, storage)
                            {
                                crate::log(&format!(
                                    "STORAGE CRITICAL: Failed to save chains after recv, skipping ACK: {}",
                                    e
                                ));
                                continue;
                            }
                            // Flag to update sync records after borrow ends
                            need_sync_records_update = true;
                        }

                        // Add message to contact's message list and persist — UNLESS this is the hidden chain-weave probe, which advances/ACKs the chain but must never surface a bubble or chime. For the probe we only flip `their_probe_seen` (their TX / our RX proven) and try to seal the chain.
                        if is_chain_probe {
                            if let Some(contact) = self.contacts.get_mut(contact_idx) {
                                contact.their_probe_seen = true;
                            }
                            crate::log("CHAIN-PROBE: received peer's chain-weave probe — RX chain proven");
                            recv_seal_idx = Some(contact_idx);
                        } else if let Some(contact) = self.contacts.get_mut(contact_idx) {
                            // Any real received message means the chain is demonstrably working end-to-end in at least the RX direction — belt-and-suspenders toward woven.
                            contact.their_probe_seen = true;
                            // Use actual eagle_time and sorted insert for correct chronological order
                            contact.insert_message_sorted(
                                ChatMessage::new_with_timestamp(
                                    message_text,
                                    false,     // is_outgoing = false (received)
                                    timestamp, // Use message's actual eagle_time, not current time
                                )
                                // Persist the ACK hash so a later duplicate (our ACK was lost) can be re-ACKed from storage — keeps the sender's chain from stalling.
                                .with_ack_hash(plaintext_hash),
                            );
                            contact.message_scroll_offset = 0.0; // Scroll to show new message
                            changed = true;

                            // Persist messages for UI
                            if let Some(storage) = self.storage.as_ref() {
                                if let Err(e) =
                                    crate::storage::contacts::save_messages(contact, storage)
                                {
                                    crate::log(&format!("STORAGE: Failed to save messages: {}", e));
                                }
                            }

                            // Per-contact notification chime: the sender's relationship digest → deterministic modal bell (chirp crate) — the SAME digest that colours their handle and messages, so ears and eyes agree. The handle TEXT never touches the session store by design; the pre-PoW hashes are the canonical identity material. Synthesis (~a second of f64 modal math) + playback run on a detached thread so the receive loop never blocks; desktop-only (Android gets platform notifications).
                            #[cfg(not(any(target_os = "redox", target_os = "android")))]
                            {
                                let digest = relationship_digest(&from_handle_hash, &our_handle_hash);
                                std::thread::spawn(move || {
                                    chirp::Chirp::from_hash(digest).play_blocking().unwrap_or_else(|e| crate::log(&format!("CHIME: {e}")));
                                });
                            }
                            // A real inbound message proves both directions once ACKed, but even the RX half alone can seal if our TX was already ACK-confirmed.
                            recv_seal_idx = Some(contact_idx);
                        }

                        // *** THEN send ACK - if we crash here, sender will resend, we can dedup *** Get recipient pubkey for relay fallback
                        let recipient_pubkey = self
                            .contacts
                            .get(contact_idx)
                            .map(|c| *c.public_identity.as_bytes())
                            .unwrap_or([0u8; 32]);
                        // The re-ACK source is now the per-message ack_hash persisted on the stored ChatMessage (see the duplicate handler above + with_ack_hash below), which heals a lost ACK for ANY message — not just the most recent. The old single-slot last_acked is retired.
                        if let Some(ref checker) = self.status_checker {
                            checker.send_ack(AckRequest {
                                peer_addr: sender_addr,
                                recipient_pubkey,
                                conversation_token,
                                acked_eagle_time: timestamp,
                                plaintext_hash,
                            });
                            crate::log(&format!(
                                "CHAT: Sent ACK to {} (eagle_time {}, hash {}...)",
                                handle,
                                timestamp,
                                hex::encode(&plaintext_hash[..8])
                            ));
                        }
                        let _ = fid; // We looked up by token, fid is available if needed
                    } else {
                        crate::log(&format!(
                            "CHAT: No friendship found for conversation_token {}...",
                            hex::encode(&conversation_token[..8])
                        ));
                    }

                    // Defer the chain-weave seal until after the loop (the outer `checker` borrow blocks `&mut self` here). No-op later unless both directions are proven.
                    if let Some(idx) = recv_seal_idx {
                        chain_seal_indices.push(idx);
                    }

                    // Flag to update sync records after outer loop (checker borrow must end first)
                    if need_sync_records_update {
                        need_sync_update = true;
                    }
                }
                StatusUpdate::MessageAck {
                    conversation_token,
                    acked_eagle_time,
                    plaintext_hash,
                } => {
                    // Get our handle_hash
                    let our_handle_hash = match self.session.as_ref().map(|s| s.identity_seed) {
                        Some(h) => h,
                        None => {
                            crate::log("CHAT: No user_identity_seed - cannot process ACK");
                            continue;
                        }
                    };
                    // Sibling pid candidate, resolved BEFORE the chains borrow (see the ChatMessage arm).
                    let our_sibling_pid = self.our_sibling_pid();

                    // Find friendship by conversation_token
                    let chains_result = self
                        .friendship_chains
                        .iter_mut()
                        .find(|(_, c)| c.conversation_token == conversation_token);

                    // Contact index to seal AFTER the `chains` borrow ends (seal needs &mut self).
                    let mut ack_sealed_idx: Option<usize> = None;
                    if let Some((_, chains)) = chains_result {
                        // Party-id seam: whichever of (identity seed, sibling pid) is a participant is "us".
                        let our_handle_hash = if chains.participants().contains(&our_handle_hash) {
                            our_handle_hash
                        } else if let Some(pid) =
                            our_sibling_pid.filter(|p| chains.participants().contains(p))
                        {
                            pid
                        } else {
                            crate::log("CHAT: we are not a participant in these chains (ACK)");
                            continue;
                        };
                        // For 2-party chats, the ACK sender is the "other" participant
                        let from_handle_hash = match chains.other_participant(&our_handle_hash) {
                            Some(h) => *h,
                            None => {
                                crate::log("CHAT: Could not determine ACK sender");
                                continue;
                            }
                        };

                        // Find contact by their handle_hash
                        let contact_info = self.contacts.iter().enumerate().find_map(|(idx, c)| {
                            if c.handle_hash == from_handle_hash {
                                Some((idx, c.display_name()))
                            } else {
                                None
                            }
                        });

                        let (contact_idx, handle) = match contact_info {
                            Some((idx, h)) => (idx, h),
                            None => {
                                crate::log(&format!(
                                    "CHAT: Contact not found for ACK from handle_hash {}...",
                                    hex::encode(&from_handle_hash[..8])
                                ));
                                continue;
                            }
                        };

                        crate::log(&format!(
                            "CHAT: ACK received from {} for eagle_time {} (hash: {}...)",
                            handle,
                            acked_eagle_time,
                            hex::encode(&plaintext_hash[..8])
                        ));

                        // Process ACK: advance our chain and remove pending message
                        if chains.process_ack(&our_handle_hash, acked_eagle_time, &plaintext_hash) {
                            crate::log(&format!(
                                "CHAT: Chain advanced for {} (ACK verified)",
                                handle
                            ));

                            // Our TX chain just advanced on a matching ACK — their RX is proven. Record it so the chain-weave can seal (sealing itself happens after the `chains` borrow ends, below). This is the "our TX / their RX" half of woven.
                            if let Some(contact) = self.contacts.get_mut(contact_idx) {
                                contact.chain_advanced_by_ack = true;
                            }
                            ack_sealed_idx = Some(contact_idx);

                            // First ACK confirms both sides have working chains - safe to zeroize CLUTCH keypairs
                            if let Some(contact) = self.contacts.get_mut(contact_idx) {
                                if contact.clutch_our_keypairs.is_some() {
                                    let their_identity_seed = contact.handle_hash;
                                    crate::log(&format!(
                                        "CLUTCH: First ACK from {} - zeroizing ephemeral keypairs",
                                        crate::fp(&contact.handle_proof)
                                    ));
                                    if let Some(ref mut keys) = contact.clutch_our_keypairs {
                                        keys.zeroize();
                                    }
                                    contact.clutch_our_keypairs = None;
                                    for slot in &mut contact.clutch_slots {
                                        slot.offer = None;
                                        if let Some(ref mut s) = slot.kem_secrets_from_them {
                                            s.zeroize();
                                        }
                                        if let Some(ref mut s) = slot.kem_secrets_to_them {
                                            s.zeroize();
                                        }
                                        slot.kem_secrets_from_them = None;
                                        slot.kem_secrets_to_them = None;
                                    }

                                    // Delete persisted keypairs file (no longer needed)
                                    if let Some(storage) = self.storage.as_ref() {
                                        if let Err(e) =
                                            crate::storage::contacts::delete_clutch_keypairs(
                                                &their_identity_seed,
                                                storage,
                                            )
                                        {
                                            crate::log(&format!(
                                                "CLUTCH: Failed to delete keypairs file for seed {}: {}",
                                                hex::encode(&their_identity_seed[..4]),
                                                e
                                            ));
                                        }
                                    }
                                }
                            }

                            // Persist chains (AGENT.md: every change hits disk)
                            if let Some(storage) = self.storage.as_ref() {
                                if let Err(e) = crate::storage::friendship::save_friendship_chains(
                                    chains, storage,
                                ) {
                                    crate::log(&format!(
                                        "STORAGE CRITICAL: Failed to save chains after ACK: {}",
                                        e
                                    ));
                                }
                            }
                        } else {
                            // No pending message matched. Two cases: (a) a DUPLICATE ACK — dual-path racing (P3) delivers the same ACK on both the LAN and public path, so the second copy arrives after the first already advanced + cleared the pending entry; (b) a genuinely UNKNOWN ACK. Tell them apart via the outgoing message: if it exists and is already `delivered`, this is the benign duplicate — log at DEBUG so it stops reading as a failure.
                            let is_dup = self.contacts.get(contact_idx).is_some_and(|c| {
                                c.messages.iter().any(|m| {
                                    m.is_outgoing && m.delivered && m.timestamp == acked_eagle_time
                                })
                            });
                            if is_dup {
                                crate::log_at(
                                    crate::LogLevel::Debug,
                                    &format!(
                                        "CHAT: Duplicate ACK from {} (eagle_time {}) — already delivered, dual-path echo",
                                        handle, acked_eagle_time
                                    ),
                                );
                            } else {
                                crate::log(&format!(
                                    "CHAT: ACK verification failed for {} (no matching pending message)",
                                    handle
                                ));
                            }
                        }

                        // Mark message as delivered in UI
                        if let Some(contact) = self.contacts.get_mut(contact_idx) {
                            // Find message by matching eagle_time (exact i64 oscillations)
                            let mut found_msg = false;
                            for msg in contact.messages.iter_mut().rev() {
                                if msg.is_outgoing && !msg.delivered {
                                    // Match by eagle_time (exact i64 match)
                                    if msg.timestamp == acked_eagle_time {
                                        msg.delivered = true;
                                        found_msg = true;
                                        changed = true;
                                        break;
                                    }
                                }
                            }

                            // Persist delivered status (AGENT.md: every change hits disk)
                            if found_msg {
                                if let Some(storage) = self.storage.as_ref() {
                                    if let Err(e) =
                                        crate::storage::contacts::save_messages(contact, storage)
                                    {
                                        crate::log(&format!(
                                            "STORAGE: Failed to save delivered status: {}",
                                            e
                                        ));
                                    }
                                }
                            }
                        }
                    } else {
                        crate::log(&format!(
                            "CHAT: No friendship found for ACK conversation_token {}...",
                            hex::encode(&conversation_token[..8])
                        ));
                    }

                    // Defer the chain-weave seal until after the loop (outer `checker` borrow blocks `&mut self` here). No-op later unless both directions are proven.
                    if let Some(idx) = ack_sealed_idx {
                        chain_seal_indices.push(idx);
                    }
                }

                // PT large transfer received (fallback - normally parsed in status.rs) This only fires if the PT data wasn't recognized as CLUTCH message
                StatusUpdate::PTReceived { peer_addr, data } => {
                    crate::log(&format!(
                        "PT: Received unknown {} bytes from {} (not CLUTCH)",
                        data.len(),
                        peer_addr
                    ));
                }

                // PT outbound transfer completed
                StatusUpdate::PTSendComplete { peer_addr } => {
                    crate::log(&format!("PT: Outbound transfer to {} completed", peer_addr));
                    // TODO: Track completion for full CLUTCH flow
                }

                // Full CLUTCH offer received (~548KB with all 8 pubkeys) Payload is already parsed and signature verified by status.rs
                StatusUpdate::ClutchOfferReceived {
                    conversation_token,
                    offer_provenance, // Unique per offer (VSF hp field)
                    sender_pubkey,
                    payload,
                    sender_addr: raw_sender_addr,
                } => {
                    use crate::crypto::clutch::{
                        derive_conversation_token, ClutchKemSharedSecrets, ClutchOfferPayload,
                    };
                    use crate::network::status::ClutchOfferRequest;
                    use crate::types::ClutchState;

                    crate::log(&format!(
                        "CLUTCH: Processing ClutchOfferReceived from {} (contacts={})",
                        raw_sender_addr,
                        self.contacts.len()
                    ));

                    // Normalize to port 4383 (TCP source port is ephemeral)
                    let sender_addr =
                        std::net::SocketAddr::new(raw_sender_addr.ip(), crate::PHOTON_PORT);

                    // Get our handle_hash
                    let our_handle_hash = match self.session.as_ref().map(|s| s.identity_seed) {
                        Some(h) => h,
                        None => {
                            #[cfg(feature = "development")]
                            #[cfg(feature = "development")]
                            crate::log("CLUTCH: No user_identity_seed available");
                            continue;
                        }
                    };
                    let our_sibling_pid = self.our_sibling_pid();

                    // Find contact by conversation_token (compute token for each contact and match). Party-id seam: sibling candidates token with the device-derived pid pair; the resolved "our" id shadows the seed for the whole arm.
                    let (their_handle_hash, our_handle_hash) = match self
                        .contacts
                        .iter()
                        .find_map(|c| {
                            let our = if c.is_sibling {
                                our_sibling_pid?
                            } else {
                                our_handle_hash
                            };
                            (derive_conversation_token(&[our, c.handle_hash])
                                == conversation_token)
                                .then_some((c.handle_hash, our))
                        }) {
                        Some(pair) => pair,
                        None => {
                            crate::log(&format!(
                                "CLUTCH: Received offer with unknown conversation_token {}",
                                hex::encode(&conversation_token[..8])
                            ));
                            continue;
                        }
                    };

                    crate::log(&format!(
                        "CLUTCH: Received full offer (VSF verified) from {} tok={}...",
                        sender_addr,
                        hex::encode(&conversation_token[..8])
                    ));

                    // Gate: the sender must be a CURRENTLY-TRUSTED device of this contact (fold-respecting `knows_device`). Post-fold this widens to ANY current fleet member (a friend's 2nd device can now CLUTCH — was pinned to first-met only) AND revokes a removed device (it fails membership); pre-fold + siblings pin to the one known device exactly as before.
                    let sender_known = self
                        .contacts
                        .iter()
                        .find(|c| c.handle_hash == their_handle_hash)
                        .map(|c| c.knows_device(&sender_pubkey));
                    match sender_known {
                        None => {
                            #[cfg(feature = "development")]
                            #[cfg(feature = "development")]
                            crate::log("CLUTCH: Received offer from unknown contact");
                            continue;
                        }
                        Some(false) => {
                            crate::log(&format!(
                                "CLUTCH: offer from untrusted/removed device {} for {} — dropping",
                                hex::encode(&sender_pubkey[..8]),
                                hex::encode(&their_handle_hash[..8])
                            ));
                            continue;
                        }
                        Some(true) => {} // Trusted current device — proceed
                    }

                    // The payload is already parsed
                    let their_offer = payload;

                    // Find contact by handle_hash
                    let mut rekey_request: Option<(ContactId, [u8; 32])> = None;
                    let mut chains_to_remove: Vec<FriendshipId> = Vec::new();
                    // Deferred KEM encapsulation spawn (to avoid borrow conflict)
                    let mut kem_encap_spawn: Option<(
                        ContactId,
                        ClutchOfferPayload,
                        [u8; 32],
                        [u8; 32],
                        std::net::SocketAddr,
                    )> = None;

                    for (idx, contact) in self.contacts.iter_mut().enumerate() {
                        if contact.handle_hash == their_handle_hash {
                            contact.ip = Some(sender_addr);
                            // Authenticated CLUTCH traffic from them ⇒ reachable right now ⇒ show online immediately, don't wait for the next pong.
                            if !contact.is_online {
                                contact.is_online = true;
                                changed = true;
                                crate::log(&format!(
                                    "Status: {} is now ONLINE (CLUTCH)",
                                    crate::fp(&contact.handle_proof)
                                ));
                            }

                            // Simple re-key logic: if stored keys don't match received keys, re-key. Same keys = duplicate/stale (ignore). Different/no keys = accept.
                            let stored_hqc_pub = contact
                                .get_slot(&their_handle_hash)
                                .and_then(|slot| slot.offer.as_ref())
                                .map(|o| o.hqc256_public.clone());

                            if let Some(stored_keys) = stored_hqc_pub {
                                if stored_keys == their_offer.hqc256_public {
                                    // Same keys - check if we already sent KEM response If so, peer didn't receive it - re-send!
                                    let already_sent_kem = contact
                                        .get_slot(&our_handle_hash)
                                        .map(|s| s.kem_secrets_to_them.is_some())
                                        .unwrap_or(false);

                                    if already_sent_kem {
                                        // We already sent KEM response but peer resent offer They didn't receive it - trigger re-send
                                        crate::log(&format!(
                                            "CLUTCH: Re-sending KEM response to {} (peer resent same offer)",
                                            crate::fp(&contact.handle_proof)
                                        ));
                                        // The missing half of the deadlock recovery. The peer re-sending their offer means they're stuck, and the usual cause is that OUR offer never reached them — its one send may have gone to an address not yet confirmed reachable (e.g. their LAN IPv4 before their public/IPv6 was known) and been lost. Re-sending only our KEM can't help: they can't answer an offer they never received, so they keep re-sending theirs and queuing our KEMs forever. Re-arm our offer so the online/pong handler re-transmits it via race_addrs — now to the address their packets are actually arriving from. (Their side re-sends THEIR offer via the pending-KEM branch above; this is the symmetric OUR-offer resend.)
                                        if contact.clutch_state == ClutchState::Pending {
                                            contact.clutch_offer_sent = false;
                                        }
                                        // Don't continue - fall thru to re-send KEM below
                                    } else {
                                        // Same keys but no KEM sent yet - truly duplicate, ignore
                                        crate::log(&format!(
                                            "CLUTCH: Ignoring duplicate offer from {} (same keys, no KEM sent yet)",
                                            crate::fp(&contact.handle_proof)
                                        ));
                                        continue;
                                    }
                                } else {
                                    // Different keys from them - but DON'T immediately nuke! This prevents infinite re-key loops where both sides keep regenerating.
                                    //
                                    // Strategy: If we have keypairs, just update their offer and continue. We'll send our existing offer, they'll either:
                                    // - Accept it (converge) if they're mid-ceremony
                                    // - Send KEM response (complete) if they're ahead
                                    //
                                    // Guard against a FALSE re-key: a peer we already completed with re-sends its offer (retransmit, or our slots got zeroized post- completion so stored_hqc_pub no longer matches). At completion we saved their HQC pubkey PREFIX precisely to recognize this. If the incoming offer matches what we completed with, it's the SAME peer — ignore it, do NOT nuke. Only a genuinely DIFFERENT key (they truly re-keyed / lost their chains) should trigger a re-key. Without this, a Complete↔Complete pair bounced back to Pending on a stray offer ("it completed, then went back to Pending after a message").
                                    if contact.clutch_state == ClutchState::Complete {
                                        let their_prefix: [u8; 8] = their_offer.hqc256_public
                                            [..8]
                                            .try_into()
                                            .unwrap_or_default();
                                        if contact.completed_their_hqc_prefix == Some(their_prefix) {
                                            crate::log(&format!(
                                                "CLUTCH: Ignoring offer from {} — matches the key we already completed with (no re-key)",
                                                crate::fp(&contact.handle_proof)
                                            ));
                                            continue;
                                        }
                                        // Post-weave cooldown (see the twin guard in the no-keypairs branch below): a different-keyed offer arriving right after we wove is a crossed pre-completion re-offer from the peer's own racing ceremony, not a deliberate reset. Ignore it briefly so we don't nuke a just-woven chain into a divergent re-key. A genuine reset persists past the window.
                                        const REKEY_COOLDOWN: std::time::Duration =
                                            std::time::Duration::from_secs(10);
                                        if contact
                                            .clutch_completed_at
                                            .is_some_and(|t| t.elapsed() < REKEY_COOLDOWN)
                                        {
                                            crate::log(&format!(
                                                "CLUTCH: Ignoring different-keyed offer from {} — completed {}ms ago (post-completion re-key cooldown)",
                                                crate::fp(&contact.handle_proof),
                                                contact.clutch_completed_at.map(|t| t.elapsed().as_millis()).unwrap_or(0)
                                            ));
                                            continue;
                                        }
                                        crate::log(&format!(
                                            "CLUTCH: Re-key from {} - we're Complete, they have new keys, nuking for fresh ceremony",
                                            crate::fp(&contact.handle_proof)
                                        ));
                                        // Full re-key: nuke everything
                                        contact.clutch_our_keypairs = None;
                                        contact.clutch_slots.clear();
                                        contact.ceremony_id = None;
                                        contact.offer_provenances.clear();
                                        contact.clutch_pending_kem = None;
                                        contact.clutch_offer_sent = false;
                                        contact.clutch_state = ClutchState::Pending;
                                        contact.completed_their_hqc_prefix = None;
                                        if let Some(old_friendship_id) =
                                            contact.friendship_id.take()
                                        {
                                            crate::log(&format!(
                                                "CLUTCH: Invalidating old chains for {}",
                                                crate::fp(&contact.handle_proof)
                                            ));
                                            chains_to_remove.push(old_friendship_id);
                                        }
                                        rekey_request =
                                            Some((contact.id.clone(), contact.handle_hash));
                                    } else {
                                        // Not Complete - just update their offer, don't regenerate our keys
                                        crate::log(&format!(
                                            "CLUTCH: {} sent new keys but we're mid-ceremony (state={:?}) - updating their offer, keeping our keys",
                                            crate::fp(&contact.handle_proof), contact.clutch_state
                                        ));
                                        // Clear their old offer data so we use the new one
                                        if let Some(slot) = contact.get_slot_mut(&their_handle_hash)
                                        {
                                            slot.offer = None;
                                            slot.kem_secrets_from_them = None;
                                        }
                                        // Clear our old KEM encap - it was for their OLD keys! We need fresh encapsulation against their new pubkeys.
                                        if let Some(slot) = contact.get_slot_mut(&our_handle_hash) {
                                            slot.kem_secrets_to_them = None;
                                            slot.kem_response_for_resend = None;
                                        }
                                        contact.clutch_kem_encap_in_progress = false;
                                        // Clear ceremony_id so it gets recomputed with new provenance
                                        contact.ceremony_id = None;
                                        contact.offer_provenances.retain(|p| {
                                            // Keep our provenance, remove their old one Our provenance is computed from our handle_hash This is a bit hacky but works for 2-party
                                            p != &offer_provenance
                                        });
                                        // Don't trigger rekey_request - we keep our keys
                                    }
                                }
                            }
                            // No stored keys = fresh start, accept offer below

                            // Initialize slots if not already done
                            if contact.clutch_slots.is_empty() {
                                contact.init_clutch_slots(our_handle_hash);
                            }

                            // Store their offer in their slot
                            if let Some(slot) = contact.get_slot_mut(&their_handle_hash) {
                                slot.offer = Some(their_offer.clone());
                                crate::log(&format!(
                                    "CLUTCH: Stored offer from {} in slot",
                                    crate::fp(&contact.handle_proof)
                                ));
                            }

                            // Store OUR offer in OUR slot too — every slot needs offer + a KEM contribution to be complete (PartySlot::is_complete). When their offer arrives first and we go straight to the KEM-response path, our own slot would otherwise keep offer=None forever, so all_slots_complete never fires and the ceremony never runs (the one-sided-nuke re-key stall: we have keys + sent a KEM, but our local offer was never recorded).
                            if contact
                                .get_slot(&our_handle_hash)
                                .map(|s| s.offer.is_none())
                                .unwrap_or(false)
                            {
                                if let Some(ref keypairs) = contact.clutch_our_keypairs {
                                    let our_offer =
                                        clutch::ClutchOfferPayload::from_keypairs(keypairs);
                                    if let Some(local_slot) = contact.get_slot_mut(&our_handle_hash)
                                    {
                                        local_slot.offer = Some(our_offer);
                                        crate::log(
                                            "CLUTCH: Stored our own offer in local slot (on offer-received)",
                                        );
                                    }
                                }
                            }

                            // Store their offer_provenance for ceremony_id derivation
                            if !contact.offer_provenances.contains(&offer_provenance) {
                                contact.offer_provenances.push(offer_provenance);
                                crate::log(&format!(
                                    "CLUTCH: Stored offer_provenance from {} (now have {})",
                                    crate::fp(&contact.handle_proof),
                                    contact.offer_provenances.len()
                                ));
                            }

                            // Compute ceremony_id if we have all provenances (2 for DM)
                            let required_provenances = 2;
                            if contact.ceremony_id.is_none()
                                && contact.offer_provenances.len() >= required_provenances
                            {
                                use crate::types::CeremonyId;
                                let ceremony_id = *CeremonyId::derive(
                                    &[our_handle_hash, contact.handle_hash],
                                    &contact.offer_provenances,
                                )
                                .as_bytes();
                                contact.ceremony_id = Some(ceremony_id);
                                crate::log(&format!(
                                    "CLUTCH: Derived ceremony_id={}... from {} offer_provenances",
                                    hex::encode(&ceremony_id[..4]),
                                    contact.offer_provenances.len()
                                ));

                                // Process any pending KEM response that arrived before ceremony_id
                                if let Some(pending_kem) = contact.clutch_pending_kem.take() {
                                    crate::log(&format!(
                                        "CLUTCH: Processing queued KEM response from {} (ceremony_id now available)",
                                        crate::fp(&contact.handle_proof)
                                    ));
                                    // Decapsulate remote KEM (remote encapsulated to local pubkeys)
                                    if let Some(ref local_keys) = contact.clutch_our_keypairs {
                                        let remote_secrets =
                                            ClutchKemSharedSecrets::decapsulate_from_peer(
                                                &pending_kem,
                                                local_keys,
                                            );
                                        // Store remote secrets in remote slot
                                        if let Some(remote_slot) =
                                            contact.get_slot_mut(&their_handle_hash)
                                        {
                                            remote_slot.kem_secrets_from_them =
                                                Some(remote_secrets);
                                            crate::log(&format!(
                                                "CLUTCH: Decapsulated queued KEM from {} - stored in slot",
                                                crate::fp(&contact.handle_proof)
                                            ));
                                        }
                                    }
                                }
                            }

                            // Persist slot state (offer, provenances, ceremony_id)
                            if let Some(storage) = self.storage.as_ref() {
                                if let Err(e) = crate::storage::contacts::save_clutch_slots(
                                    &contact.clutch_slots,
                                    &contact.offer_provenances,
                                    contact.ceremony_id,
                                    &contact.handle_hash,
                                    storage,
                                ) {
                                    crate::log(&format!(
                                        "CLUTCH: Failed to save slots for {}: {}",
                                        crate::fp(&contact.handle_proof), e
                                    ));
                                }
                            }

                            // If we have keypairs, send our offer (if not sent) and KEM response
                            if let Some(ref keypairs) = contact.clutch_our_keypairs {
                                // Compute conversation_token once for this contact
                                let conv_token = derive_conversation_token(&[
                                    our_handle_hash,
                                    contact.handle_hash,
                                ]);

                                // Send our offer if not already sent
                                if !contact.clutch_offer_sent {
                                    use crate::network::fgtw::protocol::build_clutch_offer_vsf;

                                    let our_offer = ClutchOfferPayload::from_keypairs(keypairs);

                                    // Build VSF and capture our offer_provenance
                                    match build_clutch_offer_vsf(
                                        &conv_token,
                                        &our_offer,
                                        self.device_keypair
                                            .as_ref()
                                            .expect("device_keypair set in init")
                                            .public
                                            .as_bytes(),
                                        self.device_keypair
                                            .as_ref()
                                            .expect("device_keypair set in init")
                                            .secret
                                            .as_bytes(),
                                    ) {
                                        Ok((vsf_bytes, our_offer_provenance)) => {
                                            // Store our offer provenance
                                            if !contact
                                                .offer_provenances
                                                .contains(&our_offer_provenance)
                                            {
                                                contact
                                                    .offer_provenances
                                                    .push(our_offer_provenance);
                                            }

                                            // The offer arrived from sender_addr, so that path is known-reachable — use it as primary and race the contact's other known address as the alternate.
                                            let alt = contact
                                                .race_addrs()
                                                .and_then(|(p, a)| a.or(Some(p)))
                                                .filter(|a| *a != sender_addr);
                                            checker.send_offer(ClutchOfferRequest {
                                                peer_addr: sender_addr,
                                                alt_addr: alt,
                                                vsf_bytes,
                                            });
                                            contact.clutch_offer_sent = true;
                                            // Store local offer in local slot too
                                            if let Some(local_slot) =
                                                contact.get_slot_mut(&our_handle_hash)
                                            {
                                                local_slot.offer = Some(our_offer);
                                            }
                                            crate::log(&format!(
                                                "CLUTCH: Sent full offer to {} (prov={}...)",
                                                crate::fp(&contact.handle_proof),
                                                hex::encode(&our_offer_provenance[..4])
                                            ));

                                            // Compute ceremony_id now that we have both provenances
                                            if contact.ceremony_id.is_none()
                                                && contact.offer_provenances.len()
                                                    >= required_provenances
                                            {
                                                use crate::types::CeremonyId;
                                                let ceremony_id = *CeremonyId::derive(
                                                    &[our_handle_hash, contact.handle_hash],
                                                    &contact.offer_provenances,
                                                )
                                                .as_bytes();
                                                contact.ceremony_id = Some(ceremony_id);
                                                crate::log(&format!(
                                                    "CLUTCH: Derived ceremony_id={}... after sending offer",
                                                    hex::encode(&ceremony_id[..4])
                                                ));
                                            }

                                            // Persist provenance/ceremony_id immediately
                                            if let Some(storage) = self.storage.as_ref() {
                                                if let Err(e) =
                                                    crate::storage::contacts::save_clutch_slots(
                                                        &contact.clutch_slots,
                                                        &contact.offer_provenances,
                                                        contact.ceremony_id,
                                                        &contact.handle_hash,
                                                        storage,
                                                    )
                                                {
                                                    crate::log(&format!(
                                                        "Failed to persist CLUTCH provenance: {}",
                                                        e
                                                    ));
                                                }
                                            }
                                        }
                                        Err(e) => {
                                            crate::log(&format!(
                                                "CLUTCH: Failed to build offer VSF: {}",
                                                e
                                            ));
                                        }
                                    }
                                }

                                // Send KEM response (encapsulate to remote pubkeys) Check if we haven't already sent (kem_secrets_to_them in local slot) KEM response requires ceremony_id (for wire format verification)
                                let already_sent_kem = contact
                                    .get_slot(&our_handle_hash)
                                    .map(|s| s.kem_secrets_to_them.is_some())
                                    .unwrap_or(false);

                                // Check for re-send case: we have stored payload from previous send
                                let resend_payload = contact
                                    .get_slot(&our_handle_hash)
                                    .and_then(|s| s.kem_response_for_resend.clone());

                                if let Some(kem_response) = resend_payload {
                                    // Re-send using stored payload
                                    if let Some(ceremony_id) = contact.ceremony_id {
                                        use crate::network::status::ClutchKemResponseRequest;

                                        let alt = contact
                                            .race_addrs()
                                            .and_then(|(p, a)| a.or(Some(p)))
                                            .filter(|a| *a != sender_addr);
                                        checker.send_kem_response(ClutchKemResponseRequest {
                                            peer_addr: sender_addr,
                                            alt_addr: alt,
                                            conversation_token: conv_token,
                                            ceremony_id,
                                            payload: kem_response,
                                            device_pubkey: *self
                                                .device_keypair
                                                .as_ref()
                                                .expect("device_keypair set in init")
                                                .public
                                                .as_bytes(),
                                            device_secret: *self
                                                .device_keypair
                                                .as_ref()
                                                .expect("device_keypair set in init")
                                                .secret
                                                .as_bytes(),
                                        });
                                        crate::log(&format!(
                                            "CLUTCH: Re-sent KEM response to {}",
                                            crate::fp(&contact.handle_proof)
                                        ));
                                    }
                                } else if !already_sent_kem && !contact.clutch_kem_encap_in_progress
                                {
                                    if let Some(ceremony_id) = contact.ceremony_id {
                                        // Defer spawn for KEM encapsulation (to avoid borrow conflict) (PQ crypto is slow ~800ms, would block UI/network)
                                        contact.clutch_kem_encap_in_progress = true;
                                        kem_encap_spawn = Some((
                                            contact.id.clone(),
                                            their_offer.clone(),
                                            ceremony_id,
                                            conv_token,
                                            sender_addr,
                                        ));
                                        crate::log(&format!(
                                            "CLUTCH: Will spawn KEM encapsulation for {}",
                                            crate::fp(&contact.handle_proof)
                                        ));
                                        changed = true;
                                    } else {
                                        crate::log(&format!(
                                            "CLUTCH: Deferring KEM response to {} - waiting for ceremony_id",
                                            crate::fp(&contact.handle_proof)
                                        ));
                                    }
                                }

                                // Check if ceremony is complete (defer to after outer loop)
                                if contact.all_slots_complete() {
                                    ceremony_completions.push(idx);
                                    changed = true;
                                }
                            } else if contact.clutch_our_keypairs.is_none() {
                                if contact.clutch_keygen_in_progress {
                                    // Keygen already running - don't spawn another
                                    crate::log(&format!(
                                        "CLUTCH: Received offer from {} but keygen already in progress - waiting",
                                        crate::fp(&contact.handle_proof)
                                    ));
                                } else {
                                    // No keypairs - need to respond (whether Complete or not) If Complete: peer lost their chains, accept re-key If not Complete: restart mid-ceremony or fresh re-key
                                    if contact.clutch_state == ClutchState::Complete {
                                        // POST-WEAVE RE-KEY COOLDOWN. Completion zeroizes our ephemeral keypairs (is_none here), so a peer's offer that was in flight just before they saw our completion lands right after we weave and would trip the re-key path below — a SPURIOUS re-key that, when both sides do it near-simultaneously, storms into divergent ceremonies (observed: two devices stuck at 5/8 and 7/8 forever). Within the cooldown, ignore the stray offer: a crossed leftover stops within ~1s (the peer completes too). A GENUINE reset peer keeps sending and re-keys once the window passes.
                                        const REKEY_COOLDOWN: std::time::Duration =
                                            std::time::Duration::from_secs(10);
                                        if contact
                                            .clutch_completed_at
                                            .is_some_and(|t| t.elapsed() < REKEY_COOLDOWN)
                                        {
                                            crate::log(&format!(
                                                "CLUTCH: Ignoring offer from {} — completed {}ms ago (post-completion re-key cooldown; likely a crossed pre-completion offer, not a reset)",
                                                crate::fp(&contact.handle_proof),
                                                contact.clutch_completed_at.map(|t| t.elapsed().as_millis()).unwrap_or(0)
                                            ));
                                            continue;
                                        }
                                        // Peer is sending an offer while we think we're Complete. This means either:
                                        // 1. Same HQC prefix: peer missed our KEM response (can't re-send without keypairs)
                                        // 2. Different HQC prefix: peer lost chains, wants re-key
                                        //
                                        // Since we have NO keypairs here (we're in the is_none branch), we can't re-respond even to the same offer. Accept as re-key.
                                        //
                                        // Note: If peer keeps re-sending same offer, both sides will eventually converge on a fresh ceremony (peer will regenerate keys after timeout).
                                        crate::log(&format!(
                                            "CLUTCH: Received offer from {} while Complete - peer lost chains, accepting re-key",
                                            crate::fp(&contact.handle_proof)
                                        ));
                                        // Delete our old chains - they're useless now
                                        if let Some(fid) = contact.friendship_id {
                                            chains_to_remove.push(fid);
                                        }
                                        // Reset ALL CLUTCH state for new ceremony
                                        contact.clutch_state = ClutchState::Pending;
                                        contact.friendship_id = None;
                                        contact.completed_their_hqc_prefix = None;
                                        contact.clutch_our_keypairs = None;
                                        contact.clutch_slots.clear();
                                        contact.ceremony_id = None;
                                        contact.offer_provenances.clear(); // Clear for fresh ceremony nonce
                                        contact.clutch_pending_kem = None;
                                        contact.clutch_offer_sent = false;
                                        contact.clutch_our_eggs_proof = None;
                                        contact.clutch_their_eggs_proof = None;
                                        // Re-initialize slots and store their offer (was stored earlier but we just cleared)
                                        contact.init_clutch_slots(our_handle_hash);
                                        if let Some(slot) = contact.get_slot_mut(&their_handle_hash)
                                        {
                                            slot.offer = Some(their_offer.clone());
                                        }
                                        // Store their offer_provenance (was cleared, need to re-add)
                                        if !contact.offer_provenances.contains(&offer_provenance) {
                                            contact.offer_provenances.push(offer_provenance);
                                        }

                                        // Persist re-key state immediately
                                        if let Some(storage) = self.storage.as_ref() {
                                            if let Err(e) =
                                                crate::storage::contacts::save_clutch_slots(
                                                    &contact.clutch_slots,
                                                    &contact.offer_provenances,
                                                    contact.ceremony_id,
                                                    &contact.handle_hash,
                                                    storage,
                                                )
                                            {
                                                crate::log(&format!(
                                                    "Failed to persist re-key CLUTCH state: {}",
                                                    e
                                                ));
                                            }
                                        }

                                        // Trigger keygen for fresh re-key ceremony
                                        contact.clutch_keygen_in_progress = true;
                                        rekey_request =
                                            Some((contact.id.clone(), contact.handle_hash));
                                    } else if contact.clutch_state == ClutchState::AwaitingProof {
                                        // We're waiting for their proof, but they sent an offer. Check if same keys (retransmit) or different (peer reset)
                                        let their_slot = contact.get_slot(&their_handle_hash);
                                        let stored_hqc = their_slot
                                            .and_then(|s| s.offer.as_ref())
                                            .map(|o| &o.hqc256_public);
                                        let is_same_keys = stored_hqc
                                            .map(|h| h == &their_offer.hqc256_public)
                                            .unwrap_or(false);

                                        if is_same_keys {
                                            crate::log(&format!(
                                                "CLUTCH: Ignoring retransmit from {} (already AwaitingProof)",
                                                crate::fp(&contact.handle_proof)
                                            ));
                                            break;
                                        }

                                        // Different keys = peer reset. Clear their slot and reset to Pending.
                                        crate::log(&format!(
                                            "CLUTCH: Peer {} reset while we were AwaitingProof - resetting",
                                            crate::fp(&contact.handle_proof)
                                        ));
                                        if let Some(slot) = contact.get_slot_mut(&their_handle_hash)
                                        {
                                            slot.offer = None;
                                            slot.kem_secrets_from_them = None;
                                        }
                                        contact.clutch_state = ClutchState::Pending;
                                        contact.clutch_offer_sent = false;
                                        contact.ceremony_id = None;
                                        contact.clutch_our_eggs_proof = None;
                                        contact.clutch_their_eggs_proof = None;
                                        // Remove their old provenance (keep ours)
                                        contact
                                            .offer_provenances
                                            .retain(|p| p != &offer_provenance);
                                        // Fall thru - normal flow will store new offer and trigger keygen
                                    } else {
                                        crate::log(&format!(
                                            "CLUTCH: Received offer from {} but no keypairs (state={:?}) - triggering keygen",
                                            crate::fp(&contact.handle_proof), contact.clutch_state
                                        ));
                                        contact.clutch_keygen_in_progress = true;
                                        rekey_request =
                                            Some((contact.id.clone(), contact.handle_hash));
                                    }
                                }
                            }
                            break;
                        }
                    }

                    // Remove invalidated chains from memory and disk
                    for old_id in chains_to_remove {
                        // Scrub the doomed chains' history key before dropping them (re-key path — the fresh ceremony derives its own).
                        for (id, chains) in self.friendship_chains.iter_mut() {
                            if *id == old_id {
                                chains.zeroize_history_key();
                            }
                        }
                        self.friendship_chains.retain(|(id, _)| *id != old_id);
                        // Delete from disk
                        if let Some(storage) = self.storage.as_ref() {
                            if let Err(e) = crate::storage::friendship::delete_friendship_chains(
                                &old_id, storage,
                            ) {
                                crate::log(&format!("CLUTCH: Failed to delete old chains: {}", e));
                            }
                        }
                    }

                    // Spawn re-key keygen after releasing mutable borrow
                    if let Some((contact_id, their_handle_hash)) = rekey_request {
                        self.spawn_clutch_keygen(contact_id, our_identity_seed, their_handle_hash);
                    }

                    // Spawn deferred KEM encapsulation after releasing mutable borrow
                    if let Some((contact_id, offer, ceremony_id, conv_token, peer_addr)) =
                        kem_encap_spawn
                    {
                        self.spawn_clutch_kem_encap(
                            contact_id,
                            offer,
                            ceremony_id,
                            conv_token,
                            peer_addr,
                        );
                    }
                }

                // CLUTCH KEM response received (~31KB with 4 ciphertexts) Payload is already parsed and signature verified by status.rs
                StatusUpdate::ClutchKemResponseReceived {
                    conversation_token,
                    ceremony_id: received_ceremony_id,
                    sender_pubkey,
                    payload,
                    sender_addr: raw_sender_addr,
                } => {
                    use crate::crypto::clutch::{
                        derive_conversation_token, ClutchKemSharedSecrets,
                    };

                    // Normalize to port 4383 (TCP source port is ephemeral)
                    let sender_addr =
                        std::net::SocketAddr::new(raw_sender_addr.ip(), crate::PHOTON_PORT);

                    // Get our handle_hash
                    let our_handle_hash = match self.session.as_ref().map(|s| s.identity_seed) {
                        Some(h) => h,
                        None => {
                            #[cfg(feature = "development")]
                            #[cfg(feature = "development")]
                            crate::log("CLUTCH: No user_identity_seed available");
                            continue;
                        }
                    };
                    let our_sibling_pid = self.our_sibling_pid();

                    // Find contact by conversation_token. Party-id seam: sibling candidates token with the device-derived pid pair; the resolved "our" id shadows the seed for the whole arm.
                    let (their_handle_hash, our_handle_hash) = match self
                        .contacts
                        .iter()
                        .find_map(|c| {
                            let our = if c.is_sibling {
                                our_sibling_pid?
                            } else {
                                our_handle_hash
                            };
                            (derive_conversation_token(&[our, c.handle_hash])
                                == conversation_token)
                                .then_some((c.handle_hash, our))
                        }) {
                        Some(pair) => pair,
                        None => {
                            crate::log(&format!(
                                "CLUTCH: Received KEM response with unknown conversation_token {}",
                                hex::encode(&conversation_token[..8])
                            ));
                            continue;
                        }
                    };

                    crate::log(&format!(
                        "CLUTCH: Received KEM response (VSF verified) from {} tok={}...",
                        sender_addr,
                        hex::encode(&conversation_token[..8])
                    ));

                    // Gate: sender must be a currently-trusted device of this contact (fold-respecting). See the offer gate for the widen/revoke rationale.
                    let sender_known = self
                        .contacts
                        .iter()
                        .find(|c| c.handle_hash == their_handle_hash)
                        .map(|c| c.knows_device(&sender_pubkey));
                    match sender_known {
                        None => {
                            #[cfg(feature = "development")]
                            #[cfg(feature = "development")]
                            crate::log("CLUTCH: Received KEM response from unknown contact");
                            continue;
                        }
                        Some(false) => {
                            crate::log(&format!(
                                "CLUTCH: KEM from untrusted/removed device {} — dropping",
                                hex::encode(&sender_pubkey[..8])
                            ));
                            continue;
                        }
                        Some(true) => {}
                    }

                    // The payload is already parsed
                    let their_kem = payload;

                    // Find contact by handle_hash
                    for (idx, contact) in self.contacts.iter_mut().enumerate() {
                        if contact.handle_hash == their_handle_hash {
                            contact.ip = Some(sender_addr);
                            // Authenticated CLUTCH traffic from them ⇒ reachable right now ⇒ show online immediately, don't wait for the next pong.
                            if !contact.is_online {
                                contact.is_online = true;
                                changed = true;
                                crate::log(&format!(
                                    "Status: {} is now ONLINE (CLUTCH)",
                                    crate::fp(&contact.handle_proof)
                                ));
                            }

                            // Verify ceremony_id matches (if we have one)
                            if let Some(our_ceremony_id) = contact.ceremony_id {
                                if received_ceremony_id != our_ceremony_id {
                                    crate::log(&format!(
                                        "CLUTCH: ceremony_id mismatch! Received {:02x}{:02x}..., expected {:02x}{:02x}...",
                                        received_ceremony_id[0], received_ceremony_id[1],
                                        our_ceremony_id[0], our_ceremony_id[1]
                                    ));
                                    continue;
                                }
                            } else {
                                // No ceremony_id yet - check if we have keypairs and if KEM targets them This happens when keypairs are loaded from disk but offers not yet exchanged
                                if let Some(our_keys_cloned) = contact.clutch_our_keypairs.clone() {
                                    let our_hqc_prefix: [u8; 8] =
                                        our_keys_cloned.hqc256_public[..8].try_into().unwrap();
                                    let all_zeros = their_kem.target_hqc_pub_prefix == [0u8; 8];
                                    if !all_zeros
                                        && their_kem.target_hqc_pub_prefix != our_hqc_prefix
                                    {
                                        // KEM targets different keys - truly stale, discard
                                        crate::log(&format!(
                                            "CLUTCH: KEM response from {} targets old keys (HQC {}) - discarding",
                                            crate::fp(&contact.handle_proof),
                                            hex::encode(&their_kem.target_hqc_pub_prefix)
                                        ));
                                        break;
                                    }
                                    // KEM targets our current keys but we don't have ceremony_id yet — which means we haven't processed THEIR offer yet (ceremony_id derives from both offers). We can't complete without their offer (no offer → can't encapsulate our KEM → our slot never completes). QUEUE the KEM and wait for their offer to arrive; the ClutchOfferReceived path drains clutch_pending_kem once both offers are in and ceremony_id is derived. Their offer is a reliable PT stream now, so it WILL arrive — no deadlock. (The old "adopt ceremony_id + decapsulate + break" shortcut left our own slot incomplete and hung CLUTCH Pending forever.)
                                    let _ = (our_keys_cloned, received_ceremony_id);
                                    crate::log(&format!(
                                        "CLUTCH: KEM from {} arrived before their offer/ceremony_id - queuing until offer arrives",
                                        crate::fp(&contact.handle_proof)
                                    ));
                                    contact.clutch_pending_kem = Some(their_kem.clone());
                                    break;
                                } else {
                                    // No keypairs at all - stale KEM encrypted to unknown keys
                                    crate::log(&format!(
                                        "CLUTCH: KEM response from {} arrived before keygen - discarding (encrypted to old keys)",
                                        crate::fp(&contact.handle_proof)
                                    ));
                                    break;
                                }
                            }

                            // Initialize slots if needed
                            if contact.clutch_slots.is_empty() {
                                contact.init_clutch_slots(our_handle_hash);
                            }

                            // Verify KEM response targets our CURRENT HQC public key This prevents panics from stale KEM responses encrypted to old keys
                            if let Some(ref our_keys) = contact.clutch_our_keypairs {
                                let our_hqc_prefix: [u8; 8] =
                                    our_keys.hqc256_public[..8].try_into().unwrap();
                                let all_zeros = their_kem.target_hqc_pub_prefix == [0u8; 8];
                                if !all_zeros && their_kem.target_hqc_pub_prefix != our_hqc_prefix {
                                    crate::log(&format!(
                                        "CLUTCH: Stale KEM response from {} - target HQC {} != our HQC {} (discarding)",
                                        crate::fp(&contact.handle_proof),
                                        hex::encode(&their_kem.target_hqc_pub_prefix),
                                        hex::encode(&our_hqc_prefix)
                                    ));
                                    break;
                                }
                            }

                            // Decapsulate remote KEM response using local secret keys
                            if let Some(ref local_keys) = contact.clutch_our_keypairs {
                                let remote_secrets = ClutchKemSharedSecrets::decapsulate_from_peer(
                                    &their_kem, local_keys,
                                );

                                // Store in remote slot (secrets from remote to local)
                                if let Some(slot) = contact.get_slot_mut(&their_handle_hash) {
                                    slot.kem_secrets_from_them = Some(remote_secrets);
                                    crate::log(&format!(
                                        "CLUTCH: Decapsulated KEM from {} - stored in slot",
                                        crate::fp(&contact.handle_proof)
                                    ));
                                }

                                // Backfill OUR offer in OUR slot if missing — guarantees all_slots_complete can fire here. Covers the stall where our own offer was never recorded (offer arrived before our keygen, or the offer-received path didn't store it), leaving our slot offer=None forever even though we have keys + KEM secrets.
                                if contact
                                    .get_slot(&our_handle_hash)
                                    .map(|s| s.offer.is_none())
                                    .unwrap_or(false)
                                {
                                    if let Some(ref keypairs) = contact.clutch_our_keypairs {
                                        let our_offer = crate::crypto::clutch::ClutchOfferPayload::from_keypairs(keypairs);
                                        if let Some(local_slot) =
                                            contact.get_slot_mut(&our_handle_hash)
                                        {
                                            local_slot.offer = Some(our_offer);
                                            crate::log("CLUTCH: Backfilled our own offer in local slot (on KEM received)");
                                        }
                                    }
                                }

                                // Persist slot state after receiving KEM
                                if let Some(storage) = self.storage.as_ref() {
                                    if let Err(e) = crate::storage::contacts::save_clutch_slots(
                                        &contact.clutch_slots,
                                        &contact.offer_provenances,
                                        contact.ceremony_id,
                                        &contact.handle_hash,
                                        storage,
                                    ) {
                                        crate::log(&format!(
                                            "CLUTCH: Failed to save slots for {}: {}",
                                            crate::fp(&contact.handle_proof), e
                                        ));
                                    }
                                }
                                changed = true;

                                // Check if ceremony is complete (defer to after outer loop)
                                if contact.all_slots_complete() {
                                    ceremony_completions.push(idx);
                                    changed = true;
                                } else {
                                    // Debug: why isn't ceremony complete after KEM response?
                                    crate::log(&format!(
                                        "CLUTCH: Slots not complete after KEM response for {} - checking state:",
                                        crate::fp(&contact.handle_proof)
                                    ));
                                    for (i, slot) in contact.clutch_slots.iter().enumerate() {
                                        crate::log(&format!(
                                            "  Slot {}: offer={} from_them={} to_them={}",
                                            i,
                                            slot.offer.is_some(),
                                            slot.kem_secrets_from_them.is_some(),
                                            slot.kem_secrets_to_them.is_some()
                                        ));
                                    }
                                }
                            } else {
                                crate::log(&format!(
                                    "CLUTCH: Received KEM response but no keypairs for {}",
                                    crate::fp(&contact.handle_proof)
                                ));
                            }
                            break;
                        }
                    }
                }

                // CLUTCH complete proof received (~200 bytes with eggs_proof) Both parties exchange this to verify they derived identical eggs
                StatusUpdate::ClutchCompleteReceived {
                    conversation_token,
                    ceremony_id: _received_ceremony_id,
                    sender_pubkey,
                    payload,
                    sender_addr: raw_sender_addr,
                } => {
                    use crate::crypto::clutch::derive_conversation_token;
                    use crate::types::ClutchState;

                    // Normalize to port 4383 (TCP source port is ephemeral)
                    let sender_addr =
                        std::net::SocketAddr::new(raw_sender_addr.ip(), crate::PHOTON_PORT);

                    crate::log(&format!(
                        "CLUTCH: Received complete proof (VSF verified) from {} proof={}...",
                        sender_addr,
                        hex::encode(&payload.eggs_proof[..8])
                    ));

                    // Find contact by conversation_token. Party-id seam: sibling candidates token with the device-derived pid pair. (Our id isn't needed downstream — completion derives it internally — so it's discarded.)
                    let our_sibling_pid = self.our_sibling_pid();
                    let (their_handle_hash, _our_handle_hash) = match self
                        .contacts
                        .iter()
                        .find_map(|c| {
                            let our = if c.is_sibling {
                                our_sibling_pid?
                            } else {
                                our_handle_hash
                            };
                            (derive_conversation_token(&[our, c.handle_hash])
                                == conversation_token)
                                .then_some((c.handle_hash, our))
                        }) {
                        Some(pair) => pair,
                        None => {
                            crate::log(&format!(
                                "CLUTCH: Received complete proof with unknown conversation_token {}",
                                hex::encode(&conversation_token[..8])
                            ));
                            continue;
                        }
                    };

                    // Gate: sender must be a currently-trusted device of this contact (fold-respecting). See the offer gate for the widen/revoke rationale.
                    let sender_known = self
                        .contacts
                        .iter()
                        .find(|c| c.handle_hash == their_handle_hash)
                        .map(|c| c.knows_device(&sender_pubkey));
                    match sender_known {
                        None => {
                            #[cfg(feature = "development")]
                            #[cfg(feature = "development")]
                            crate::log("CLUTCH: Received proof from unknown contact");
                            continue;
                        }
                        Some(false) => {
                            crate::log(&format!(
                                "CLUTCH: proof from untrusted/removed device {} — dropping",
                                hex::encode(&sender_pubkey[..8])
                            ));
                            continue;
                        }
                        Some(true) => {}
                    }

                    // Find contact and process proof
                    let mut newly_complete_idx: Option<usize> = None;
                    for (contact_idx, contact) in self.contacts.iter_mut().enumerate() {
                        if contact.handle_hash == their_handle_hash {
                            contact.ip = Some(sender_addr);
                            // Authenticated CLUTCH traffic from them ⇒ reachable right now ⇒ show online immediately, don't wait for the next pong.
                            if !contact.is_online {
                                contact.is_online = true;
                                changed = true;
                                crate::log(&format!(
                                    "Status: {} is now ONLINE (CLUTCH)",
                                    crate::fp(&contact.handle_proof)
                                ));
                            }
                            // We just received + VSF-verified an authenticated message from them — they're reachable NOW, so reflect online immediately instead of waiting for the next pong (fixes "CLUTCH completed but still shows offline").
                            if !contact.is_online {
                                contact.is_online = true;
                                changed = true;
                                crate::log(&format!(
                                    "Status: {} is now ONLINE (CLUTCH complete)",
                                    crate::fp(&contact.handle_proof)
                                ));
                            }

                            match contact.clutch_state {
                                ClutchState::AwaitingProof => {
                                    // We have our proof - verify theirs matches
                                    if let Some(our_proof) = contact.clutch_our_eggs_proof {
                                        if payload.eggs_proof == our_proof {
                                            // SUCCESS! Both parties computed same eggs
                                            crate::log(&format!(
                                                "CLUTCH: Proof verified with {}! ✓ proof={}...",
                                                crate::fp(&contact.handle_proof),
                                                hex::encode(&our_proof[..8])
                                            ));
                                            contact.clutch_state = ClutchState::Complete;
                                            contact.clutch_completed_at = Some(std::time::Instant::now()); // arm the post-completion re-key cooldown (before the ~1s-later weave)
                                            // Fresh ceremony = fresh chain: void any prior weave seal so the probe refires (see the twin reset at the Early-proof-verified site for the full deadlock story).
                                            contact.chain_woven = false;
                                            contact.probe_sent = false;
                                            contact.their_probe_seen = false;
                                            contact.chain_advanced_by_ack = false;
                                            newly_complete_idx = Some(contact_idx);
                                            // Store their HQC pub prefix to detect stale offers after restart
                                            if let Some(their_slot) =
                                                contact.get_slot(&contact.handle_hash)
                                            {
                                                if let Some(ref their_offer) = their_slot.offer {
                                                    let prefix: [u8; 8] = their_offer.hqc256_public
                                                        [..8]
                                                        .try_into()
                                                        .unwrap_or_default();
                                                    contact.completed_their_hqc_prefix =
                                                        Some(prefix);
                                                }
                                            }
                                            // Keep our proof + resend budget: we just verified theirs, but ours may still be in flight or dropped. ping_contacts drains the budget over the next few cycles, then clears it — so neither side strands.
                                            contact.clutch_their_eggs_proof = None;
                                            changed = true;

                                            // NOTE: Don't clear PT sends here - our ClutchComplete proof might still be in flight to them. Let it finish.

                                            // Save Complete state to disk immediately
                                            if let Some(storage) = self.storage.as_ref() {
                                                if let Err(e) =
                                                    crate::storage::contacts::save_contact(
                                                        contact, storage,
                                                    )
                                                {
                                                    crate::log(&format!(
                                                        "Failed to save Complete state: {}",
                                                        e
                                                    ));
                                                } else {
                                                    crate::log(&format!(
                                                        "CLUTCH: Saved {} Complete state to disk",
                                                        crate::fp(&contact.handle_proof)
                                                    ));
                                                }
                                            }
                                        } else {
                                            // Proof mismatch — NEVER panic. The common cause is a CROSSED / SUPERSEDED ceremony round: a fresh offer/KEM arrived after we computed our eggs, so their proof is from a different ceremony instance than ours. It is not an attack signal (a forged proof can't pass the read_verified signature gate that got us here). Reset to Pending so the re-key machinery re-establishes ONE agreed ceremony — identical to the mismatch handling in check_clutch_ceremonies.
                                            crate::log(&format!(
                                                "CLUTCH: proof mismatch with {} (likely a crossed/superseded round) — resetting to re-key. ours={}... theirs={}...",
                                                crate::fp(&contact.handle_proof),
                                                hex::encode(&our_proof[..8]),
                                                hex::encode(&payload.eggs_proof[..8])
                                            ));
                                            contact.clutch_state = ClutchState::Pending;
                                            contact.clutch_our_eggs_proof = None;
                                            contact.clutch_their_eggs_proof = None;
                                            changed = true;
                                        }
                                    } else {
                                        // Race condition: proof arrived before check_clutch_ceremonies processed our ceremony result. Store theirs for when we're ready.
                                        crate::log(&format!(
                                            "CLUTCH: Storing early proof from {} (AwaitingProof but our result not processed yet)",
                                            crate::fp(&contact.handle_proof)
                                        ));
                                        contact.clutch_their_eggs_proof = Some(payload.eggs_proof);
                                        changed = true;
                                    }
                                }
                                ClutchState::Pending => {
                                    // We haven't computed our proof yet - store theirs for later
                                    crate::log(&format!(
                                        "CLUTCH: Storing early proof from {} (we're still in Pending)",
                                        crate::fp(&contact.handle_proof)
                                    ));
                                    contact.clutch_their_eggs_proof = Some(payload.eggs_proof);
                                    changed = true;
                                }
                                ClutchState::Complete => {
                                    // We're Complete but the peer is STILL sending its proof — that means our ClutchComplete never reached them (a dropped proof strands them in AwaitingProof forever, since we'd otherwise ignore the duplicate). Treat the duplicate as an implicit re-request: re-arm our proof-resend budget so the next ping cycle re-sends our ClutchComplete. This is the recovery half of the asymmetric-completion bug (the other half is the AwaitingProof side re-sending its proof while the peer is online).
                                    if contact.chain_woven {
                                        // The chain is proven end-to-end (probe exchanged + ACKed), so a duplicate proof is just late network echo — stop rebroadcasting.
                                        crate::log(&format!(
                                            "CLUTCH: Ignoring duplicate proof from {} — chain already woven, rebroadcast retired",
                                            crate::fp(&contact.handle_proof)
                                        ));
                                    } else if contact.clutch_our_eggs_proof.is_some()
                                        && contact.ceremony_id.is_some()
                                    {
                                        contact.clutch_proof_resends_left = 5;
                                        changed = true;
                                        crate::log(&format!(
                                            "CLUTCH: Re-arming proof resend to {} — they re-sent their proof (our ClutchComplete was likely lost)",
                                            crate::fp(&contact.handle_proof)
                                        ));
                                    } else {
                                        crate::log(&format!(
                                            "CLUTCH: Duplicate proof from {} but our proof/ceremony cleared — cannot re-send",
                                            crate::fp(&contact.handle_proof)
                                        ));
                                    }
                                }
                            }
                            break;
                        }
                    }
                    // If this proof took the contact to Complete, fire the one hidden chain-weave probe — deferred past the outer `checker` borrow like the other helpers.
                    if let Some(idx) = newly_complete_idx {
                        chain_probe_indices.push(idx);
                    }
                }

                // LAN peer discovered via broadcast (NAT hairpinning workaround)
                StatusUpdate::LanPeerDiscovered {
                    handle_proof,
                    local_ip,
                    port,
                } => {
                    // Find contact by handle_proof and store their LAN IP + port. Siblings AND the self-contact are skipped — an own-hp broadcast carries only (hp, port) with no device disambiguation, so it can't say WHICH of our devices it came from; sibling addresses flow via FGTW peer rows + pong source addresses instead.
                    for (idx, contact) in self.contacts.iter_mut().enumerate() {
                        if !contact.is_sibling
                            && contact.handle_hash != our_handle_hash
                            && contact.handle_proof == handle_proof
                        {
                            let old_local = contact.local_ip;
                            let old_port = contact.local_port;
                            contact.local_ip = Some(local_ip);
                            contact.local_port = Some(port);
                            if old_local != Some(local_ip) || old_port != Some(port) {
                                crate::log(&format!(
                                    "LAN: Discovered {} at local {}:{}",
                                    crate::fp(&contact.handle_proof), local_ip, port
                                ));
                                // Ping immediately so we don't wait for next scheduled cycle
                                lan_ping_indices.push(idx);
                                changed = true;
                            }
                            break;
                        }
                    }
                }
                // A peer asked for our avatar. Policy: reply ONLY if they are a MUTUAL contact — i.e. a completed CLUTCH ceremony, which is cryptographically impossible unless both added each other. A friend gets our avatar straight from us; anyone else is ignored (they fall back to FGTW, or get nothing). We reply with our OWN avatar VSF bytes.
                StatusUpdate::AvatarRequestReceived {
                    sender_pubkey,
                    sender_addr,
                } => {
                    let is_mutual = self
                        .contacts
                        .iter()
                        .any(|c| c.knows_device(&sender_pubkey.key) && c.is_mutual());
                    if !is_mutual {
                        crate::log(
                            "Avatar: ignoring avatar request from a non-mutual peer (not Complete)",
                        );
                    } else if let (Some(session), Some(storage), Some(checker)) = (
                        self.session.as_ref(),
                        self.storage.as_ref(),
                        self.status_checker.as_ref(),
                    ) {
                        // Read our own avatar VSF straight from the vault (the same blob we publish to FGTW). No avatar stored → nothing to send; the peer falls back to FGTW.
                        match storage
                            .read_addr(&crate::storage::vault_key("avatar", &session.identity_seed))
                        {
                            Ok(Some(avatar_vsf)) => {
                                // Validate the vault bytes before we device-sign and ship them: an error frame or a body that doesn't decode would be signed as a "poisoned" avatar the friend then can't decode. Reject an error frame outright, and require a full verify+decrypt+decode against our own seed before serving.
                                let servable = fgtw::client::error_frame(&avatar_vsf).is_none()
                                    && crate::ui::avatar::load_avatar_from_bytes_from_seed(
                                        &avatar_vsf,
                                        &session.identity_seed,
                                    )
                                    .is_some();
                                if !servable {
                                    crate::log(
                                        "Avatar: local avatar bytes failed to validate, not serving to peer",
                                    );
                                } else {
                                    crate::log(&format!(
                                        "Avatar: sending our avatar to mutual peer ({} bytes)",
                                        avatar_vsf.len()
                                    ));
                                    checker.send_avatar_response(
                                        crate::network::status::AvatarResponseSend {
                                            peer_addr: sender_addr,
                                            recipient_pubkey: *sender_pubkey.as_bytes(),
                                            avatar_vsf,
                                        },
                                    );
                                }
                            }
                            _ => crate::log("Avatar: mutual peer requested avatar, but we have none"),
                        }
                    }
                }
                // A peer sent us their avatar. Policy: install ONLY if the responder is a MUTUAL (Complete) contact — otherwise anyone could push us an arbitrary avatar. The wire layer already verified the bytes are signed by responder_pubkey; here we bind that pubkey to a friendship before trusting it. Decode + cache + install on that contact.
                StatusUpdate::AvatarReceived {
                    responder_pubkey,
                    avatar_vsf,
                    sender_addr: _,
                } => {
                    let target = self
                        .contacts
                        .iter()
                        .position(|c| c.knows_device(&responder_pubkey.key) && c.is_mutual());
                    match target {
                        None => crate::log(
                            "Avatar: ignoring avatar from a non-mutual peer (not a Complete contact)",
                        ),
                        Some(idx) => {
                            let party_id = self.contacts[idx].handle_hash;
                            let mut pin_key = [0u8; 32];
                            pin_key.copy_from_slice(&self.contacts[idx].avatar_pin[..32]);
                            // Decode the AVIF-in-VSF to display pixels with the PINNED key (same as an FGTW download under the pin-set).
                            match crate::ui::avatar::load_avatar_from_bytes_with_key(
                                &avatar_vsf,
                                &pin_key,
                            ) {
                                Some((_, vsf_rgb)) => {
                                    // Cache it (party-id scope) so a restart shows it without another round-trip.
                                    if let Some(storage) = self.storage.as_ref() {
                                        let _ = crate::ui::avatar::save_avatar_to_cache_from_seed(
                                            &party_id,
                                            &avatar_vsf,
                                            storage,
                                        );
                                    }
                                    let display =
                                        crate::ui::colour_convert::vsf_rgb_to_bt2020(&vsf_rgb);
                                    let contact = &mut self.contacts[idx];
                                    contact.avatar_pixels = Some(display);
                                    contact.avatar_scaled = None;
                                    contact.avatar_scaled_diameter = 0;
                                    changed = true;
                                    crate::log("Avatar: installed mutual peer's avatar (P2P)");
                                }
                                None => crate::log("Avatar: failed to decode peer avatar bytes"),
                            }
                        }
                    }
                }

                // A friend (post-reset / new device) is asking for conversation history. Serve one newest-first page from our rārangi rows, sealed under the friendship history key. Authorization is OURS to do (the RX worker only verified the signature): the signer must be a known device of the contact this conversation belongs to, and mutual.
                StatusUpdate::HistoryRequestReceived {
                    conversation_token,
                    before_osc,
                    limit,
                    request_id,
                    sent_osc,
                    sender_pubkey,
                    sender_addr,
                } => {
                    let now = vsf::eagle_time_oscillations();
                    // Staleness cap: a hist_req older than ~10 min is a replay or a badly delayed duplicate — pages are useless to an attacker (sealed) but serving costs us I/O.
                    const HIST_STALE_OSC: i64 = 600 * crate::OSC_PER_SEC;
                    let stale = sent_osc != 0 && now.saturating_sub(sent_osc) > HIST_STALE_OSC;

                    // Per-conversation dedup (rid) + cadence cap (≥500ms between served pages).
                    let entry = self
                        .history_serve
                        .entry(conversation_token)
                        .or_insert_with(|| (0, std::collections::VecDeque::new()));
                    let duplicate = entry.1.contains(&request_id);
                    let too_fast = now.saturating_sub(entry.0) < crate::OSC_PER_SEC / 2;

                    if !stale && !duplicate && !too_fast {
                        entry.0 = now;
                        entry.1.push_back(request_id);
                        while entry.1.len() > 8 {
                            entry.1.pop_front();
                        }

                        // Bind token → chains (history key) → the OTHER participant → contact, and require the requesting device to belong to that exact contact + be mutual.
                        let our_seed = self.session.as_ref().map(|s| s.identity_seed);
                        let key_and_other = self
                            .friendship_chains
                            .iter()
                            .find(|(_, c)| c.conversation_token == conversation_token)
                            .and_then(|(_, c)| {
                                let key = c.history_key().copied()?;
                                let other = c
                                    .participants()
                                    .iter()
                                    .find(|p| Some(**p) != our_seed)
                                    .copied()?;
                                Some((key, other))
                            });
                        let contact_idx = key_and_other.and_then(|(_, other)| {
                            self.contacts.iter().position(|c| {
                                // Friend chains only — a sibling chain's "other ≠ our seed" resolution is ambiguous (both participant pids differ from the seed); fleet history sync is its own later phase.
                                !c.is_sibling
                                    && c.handle_hash == other
                                    && c.knows_device(&sender_pubkey.key)
                                    && c.is_mutual()
                            })
                        });

                        if let (Some((key, _)), Some(idx), Some(storage), Some(checker)) = (
                            key_and_other,
                            contact_idx,
                            self.storage.as_ref(),
                            self.status_checker.as_ref(),
                        ) {
                            use crate::network::history_pages::{
                                seal_history_page, HistoryPagePlain, HistoryRow, MAX_PAGE_BYTES,
                                MAX_PAGE_ROWS,
                            };
                            let their_seed = self.contacts[idx].handle_hash;
                            let page_limit = (limit as usize).clamp(1, MAX_PAGE_ROWS);
                            match crate::storage::contacts::load_message_page_before(
                                &their_seed,
                                before_osc,
                                page_limit,
                                MAX_PAGE_BYTES,
                                storage,
                            ) {
                                Ok((rows, more)) => {
                                    // Cursor progresses over ALL returned rows (probe rows included) so a probe-heavy stretch can't stall the walk; the probe rows themselves are filtered out of what we ship.
                                    let oldest_osc =
                                        rows.first().map(|m| m.timestamp).unwrap_or(before_osc);
                                    let hist_rows: Vec<HistoryRow> = rows
                                        .iter()
                                        .filter(|m| m.content != crate::types::CHAIN_PROBE_MARKER)
                                        .map(|m| HistoryRow {
                                            timestamp: m.timestamp,
                                            content: m.content.clone(),
                                            sender_outgoing: m.is_outgoing,
                                            delivered: m.delivered,
                                        })
                                        .collect();
                                    let page = HistoryPagePlain {
                                        rows: hist_rows,
                                        oldest_osc,
                                        more,
                                    };
                                    let device_pubkey = *self
                                        .device_keypair
                                        .as_ref()
                                        .expect("device_keypair set in init")
                                        .public
                                        .as_bytes();
                                    let device_secret = *self
                                        .device_keypair
                                        .as_ref()
                                        .expect("device_keypair set in init")
                                        .secret
                                        .as_bytes();
                                    match seal_history_page(&page, &key).and_then(|sealed| {
                                        crate::network::fgtw::protocol::build_history_page_vsf(
                                            &conversation_token,
                                            &request_id,
                                            sealed,
                                            &device_pubkey,
                                            &device_secret,
                                        )
                                    }) {
                                        Ok(vsf_bytes) => {
                                            crate::log(&format!(
                                                "HISTORY: serving page ({} rows, more={}) to {}",
                                                page.rows.len(),
                                                page.more,
                                                sender_addr
                                            ));
                                            checker.send_history(
                                                crate::network::status::HistorySendRequest {
                                                    peer_addr: sender_addr,
                                                    alt_addr: None,
                                                    recipient_pubkey: *sender_pubkey.as_bytes(),
                                                    vsf_bytes,
                                                },
                                            );
                                        }
                                        Err(e) => crate::log(&format!(
                                            "HISTORY: page build failed: {e}"
                                        )),
                                    }
                                }
                                Err(e) => {
                                    crate::log(&format!("HISTORY: page read failed: {e}"))
                                }
                            }
                        } else {
                            crate::log(
                                "HISTORY: request rejected (no key / unknown device / not mutual)",
                            );
                        }
                    }
                }

                // A history page arrived for our recovery. Open it with the friendship history key, merge rows (direction flipped, friend-attested), advance the cursor, persist.
                StatusUpdate::HistoryPageReceived {
                    conversation_token,
                    request_id,
                    sealed,
                    sender_pubkey: _,
                    sender_addr: _,
                } => {
                    let our_seed = self.session.as_ref().map(|s| s.identity_seed);
                    let key_and_other = self
                        .friendship_chains
                        .iter()
                        .find(|(_, c)| c.conversation_token == conversation_token)
                        .and_then(|(_, c)| {
                            let key = c.history_key().copied()?;
                            let other = c
                                .participants()
                                .iter()
                                .find(|p| Some(**p) != our_seed)
                                .copied()?;
                            Some((key, other))
                        });
                    let contact_idx = key_and_other.and_then(|(_, other)| {
                        self.contacts.iter().position(|c| c.handle_hash == other)
                    });

                    if let (Some((key, _)), Some(idx)) = (key_and_other, contact_idx) {
                        // rid must match our in-flight request — a page we didn't ask for (or asked for long ago) is dropped; merging is idempotent so a raced duplicate that DOES match is harmless.
                        let rid_matches = self.contacts[idx]
                            .history_recovery
                            .as_ref()
                            .and_then(|r| r.in_flight.as_ref())
                            .is_some_and(|(rid, _, _)| *rid == request_id);
                        if rid_matches {
                            match crate::network::history_pages::open_history_page(&sealed, &key) {
                                Ok(page) => {
                                    let contact = &mut self.contacts[idx];
                                    // Merge: flip direction to OUR perspective; recovered-outgoing is delivered by definition (the friend has it); dedup on (timestamp, content) against what we already hold.
                                    let mut fresh: Vec<crate::types::ChatMessage> = Vec::new();
                                    for row in &page.rows {
                                        if row.content == crate::types::CHAIN_PROBE_MARKER {
                                            continue;
                                        }
                                        let is_outgoing = !row.sender_outgoing;
                                        let already = contact
                                            .messages
                                            .iter()
                                            .any(|m| {
                                                m.timestamp == row.timestamp
                                                    && m.content == row.content
                                            });
                                        if already {
                                            continue;
                                        }
                                        let msg = crate::types::ChatMessage {
                                            content: row.content.clone(),
                                            timestamp: row.timestamp,
                                            is_outgoing,
                                            delivered: is_outgoing,
                                            ack_hash: None,
                                            recovered: true,
                                        };
                                        contact.insert_message_sorted(msg.clone());
                                        fresh.push(msg);
                                    }

                                    // Cursor + completion. Early-stop: if history was already complete before this (re-)kickoff and the page brought nothing new, we're still complete — a routine re-key on an intact pair stops after one page instead of re-walking years.
                                    let their_seed = contact.handle_hash;
                                    if let Some(rec) = contact.history_recovery.as_mut() {
                                        rec.in_flight = None;
                                        if page.oldest_osc < rec.oldest_recovered_osc {
                                            rec.oldest_recovered_osc = page.oldest_osc;
                                        }
                                        if !page.more
                                            || (rec.was_complete_before && fresh.is_empty())
                                        {
                                            rec.complete = true;
                                        }
                                    }
                                    crate::log(&format!(
                                        "HISTORY: merged page ({} new of {} rows, more={}, complete={})",
                                        fresh.len(),
                                        page.rows.len(),
                                        page.more,
                                        contact
                                            .history_recovery
                                            .as_ref()
                                            .is_some_and(|r| r.complete)
                                    ));

                                    // Persist the new rows + the cursor (AGENT.md: every change hits disk).
                                    if let Some(storage) = self.storage.as_ref() {
                                        if !fresh.is_empty() {
                                            if let Err(e) = crate::storage::contacts::save_messages_page(
                                                &their_seed,
                                                &fresh,
                                                storage,
                                            ) {
                                                crate::log(&format!(
                                                    "HISTORY: page persist failed: {e}"
                                                ));
                                            }
                                        }
                                        let contact_ref = &self.contacts[idx];
                                        if let Err(e) =
                                            crate::storage::contacts::save_contact(contact_ref, storage)
                                        {
                                            crate::log(&format!(
                                                "HISTORY: cursor persist failed: {e}"
                                            ));
                                        }
                                    }
                                    changed = true;
                                }
                                Err(e) => {
                                    crate::log(&format!("HISTORY: page open failed ({e}) — dropped"))
                                }
                            }
                        }
                    }
                }

                StatusUpdate::BlindFrameReceived {
                    kind,
                    conversation_token,
                    request_id,
                    blob,
                    found,
                    sent_osc,
                    sender_pubkey,
                    sender_addr,
                } => {
                    use crate::network::fgtw::protocol::BlindFrameKind;

                    // Staleness: an old frame is a replay/duplicate — drop before any state change.
                    let now = vsf::eagle_time_oscillations();
                    const BLIND_STALE_OSC: i64 = 600 * crate::OSC_PER_SEC;
                    if sent_osc != 0 && now.saturating_sub(sent_osc) > BLIND_STALE_OSC {
                        continue;
                    }
                    let Some(our_seed) = self.session.as_ref().map(|s| s.identity_seed) else {
                        continue;
                    };

                    match kind {
                        // A friend's device deposits its blind with us (or asks for it back) — or a FLEET SIBLING asks for S over its own token. Authorization for all: the token must resolve to a contact AND the signer must be a device we trust for it AND the relationship must be mutual (for a sibling that means the exact device + Complete ceremony).
                        BlindFrameKind::Put | BlindFrameKind::Get => {
                            let our_sibling_pid = self.our_sibling_pid();
                            let cidx = self.contacts.iter().position(|c| {
                                let our = if c.is_sibling {
                                    match our_sibling_pid {
                                        Some(p) => p,
                                        None => return false,
                                    }
                                } else {
                                    our_seed
                                };
                                c.is_mutual()
                                    && c.knows_device(&sender_pubkey.key)
                                    && crate::crypto::clutch::derive_conversation_token(&[
                                        our,
                                        c.handle_hash,
                                    ]) == conversation_token
                            });
                            let Some(idx) = cidx else {
                                crate::log("BLIND: put/get REJECTED (unknown token or unauthorized device)");
                                continue;
                            };

                            if kind == BlindFrameKind::Put && self.contacts[idx].is_sibling {
                                // Siblings never deposit OTP blinds (they serve S directly) — a put on a sibling token is a protocol violation.
                                crate::log("BLIND: put on a sibling token REJECTED");
                                continue;
                            }
                            if kind == BlindFrameKind::Get && self.contacts[idx].is_sibling {
                                // Sibling S-transfer: serve S sealed under the sibling chains' history key — only when OUR S is Live (a provisional S has no durable recovery anchor yet; the sibling keeps probing and adopts once we're Live, or generates if everyone misses).
                                let blob_opt: Option<Vec<u8>> =
                                    self.private_s.live().and_then(|(s, _)| {
                                        let fid = self.contacts[idx].friendship_id?;
                                        let (_, chains) = self
                                            .friendship_chains
                                            .iter()
                                            .find(|(id, _)| *id == fid)?;
                                        let key = chains.history_key().copied()?;
                                        crate::crypto::blind::seal_sibling_s(s, &key).ok()
                                    });
                                if let (Some(kp), Some(checker)) =
                                    (self.device_keypair.as_ref(), self.status_checker.as_ref())
                                {
                                    match crate::network::fgtw::protocol::build_blind_srv_vsf(
                                        &conversation_token,
                                        &request_id,
                                        blob_opt.as_deref(),
                                        kp.public.as_bytes(),
                                        kp.secret.as_bytes(),
                                    ) {
                                        Ok(vsf_bytes) => {
                                            let (primary, alt) = self.contacts[idx]
                                                .race_addrs()
                                                .unwrap_or((sender_addr, None));
                                            checker.send_history(
                                                crate::network::status::HistorySendRequest {
                                                    peer_addr: primary,
                                                    alt_addr: alt,
                                                    recipient_pubkey: sender_pubkey.key,
                                                    vsf_bytes,
                                                },
                                            );
                                            crate::log(&format!(
                                                "BLIND: served {} to sibling device {}",
                                                if blob_opt.is_some() { "sealed S" } else { "found=0 (no live S)" },
                                                hex::encode(&sender_pubkey.key[..4])
                                            ));
                                        }
                                        Err(e) => crate::log(&format!("BLIND: sibling srv build failed: {e}")),
                                    }
                                }
                                continue;
                            }

                            if kind == BlindFrameKind::Put {
                                if blob.len() != crate::crypto::blind::BLIND_BLOB_LEN {
                                    crate::log("BLIND: put REJECTED (bad blob length)");
                                    continue;
                                }
                                // Upsert by depositor device — a redeposit (re-key, S regen) replaces. Idempotent, so a duplicate put just re-acks (lost-ack heal).
                                let c = &mut self.contacts[idx];
                                if let Some(entry) = c
                                    .deposited_blinds
                                    .iter_mut()
                                    .find(|(d, _, _)| *d == sender_pubkey.key)
                                {
                                    entry.1 = blob.clone();
                                    entry.2 = now;
                                } else {
                                    c.deposited_blinds.push((sender_pubkey.key, blob.clone(), now));
                                }
                                // DISK COMMIT BEFORE THE ACK — the ack is the depositor's Provisional→Live edge, so it must attest durable storage, not RAM.
                                let committed = match self.storage.as_ref() {
                                    Some(storage) => match crate::storage::contacts::save_contact_state(
                                        &self.contacts[idx],
                                        storage,
                                    ) {
                                        Ok(()) => true,
                                        Err(e) => {
                                            crate::log(&format!("BLIND: deposit persist failed: {e}"));
                                            false
                                        }
                                    },
                                    None => false,
                                };
                                if committed {
                                    if let (Some(kp), Some(checker)) =
                                        (self.device_keypair.as_ref(), self.status_checker.as_ref())
                                    {
                                        match crate::network::fgtw::protocol::build_blind_ack_vsf(
                                            &conversation_token,
                                            &request_id,
                                            kp.public.as_bytes(),
                                            kp.secret.as_bytes(),
                                        ) {
                                            Ok(vsf_bytes) => {
                                                let (primary, alt) = self.contacts[idx]
                                                    .race_addrs()
                                                    .unwrap_or((sender_addr, None));
                                                checker.send_history(
                                                    crate::network::status::HistorySendRequest {
                                                        peer_addr: primary,
                                                        alt_addr: alt,
                                                        recipient_pubkey: sender_pubkey.key,
                                                        vsf_bytes,
                                                    },
                                                );
                                                crate::log(&format!(
                                                    "BLIND: stored deposit from {} device {} — acked (disk-committed)",
                                                    crate::fp(&self.contacts[idx].handle_proof),
                                                    hex::encode(&sender_pubkey.key[..4])
                                                ));
                                            }
                                            Err(e) => crate::log(&format!("BLIND: ack build failed: {e}")),
                                        }
                                    }
                                }
                            } else {
                                // Get: serve THE SIGNER's deposit back (or an explicit miss — the probe-before-generate signal).
                                let blob_opt = self.contacts[idx]
                                    .deposited_blinds
                                    .iter()
                                    .find(|(d, _, _)| *d == sender_pubkey.key)
                                    .map(|(_, b, _)| b.clone());
                                if let (Some(kp), Some(checker)) =
                                    (self.device_keypair.as_ref(), self.status_checker.as_ref())
                                {
                                    match crate::network::fgtw::protocol::build_blind_srv_vsf(
                                        &conversation_token,
                                        &request_id,
                                        blob_opt.as_deref(),
                                        kp.public.as_bytes(),
                                        kp.secret.as_bytes(),
                                    ) {
                                        Ok(vsf_bytes) => {
                                            let (primary, alt) = self.contacts[idx]
                                                .race_addrs()
                                                .unwrap_or((sender_addr, None));
                                            checker.send_history(
                                                crate::network::status::HistorySendRequest {
                                                    peer_addr: primary,
                                                    alt_addr: alt,
                                                    recipient_pubkey: sender_pubkey.key,
                                                    vsf_bytes,
                                                },
                                            );
                                            crate::log(&format!(
                                                "BLIND: served {} to {} device {}",
                                                if blob_opt.is_some() { "deposit" } else { "found=0 (no deposit)" },
                                                crate::fp(&self.contacts[idx].handle_proof),
                                                hex::encode(&sender_pubkey.key[..4])
                                            ));
                                        }
                                        Err(e) => crate::log(&format!("BLIND: srv build failed: {e}")),
                                    }
                                }
                            }
                        }

                        // Our deposit is disk-confirmed at the friend: rid must match our in-flight put.
                        BlindFrameKind::Ack => {
                            let Some(idx) = self.contacts.iter().position(|c| {
                                c.blind_in_flight
                                    .map_or(false, |(r, _, is_get)| r == request_id && !is_get)
                            }) else {
                                continue; // not ours / already resolved — duplicate ack, harmless
                            };
                            self.contacts[idx].blind_in_flight = None;
                            self.contacts[idx].blind_deposited = true;
                            if let Some(storage) = self.storage.as_ref() {
                                if let Err(e) = crate::storage::contacts::save_contact_state(
                                    &self.contacts[idx],
                                    storage,
                                ) {
                                    crate::log(&format!("BLIND: deposited-flag persist failed: {e}"));
                                }
                            }
                            crate::log(&format!(
                                "BLIND: deposit confirmed at {}",
                                crate::fp(&self.contacts[idx].handle_proof)
                            ));
                            // First confirmation flips Provisional → Live: from here S may author tags, because at least one friend durably holds the recovery blind.
                            if matches!(self.private_s, crate::crypto::blind::PrivateS::Provisional(_)) {
                                if let crate::crypto::blind::PrivateS::Provisional(s) =
                                    std::mem::take(&mut self.private_s)
                                {
                                    let sid = crate::crypto::blind::s_id(&s);
                                    crate::log(&format!("S: live (s_id={})", hex::encode(sid)));
                                    self.private_s =
                                        crate::crypto::blind::PrivateS::Live { s, s_id: sid };
                                }
                            }
                        }

                        // Answer to OUR probe: rid must match the in-flight get.
                        BlindFrameKind::Srv => {
                            let Some(idx) = self.contacts.iter().position(|c| {
                                c.blind_in_flight
                                    .map_or(false, |(r, _, is_get)| r == request_id && is_get)
                            }) else {
                                continue; // unsolicited/expired — drop
                            };
                            self.contacts[idx].blind_in_flight = None;

                            if found && self.contacts[idx].is_sibling {
                                // Sibling served S sealed under the sibling chains' history key. Adopt it; on a live-vs-live epoch clash both sides converge on the LOWER s_id deterministically (split-brain healing: only possible when two fresh devices genesised with zero shared friends).
                                let opened = {
                                    let fid = self.contacts[idx].friendship_id;
                                    fid.and_then(|fid| {
                                        self.friendship_chains
                                            .iter()
                                            .find(|(id, _)| *id == fid)
                                            .and_then(|(_, chains)| chains.history_key().copied())
                                    })
                                    .and_then(|key| crate::crypto::blind::open_sibling_s(&blob, &key))
                                };
                                match opened {
                                    Some(s) => {
                                        let sid = crate::crypto::blind::s_id(&s);
                                        match &self.private_s {
                                            crate::crypto::blind::PrivateS::Live { s_id, .. }
                                                if *s_id != sid =>
                                            {
                                                if sid < *s_id {
                                                    crate::log(&format!(
                                                        "S: CRITICAL — divergent epochs across the fleet; ADOPTING the lower ({} < {}) and redepositing everywhere",
                                                        hex::encode(sid),
                                                        hex::encode(s_id)
                                                    ));
                                                    self.private_s =
                                                        crate::crypto::blind::PrivateS::Live {
                                                            s,
                                                            s_id: sid,
                                                        };
                                                    for c in self.contacts.iter_mut() {
                                                        if !c.is_sibling {
                                                            c.blind_deposited = false;
                                                        }
                                                    }
                                                } else {
                                                    crate::log(&format!(
                                                        "S: CRITICAL — divergent epochs across the fleet; keeping the lower ({} < {}), sibling converges on its next probe",
                                                        hex::encode(s_id),
                                                        hex::encode(sid)
                                                    ));
                                                }
                                            }
                                            crate::crypto::blind::PrivateS::Live { .. } => {
                                                crate::log("S: sibling cross-check OK (same epoch)");
                                            }
                                            _ => {
                                                crate::log(&format!(
                                                    "S: adopted from fleet sibling (check OK, s_id={})",
                                                    hex::encode(sid)
                                                ));
                                                self.private_s =
                                                    crate::crypto::blind::PrivateS::Live {
                                                        s,
                                                        s_id: sid,
                                                    };
                                            }
                                        }
                                    }
                                    None => {
                                        crate::log(
                                            "BLIND: CRITICAL — sibling-served S failed AEAD/check; treating as miss",
                                        );
                                        self.contacts[idx].blind_probe_missed = true;
                                        check_s_genesis = true;
                                    }
                                }
                                continue;
                            }

                            if found && blob.len() == crate::crypto::blind::BLIND_BLOB_LEN {
                                let Some(kp) = self.device_keypair.as_ref() else { continue };
                                let device_secret = *kp.secret.as_bytes();
                                let pad = crate::crypto::blind::derive_blind_pad(
                                    &device_secret,
                                    &self.contacts[idx].handle_hash,
                                );
                                match crate::crypto::blind::open_blind_blob(&blob, &pad) {
                                    Some(s) => {
                                        let sid = crate::crypto::blind::s_id(&s);
                                        match &self.private_s {
                                            crate::crypto::blind::PrivateS::Live { s_id, .. }
                                                if *s_id != sid =>
                                            {
                                                // Split-brain: a friend holds a DIFFERENT epoch than the S we're running. Keep ours (it has live confirmations); the redeposit driver will overwrite theirs.
                                                crate::log(&format!(
                                                    "BLIND: CRITICAL — divergent S epoch from {} (theirs {}, ours {}); keeping ours + redepositing",
                                                    crate::fp(&self.contacts[idx].handle_proof),
                                                    hex::encode(sid),
                                                    hex::encode(s_id)
                                                ));
                                                self.contacts[idx].blind_deposited = false;
                                            }
                                            crate::crypto::blind::PrivateS::Live { .. } => {
                                                crate::log("BLIND: cross-check OK (same S epoch)");
                                            }
                                            _ => {
                                                crate::log(&format!(
                                                    "S: reconstituted from friend blind (check OK, s_id={})",
                                                    hex::encode(sid)
                                                ));
                                                self.private_s = crate::crypto::blind::PrivateS::Live {
                                                    s,
                                                    s_id: sid,
                                                };
                                                // A served deposit IS a confirmed deposit at this friend.
                                                self.contacts[idx].blind_deposited = true;
                                                if let Some(storage) = self.storage.as_ref() {
                                                    let _ = crate::storage::contacts::save_contact_state(
                                                        &self.contacts[idx],
                                                        storage,
                                                    );
                                                }
                                            }
                                        }
                                    }
                                    None => {
                                        // Tampered blob or a foreign device's deposit under our key — treat as a miss for THIS friend, loudly (a valid deposit at another friend must still win over genesis).
                                        crate::log(&format!(
                                            "BLIND: CRITICAL — served blob failed the check from {} (tampered?); treating as miss",
                                            crate::fp(&self.contacts[idx].handle_proof)
                                        ));
                                        self.contacts[idx].blind_probe_missed = true;
                                        check_s_genesis = true;
                                    }
                                }
                            } else {
                                crate::log(&format!(
                                    "BLIND: no deposit at {} (found=0)",
                                    crate::fp(&self.contacts[idx].handle_proof)
                                ));
                                self.contacts[idx].blind_probe_missed = true;
                                check_s_genesis = true;
                            }
                        }
                    }
                }

                StatusUpdate::ReflexiveLearned { addr } => {
                    // Our own public address, learned via peer-echoed reflection on the live UDP data socket. Store it for candidate gathering and the announce to publish (so our `PeerRecord.ip` is the real data-socket address, not fgtw.org's cone-only TLS view).
                    if self.our_reflexive != Some(addr) {
                        self.our_reflexive = Some(addr);
                        crate::log(&format!("TRAVERSE: our reflexive address = {}", addr));
                    }
                }

                StatusUpdate::PathValidated { peer_pubkey, remote } => {
                    // A hole-punch (or keepalive) round-tripped. Record/refresh it on the matching contact (any device in the friend's fleet) so `race_addrs` prefers this direct path, keeping the public/LAN as the alternate. First-wins on the address (we stop full-punching once a path is set, so among a single cycle's candidates the first to round-trip — ≈ the lowest-latency path — wins); the timestamp is refreshed on every ack for that same path (keepalive liveness). Any validation clears the graceful-failure counter.
                    let now = std::time::Instant::now();
                    if let Some(contact) = self
                        .contacts
                        .iter_mut()
                        .find(|c| c.knows_device(&peer_pubkey.key))
                    {
                        contact.punch_unvalidated_cycles = 0;
                        match contact.validated_path {
                            None => {
                                crate::log(&format!(
                                    "TRAVERSE: path validated to {} = {}",
                                    crate::fp(&contact.handle_proof).as_str(),
                                    remote
                                ));
                                contact.validated_path = Some((remote, now));
                            }
                            Some((existing, _)) if existing == remote => {
                                // Keepalive ack for the current path — refresh liveness.
                                contact.validated_path = Some((remote, now));
                            }
                            Some(_) => { /* a different candidate acked; keep the first-won path */ }
                        }
                    }
                }
            }
        }

        // Process deferred ceremony completions (after releasing checker borrow)
        for idx in ceremony_completions {
            self.complete_clutch_ceremony_by_idx(idx);
            changed = true;
        }

        // Deferred probe-before-generate verdict (a blind_srv miss landed while S was None).
        if check_s_genesis {
            self.maybe_generate_s();
        }

        // Ping contacts immediately when a new LAN address is discovered Fixes timing gap: startup ping fires before first LAN discovery arrives
        for idx in lan_ping_indices {
            self.ping_contact(idx);
        }

        // Chain-weave probe (deferred past the checker borrow): fire the one hidden probe for any contact that just reached CLUTCH Complete, then seal any contact whose chain is now proven both ways (their probe seen + our TX ACK-advanced). Order: probe first, then seal, so a probe+ACK that both landed in this same drain still seals in the same pass.
        for idx in chain_probe_indices {
            self.maybe_send_chain_probe(idx);
        }
        for idx in chain_seal_indices {
            self.seal_chain_if_ready(idx);
        }

        // Retransmit pending messages to contacts that just came online Use last_received_ef6 from pong to only retransmit messages they don't have
        for (fid, peer_addr, alt_addr, handle, recipient_pubkey, last_received_ef6) in retransmit_requests {
            if let Some((_, chains)) = self.friendship_chains.iter().find(|(id, _)| *id == fid) {
                let pending = chains.pending_messages();
                if !pending.is_empty() {
                    // Filter to only messages newer than what peer has received
                    let to_retransmit: Vec<_> = pending
                        .iter()
                        .filter(|msg| {
                            if let Some(their_last) = last_received_ef6 {
                                msg.eagle_time > their_last
                            } else {
                                // No sync info from peer - retransmit all
                                true
                            }
                        })
                        .collect();

                    if !to_retransmit.is_empty() {
                        crate::log(&format!(
                            "CHAT: Retransmitting {} of {} pending message(s) to {} (came online, last_received={:?})",
                            to_retransmit.len(),
                            pending.len(),
                            handle,
                            last_received_ef6
                        ));
                        let conversation_token = chains.conversation_token;
                        for msg in to_retransmit {
                            if let Some(ref checker) = self.status_checker {
                                checker.send_message(crate::network::status::MessageRequest {
                                    peer_addr,
                                    alt_addr,
                                    recipient_pubkey,
                                    conversation_token,
                                    prev_msg_hp: msg.prev_msg_hp,
                                    ciphertext: msg.ciphertext.clone(),
                                    eagle_time: msg.eagle_time,
                                });
                                crate::log(&format!(
                                    "CHAT: Retransmitted msg with eagle_time {} to {}",
                                    msg.eagle_time, handle
                                ));
                            }
                        }
                    } else if !pending.is_empty() {
                        crate::log(&format!(
                            "CHAT: {} pending messages but peer already has them (last_received={:?})",
                            pending.len(), last_received_ef6
                        ));
                    }
                }
            }
        }

        // Reliability: per-message retransmit with exponential backoff. The came-online loop above only fires on the offline→online EDGE, so a message (or its ACK) dropped while the peer was already online would otherwise never be resent — the exact desync seen live (msg 1 ACKed, msg 2 garbage because the sender's chain never advanced on a lost ACK). This sweep runs every tick and resends any unacked pending whose backoff deadline has passed, until an ACK clears it or it exhausts its attempts.
        self.retransmit_due_messages();

        // History recovery: fire the next backfill page request for any contact mid-recovery (newest-first cursor; urgent jumps the trickle interval; in-flight expiry re-requests lost pages).
        self.drive_history_recovery();

        // Private-identity-secret S: probe/reconstitute/deposit blinds toward whichever friends need an op (no-op at steady state).
        self.drive_blind_ops();

        // NOTE: Proactive CLUTCH initiation is now handled via background keygen:
        // 1. spawn_clutch_keygen() is called when contact is added (background thread)
        // 2. check_clutch_keygens() processes results, stores keypairs + ceremony_id
        // 3. Offers are sent from check_clutch_keygens or the KeysGenerated handler above
        // This avoids UI freeze from synchronous McEliece keygen (~100ms) and handle_proof (~1s)

        // Update sync records if any messages were received (for pong responses)
        if need_sync_update {
            self.update_sync_records();
        }

        changed
    }

    /// Send a message to the currently selected contact Returns true if message was sent successfully
    fn textboxes_mut(&mut self) -> impl Iterator<Item = (TextboxRole, &mut Textbox)> {
        [
            self.textbox
                .as_mut()
                .map(|t| (TextboxRole::LaunchHandle, t)),
            self.contacts_textbox
                .as_mut()
                .map(|t| (TextboxRole::ContactsSearch, t)),
            self.message_textbox
                .as_mut()
                .map(|t| (TextboxRole::MessageCompose, t)),
            self.settings_note_textbox
                .as_mut()
                .map(|t| (TextboxRole::SettingsNote, t)),
        ]
        .into_iter()
        .flatten()
    }

    /// Drive the disabled state of every textbox + its sibling button off the "query in flight" busy flags, in ONE place. A busy field returns `None` from its fluor capability accessors, so click / key / Tab / hover dispatch skip it for free — replacing the per-screen hand-rolled "swallow the click / force hover off / lock the field" code that used to live scattered across `on_event`. Symmetric across screens: the launch handle field + Attest button freeze while attesting (`!can_edit_handle()`), the contacts search box + plus button freeze while an add-friend search is in flight (`add_in_flight`).
    ///
    /// Order matters: if the currently-focused widget is about to be disabled, release focus FIRST (via `change_focus(None)`), because a disabled widget's `focus()` accessor returns `None` and `apply_focus_change` could no longer reach it to clear `set_focused`. Called every `tick`; `set_enabled` is idempotent so steady-state frames are free.
    fn sync_busy_freeze(&mut self) {
        let busy_launch = matches!(self.state, AppState::Launch(ref s) if !s.can_edit_handle());
        let busy_contacts = self.add_in_flight;

        // Release focus before disabling the widget that holds it.
        let focused = self.focused;
        let focus_on_launch = self.textbox.as_ref().map(|t| t.hit_id()) == focused
            || self.attest_btn.as_ref().map(|b| b.hit_id()) == focused;
        let focus_on_contacts = self.contacts_textbox.as_ref().map(|t| t.hit_id()) == focused
            || self.contacts_plus_btn.as_ref().map(|b| b.hit_id()) == focused;
        if (busy_launch && focus_on_launch) || (busy_contacts && focus_on_contacts) {
            self.change_focus(None);
        }

        if let Some(tb) = self.textbox.as_mut() {
            tb.set_enabled(!busy_launch);
        }
        if let Some(btn) = self.attest_btn.as_mut() {
            btn.set_enabled(!busy_launch);
        }
        if let Some(tb) = self.contacts_textbox.as_mut() {
            tb.set_enabled(!busy_contacts);
        }
        if let Some(btn) = self.contacts_plus_btn.as_mut() {
            btn.set_enabled(!busy_contacts);
        }
    }

    /// True iff `id` belongs to one of photon's textboxes. Used by `change_focus` to detect focus transitions into / out of a text-input target so the Android IME show/hide signal can be triggered.
    fn is_textbox(&mut self, id: Option<HitId>) -> bool {
        let Some(id) = id else {
            return false;
        };
        self.textboxes_mut().any(|(_, t)| t.hit_id() == id)
    }

    /// The textbox that currently holds focus, or `None`. The Android IME commit path routes the committed string here, since (unlike desktop keys) it has no focus-generic dispatcher.
    fn focused_textbox_mut(&mut self) -> Option<&mut Textbox> {
        let focused = self.focused?;
        self.textboxes_mut()
            .find(|(_, t)| t.hit_id() == focused)
            .map(|(_, t)| t)
    }

    /// True iff both `[` and `]` are currently held. A bracket is "held" if its press timestamp is more recent than its release timestamp, OR the release was within [`CHORD_RELEASE_GRACE`] — that grace absorbs X11's habit of firing a synthetic Release for a held key the instant another key is pressed.
    fn brackets_held(&self, now: Instant) -> bool {
        fn key_held(press: Option<Instant>, release: Option<Instant>, now: Instant) -> bool {
            match (press, release) {
                (Some(p), Some(r)) => p > r || now.duration_since(r) < CHORD_RELEASE_GRACE,
                (Some(_), None) => true,
                _ => false,
            }
        }
        key_held(self.chord_lb_press, self.chord_lb_release, now)
            && key_held(self.chord_rb_press, self.chord_rb_release, now)
    }

    /// Delete every `.vsf` in the Photon app dirs (the on-disk vault: contacts, CLUTCH slots, ephemeral keypairs, friendship chains, plus old-path strays and derivation-change orphans). Returns the count deleted. Shared by the `[]n` (nuke, keep running) and `[]x` (nuke + exit) chords; `tag` prefixes the log lines so you can tell which fired. Does NOT touch the tohu session or any in-memory state — callers handle that.
    fn dev_wipe_vault_files(tag: &str) -> usize {
        let mut count = 0usize;
        let wipe_dir = |dir: Option<std::path::PathBuf>, count: &mut usize| {
            let Some(base) = dir else { return };
            let app_dir = base.join(crate::storage::APP.dir);
            let rd = match std::fs::read_dir(&app_dir) {
                Ok(rd) => rd,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => return,
                Err(e) => {
                    eprintln!("{} WARN: read_dir {}: {}", tag, app_dir.display(), e);
                    return;
                }
            };
            for entry in rd.flatten() {
                let p = entry.path();
                if p.extension().map_or(false, |e| e == "vsf") {
                    match std::fs::remove_file(&p) {
                        Ok(()) => {
                            eprintln!("{} deleted {}", tag, p.display());
                            *count += 1;
                        }
                        Err(e) => eprintln!("{} WARN: could not delete {}: {}", tag, p.display(), e),
                    }
                }
            }
        };
        #[cfg(not(target_os = "android"))]
        {
            wipe_dir(dirs::config_dir(), &mut count);
            wipe_dir(dirs::data_dir(), &mut count);
        }
        #[cfg(target_os = "android")]
        {
            // On Android the vault lives in the JNI-injected dirs, NOT dirs::config_dir() (which doesn't resolve there) — walk the actual (primary, shadow) pair so a clean genuinely nukes the vault.
            if let Some((primary, shadow)) = crate::storage::android_vault_dirs() {
                wipe_dir(Some(std::path::PathBuf::from(primary)), &mut count);
                if !shadow.is_empty() {
                    wipe_dir(Some(std::path::PathBuf::from(shadow)), &mut count);
                }
            }
        }
        count
    }

    /// Fully clean this device for a new owner / a fresh identity: nuke the on-disk vault (all `.vsf` — contacts, chains, keypairs, the cached fleet key) AND clear the tohu session (identity_seed + vault_seed + handle_proof), drop all in-memory state, and drop back to the attest screen. The device KEY is fingerprint-derived (not stored) so it survives — but with no identity bound and an empty vault the device is a blank slate: a new owner types their handle to attest fresh, or JOINs another fleet. This is `[]n` + `[]u` combined, exposed as a real (non-dev-chord) action for the Security page + the removed-device "start fresh" path; the `-1` broadcast signal tells the Android host to drop its sticky session too.
    fn clean_device_for_reuse(&mut self) {
        let count = Self::dev_wipe_vault_files("clean");
        tohu::clear_session();
        self.session = None;
        self.private_s = crate::crypto::blind::PrivateS::None; // zeroized on overwrite
        self.contacts.clear();
        self.friendship_chains.clear();
        if let Ok(mut pks) = self.contact_pubkeys.lock() {
            pks.clear();
        }
        self.storage = None; // next attest re-opens a fresh vault
        self.pending_broadcast_signal = -1; // Android: drop the sticky session broadcast
        self.state = AppState::Launch(LaunchState::Fresh);
        self.refocus_handle_select_all();
        crate::log(&format!(
            "CLEAN: wiped {count} vault file(s) + cleared session — device is a blank slate, ready to attest fresh or join another fleet"
        ));
    }

    /// Dispatch a chord action character (`a`, `h`, `p`, etc.) that was pressed while both brackets are held. Returns true if anything happened (caller should request a redraw); false for unknown letters (no-op fallthrough — no whitelist so new bindings only add to dispatch, not gating).
    fn handle_chord_action(&mut self, ac: char, ctx: &mut Context) -> bool {
        use std::sync::atomic::Ordering;
        let mut acted = true;
        match ac {
            'h' => {
                self.show_hitmask = !self.show_hitmask;
                paint::DEBUG_SHOW_HITMASK.store(self.show_hitmask, Ordering::Relaxed);
                eprintln!("[]h hitmask = {}", self.show_hitmask);
                if self.show_hitmask {
                    // xorshift32 seeded from process nanos → 256 random opaque RGBs stored in α + darkness. Fresh palette every toggle so distinct IDs always pop visually.
                    let seed = (std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.subsec_nanos())
                        .unwrap_or(1))
                        | 1;
                    let mut s = seed;
                    self.debug_hit_colours.clear();
                    self.debug_hit_colours.reserve(256);
                    for _ in 0..256 {
                        s ^= s << 13;
                        s ^= s >> 17;
                        s ^= s << 5;
                        let r = (s >> 16) & 0xFF;
                        s ^= s << 13;
                        s ^= s >> 17;
                        s ^= s << 5;
                        let g = (s >> 16) & 0xFF;
                        s ^= s << 13;
                        s ^= s >> 17;
                        s ^= s << 5;
                        let b = (s >> 16) & 0xFF;
                        let visible = (r << 16) | (g << 8) | b;
                        let dark = visible ^ 0x00FFFFFF;
                        self.debug_hit_colours.push(0xFF000000 | dark);
                    }
                }
            }
            'p' => {
                let cur = paint::DEBUG_SKIP_PREMULT.load(Ordering::Relaxed);
                paint::DEBUG_SKIP_PREMULT.store(!cur, Ordering::Relaxed);
                eprintln!("[]p skip-premult = {}", !cur);
            }
            'a' => {
                // Cycle: off (0) → grayscale (1) → force-opaque (2) → off.
                let cur = paint::DEBUG_SHOW_ALPHA.load(Ordering::Relaxed);
                let next = (cur + 1) % 3;
                paint::DEBUG_SHOW_ALPHA.store(next, Ordering::Relaxed);
                let label = match next {
                    0 => "off",
                    1 => "grayscale",
                    _ => "force-opaque",
                };
                eprintln!("[]a show-alpha = {} ({})", next, label);
            }
            'c' => {
                let cur = paint::DEBUG_SKIP_CHROME.load(Ordering::Relaxed);
                paint::DEBUG_SKIP_CHROME.store(!cur, Ordering::Relaxed);
                if let Some(chrome) = self.chrome.as_mut() {
                    chrome.invalidate_chrome();
                }
                eprintln!("[]c skip-chrome = {}", !cur);
            }
            'l' => {
                let cur = paint::DEBUG_SKIP_CONTROLS.load(Ordering::Relaxed);
                paint::DEBUG_SKIP_CONTROLS.store(!cur, Ordering::Relaxed);
                if let Some(chrome) = self.chrome.as_mut() {
                    chrome.invalidate_chrome();
                }
                eprintln!("[]l skip-controls = {}", !cur);
            }
            'r' => {
                if let Some(chrome) = self.chrome.as_mut() {
                    chrome.invalidate_bg();
                    chrome.invalidate_chrome();
                }
                eprintln!("[]r force-redraw");
            }
            'f' => {
                let cur = paint::DEBUG_SHOW_FPS.load(Ordering::Relaxed);
                paint::DEBUG_SHOW_FPS.store(!cur, Ordering::Relaxed);
                eprintln!("[]f fps-strip = {}", !cur);
            }
            'w' => {
                let cur = paint::DEBUG_SHOW_DAMAGE.load(Ordering::Relaxed);
                paint::DEBUG_SHOW_DAMAGE.store(!cur, Ordering::Relaxed);
                eprintln!("[]w damage-outline = {}", !cur);
            }
            'd' => {
                let cur = paint::DEBUG_SHOW_FADE.load(Ordering::Relaxed);
                paint::DEBUG_SHOW_FADE.store(!cur, Ordering::Relaxed);
                eprintln!("[]d screen-decay = {}", !cur);
            }
            'b' => {
                let cur = paint::DEBUG_SHOW_OPAQUE_SCAN.load(Ordering::Relaxed);
                paint::DEBUG_SHOW_OPAQUE_SCAN.store(!cur, Ordering::Relaxed);
                eprintln!("[]b opaque-scan tint = {}", !cur);
            }
            'n' => {
                // Nuke the local VAULT only — wipes every .vsf in the Photon app dirs (contacts, CLUTCH slots, ephemeral keypairs, friendship chains; also catches old-path strays and derivation-change orphans). Deliberately does NOT touch the tohu session: the identity_seed/vault_seed/handle_proof stay in memory + cache, so you remain attested on Ready with a freshly-empty vault. To clear the identity itself, use []u (de-attest). Only fires in development builds.
                let count = Self::dev_wipe_vault_files("[]n");
                // Drop the in-memory vault state so the UI reflects the wipe immediately. Keep the session + a live FlatStorage handle: it points at the now-empty dir and recreates files lazily on the next write, so the app stays usable without a relaunch.
                self.contacts.clear();
                self.friendship_chains.clear();
                if let Ok(mut pks) = self.contact_pubkeys.lock() {
                    pks.clear();
                }
                // Drop S too (zeroized) — the reset-recovery E2E is exactly "[]n then reconstitute from a friend's blind"; keeping it in RAM would fake the recovery.
                self.private_s = crate::crypto::blind::PrivateS::None;
                eprintln!(
                    "[]n nuked {} vault file(s); session kept (still attested)",
                    count
                );
            }
            'u' => {
                // De-attest — clear the tohu session (identity_seed/vault_seed/handle_proof) and drop back to the attest screen, leaving the vault on disk intact. The identity is deterministic from the handle, so re-typing it re-derives the same roots. Mirror of []n: []u forgets WHO you are, []n forgets WHAT you've stored. Only fires in development builds.
                tohu::clear_session();
                self.session = None;
                self.private_s = crate::crypto::blind::PrivateS::None; // zeroized on overwrite — no identity, no S
                self.pending_broadcast_signal = -1; // Android: drop the sticky session broadcast.
                self.state = AppState::Launch(LaunchState::Fresh);
                self.refocus_handle_select_all();
                eprintln!("[]u de-attested; session cleared — re-type handle to re-attest");
            }
            'x' => {
                // Full clean-slate reset for the dev loop: nuke the vault ([]n), clear the session ([]u), then KILL the process so the window dies and the next launch starts truly fresh — no lingering in-memory state, no half-reset UI. The disk wipe is the part that must persist; everything else dies with the process, so we exit right after.
                let count = Self::dev_wipe_vault_files("[]x");
                tohu::clear_session();
                crate::clear_log(); // wipe photon.log.vsf too — a clean relaunch leaves no trace
                eprintln!(
                    "[]x nuked {} vault file(s) + de-attested + wiped logs; exiting for a clean relaunch",
                    count
                );
                std::process::exit(0);
            }
            _ => acted = false,
        }
        if acted {
            ctx.window.request_redraw();
        }
        acted
    }
}

/// Convert a [`PixelRect`] to the centre+dimensions float quadruple fluor widgets expect. Pure geometric translation — no clamping, no rounding tricks; pixel ints flow straight into `Coord` (= `f32`).
fn rect_center_dims(r: PixelRect) -> (Coord, Coord, Coord, Coord) {
    let w = (r.x1 - r.x0) as Coord;
    let h = (r.y1 - r.y0) as Coord;
    let cx = r.x0 as Coord + w * 0.5;
    let cy = r.y0 as Coord + h * 0.5;
    (cx, cy, w, h)
}

/// Bounding box of a [`Button`]'s pill rect in pixel coords, returned as `(x0, y0, x1, y1)`. Used by the overlay re-stamp pass for the contacts-page plus button — see the `render` flow where the button paints topmost but its hit stamp gets clobbered by the textbox painting under it.

/// True if `ip` is a private / non-routable address that must NOT be stored as a contact's public (`ip`) address — it belongs in `local_ip` instead. Covers IPv4 RFC1918 (10/8, 172.16/12, 192.168/16), link-local (169.254/16), loopback; IPv6 loopback, link-local (fe80::/10), unique-local (fc00::/7); and IPv4-mapped IPv6 (`::ffff:a.b.c.d`) by unwrapping to the embedded v4 (the ping/pong path reports LAN sources in exactly this mapped form, e.g. `::ffff:<lan-ip>`).
fn is_private_addr(ip: &std::net::IpAddr) -> bool {
    fn v4_private(o: [u8; 4]) -> bool {
        o[0] == 10
            || (o[0] == 172 && (16..=31).contains(&o[1]))
            || (o[0] == 192 && o[1] == 168)
            || (o[0] == 169 && o[1] == 254) // link-local
            || o[0] == 127 // loopback
    }
    match ip {
        std::net::IpAddr::V4(v4) => v4_private(v4.octets()),
        std::net::IpAddr::V6(v6) => {
            if let Some(mapped) = v6.to_ipv4_mapped() {
                return v4_private(mapped.octets());
            }
            let seg = v6.segments();
            v6.is_loopback()
                || (seg[0] & 0xffc0) == 0xfe80 // link-local fe80::/10
                || (seg[0] & 0xfe00) == 0xfc00 // unique-local fc00::/7
        }
    }
}

/// Stamp `hit_id` into every pixel of `hit_map` whose centre is inside the circle at `(cx, cy)` with radius `radius`. Bbox-clipped to the buffer extent; squared-distance test, no sqrt.
fn stamp_hit_circle(
    hit_map: &mut [HitId],
    buf_w: usize,
    buf_h: usize,
    cx: f32,
    cy: f32,
    radius: f32,
    hit_id: HitId,
) {
    if radius <= 0.0 || buf_w == 0 || buf_h == 0 {
        return;
    }
    let r2 = radius * radius;
    let x_min = ((cx - radius).max(0.0) as usize).min(buf_w);
    let x_max = ((cx + radius + 1.0).max(0.0) as usize).min(buf_w);
    let y_min = ((cy - radius).max(0.0) as usize).min(buf_h);
    let y_max = ((cy + radius + 1.0).max(0.0) as usize).min(buf_h);
    for y in y_min..y_max {
        let dy = (y as f32 + 0.5) - cy;
        let dy2 = dy * dy;
        let row_base = y * buf_w;
        for x in x_min..x_max {
            let dx = (x as f32 + 0.5) - cx;
            if dx * dx + dy2 <= r2 {
                hit_map[row_base + x] = hit_id;
            }
        }
    }
}

/// Draw one left-aligned settings text line vertically centred in `row`, indented a little from the row's left edge. Used for page titles, field labels, and placeholder read-outs on the settings stub.
/// Row count each settings page body lays out (must match the `split_v([1.0; N])` in that page's render arm). Drives the content-scroll extent clamp. Keep in sync when a page gains/loses rows.
fn settings_page_rows(page: SettingsPage) -> usize {
    match page {
        SettingsPage::You => 7,
        SettingsPage::Diagnostics => 10,
        _ => 8,
    }
}

fn settings_line(
    canvas: &mut Canvas,
    text: &mut fluor::text::TextRenderer,
    row: fluor::region::Region,
    s: &str,
    size: Coord,
    colour: u32,
    weight: u16,
) {
    text.draw_text_left_u32(
        canvas,
        s,
        row.x + size * 0.3,
        row.center_y(),
        size,
        weight,
        colour,
        "Oxanium",
        None,
        None,
        None,
    );
}

/// Draw an inert stub action pill filling `rect`: a Button-family squircle (fill + two-tone raised edge) with a centred label, hit-stamped with `hit_id`. STUB only — clicks land in the settings dispatch range and log a line; nothing functional fires. Kept immediate-mode (not a persistent `Button`) because the panel has many one-off action pills and a stub doesn't need each to carry click-counter state.
fn draw_stub_pill(
    canvas: &mut Canvas,
    text: &mut fluor::text::TextRenderer,
    hit_map: &mut [HitId],
    buf_w: usize,
    buf_h: usize,
    rect: fluor::region::Region,
    label: &str,
    hit_id: HitId,
    pressed_hit: HitId,
) {
    draw_stub_pill_styled(canvas, text, hit_map, buf_w, buf_h, rect, label, hit_id, pressed_hit, true);
}

/// Greyed, inert variant of [`draw_stub_pill`]: dim label, NO hit stamp — the settings restamp pass has already cleared the region to HIT_NONE, so a click on the pill dispatches nowhere. (Guard the action's handler too: the hit map is one frame stale across an enable→disable transition.)
fn draw_stub_pill_disabled(
    canvas: &mut Canvas,
    text: &mut fluor::text::TextRenderer,
    hit_map: &mut [HitId],
    buf_w: usize,
    buf_h: usize,
    rect: fluor::region::Region,
    label: &str,
    hit_id: HitId,
    pressed_hit: HitId,
) {
    draw_stub_pill_styled(canvas, text, hit_map, buf_w, buf_h, rect, label, hit_id, pressed_hit, false);
}

#[allow(clippy::too_many_arguments)]
fn draw_stub_pill_styled(
    canvas: &mut Canvas,
    text: &mut fluor::text::TextRenderer,
    hit_map: &mut [HitId],
    buf_w: usize,
    buf_h: usize,
    rect: fluor::region::Region,
    label: &str,
    hit_id: HitId,
    pressed_hit: HitId,
    enabled: bool,
) {
    if rect.w <= 0.0 || rect.h <= 0.0 {
        return;
    }
    // Held: a pointer is down on this pill and a release here will fire it (press-hold-release). Only an enabled pill can be held; a drag-off clears `pressed_hit` so the fill drops back to BUTTON_FILL.
    let held = enabled && hit_id != HIT_NONE && hit_id == pressed_hit;
    let mut font_size = rect.h * 0.5;
    // Label first (topmost-first): centred in the pill.
    let mut tw = text.measure_text_width(label, font_size, 400, "Open Sans");
    // Fit order: SHRINK the font toward the slot first (pills sharing a row must not collide with their neighbours), then widen the bg only if the font hit its readability floor — so a long label on a full-width row still gets wrapped rather than truncated.
    let max_w = rect.w * 0.96;
    if tw + font_size * 1.6 > max_w {
        let scaled = font_size * max_w / (tw + font_size * 1.6);
        font_size = scaled.max(9.0).min(font_size);
        tw = text.measure_text_width(label, font_size, 400, "Open Sans");
    }
    let need_w = tw + font_size * 1.6;
    let (px, pw) = if need_w > rect.w { (rect.center_x() - need_w * 0.5, need_w) } else { (rect.x, rect.w) };
    let w = pw as isize;
    let h = rect.h as isize;
    let x0 = px as isize;
    let y0 = rect.y as isize;
    let stroke = (font_size / 32.0) as isize + 1;
    // Disabled label dims to ~half the enabled brightness — same fill and edges, so the pill reads "present but inert" rather than vanished.
    let label_colour = if enabled {
        fluor::theme::TEXTBOX_TEXT
    } else {
        fluor::theme::dark(fluor::theme::fmt(0x00_70_70_6E))
    };
    text.draw_text_left_u32(
        canvas,
        label,
        rect.center_x() - tw * 0.5,
        rect.center_y(),
        font_size,
        400,
        label_colour,
        "Open Sans",
        None,
        None,
        None,
    );
    let inner_w = (w - 2 * stroke).max(0);
    let inner_h = (h - 2 * stroke).max(0);
    if inner_w > 0 && inner_h > 0 {
        paint::draw_squircle_pill_f(
            canvas,
            x0 + stroke,
            y0 + stroke,
            inner_w,
            inner_h,
            if held { fluor::theme::BUTTON_HELD } else { fluor::theme::BUTTON_FILL },
            1.75,
        );
    }
    // Two-tone raised edge.
    paint::draw_squircle_pill_two_tone_f(
        canvas,
        x0,
        y0,
        w,
        h,
        fluor::theme::TEXTBOX_SHADOW_EDGE,
        fluor::theme::TEXTBOX_LIGHT_EDGE,
        1.75,
        None,
        0,
    );
    // Stamp the whole pill bbox so the entire pill is clickable (the two-tone pass only stamps the edge band). A disabled pill stamps nothing — the region stays HIT_NONE from the settings restamp pass.
    if enabled {
        restamp_hit_rect(hit_map, buf_w, buf_h, x0, y0, x0 + w, y0 + h, hit_id);
    }
}

/// Stamp `hit_id` over every pixel in `[x0, x1) × [y0, y1)` of `hit_map`. Used to reclaim hit-test coverage for a widget that paints visually on top of another but whose hit stamps were overwritten by the under-blend partner's later stamping pass (the contacts-page plus button overlaid inside the textbox). Bbox over-stamp — corners outside the pill silhouette claim a few extra pixels, which dispatches those clicks to the button. Acceptable UX since the area is tiny and inside the pill anyway.
fn restamp_hit_rect(
    hit_map: &mut [HitId],
    buf_w: usize,
    buf_h: usize,
    x0: isize,
    y0: isize,
    x1: isize,
    y1: isize,
    hit_id: HitId,
) {
    let xs = x0.max(0) as usize;
    let ys = y0.max(0) as usize;
    let xe = (x1.max(0) as usize).min(buf_w);
    let ye = (y1.max(0) as usize).min(buf_h);
    for y in ys..ye {
        let row_base = y * buf_w;
        for x in xs..xe {
            hit_map[row_base + x] = hit_id;
        }
    }
}
