//! [`PhotonApp`]: the [`fluor::host::app::FluorApp`] impl that hosts Photon on desktop. Owns the app state machine (`AppState`), network handles, contact list, and the per-screen widgets (Launch / Ready / Searching / Conversation), drawing the chrome (perimeter, shadow, window buttons, app-icon orb) plus each screen's content, and routing cross-thread wake-ups thru `FluorApp::on_user_event` with the [`super::PhotonEvent`] payload.
//! This root file holds the struct, its shared helper fns/types, and the small impls (constructor, `Container`, `Default`); the method bodies live in the per-concern child modules declared below.

use super::chromatic_wave::{chromatic_wave, chromatic_wave_clipped};
use super::launch_layout::{AttestBlockLayout, LaunchLayout};
use super::photon_logo::{paint_photon_logo, paint_photon_logo_clipped};
use super::ready_layout::ReadyLayout;
use super::settings_layout::SettingsLayout;
use super::state::{AppState, ContactPage, LaunchState, SettingsPage};
use super::theme;
use super::PhotonEvent;
#[cfg(not(target_os = "android"))]
use crate::network::fgtw::get_machine_fingerprint;
use crate::network::fgtw::PeerStore;
use crate::network::{
    ClutchCeremonyResult, ClutchKemEncapResult, ClutchKeygenResult, HandleQuery, QueryResult,
};
use fluor::text::TextStyle;
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

// The method bodies live in per-concern child modules: each is a further `impl PhotonApp` (plus the `FluorApp` trait impl in `driver`) over this same struct, glob-importing the root's items via `use super::*;`.
mod attachments;
mod bridge;
mod call_ui;
mod ceremony;
mod conversation;
mod devices;
mod driver;
mod input;
mod launch;
mod messaging;
mod peers;
mod protocol;
mod render;
mod settings;
mod status;
mod sync;

/// How long after a `[`/`]` release we still treat the bracket as "held" for chord purposes. X11 fires a synthetic Release for the held bracket the instant the action key is pressed; this grace absorbs that round-trip so chords fire reliably.
const CHORD_RELEASE_GRACE: Duration = Duration::from_millis(40);

/// Deploy version = the crate's MINOR number, baked in at compile time. The scheme: `major.minor.patch` where `deploy.sh` bumps the MINOR and ships `X.Y.0` (patch 0 is RESERVED for releases), and every dev publish bumps the PATCH (≥1, reset to 1 after each release). The dozenal display cues off the minor; a dev build appends `.patch` (also dozenal).
fn deploy_version() -> u32 {
    env!("CARGO_PKG_VERSION_MINOR").parse().unwrap_or(0)
}

/// Dev-publish patch counter — 0 on a release build (`X.Y.0`), ≥1 on any published dev build.
fn dev_patch() -> u32 {
    env!("PHOTON_VERSION_PATCH").parse().unwrap_or(0)
}

/// A manifest version tuple in dozenal glyphs: major omitted while 0, `.patch` only when ≥1 — the same omissions the wire uses.
fn dozenal_version_tuple(v: (usize, usize, usize)) -> String {
    let (maj, min, pat) = v;
    let mut s = String::new();
    if maj > 0 {
        s.push_str(&crate::dozenal_glyphs(maj as u32));
        s.push('.');
    }
    s.push_str(&crate::dozenal_glyphs(min as u32));
    if pat > 0 {
        s.push('.');
        s.push_str(&crate::dozenal_glyphs(pat as u32));
    }
    s
}

/// The displayed dozenal version: minor in dozenal glyphs, plus `.patch` (dozenal) when this is a dev build (patch ≥ 1 — releases are always X.Y.0 and show bare).
fn version_dozenal_glyphs() -> String {
    let mut s = crate::dozenal_glyphs(deploy_version());
    let p = dev_patch();
    if p > 0 {
        s.push('.');
        s.push_str(&crate::dozenal_glyphs(p));
    }
    s
}

/// Spell `n` in dozenal digit names, most-significant first (e.g. dozenal `21` → "Zilor Zila"). The written-out companion to [`dozenal_glyphs`].
/// Number of pips in each posture meter (Security / Recovery): low / medium / high. Kept for the dedicated Security page after the meters were pulled off the Ready strip (they read as ambient noise there).
#[allow(dead_code)]
const POSTURE_PIPS: usize = 3;

/// Filled-pip colour for a meter showing `filled` of [`POSTURE_PIPS`].
#[allow(dead_code)]
fn posture_colour(filled: usize) -> u32 {
    match filled {
        0 | 1 => *theme::POSTURE_LOW_COLOUR,
        2 => *theme::POSTURE_MID_COLOUR,
        _ => *theme::POSTURE_HIGH_COLOUR,
    }
}

/// Security and Recovery posture for the current identity — each a count of filled pips out of [`POSTURE_PIPS`]. Two orthogonal axes, surfaced on the Ready-screen bottom strip: * Security — how hard it is for an attacker to steal or forge this identity. Bounded by the device root. Today every platform derives `device_secret` from a *readable* fingerprint (Linux machine-id, Windows MachineGuid, macOS IOPlatformUUID), so same-privilege code can lift it: 1 pip everywhere. A root-gated firmware fact would be 2; a hardware enclave or PIPE, 3.
/// * Recovery — how hard it is for the *owner* to lose this identity for good. For a single device it is whether the root survives a factory reset: macOS's IOPlatformUUID is firmware and re-derives after a wipe (2); Linux machine-id, Windows MachineGuid and Android's ANDROID_ID are software / reset-volatile (1). Device redundancy (Mirrored), a durable anchor (desktop/PIPE) and social vouching raise this toward 3.
///
/// This is the single seam multi-device, vouching and PIPE plug into: they change what this returns and nothing else.
#[allow(dead_code)]
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

/// Draw a check mark centred at (cx, cy) in a `size`×`size` box — the send button's glyph while an EDIT is armed (commit-the-correction, visually distinct from the send arrowhead by shape AND colour). Same contract as `draw_up_arrowhead`: a filled polygon (two stroke arms as one 6-vertex even-odd shape), source-over onto the already-drawn pill, 1px boundary feather, NEVER touches the hit map. A drawn primitive, not text — the Android font lacks the glyph codepoints (the "→ rendered blank" lesson at the send button's construction site).
fn draw_check_mark(canvas: &mut Canvas, cx: f32, cy: f32, size: f32, colour: u32) {
    // The two arms as a closed hexagon (unit box, y down): short arm down-right, long arm up-right, ~0.16 stroke weight.
    let p = |u: f32, v: f32| (cx + (u - 0.5) * size, cy + (v - 0.5) * size);
    let verts = [
        p(0.08, 0.52),
        p(0.38, 0.82),
        p(0.92, 0.24),
        p(0.79, 0.12),
        p(0.38, 0.57),
        p(0.20, 0.40),
    ];

    let (w, h) = (canvas.width, canvas.height);
    let x0 = (cx - size * 0.5 - 1.0).floor().max(0.0) as usize;
    let x1 = ((cx + size * 0.5 + 1.0).ceil() as usize).min(w);
    let y0 = (cy - size * 0.5 - 1.0).floor().max(0.0) as usize;
    let y1 = ((cy + size * 0.5 + 1.0).ceil() as usize).min(h);
    if x0 >= x1 || y0 >= y1 {
        return;
    }
    canvas.damage.add_bounds(x0, y0, x1, y1);
    let glyph_a = ((colour >> 24) & 0xFF) as f32 / 255.0;
    let (gr, gg, gb) = (
        ((colour >> 16) & 0xFF) as f32,
        ((colour >> 8) & 0xFF) as f32,
        (colour & 0xFF) as f32,
    );
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

// Tiered presence-ping cadence — frequent while the user is engaged, sparse once they've walked away, so an idle/unfocused window isn't waking the radio every few seconds for rings nobody is watching. The tier is chosen by time-since-last-interaction; any interaction (input or focus gain) resets the clock AND fires an immediate sweep, so presence is always fresh the moment the user looks, regardless of how far the cadence had backed off.
/// Active tier: sweep every 5s while interacting (idle < `PRESENCE_IDLE_NEAR`).
const PRESENCE_PING_ACTIVE: std::time::Duration = std::time::Duration::from_secs(5);
/// Idle tier: sweep every 1min once idle past `PRESENCE_IDLE_NEAR`.
const PRESENCE_PING_IDLE: std::time::Duration = std::time::Duration::from_secs(60);
/// Deep-idle tier: sweep every 15min once idle past `PRESENCE_IDLE_FAR`.
const PRESENCE_PING_DEEP: std::time::Duration = std::time::Duration::from_secs(15 * 60);

/// One CLUTCH round's lifetime, eagle-time. 5 min: a relay ceremony (offer+KEM+proof, each a 5-30s store-and-forward hop) can run 1-2 min, and the round's keys must stay valid the whole time. Everything that judges a round or its owner "stale" shares this clock: the re-key sweep, the resume round-restore, and the §4.2 responder-exception ownership gate.
const CLUTCH_ROUND_TTL_OSC: i64 = 300 * vsf::OSCILLATIONS_PER_SECOND as i64;
/// Idle past this → drop from active (5s) to idle (1min).
const PRESENCE_IDLE_NEAR: std::time::Duration = std::time::Duration::from_secs(30);
/// Idle past this → drop from idle (1min) to deep-idle (15min).
const PRESENCE_IDLE_FAR: std::time::Duration = std::time::Duration::from_secs(10 * 60);
/// Cap on the presence-sweep interval while ANY validated direct path is held. The presence ping doubles as the NAT keepalive for that path (its ack refreshes the mapping), and NAT UDP mappings — especially CGNAT — expire well under a minute, so the idle/deep taper would silently kill a live direct path mid-session (the app keeps believing it for up to PATH_TTL while the mapping is already dead). Clamping to 20s keeps held paths warm under common NAT timeouts. Only ever makes the sweep *more* frequent, never less, so presence liveness is unaffected. Supersedes the never-wired `traverse::session::keepalive_due`.
const VALIDATED_PATH_KEEPALIVE: std::time::Duration = std::time::Duration::from_secs(20);

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

/// Encode a LINEAR VSF RGB triple (party/relationship colours arrive already-linear, not γ2-encoded) for the framebuffer, matching theme.rs's display doctrine: macOS ships raw into its VSF-ICC-tagged surface; every other platform converts VSF→Rec.2020 primaries with a sqrt (γ2) transfer — never the sRGB OETF. Then fluor's α+darkness storage.
fn vsf_rgb_to_stored(rgb_vsf: [f32; 3]) -> u32 {
    // macOS: surface is ICC-tagged VSF RGB, so sqrt-encode the raw linear value (γ2) with no matrix.
    #[cfg(target_os = "macos")]
    let out = rgb_vsf;
    // Android + Linux/Windows: VSF → Rec.2020 primaries (E→D65 baked in), then sqrt.
    #[cfg(not(target_os = "macos"))]
    let out = vsf::colour::convert::apply_matrix_3x3_f32(&vsf::colour::VSF_RGB2REC2020, &rgb_vsf);
    let e = |x: f32| (x.clamp(0.0, 1.0).sqrt() * 255.0).round() as u32;
    fluor::theme::dark(fluor::theme::fmt(
        (e(out[0]) << 16) | (e(out[1]) << 8) | e(out[2]),
    ))
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
    // Display gamut for the ray clip is Rec.2020 now (colour doctrine: assume wide-gamut, tag BT.2020) — clipping against sRGB needlessly muted saturated party colours a wide panel can actually show. macOS ships raw VSF so its own gamut IS the VSF cube (the first clip already covers it); Rec.2020 is the honest shared display target for the rest.
    use vsf::colour::VSF_RGB2REC2020;

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
    // The same ray expressed in linear Rec.2020 (linear map ⇒ still a ray): clip against the display cube too, so the colour never clips on a wide-gamut panel.
    let grey_s = apply(&VSF_RGB2REC2020, grey);
    let dir_s = apply(&VSF_RGB2REC2020, dir);
    let t_rec = ray_box_t(grey_s, dir_s);
    let t_max = t_vsf.min(t_rec);

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

/// A history page opened OFF the UI thread, posted back for the drain to merge. The kete open of an up-to-MAX_PAGE_BYTES page ran inline on the render thread — 210-485ms per page in the field (2026-08-08), the single largest status-arm stall. Only the frame's identity travels; the drain re-derives contact/rid/sibling state fresh, because the indexes and in-flight cursor it would have captured at dispatch can shift while the worker runs.
struct HistPageOpened {
    conversation_token: [u8; 32],
    request_id: [u8; 32],
    sender_pubkey: crate::types::DevicePubkey,
    /// `None` = the page ARRIVED but failed to decrypt (key/era divergence) — the drain counts these toward the divergence park instead of silently re-requesting the same undecryptable page forever. The fp names the key that failed, for the park's key-change resume test.
    page: Option<crate::network::history_pages::HistoryPagePlain>,
    open_key_fp: [u8; 4],
}

/// A queued background job — boxed so heterogeneous closures share one worker.
type Job = Box<dyn FnOnce() + Send>;

/// Spawn ONE immortal named worker draining a job queue. Two of these replace the spawn-per-item pattern: a launch burst of fifty history pages once spawned fifty threads at once — fifty 8MB stack mmaps plus fifty kete opens contending with the render thread, the unattributed 400-1700ms ticks in the 2026-08-08 field log. Jobs carry their own reply channels and storage handles, so a session swap is safe: a stale job posts into a dropped receiver, harmlessly.
fn spawn_job_worker(name: &'static str) -> std::sync::mpsc::Sender<Job> {
    let (tx, rx) = std::sync::mpsc::channel::<Job>();
    std::thread::Builder::new()
        .name(name.to_string())
        .spawn(move || {
            while let Ok(job) = rx.recv() {
                job();
            }
        })
        .expect("job worker spawn at construction");
    tx
}

/// Enqueue on a job worker — same call shape as the `std::thread::spawn` it replaces, so converted sites keep their closure bodies verbatim.
fn queue_job(tx: &std::sync::mpsc::Sender<Job>, job: impl FnOnce() + Send + 'static) {
    let _ = tx.send(Box::new(job));
}

/// RAII release for the fleet-heal latch: `acquire` CASes false→true and hands back a guard whose Drop stores false — so a panic anywhere in a heal (fold, rotate, push) can never park key sync for the whole session behind a latch nothing clears. The panic still kills its worker thread; the next sync edge retries against a released latch.
struct HealLatch(std::sync::Arc<std::sync::atomic::AtomicBool>);
impl HealLatch {
    fn acquire(flag: &std::sync::Arc<std::sync::atomic::AtomicBool>) -> Option<Self> {
        use std::sync::atomic::Ordering;
        flag.compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .ok()
            .map(|_| HealLatch(flag.clone()))
    }
}
impl Drop for HealLatch {
    fn drop(&mut self) {
        self.0.store(false, std::sync::atomic::Ordering::Release);
    }
}

/// A signal the chains writer fires only AFTER its snapshot lands on disk — the durable-then-signal half of the commit-point law, kept intact with the write off the UI thread. Receive: persist-before-ACK (a lost write withholds the ACK; the sender retransmits and we re-process). Send: persist-before-transmit (a crash before the write means no frame ever left; the reload re-sends from the persisted tip, never at a stale position).
enum ChainsPostDurable {
    Ack(
        std::sync::mpsc::Sender<crate::network::status::AckRequest>,
        crate::network::status::AckRequest,
    ),
    Message(
        std::sync::mpsc::Sender<crate::network::status::MessageRequest>,
        crate::network::status::MessageRequest,
    ),
    /// A fresh ceremony's ClutchComplete proof: the peer must never hold a proof whose backing chains we could still lose to a crash, so it fires only once those chains are durable (2026-08-15 — this was the inline save-then-send in the ceremony drain).
    CeremonyProof(
        std::sync::mpsc::Sender<crate::network::status::ClutchCompleteRequest>,
        crate::network::status::ClutchCompleteRequest,
    ),
}

impl ChainsPostDurable {
    fn fire(self) {
        match self {
            ChainsPostDurable::Ack(tx, req) => {
                let _ = tx.send(req);
            }
            ChainsPostDurable::Message(tx, req) => {
                let _ = tx.send(req);
            }
            ChainsPostDurable::CeremonyProof(tx, req) => {
                let _ = tx.send(req);
            }
        }
    }
}

/// The message writer's verdict for rows whose bright flip + sibling push wait on the DISK — the write-confirm-then-send law applied to the zero-remote path, where the vault IS the recipient (2026-08-21 erasure ticket: the self row rendered bright and pushed to siblings before any write, so a refused persist left a RAM ghost that looked safely stored and vanished at relaunch). `err: None` is the durable edge: the drain flips the named rows bright and releases their sibling push. `err: Some` leaves them faint with a LOUD toast — the resend pill is the retry.
struct MessagesDurableVerdict {
    conv_id: crate::types::friendship::FriendshipId,
    rows: Vec<i64>,
    err: Option<String>,
}

/// A send's braid encrypt finished OFF the UI thread. The drain CAS-commits the advance (prepare_send_commit) and rides the durable-transmit writer; `result: None` means the encrypt itself failed (lane vanished) — the drain still clears the per-friendship encrypt gate either way.
struct BraidTxEncrypted {
    friendship_id: FriendshipId,
    conversation_token: [u8; 32],
    eagle_time: i64,
    salt_text: Vec<u8>,
    woven_strands: Vec<Vec<u8>>,
    peer_addr: std::net::SocketAddr,
    alt_addr: Option<std::net::SocketAddr>,
    recipient_pubkey: [u8; 32],
    relay_to: Vec<[u8; 32]>,
    text_len: usize,
    result: Option<BraidTxWire>,
}

/// The wire half a send encrypt produced: everything prepare_send_commit and the MessageRequest need.
struct BraidTxWire {
    ciphertext: Vec<u8>,
    prev_msg_hp: [u8; 32],
    msg_hp: [u8; 32],
    plaintext_hash: [u8; 32],
    lane: [u8; 32],
    expected_key: [u8; 32],
}

/// A chat frame's braid decrypt finished OFF the UI thread. The arm dispatches only after its cheap gates pass (auth, dup, chain-link verify); the worker runs the pure crypto (salt → memory-hard scratch → layer peel) against a snapshot of the lane; commit_braid_rx re-gates against CURRENT state and runs everything after the decrypt. Serialization is free: until a frame commits, its successor's chain-link verify fails and gap-buffers, exactly as when the decrypt was inline.
struct BraidRxDecrypted {
    conversation_token: [u8; 32],
    lane: [u8; 32],
    prev_msg_hp: [u8; 32],
    timestamp: i64,
    sender_addr: std::net::SocketAddr,
    sender_pubkey: crate::types::DevicePubkey,
    plaintext: Vec<u8>,
}

/// A sibling chain_sync blob opened + decoded OFF the UI thread. The kete open ran inline — 17KB+ per lane, and a fresh sibling join repushes every friendship at once, an adopt storm on the render thread. The drain re-gates the sender (lockout can land mid-flight) and runs the cheap position-compare adopt.
struct ChainSyncOpened {
    conversation_token: [u8; 32],
    sender_pubkey: crate::types::DevicePubkey,
    incoming: crate::types::friendship::FriendshipChains,
}

/// A verified+stored attachment blob, posted from the off-thread worker back to the UI drain. Carries only what the drain needs to confirm receipt (attach_have) and refresh the view; the plaintext is already on disk.
struct AttachInstalled {
    conversation_token: [u8; 32],
    content_hash: [u8; 32],
    sender_pubkey: crate::types::DevicePubkey,
    sender_addr: std::net::SocketAddr,
    len: usize,
}

/// Greedy word-wrap for the message list: split `s` into lines that each measure ≤ `max_w` under `style`. Word widths are measured individually and summed (kerning across a space is negligible at chat sizes), so the cost is O(words), not O(words²) re-shapes. A single word wider than the line hard-breaks by chars — a pasted URL/hash must wrap, not vanish off-screen. Empty input yields one empty line so the row keeps its height.
/// Bubble DISPLAY text for a row: attachment rows render as a pill line — paperclip, name, dozenal size, and an actions hint while the blob isn't held locally. Everything else passes thru. The raw marker string never reaches a glyph.
/// The starter reaction vocabulary, default order — the strip re-ranks by fleet-wide usage on top of this (stable sort, so ties keep this order). Emoji-as-text is the proven path (the paperclip pill). The heart is BARE U+2764 (no VS16): fluor's per-codepoint text path renders the trailing U+FE0F variation-selector as a `.notdef` tofu box beside the heart, and the reaction is stored/compared bare anyway (see `current_reaction` tests), so the selector only ever hurt.
const DEFAULT_REACTIONS: [&str; 5] = [
    "\u{1F44D}",
    "\u{2764}",
    "\u{1F602}",
    "\u{1F62E}",
    "\u{1F44E}",
];

fn display_content(content: &str) -> String {
    if let Some((hash, name, size)) = crate::types::parse_attachment_content(content) {
        let (units, label) = crate::types::size_units(size);
        let held = crate::storage::blob_present(&hash);
        if name == "call.audio" {
            // A kept call recording — a PLAY affordance, not a file. Two fixes ride here (Nick's field report):
            //  - ▶ (U+25B6) replaces the paperclip: 📎 (U+1F4CE) has no glyph in the bubble font and rendered as a tofu rectangle; ▶ IS covered (the Ended panel's Play button uses it).
            //  - the size renders in DOZENAL (fmt_num honours the fleet toggle) now that the toggle exists — the row is drawn in Oxanium (see the call.audio font switch in render.rs) so the dozenal control-byte glyphs resolve instead of tofu-ing.
            let tail = if held { "" } else { " \u{2014} fetching\u{2026}" };
            format!(
                "\u{25B6} recording \u{00B7} {}\u{202F}{}{}",
                crate::fmt_num(units),
                label,
                tail
            )
        } else {
            // Files keep the paperclip + DECIMAL size in the default bubble font: that font can't render dozenal glyphs (they are Oxanium-only control bytes), so routing a file size through fmt_num here would tofu. Converting files needs the same Oxanium-row treatment call.audio got — a deliberate follow-up, not a silent regression.
            let state = if held { "" } else { " \u{2014} tap for actions" };
            format!("\u{1F4CE} {} \u{00B7} {}\u{202F}{}{}", name, units, label, state)
        }
    } else {
        // Reference rows (reply/edit/react) need no stripping: their content IS the bare body/glyph — the reference is a typed FIELD, never a string encoding.
        content.to_string()
    }
}

/// Is this row a BUBBLE in the stream? One source of truth for the renderer's visible-list filter AND the tap-to-jump scroll walk — the two must count identically or a jump lands off-target. Control rows and tombstones never draw; reaction rows resolve onto their target; an edit row hides while its target exists (renders standalone only when the target never synced).
fn chat_row_visible(raw: &[crate::types::ChatMessage], m: &crate::types::ChatMessage) -> bool {
    if crate::types::is_control_content(&m.content) || m.deleted {
        return false;
    }
    if matches!(m.reference, Some((crate::types::RefKind::React, _))) {
        return false;
    }
    // BridgeReset (the peer opened the bridge) and BridgeCtl (a Stop press) are hidden control rows — never bubbles.
    if matches!(
        m.reference,
        Some((crate::types::RefKind::BridgeReset | crate::types::RefKind::BridgeCtl, _))
    ) {
        return false;
    }
    // A silent-success bridge final (cd, touch — exit 0, no output) is a durable EMPTY BridgeOut row: it must exist (it carries the exit that releases the prompt gate, and the held-row re-serve rebuilds from it) but never draws — the command's ACK plus the Stop pill clearing is the whole story (Nick 2026-08-31).
    if m.content.is_empty() && matches!(m.reference, Some((crate::types::RefKind::BridgeOut, _))) {
        return false;
    }
    if let Some((crate::types::RefKind::Edit, t)) = m.reference {
        return !raw.iter().any(|x| {
            x.timestamp == t
                && !crate::types::is_control_content(&x.content)
                && !matches!(x.reference, Some((crate::types::RefKind::Edit, _)))
        });
    }
    true
}

/// Current reaction per target per direction — newest live wins, empty glyph = retracted. [0]=theirs, [1]=ours. Shared by the renderer and the scroll walk (a reacted row is one line taller).
fn build_react_over(
    raw: &[crate::types::ChatMessage],
) -> std::collections::HashMap<i64, [Option<(i64, String)>; 2]> {
    let mut out: std::collections::HashMap<i64, [Option<(i64, String)>; 2]> =
        std::collections::HashMap::new();
    for m in raw.iter().filter(|m| !m.deleted) {
        if let Some((crate::types::RefKind::React, t)) = m.reference {
            let slot = &mut out.entry(t).or_default()[m.is_outgoing as usize];
            if slot.as_ref().map_or(true, |(ts, _)| m.timestamp >= *ts) {
                *slot = Some((m.timestamp, m.content.clone()));
            }
        }
    }
    out
}

/// Does the react_over map hold any LIVE glyph for this row? (The retract leaves a slot with an empty string.)
fn row_has_reaction(
    over: &std::collections::HashMap<i64, [Option<(i64, String)>; 2]>,
    ts: i64,
) -> bool {
    over.get(&ts).is_some_and(|slots| {
        slots
            .iter()
            .any(|s| s.as_ref().is_some_and(|(_, g)| !g.is_empty()))
    })
}

fn wrap_text_lines(
    tr: &mut fluor::text::TextRenderer,
    s: &str,
    style: &TextStyle,
    max_w: f32,
) -> Vec<String> {
    // Explicit newlines are HARD breaks: each segment word-wraps independently, an empty segment keeps its blank line. Without this a multi-line message word-wrapped as one soup line and its drawn lines OVERLAPPED (field, 2026-08-09 — the renderer stacks by wrapped-line count, which undercounted).
    if s.contains('\n') {
        return s
            .split('\n')
            .flat_map(|seg| wrap_text_lines(tr, seg, style, max_w))
            .collect();
    }
    let space_w = tr.measure_text(" ", style);
    let mut lines: Vec<String> = Vec::new();
    let mut cur = String::new();
    let mut cur_w = 0.0f32;
    for word in s.split(' ') {
        let ww = tr.measure_text(word, style);
        if !cur.is_empty() && cur_w + space_w + ww > max_w {
            lines.push(std::mem::take(&mut cur));
            cur_w = 0.0;
        }
        if ww > max_w && cur.is_empty() {
            // Over-long word: break by chars.
            let mut pw = 0.0f32;
            for ch in word.chars() {
                let cw = tr.measure_text(ch.encode_utf8(&mut [0u8; 4]), style);
                if !cur.is_empty() && pw + cw > max_w {
                    lines.push(std::mem::take(&mut cur));
                    pw = 0.0;
                }
                cur.push(ch);
                pw += cw;
            }
            cur_w = pw;
            continue;
        }
        if cur.is_empty() {
            cur = word.to_string();
            cur_w = ww;
        } else {
            cur.push(' ');
            cur.push_str(word);
            cur_w += space_w + ww;
        }
    }
    if !cur.is_empty() || lines.is_empty() {
        lines.push(cur);
    }
    lines
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
    (
        "x",
        "Nuke vault + un-attest + wipe logs + EXIT for a clean relaunch (dev only)",
    ),
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
    // In lockstep with fluor's draw_chord_hint (paint.rs). The old `.clamp(font_size*22, font_size*36)` was a dead no-op — span*0.45 always sat inside [0.308span, 0.504span] — so it's just the span fraction, no clamp.
    let panel_w = span * 0.45;
    let cx = vw as f32 * 0.5;
    let cy = vh as f32 * 0.4;
    let x0 = (cx - panel_w * 0.5).max(0.0) as usize;
    let y0 = (cy - panel_h * 0.5).max(0.0) as usize;
    let x1 = ((cx + panel_w * 0.5).max(0.0) as usize).min(vw);
    let y1 = ((cy + panel_h * 0.5).max(0.0) as usize).min(vh);
    PixelRect::new(x0, y0, x1, y1)
}

/// Which textbox a registry entry is, so callers that need per-role behaviour can branch (freeze keys off Launch-vs-Contacts busy state; the launch box gates the Attest button; the contacts box filters the contact list). Generic concerns — focus, IME routing, blink — ignore the role and treat every entry the same. The conversation compose bar is NOT here: it's the dedicated MultiTextbox with its own branches at every registry seam.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TextboxRole {
    LaunchHandle,
    ContactsSearch,
    /// The Diagnostics optional-note field — in the registry so click-to-focus raises the Android IME + blinkie like every other box.
    SettingsNote,
    /// Any You-page profile field (display name, first, email, a custom one, …) or the add-a-field entry — same registry so click-to-focus raises the IME + blinkie. The form treats them all alike; the `field_id` that distinguishes them lives on [`ProfileField`], not here.
    ProfileField,
    /// The unattended-confirm modal's handle-entry box — in the registry so it takes keys / IME / blink like every other box (without this it rendered but couldn't be typed into).
    UnattendedConfirm,
}

/// One editable profile field on the You page: a `field_id` (the VSF dictionary label, also the `profile.<id>` settings key), a human label, its taxonomy tier, and the text box holding the working value. Custom fields are user-added (registered in `profile._custom`) and grouped under a "Custom" header. See docs/contact-system.md "The field taxonomy".
struct ProfileField {
    field_id: String,
    label: String,
    // Custom-ness is carried by `tier == "custom"` (what the renderer groups on) — a separate bool was write-only redundancy.
    tier: &'static str,
    tb: Textbox,
    /// Companion tag box (phone instances: home / work / custom, free text). Persisted as `profile.<id>_label`. `None` for untagged fields.
    tag_tb: Option<Textbox>,
    /// Default-share checkbox: checked = this field auto-shares with NEW contacts (per-contact toggles live on the contact panel). `None` for the display name, which is public and always shared. Persisted as `share.<id>`, fleet-synced like the value.
    share_cb: Option<fluor::widgets::Checkbox>,
}

/// Multi-instance ("expandable") field bases: filling the LAST instance reveals an empty next one (addr → addr2 → addr3 …), so a second address/email/phone/website is always one keystroke away — and never shown before it's needed. Singletons (SSN, passport, licence, …) are NOT here and never expand. The bool = instances carry a companion tag box (phone: home/work/custom).
const EXPANDABLE_FIELDS: &[(&str, bool)] = &[
    ("addr", false),
    ("email", false),
    ("phone", true),
    ("web", false),
    ("alt_msg", false),
];

/// The standard profile fields in taxonomy order: (field_id, display label, tier). `name` is the always-granted display-name slot (formerly the lone name box); every other field defaults UNSHARED. Mirrors the table in docs/contact-system.md — keep the two in sync.
const STD_PROFILE_FIELDS: &[(&str, &str, &str)] = &[
    ("name", "Preferred name", "name"),
    ("first", "First", "name"),
    ("middle", "Middle", "name"),
    ("last", "Last", "name"),
    ("nick", "Nickname", "name"),
    ("prefix", "Prefix", "name"),
    ("suffix", "Suffix", "name"),
    ("maiden", "Maiden", "name"),
    ("phon", "Pronunciation", "name"),
    ("email", "Email", "reach"),
    ("phone", "Phone", "reach"),
    ("web", "Website", "reach"),
    ("alt_msg", "Other messaging", "reach"),
    ("addr", "Address", "place"),
    ("geo", "Lat / lon", "place"),
    ("tz", "Timezone", "place"),
    ("dob", "Date of birth", "personal"),
    ("pronouns", "Pronouns", "personal"),
    ("gender", "Gender", "personal"),
    ("lang", "Languages", "personal"),
    ("bio", "Short bio", "personal"),
    ("org", "Organisation", "work"),
    ("title", "Job title", "work"),
    ("ssn", "National ID / SSN", "sensitive"),
    ("passport", "Passport", "sensitive"),
    ("license", "Driver's licence", "sensitive"),
    ("tax_id", "Tax ID", "sensitive"),
    ("emergency", "Emergency contact", "sensitive"),
];

/// Tier display order + header titles. `custom` always sorts last — user-added fields (a second address, etc.).
const PROFILE_TIERS: &[(&str, &str)] = &[
    ("name", "Name"),
    ("reach", "Reach"),
    ("place", "Place"),
    ("personal", "Personal"),
    ("work", "Work"),
    ("sensitive", "Sensitive — you can't un-give these"),
    ("custom", "Custom"),
];

/// One laid-out row of the You page. Render, layout, and scroll-extent passes all build this SAME plan (via [`you_rows_plan`]) so their row counts and positions never drift.
enum YouRow {
    /// Category header (tier title).
    Header(&'static str),
    /// An editable field — index into `you_fields`. Label left, box right.
    Field(usize),
    /// "Add a custom field" sub-header.
    AddHeader,
    /// The custom-field-name entry box + "Add" pill.
    AddInput,
    /// "Your handle IS your identity" reassurance line.
    Note,
    /// "Identity" header.
    IdentityHeader,
    /// The identity fingerprint read-out.
    IdentityFp,
    /// "Update" action pill.
    SavePill,
    /// Empty breathing row (between the action pills).
    Blank,
    /// "Change avatar…" action pill.
    AvatarPill,
}

/// Build the ordered You-page row plan from the current field set: fields grouped under their tier header (only non-empty tiers get a header), then the add-field affordance, the reassurance note, the identity read-out, and the action pills. Pure over `fields` so render / layout / scroll-extent all agree on the row count and order.
fn you_rows_plan(fields: &[ProfileField]) -> Vec<YouRow> {
    let mut rows = Vec::new();
    for &(tier, title) in PROFILE_TIERS {
        let mut any = false;
        for (i, f) in fields.iter().enumerate() {
            if f.tier == tier {
                if !any {
                    rows.push(YouRow::Header(title));
                    any = true;
                }
                rows.push(YouRow::Field(i));
            }
        }
    }
    rows.push(YouRow::AddHeader);
    rows.push(YouRow::AddInput);
    rows.push(YouRow::Note);
    rows.push(YouRow::IdentityHeader);
    rows.push(YouRow::IdentityFp);
    rows.push(YouRow::SavePill);
    rows.push(YouRow::Blank);
    rows.push(YouRow::AvatarPill);
    rows
}

/// The Region for the `i`th stacked row of the You page (natural line height, shifted by the content scroll). Shared by render + layout so every box sits exactly where its label draws.
fn you_row_rect(layout: &SettingsLayout, scroll: Coord, i: usize) -> fluor::region::Region {
    let inset = layout.content_inset();
    fluor::region::Region::new(
        inset.x,
        inset.y - scroll + i as Coord * layout.content_line_h(),
        inset.w,
        layout.content_line_h(),
    )
}

/// Parse one line of ANSI truecolor terminal output (vsf::inspect_vsf's format) into coloured text spans for in-app drawing. Handles `ESC[38;2;r;g;bm` (fg truecolor), `ESC[0m` (reset), and the bold/basic forms the inspector emits (`1;37` etc. → near-white); anything unrecognized resets. Colours land in fluor α+darkness via the platform pass.
fn ansi_line_to_spans(line: &str, default_colour: u32) -> Vec<(String, u32)> {
    let mut spans: Vec<(String, u32)> = Vec::new();
    let mut cur = String::new();
    let mut colour = default_colour;
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '\u{1b}' {
            cur.push(c);
            continue;
        }
        // Escape sequence: expect '[' params 'm'; anything else is dropped.
        if chars.peek() != Some(&'[') {
            continue;
        }
        chars.next();
        let mut params = String::new();
        for pc in chars.by_ref() {
            if pc == 'm' {
                break;
            }
            params.push(pc);
        }
        if !cur.is_empty() {
            spans.push((std::mem::take(&mut cur), colour));
        }
        let parts: Vec<u32> = params.split(';').filter_map(|p| p.parse().ok()).collect();
        colour = match parts.as_slice() {
            [38, 2, r, g, b] => fluor::theme::dark(fluor::theme::fmt((r << 16) | (g << 8) | b)),
            [] | [0] => default_colour,
            // Bold/basic whites the inspector uses for emphasis.
            p if p.contains(&37) || p.contains(&97) => *theme::CONTACT_NAME_COLOUR,
            _ => default_colour,
        };
    }
    if !cur.is_empty() {
        spans.push((cur, colour));
    }
    spans
}

/// Viewer cap: the newest 2^15 decoded records. A capped 16 MiB log holds more than anyone scrolls thru in-app; `photonlog` serves the full-file cases.
const DIAG_LOG_MAX_ROWS: usize = 1 << 15;

/// The Region for the `i`th decoded record on the Diagnostics log viewer: HALF the natural line height (a log wants density), below the full-height header row, shifted by the content scroll. Shared by the render loop and the extent math so the scroll bound matches the drawn rows exactly.
fn diag_log_row_rect(layout: &SettingsLayout, scroll: Coord, i: usize) -> fluor::region::Region {
    let inset = layout.content_inset();
    let line = layout.content_line_h();
    let row_h = line * 0.5;
    fluor::region::Region::new(
        inset.x,
        inset.y - scroll + 2. * line + i as Coord * row_h,
        inset.w,
        row_h,
    )
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
    /// The screen `tick()` last saw — its per-tick diff against `self.state` is THE page-change hook: any screen swap drops textbox focus (and with it the blinkey + Android IME) no matter which of the many `self.state =` sites caused it. Screen granularity, not state granularity: Launch sub-states are one screen (Error→Fresh happens ON the recovery keystroke — defocusing would eat it), Ready↔Searching share the contacts screen (the search box owns the in-flight search), each Settings page counts as its own.
    last_screen: AppState,
    /// Last time `tick()` ran the background presence ping sweep (`ping_contacts`). `None` until the first sweep. Paired with `last_interaction` to drive the tiered cadence (see `presence_ping_interval`): `tick()` re-pings when due and `wake_at()` schedules the next due sweep so presence refreshes even while idle. Without this, contacts only flipped online when you opened their conversation.
    last_presence_ping: Option<Instant>,
    /// Last time the user interacted with the app (any input event, or window focus-gain). `None` until the first interaction. The presence sweep tapers with idle time — frequent while you're actively using it, sparse when you've walked away — so an unfocused, untouched window isn't hitting the network every few seconds. Reset on interaction, which also triggers an immediate sweep so rings are fresh the instant you look. See `presence_ping_interval`.
    last_interaction: Option<Instant>,
    /// Last time an already-running device re-folded its OWN fleet chain to catch a device add/remove it may have missed. The hub `fleet` event is the fast path but best-effort (a dropped WebSocket = a missed add), so this periodic re-fold is the reliable doorbell: without it, an existing device never learns a newly-added sibling until relaunch — it wouldn't answer the new device's presence pings (→ shows it offline) and its Fleet list would stay stale. `None` until the first poll.
    last_fleet_refold: Option<Instant>,
    /// Last time we pulsed a background resume to re-fetch a stalled contact's address. Address discovery (`contact.ip`) only refreshes on attest echo / roster / search — there is no periodic re-fetch — so a contact whose initial fetch failed (flaky cellular fgtw) is stuck with no address: its CLUTCH offer can't send, name/avatar (which ride the pong) never arrive, and it loops keygen forever. While any contact is blocked this way we pulse a lightweight background resume on a fast cadence; one success learns the address and fire-on-learn punches + the offer sends. `None` until the first pulse. (Stopgap for the peer-gossip fix, TICKETS T0.)
    last_stalled_refetch: Option<Instant>,
    /// Shared peer store (self-signed routing records), cloned from HandleQuery's. Populated by fgtw fetches AND by phonebook-gossip responses (see status.rs); the app harvests learned addresses from it for stalled contacts whose own fgtw fetch keeps failing. `None` until init.
    peer_store: Option<std::sync::Arc<std::sync::Mutex<crate::network::fgtw::PeerStore>>>,
    /// HandleQuery client — owns the UDP socket, device keypair, and FGTW peer store. Submission calls `handle_query.query(handle)`; `tick()` polls `try_recv()` for results. `None` until init.
    handle_query: Option<HandleQuery>,
    /// Per-contact presence + CLUTCH ceremony driver. Shares HandleQuery's UDP socket; pings contacts, receives pongs (→ `is_online`), and runs the slot-based CLUTCH offer/KEM/complete exchange. `None` until init. Ported from the retired `app.rs` — the fluor migration left this whole subsystem behind, so contacts showed offline and CLUTCH never started.
    status_checker: Option<crate::network::status::StatusChecker>,
    /// Pubkeys the status checker will answer pings from — kept in lockstep with `self.contacts` (seeded on resume-load, appended on add). Shared `Arc<Mutex<..>>` with the checker thread.
    contact_pubkeys: crate::network::status::ContactPubkeys,
    /// Last-received-message markers per conversation, for retransmit. Inert in v1 (messaging not yet ported) — an empty shared vec the checker reads and never finds anything in.
    sync_records: crate::network::status::SyncRecordsProvider,
    /// Pairwise pong-seal keys (peer DEVICE pubkey → key), shared with the checker thread: it seals each outgoing pong's sensitive tail (sync rows + name + avatar pin) to the pinging device and opens inbound tails with the responder's entry. Derived HERE on the UI thread — friend keys from the static identity DH, sibling keys from the shared identity seed + the sorted device pair — so the identity seed itself never enters the RX worker (secret-memory hygiene). Kept in lockstep with `contact_pubkeys`: the same reseed walk refills both.
    pong_seal_keys: crate::network::status::PongSealKeys,
    /// Background CLUTCH keypair-generation results (the 8 ephemeral keypairs per ceremony). Drained in `tick` → stores keypairs on the contact + flips it to a ready-to-offer state.
    clutch_keygen_tx: std::sync::mpsc::Sender<crate::network::ClutchKeygenResult>,
    clutch_keygen_rx: std::sync::mpsc::Receiver<crate::network::ClutchKeygenResult>,
    /// Background KEM-encapsulation results (responder's reply to an offer). Drained in `tick` → sends the KEM response.
    clutch_kem_encap_tx: std::sync::mpsc::Sender<crate::network::ClutchKemEncapResult>,
    clutch_kem_encap_rx: std::sync::mpsc::Receiver<crate::network::ClutchKemEncapResult>,
    /// Background ceremony-completion results (avalanche-expand → friendship chains + eggs proof). Drained in `tick` → sends complete, marks the contact CLUTCH-complete.
    clutch_ceremony_tx: std::sync::mpsc::Sender<crate::network::ClutchCeremonyResult>,
    clutch_ceremony_rx: std::sync::mpsc::Receiver<crate::network::ClutchCeremonyResult>,
    /// KEM-decap job channel — the fourth CLUTCH job stage (2026-08-15): opening a peer's KEM response is 8 PQ decapsulations and ran inline in three drain arms, the last non-UI work on the UI thread.
    clutch_kem_decap_tx: std::sync::mpsc::Sender<crate::network::ClutchKemDecapResult>,
    clutch_kem_decap_rx: std::sync::mpsc::Receiver<crate::network::ClutchKemDecapResult>,
    /// Peer-avatar background downloads (fetched from FGTW by handle, off the UI thread). The result carries the decoded VSF-RGB pixels (or None if the peer has no avatar / fetch failed); the drain in `check_status_updates` colour-converts and installs them on the matching contact.
    avatar_dl_tx: std::sync::mpsc::Sender<crate::ui::avatar::AvatarDownloadResult>,
    avatar_dl_rx: std::sync::mpsc::Receiver<crate::ui::avatar::AvatarDownloadResult>,
    /// Attachment blobs verified + stored OFF the UI thread: the receive arm hands (sealed, key, hash, seed) to a worker that AEAD-opens the whole blob, checks its content hash, and writes it to blob storage — all heavy on the render thread inline (an arbitrary-size file). The worker posts back here on success; the drain sends the attach_have confirm (needs the keypair + checker) and clears the compose wrap.
    attach_installed_tx: std::sync::mpsc::Sender<AttachInstalled>,
    attach_installed_rx: std::sync::mpsc::Receiver<AttachInstalled>,
    /// History pages opened off-thread (see HistPageOpened) — the drain merges; merging is the cheap half since the (timestamp, content-hash) index landed.
    hist_opened_tx: std::sync::mpsc::Sender<HistPageOpened>,
    hist_opened_rx: std::sync::mpsc::Receiver<HistPageOpened>,
    /// Sibling chain_sync blobs opened off-thread (see ChainSyncOpened) — the drain adopts.
    chain_sync_opened_tx: std::sync::mpsc::Sender<ChainSyncOpened>,
    chain_sync_opened_rx: std::sync::mpsc::Receiver<ChainSyncOpened>,
    /// Chat-frame braid decrypts finished off-thread (see BraidRxDecrypted) — the drain commits.
    braid_rx_tx: std::sync::mpsc::Sender<BraidRxDecrypted>,
    braid_rx_rx: std::sync::mpsc::Receiver<BraidRxDecrypted>,
    /// Gap-refill replays minted by commit_braid_rx: buffered frames whose predecessor just committed, re-entering the arm's full gates ahead of new channel items (FIFO — a refilled N+1 processes before anything newer).
    chat_replay_queue: std::collections::VecDeque<crate::network::status::StatusUpdate>,
    /// Send braid encrypts finished off-thread (see BraidTxEncrypted) — the drain commits.
    braid_tx_tx: std::sync::mpsc::Sender<BraidTxEncrypted>,
    braid_tx_rx: std::sync::mpsc::Receiver<BraidTxEncrypted>,
    /// Friendships with a send encrypt in flight: a second dispatch would mint a second frame at the SAME lane position (the commit CAS would void it), so the gate holds the row and the commit edge re-fires it thru resend_held_messages.
    send_encrypt_busy: std::collections::HashSet<[u8; 32]>,
    /// The sealed-I/O worker: kete opens/seals, page builds, blob serves — everything AEAD-shaped queues here instead of spawning a thread per item (see spawn_job_worker).
    seal_job_tx: std::sync::mpsc::Sender<Job>,
    /// The braid-crypto worker: memory-hard scratch decrypts + encrypts. ONE worker on purpose — the lockstep already serialized same-lane frames; serializing across lanes trades a few ms of queue latency for zero render-thread contention.
    braid_job_tx: std::sync::mpsc::Sender<Job>,
    /// Handles we've already kicked an avatar download for this session, so we don't re-spawn a fetch every time a conversation is reopened or the contact list re-renders.
    avatar_dl_started: std::collections::HashSet<[u8; 32]>,
    /// Mutual peers we've sent a direct P2P AvatarRequest to, mapped to the eagle-time we sent it. The per-tick sweep asks each mutual peer once, then — if no AvatarResponse has installed an avatar within `AVATAR_P2P_FALLBACK_OSC` — falls back to FGTW. So a friend's avatar comes from the friend first, and FGTW only covers the case where the friend is offline or avatar-less.
    avatar_req_pending: std::collections::HashMap<[u8; 32], i64>,
    /// Request ids WE minted for history pages, rid → (the conversation the request was FOR, sent osc). The AUTHORITATIVE page-match: the per-conversation `in_flight` rid alone starved recovery when two contact rows resolved the same peer (field, 2026-08-10 — a duplicated contact meant the page's token resolved to one conversation while the rid lived on the OTHER's record, so every served page dropped "rid unmatched" forever). A page matching ANY rid here was asked for by us, whatever contact the token resolves to today. Entries are consumed on match and swept by the same in-flight timeout.
    hist_rid_map: std::collections::HashMap<[u8; 32], (crate::types::ConversationId, i64)>,
    /// Fleet-first keygen gate edge state — true while the gate is actively holding friend keygens (logs the hold and the release ONCE each, not per tick).
    keygen_fleet_gate_holding: bool,
    /// Blind-deposit flip-flop detector, keyed (contact hp, depositor device): (hash of the blob the CURRENT stored deposit replaced, consecutive A-B-A flips). Two photon installs sharing one device key wage an S-war — each twin's deposit replaces the other's forever (field 2026-08-21: 400 deposits from ONE device in an afternoon log, each a ~1.5s durable commit, wall-to-wall vault load). At 3 consecutive flips the commits DECIMATE 8:1 (drop-unacked; pure counter, no timer) — the war's write load collapses, a GENUINE re-key still lands within 8 retries, and a byte-identical (stable) deposit resets the detector. Runtime-only: a restart re-arms detection, which is fine.
    blind_flip: std::collections::HashMap<([u8; 32], [u8; 32]), ([u8; 32], u32)>,
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
    /// The FULL screen state the toast was first RENDERED on — captured lazily by the tick; any mismatch means the user navigated, which clears the toast (screen change = acknowledgement). The whole AppState VALUE, not its discriminant: every Settings page shares one discriminant, so page-to-page navigation inside the panel never cleared ("changing from page to page in settings doesn't clear the toast").
    ready_toast_screen: Option<AppState>,
    /// nunc-time clock sanity check: result channel + drain. The worker (one-shot, off-thread) posts the consensus-vs-system offset here; `drain_clock_check` reads it and updates `clock_off`.
    clock_check_tx: std::sync::mpsc::Sender<crate::network::ClockCheckResult>,
    clock_check_rx: std::sync::mpsc::Receiver<crate::network::ClockCheckResult>,
    /// `Some(offset_secs)` when the last consensus said the system clock is off by more than the threshold (consensus − system; positive = system behind). Drives the amber "clock off" banner. Tracks the LATEST verdict, not sticky — a corrected clock clears it on the next clean check.
    clock_off: Option<i64>,
    /// The latest nunc consensus verdict, kept even when the clock is fine: (offset_secs = consensus − system, confidence_secs). The update stamp window's forward-fail tiebreak reads this — the "honest clock" consulted exactly when a manifest claims time travel (docs/updates.md staged clock). `None` until the first successful consensus.
    clock_consensus: Option<(i64, i64)>,
    /// Watches the wall clock against the monotonic clock; a gross unexplained jump (NTP step, long sleep, or an adversary moving the clock after boot) triggers a fresh consensus re-check.
    clock_jump: crate::network::ClockJumpDetector,
    /// Fleet-inbox drain: a one-shot off-thread pull of this identity's pending worker-observed events (bind-attempt alerts, docs/fleet-inbox.md). `drain_fleet_inbox` reads the result and surfaces a notice. Kicked once per attest/resume.
    inbox_check_tx: std::sync::mpsc::Sender<Vec<crate::network::fgtw::FleetInboxEvent>>,
    inbox_check_rx: std::sync::mpsc::Receiver<Vec<crate::network::fgtw::FleetInboxEvent>>,
    /// FGTW connectivity state — flipped by `HandleQuery::try_recv_online`. Drives the top-left chrome orb's colour (red offline / green online). Starts false; the background worker reports the first real status within the first second of launch.
    online: bool,
    /// A clone of the Photon brand orb (chrome's default app_icon) kept so the orb can be RESTORED after a conversation swapped it for the peer's avatar. `None` if the orb asset failed to decode.
    photon_orb: Option<fluor::host::icon::Icon>,
    /// Which contact the top-left orb currently shows (its avatar + their presence-tier ring). `None` = the Photon orb + our own FGTW connectivity ring. Diffed each tick so the Icon rebuild happens only on a change, not every frame.
    orb_contact: Option<usize>,
    /// The FULL derived key the orb was last built from — see update_orb's diff comment. None = brand orb.
    orb_key: Option<(usize, bool, [u8; 64], u32, bool)>,
    /// The contact-list ring colours as last PAINTED, diffed each tick — the same doctrine as the orb's per-tick diff, because ring state is DERIVED (validated_path appearing, reached_via_relay flipping, TTL expiry) and half its inputs mutate without any repaint-marked event: the ring held its old colour until a page change forced a re-raster (field, 2026-08-05). Empty until the first tick.
    painted_ring_tiers: Vec<u32>,
    /// Whether the orb's current contact had an avatar when the orb was last built — part of the diff key so a mid-conversation avatar download upgrades the orb from the gradient placeholder.
    orb_had_avatar: bool,
    /// Contacts-page handle search/add textbox (Ready state). Distinct from `textbox` so content doesn't bleed between Launch (handle being attested) and Ready (handle being added as a contact).
    contacts_textbox: Option<Textbox>,
    /// Plus button to the right of `contacts_textbox` — clicking it (or pressing Enter in the textbox) triggers the add-contact flow (`HandleQuery::search`). Will eventually carry an idle "+" glyph and an in-progress rotating-hourglass animation (legacy port from `compositing.rs`); that lands when `ProgressButton` gets extracted to fluor.
    contacts_plus_btn: Option<Button>,
    /// Conversation-screen message compose box (Conversation state). Distinct from the launch/search boxes so content never bleeds between screens. Enter sends (`submit_message`); the contents encrypt onto the open contact's friendship chain.
    message_textbox: Option<fluor::widgets::MultiTextbox>,
    /// Send button overlaid inside `message_textbox`'s right edge — mirrors the contacts-screen search `+` button (same size, same overlay treatment). Clicking it sends the compose box contents, same as pressing Enter.
    message_send_btn: Option<Button>,
    /// Encrypted local storage — initialized after attestation success with the device secret + handle. Held behind an `Arc` so it can be handed to the avatar background-download/sync threads (a plain `&FlatStorage` borrow can't cross `thread::spawn`); the inner `Mutex<Vault>` makes `Arc<FlatStorage>` `Send + Sync`.
    storage: Option<std::sync::Arc<crate::storage::FlatStorage>>,
    /// Contact list. Populated from `AttestationData.contacts` on attestation success and grown by `submit_add_friend` → `HandleQuery::search` results. Persisted to FlatStorage on add.
    contacts: Vec<crate::types::Contact>,
    /// Conversations, keyed by their participant-set id. A contact row and its DM conversation are 1:1 today, so entries materialize lazily the first time something touches a conversation; nothing assumes the count matches `contacts`, which is what leaves room for zero-contact (notes-to-self) and many-contact (group) sets without changing shape.
    conversations: Vec<crate::types::Conversation>,
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
    /// Devices heard broadcasting LAN discovery under OUR OWN handle recently: (device pubkey, last heard). Feeds AddCandidate::heard_lan; entries older than LAN_HEARD_FRESH are pruned at read.
    lan_heard: Vec<([u8; 32], std::time::Instant)>,
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
    /// One-heal-at-a-time latch for the removal-rotates flow (braid.md §14.2), shared with the key-sync thread and cleared on its exit. Guards both the duplicate-rotation race and the stale-cache window (a plain key sync running mid-heal would re-cache the pre-rotation key over the fresh one).
    fleet_heal_busy: std::sync::Arc<std::sync::atomic::AtomicBool>,
    /// A sibling pair just became egged (Phase A) — mint the next fan-out epoch so that sibling finally gets a wrap. Set by the ceremony drain, consumed by the tick (the rotation needs `&self` off the drain's borrows).
    fanout_grow_pending: bool,
    /// Reader secrets that just changed and need a fresh scoped slot written for them (the ceremony drain cannot do the blocking upload itself). Drained by the tick.
    scoped_regrant_pending: Vec<[u8; 32]>,
    /// Own-avatar recovery deferred until the fleet settings carry the avatar pin (the pin is what addresses and decrypts the published copy). Cleared once the recovery actually spawns.
    self_avatar_recover_pending: Option<[u8; 32]>,
    /// Heal thread → tick: a removal rotation WE won landed; the drain runs the winner-only UI-thread follow-up (avatar bearer-pin rotate). Losers adopt the winner's key off-thread and send nothing.
    fleet_rotated_tx: std::sync::mpsc::Sender<()>,
    fleet_rotated_rx: std::sync::mpsc::Receiver<()>,
    /// Stop flag for the fleet-event subscription task (dropped app / de-attest).
    fleet_evt_stop: Option<std::sync::Arc<std::sync::atomic::AtomicBool>>,
    /// Off-thread contact-fleet refresh results: (contact handle_proof, current member pubkeys folded from their chain, chain-tip eagle time). Drained in tick into the matching contact's `fleet_members`, gated on the tip being fresher than the last adopted one, then `reseed_contact_pubkeys`. Lets a friend's NEW device be honoured — and a REMOVED device revoked — without waiting for our next launch.
    contact_members_rx:
        Option<std::sync::mpsc::Receiver<([u8; 32], Vec<[u8; 32]>, i64, [u8; 32], bool)>>,
    /// Sender half of the contact-fleet-refresh channel, kept alive so successive refreshes reuse one channel (the receiver is drained in tick).
    contact_members_tx:
        Option<std::sync::mpsc::Sender<([u8; 32], Vec<[u8; 32]>, i64, [u8; 32], bool)>>,
    /// Off-thread identity-succession results: `(contact handle_proof, Some(new_genesis) | None)`. A `Some` means `SuccessorRecord::verify_for_pin` passed against the contact's pinned genesis — the drain migrates the pin to `new_genesis` and clears `identity_superseded`, so the re-founded chain re-folds. A `None` just clears the in-flight guard (no record yet, or it failed verification — the pin stays, the contact stays a stranger). See docs/identity-succession.md.
    successor_rx: Option<std::sync::mpsc::Receiver<([u8; 32], Option<[u8; 32]>)>>,
    successor_tx: Option<std::sync::mpsc::Sender<([u8; 32], Option<[u8; 32]>)>>,
    /// Contacts with an in-flight successor check, so a superseded fold that re-arrives each refresh spawns at most one network probe at a time (cleared when its result drains). NOT a permanent suppressor: a later refresh re-probes, so a successor record that publishes AFTER the mismatch is still adopted.
    successor_inflight: std::collections::HashSet<[u8; 32]>,
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
    /// The fleet epoch spine (docs/braid.md §14.4): (k, epoch_k), vault-cached; advances only on checkpoint edges.
    fleet_epoch: Option<(u64, [u8; 32])>,
    /// The immediately-prior epoch, held one checkpoint crossing so in-flight k−1 frames still open.
    fleet_epoch_prev: Option<(u64, [u8; 32])>,
    /// A rotation (or other epoch-worthy edge) happened — the next sweep mints a checkpoint.
    /// Fleet-wide ACTIVE-CLEARER claim (notification design 2026-07-23): the newest (conversation_token, device, osc) claim known — ours or a sibling's, LWW by osc so a new device's claim displaces the old holder without coordination. `None` = nobody claims a screen. A live claim by ANOTHER sibling on a conversation suppresses our ding for its messages (that device is watching); voided when the holder's presence drops (the existing 3-strike offline verdict — crash/sleep coverage without timers).
    fleet_focus_claim: Option<([u8; 32], [u8; 32], i64)>,
    /// Fleet ATTENTION holder (2026-08-18): the device with the human's newest input, (device_pubkey, osc), LWW by osc with device-byte tie-break, Lamport-bumped at the sender so local input supersedes any clock skew. `None` = bootstrap/single-device — every gate defaults to legacy behavior. Frames flow only on the transition edge (a NON-holder receives qualifying input), so the fleet is silent while the human stays put. Both ding-suppression gates require holding attention: a parked-but-focused screen must not silently discharge alert duty while the human is demonstrably at another device. RAM-only; dies with the session. Mutate ONLY through set_fleet_attention (it mirrors the desktop banner-gate atomic).
    fleet_attention: Option<([u8; 32], i64)>,
    /// The one live call (docs/calls.md) — None = no call. Singular by design (v1); a second inbound offer gets an automatic Busy.
    active_call: Option<crate::call::ActiveCall>,
    /// Call overlay controls — retained fluor Buttons (no hand-rolled pills). Painted front-first on EVERY screen (a ring must be answerable from wherever the user is — docs/calls.md); registered cross-screen in `visit_app_widgets` so hover/press/dispatch ride the same walk as every other Button. `call_status_btn` is a non-interactive label chip (full-brightness, never in the walk, never stamped). `call_start_btn` = ☎ Call (conversation, no live call); `call_action_btn` = Answer / Hang up / Keep (phase decides the verb); `call_decline_btn` = Decline / Delete (ringing/ended only).
    call_status_btn: Option<Button>,
    call_start_btn: Option<Button>,
    call_action_btn: Option<Button>,
    call_decline_btn: Option<Button>,
    /// In-call full-screen (Active) + end-screen (Ended) controls — same retained-Button pattern as the four above. `call_speaker_btn` = speaker toggle (stubbed route intent); `call_addhandle_btn` = add-a-handle (stubbed no-op); `call_back_btn` = minimize ("back to contact"); `call_play_btn` = preview/play the recording (Ended, and the history rows).
    call_speaker_btn: Option<Button>,
    call_addhandle_btn: Option<Button>,
    call_back_btn: Option<Button>,
    call_play_btn: Option<Button>,
    /// The Active call is minimized to a strip (Phase 3) / compact bar — the full-screen call panel yields to the screen underneath so messaging + navigation stay live. Reset on every phase start and forced false on Ringing/Ended (always full-screen).
    call_minimized: bool,
    /// Speaker-toggle visual state (stub — no real device route switch in v1).
    call_speaker_on: bool,
    /// Live recording-playback handle (end-screen preview + history rows). Held so the worker keeps running (dropping the handle stops it); a new play or a starting call replaces/stops it.
    call_playback: Option<crate::call::playback::PlaybackHandle>,
    /// Runtime-only stuck-tip ledger per friendship: (the peer's advertised head for OUR lane, exhaust→re-arm ladders seen at exactly that head). The anchor-wedge detector needs tip 0; a NONZERO head that never moves while our exhausted pendings re-arm and exhaust again is the same dead lane in disguise (the peer holds those rows as forwards it can never re-ACK) — two full ladders at one head trips the rotation.
    lane_rearm_cycles: std::collections::HashMap<crate::types::friendship::FriendshipId, (i64, u8)>,
    /// Runtime-only re-serve cap, keyed per (friendship, peer DEVICE): the record's evidence tuple (tip for OUR lane, peer row_count, peer row_digest) plus ROWS ACTUALLY TRANSMITTED against exactly that testimony (2026-09-01: was bursts ATTEMPTED — the serial-send gate let ~1 row out per burst, so 2 attempted bursts spent the cap having served ~2 rows and a deeper hole parked forever; counting transmitted rows keeps the anti-loop convergence while letting the whole deficit drain). Device-keyed because each peer device pongs its OWN lane view — a fid-keyed slot flip-flopped between two devices' tips and reset the cap every cycle (the hours-long 8-rows-per-pong loop, field 2026-08-21). ANY change in the peer's testimony is the new-evidence edge that re-arms the cap; a peer that holds the rows but counts them differently (deleted/edit/reaction rows) goes quiet after the cap instead of bursting forever.
    lane_reserve_bursts: std::collections::HashMap<
        (crate::types::friendship::FriendshipId, [u8; 32]),
        (i64, u32, [u8; 32], u8),
    >,
    /// Rotating start index for the seed-registry resolve walk — the per-pulse device budget used to restart at contact zero every pulse, so head-of-list offline contacts starved everyone behind them of resolution forever.
    pb_resolve_cursor: usize,
    ckpt_mint_due: bool,
    /// Consecutive spineless-hold sweeps (chain has a Checkpoint, custody unreadable, no local spine). Each hold fires a ckpt_req at the siblings; at the third dry sweep the next tick SUPERSEDES the spine — fresh epoch seed pushed at chain_k+1, custody rewritten under the CURRENT fleet key — because a spine nobody alive can read is dead state, not authority (the 2026-08-16 field wedge: the k=1 minter was wiped, the fleet key rotated past its custody seal, and every device held forever while all fleet-plane traffic sat blocked behind the missing spine).
    ckpt_spineless_holds: u32,
    /// Total syncable rows at the last row-cadence checkpoint edge. The fleet sweep flags a mint once the total grows past CKPT_ROW_CADENCE — the burn horizon advances with TRAFFIC, not only membership edges. RAM-only: a restart re-seeds on first observation (delays one cadence, never mints spuriously).
    ckpt_rows_base: Option<usize>,
    /// A mint/bootstrap thread is in flight — the single-minter re-entry guard.
    ckpt_busy: bool,
    /// Outcome channel for the off-thread checkpoint work.
    ckpt_rx: Option<std::sync::mpsc::Receiver<CkptOutcome>>,
    /// One vault probe per session — the spine loads once, not per frame.
    ckpt_loaded: bool,
    /// Last spawn attempt — bounds bootstrap retries to the sweep cadence, not the frame rate.
    ckpt_last_attempt: Option<Instant>,
    /// In-flight fleet-roster pull; its `Ok` result merges into contacts, its `Err` triggers a retry — both drained in `tick`. `Some` = a pull is running, which also debounces re-spawns.
    roster_pull_rx: Option<std::sync::mpsc::Receiver<Result<fgtw::fstate::FleetState, String>>>,
    /// In-flight fleet-roster PUSH completion, drained in `tick`. Each push is a whole pull-merge-seal-put round trip, and launch fires many push edges back to back (re-push, weave claims, keepalive stamps, pong adoptions, reconciles) — ungated they ran CONCURRENTLY, racing each other's merge base (the reason live settings must ride along) and each one's fstate event re-pulled every sibling: 19 pushes in the first 21 seconds of a field launch (2026-08-16). `Some` = one push runs; further requests set `roster_push_queued`.
    roster_push_rx: Option<std::sync::mpsc::Receiver<()>>,
    /// A push edge fired while one was in flight — the completion drain fires ONE follow-up that snapshots the roster fresh, so every bump that landed meanwhile rides a single push.
    roster_push_queued: bool,
    /// Message-table persist worker: conversation snapshots go over this channel to ONE background thread that coalesces (latest snapshot per conversation id wins) and writes. `save_messages` is a full encrypted table rewrite — on the UI thread it was the named 600ms–5.7s stall behind every ChatMessage/MessageAck arm; off it, an ack is a field flip. Each item carries the eagle_times of rows AWAITING THE DURABLE EDGE (self rows born faint); the writer reports their verdict over `persist_done` and coalescing carries a superseded snapshot's rows onto its replacement — the newer snapshot contains them too, so one durable write answers for all.
    persist_tx: Option<
        std::sync::mpsc::Sender<(
            crate::types::Conversation,
            std::sync::Arc<crate::storage::FlatStorage>,
            Vec<i64>,
        )>,
    >,
    /// The message writer's durable verdicts riding back to the UI thread — drained each status pass (drain_persist_done): success flips the named rows bright + releases their sibling push; failure toasts and leaves them faint. The (tx, rx) pair lives for the whole app so a respawned writer keeps the same return path.
    persist_done: (
        std::sync::mpsc::Sender<MessagesDurableVerdict>,
        std::sync::mpsc::Receiver<MessagesDurableVerdict>,
    ),
    /// Chains persist worker: the SAME coalescing shape as `persist_tx` but for FriendshipChains, keyed by friendship id. The safe-to-delay saves (ACK pending-removal, chain-sync adopt) ride with no attachments; the two COMMIT-POINT saves ride with their gated signal attached (see ChainsPostDurable) — the writer fires the receive's ACK / the send's transmit only after the durable write lands, so persist-before-signal holds with zero encrypt+IO on the UI thread. Coalescing merges a superseded snapshot's signals into its replacement: the newest snapshot contains every advance the older one did.
    chains_persist_tx: Option<
        std::sync::mpsc::Sender<(
            crate::types::friendship::FriendshipChains,
            std::sync::Arc<crate::storage::FlatStorage>,
            Vec<ChainsPostDurable>,
        )>,
    >,
    /// Conversation-state persist worker: the 13-byte unread + history-cursor record, coalesced newest-per-address. The payload is tiny but the vault write is still an encrypt + file IO stall — and a history walk fired one per merged page on the render thread.
    conv_state_persist_tx: Option<
        std::sync::mpsc::Sender<(
            [u8; 32],
            [u8; 13],
            std::sync::Arc<crate::storage::FlatStorage>,
        )>,
    >,
    /// Peer-store (phonebook) persist worker: a cloned row snapshot + keypair + storage + vault addr go to ONE background thread that verifies every row, encodes, and writes. The per-row ed25519 `verify()` over a few-hundred-row store was a measured 0.5–4.4s UI-thread freeze on debug mobile (every persist, and the LAN-flap edge fired it every beacon round). Coalesced newest-wins — one store, so a queued snapshot is always superseded.
    peer_persist_tx: Option<
        std::sync::mpsc::Sender<(
            Vec<crate::network::fgtw::PeerRecord>,
            crate::network::fgtw::Keypair,
            std::sync::Arc<crate::storage::FlatStorage>,
            [u8; 32],
        )>,
    >,
    /// The phonebook has unpersisted changes. Set by `request_peer_persist`, cleared by the tick's debounce gate (`PEER_PERSIST_DEBOUNCE`) which fires the off-thread write. Coalesces a burst of gossip merges / address re-publishes into at most one write per interval — the store is a cache, so a delayed write loses nothing a re-exchange won't restore.
    peer_persist_dirty: bool,
    /// When the phonebook was last flushed to the worker — the debounce clock for `peer_persist_dirty`.
    last_peer_persist: Option<std::time::Instant>,
    /// Recently observed OWN LAN addresses (from our looped-back discovery beacons) with last-seen times. A multi-homed device loops a beacon back on EVERY interface each round, so `our_lan_ip` used to flip between them every round — and each flip cleared `self_record_published_for`, re-signing + re-publishing + persisting the record on a loop. This set makes the published choice sticky: keep the current address while it's still observed, only switch when it ages out. See `OurLanAddrObserved`.
    our_lan_ips: std::collections::HashMap<std::net::Ipv4Addr, std::time::Instant>,
    /// Last zoom value actually persisted. Android saves on the pinch-release edge (onScaleEnd → take_scale_ended); desktop saves on modifier release. This tracker suppresses redundant re-saves of the restored value.
    zoom_saved_ru: f32,
    /// Monotonic tick counter — the frame-gap fence for `pending_chain_sends` (see `drain_pending_chain_sends`).
    tick_serial: u64,
    /// Outgoing sends whose WIRE half is deferred: (contact idx, text, eagle_time, tick_serial at enqueue). The pending-grey bubble is inserted synchronously in `send_chain_message`; chain_transmit (weave selection, braid advance, chains persist, PT dispatch) runs from this queue AFTER the frame presents — running it inline meant the bubble, though inserted first, couldn't render until the whole wire half finished (the "message goes into the void" report).
    pending_chain_sends: Vec<(
        usize,
        String,
        i64,
        Option<(crate::types::RefKind, i64)>,
        Option<crate::network::message_package::BridgeWire>,
        u64,
    )>,
    /// Unique identities the seed knows (including us), off the latest signed announce ack. The Ready-screen count reads this as a floor: the peer STORE only fills by gossip now, so on a fresh session it holds nothing but our own record and would show "0 peers" to a user who can see nine friends in the contact list.
    seed_identity_count: u32,
    /// In-flight phonebook resolution: `(handle_proof, device_pubkey, public, lan)` for devices we could not otherwise address. The handle_proof binds each record to the identity it was resolved FOR, so a friend can adopt a registry-vouched device it has never met. Drained in `tick` into `device_endpoints`, which is what candidate gathering reads. `Some` = a resolve is running, which debounces re-spawns so a stalled seed can't stack requests.
    pb_resolve_rx: Option<
        std::sync::mpsc::Receiver<
            Vec<(
                [u8; 32],
                [u8; 32],
                std::net::SocketAddr,
                Option<std::net::SocketAddr>,
            )>,
        >,
    >,
    /// The linked-settings cache (per-device maps + link-to-global; docs/global-vault.md). Lazily loaded from the vault once storage + device key exist; merged from every fstate pull; every local set persists + pushes.
    fleet_settings: Option<crate::storage::fleet_settings::FleetSettings>,
    /// RAM copy of the vault's fleet key, shared with every key-writer thread. `fleet_key_cached` used to read the vault on EVERY call — and it runs once per inbound history page on the UI thread, where each read stalls behind the async writers' kete commits during a backfill storm (88-559ms status passes = the laggy window drag, field 2026-08-13). Writers refresh this beside each key write; the UI never touches the vault for a key it already holds.
    fleet_key_ram: std::sync::Arc<std::sync::Mutex<Option<[u8; 32]>>>,
    /// Set on each attest/resume: "do one roster pull as soon as the fleet key is available." The key is written by an ASYNC fan-out sync, so an immediate pull races it and loses — this flag makes tick fire the pull the moment `fleet_key_cached()` goes Some, which is the wake-up catch-up that brings a friend added on a sibling device onto this one.
    needs_initial_roster_pull: bool,
    /// Retry budget for the initial roster pull. A fresh device's pairing-recovered key is a PRE-rotation generation (adding a device rotates the fleet key via the fan-out re-key), so the first pull decrypts the current roster with a stale key and fails `aead::Error`. The in-flight `spawn_fleet_key_sync` writes the current key within ~150ms, so on a failed pull we re-arm `needs_initial_roster_pull` and retry — the pull's own ~150ms round-trip naturally spaces attempts, and this budget caps them so a genuinely-undecryptable roster gives up instead of spinning (next fleet event / relaunch re-tries).
    roster_pull_retries_left: u8,
    /// A failed roster pull parks HERE with the fleet key it failed under (None = no key was held); the tick re-fires the pull exactly when the cached key CHANGES — the key-adoption edge, never a timer (the old loop burned its whole budget before the key ever landed).
    roster_pull_parked_under: Option<Option<[u8; 32]>>,
    /// True once the pull budget above ran dry WITHOUT a successful pull — the B4 convergence hole: with the WebSocket down there is no "next fleet event", so an exhausted device had no roster until relaunch. The 45s fleet-refold edge reads this and re-arms a small budget, making the existing poll the backstop. Cleared on success and on each re-arm.
    roster_pull_exhausted: bool,
    /// This device's avatar in BT.2020 γ=2.0 u8 RGB, sized `crate::avatar::AVATAR_SIZE × AVATAR_SIZE × 3`. `None` until `on_query_result` pulls one from local storage (no saved avatar = stays `None`, Ready screen falls back to the grey placeholder).
    device_avatar_pixels: Option<Vec<u8>>,
    /// Cached Mitchell resize of `device_avatar_pixels` at the current Ready-screen circle diameter. Rebuilt on diameter change (resize / zoom).
    device_avatar_scaled: Option<Vec<u8>>,
    /// Diameter (in pixels) of `device_avatar_scaled`. `0` means no cache built yet.
    device_avatar_scaled_diameter: usize,
    /// HitId reserved for the Ready-screen self-avatar circle. Allocated in `init` alongside the other widget IDs; stamped into `chrome.hit_test_map` during the Ready render so a tap on the circle dispatches to the avatar code path (open the image picker on Android).
    avatar_hit_id: HitId,
    /// KnownHandle fork pills — pick-another-name / it's-mine (docs/lifecycle.md D1). Plain hit rects, Pressed-arm dispatch.
    known_pick_hit: HitId,
    known_mine_hit: HitId,
    /// JOINER SELECTED (docs/lifecycle.md): this just-bound device floods green with "Selected!" and HOLDS until the sponsor's confirm rotation releases the fleet key — the green the far-side human is asked to verify. Cleared when sign-in proceeds.
    joiner_selected: bool,
    /// One-shot absolute-zoom restore (the persisted per-device `display.zoom`), handed to the host via `FluorApp::take_zoom_request`. Set when settings load; the host applies + clears it.
    pending_zoom_restore: Option<f32>,
    /// Has the persisted zoom restore already been armed this process? The restore is a STARTUP action, but its trigger (`apply_settings_to_ui`) also runs on every fleet merge, so this latch is what keeps a ~15s poll from re-zooming the window forever. Set on the first `apply_settings_to_ui` regardless of whether a value existed.
    zoom_restored: bool,
    /// The picked avatar's display pixels, arriving from the OFF-THREAD set pipeline (decode runs there too — a 50MP photo must not stall a frame). Installed + repainted in tick.
    avatar_set_rx: Option<std::sync::mpsc::Receiver<Vec<u8>>>,
    /// One-shot Android image-picker request. Set when the user taps the avatar; consumed by the JNI poll (`nativePollAvatarPicker`) which signals the Activity to launch `ACTION_GET_CONTENT`. Stays `None` on idle frames so the Activity doesn't churn.
    pending_picker_request: bool,
    /// One-shot signal for the Android sticky session broadcast: 1=send, -1=clear, 0=nothing. Set by attest success and []n nuke.
    pending_broadcast_signal: i8,
    /// Android sticky-session-broadcast freshness timer. `None` = ensure on the next tick (fresh resume/attest); else the next eagle-time-jittered deadline to re-check. On each firing the poll signal goes to `2` ("ensure": Kotlin READS the sticky and re-posts ONLY if the OS evicted it — Samsung drops stickies aggressively, so this keeps the reinstall-survival capsule alive without churning a re-post every interval). Jittered 30–60 min so a fleet doesn't re-post in lockstep.
    next_session_broadcast: Option<Instant>,
    /// The OPEN conversation, by its participant-set id — never an index. An id stays valid across every roster mutation; the index it replaces needed a shift-fixup on tombstone removal and defensive `ci < len` filters at every read, and one missed fixup silently rendered someone else's conversation.
    active_conversation: Option<crate::types::ConversationId>,
    /// Base hit ID for contact rows. Row `i` gets `contact_hit_base + i`. Allocated in `init` after the other widget IDs.
    contact_hit_base: HitId,
    /// Hit ID for the "← Contacts" back button on the Conversation screen.
    back_btn_hit_id: HitId,
    /// Hit ID for the "Start fresh (wipe this device)" line on the JOIN words screen — a removed device's only self-clean path (it can't attest → can't reach Security).
    join_startfresh_hit_id: HitId,
    /// "Copy words" tappable on the JOIN words screen — puts the space-separated pairing words on the clipboard so they can ride any channel (email, messenger) to the device that types them, instead of being read + retyped by hand.
    join_copywords_hit_id: HitId,
    /// Interaction-state for the copy affordance label ("copy words" → "copied — paste them on your other device"). Cleared when the flow ends, never by a timer (no time-based UI).
    join_words_copied: bool,
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
    /// Hit id currently under the cursor, tracked across `CursorMoved` so hover only re-walks the widgets (and repaints) when it actually changes. Also the source of truth for [`Self::cursor_for`]'s I-beam decision.
    hover_hit: HitId,
    /// Whether `hover_hit` is a text-entry widget (drives the I-beam). Set by the hover walk via `Widget::is_text_input`, so it covers every textbox on every screen with no hand-list.
    hover_is_textbox: bool,
    /// Left button currently held. Gates the textbox text-pan in `CursorMoved`.
    pointer_down: bool,
    /// The textbox hit id a press engaged for the text-pan (HIT_NONE if the press wasn't on a textbox). Set on press, cleared on release — the ONE bit of state the drag needs, works for every box via `textbox_by_hit_mut`. While live, pane-scroll (wheel / touch-drag synth) is suppressed: the finger owns the TEXT, not the page.
    drag_select_hit: HitId,
    /// Text-pan grab: pointer x and the box's scroll offset at press — the drag pans the text to `grab_scroll + (x − grab_x)`, so the grabbed character (and the caret on it) stays under the finger.
    pan_grab_x: Coord,
    pan_grab_scroll: Coord,
    /// Last textbox press (id + time) for multi-click detection — a second/third press on the same box within the OS double-click interval escalates to word / all select.
    last_click_hit: HitId,
    last_click_time: Option<Instant>,
    /// 1 = single, 2 = double (word), 3 = triple (all). Resets when the streak breaks.
    click_streak: u8,

    // --- Settings panel (STUB) ---
    /// Base hit id for the settings nav-rail rows. Row `i` (page `SettingsPage::ALL[i]`) stamps `settings_nav_base + i`. Allocated in `init`.
    settings_nav_base: HitId,
    /// Contact-panel action pills (slot 0 = Boot). Allocated alongside the settings blocks.
    contact_panel_btn_base: HitId,
    /// Contact-panel nav-rail rows. Row `i` (page `ContactPage::ALL[i]`) stamps `contact_nav_base + i`.
    contact_nav_base: HitId,
    /// Boot two-tap arm (event-shown, interaction-cleared — any other press on the panel disarms).
    contact_boot_armed: bool,
    /// One-shot residency bypass: Shift+Escape sets it so the next close-requested actually exits instead of hiding.
    exit_requested: bool,
    /// Live shift-key mirror (refreshed each `on_event`) so `on_close_requested` — which gets no Context — can honor "shift+close = real exit".
    shift_held: bool,
    /// Base hit id for the conversation's visible message rows (fixed span 64). The render stamps `msg_hit_base + visible_row` per drawn row and rebuilds [`Self::msg_hit_rows`] in lockstep, so a tap resolves to the message with no knowledge of scroll or backfill. Tap = select (details strip: direction, age, delivery, copy); tap the same again = deselect.
    msg_hit_base: HitId,
    /// Hit id for the selected message's "copy" pill inside the details strip.
    msg_copy_id: HitId,
    /// Base hit id for the rest of the details-strip action pills (span 8): 0=reply, 1=edit, 2=resend, 3=delete. Copy keeps its own id above.
    msg_action_base: HitId,
    /// Visible-row → message identity map, rebuilt each conversation render, parallel to the `msg_hit_base` stamps. The identity is (timestamp, is_outgoing) — index-free so a mid-frame history backfill can't redirect a tap.
    msg_hit_rows: Vec<(i64, bool, Option<(f32, f32, i64)>)>,
    /// The list viewport height at the last conversation render — the scroll-to-message centering math needs it outside the render pass (same pattern as msg_max_scroll).
    msg_view_h: f32,
    /// Compose-box line count at the last frame — the growth edge: a change moves list_bottom, so the next frame reflows the whole scene while ordinary keystrokes stay on the narrow path.
    painted_compose_lines: usize,
    /// The message whose details strip is open: (contact idx, timestamp, is_outgoing). Keyed by identity, not list index, so backfills can't shift the selection. `None` = no strip.
    selected_msg: Option<(usize, i64, bool)>,
    /// The open strip's copy pill has fired (text on the clipboard): pill turns green + reads "copied". Event-cleared — reset whenever the selection moves or closes, never on a timer.
    selected_msg_copied: bool,
    /// Deferred delete: ((contact idx, timestamp, is_outgoing), painted). The press only ARMS this and repaints — the strip shows "deleting…" on that frame — and the tick performs the actual removal + mirror-verified persist AFTER the feedback frame painted (the synchronous save blocked the UI for a beat, reading as stuck).
    pending_delete: Option<((usize, i64, bool), bool)>,
    /// Armed reply target: the eagle_time the next send REFERENCES (a reply row, not a quote). Set by the strip's reply pill; the compose strip shows the referenced message at half alpha. Cleared on send, Esc, or conversation switch.
    compose_reply_to: Option<i64>,
    /// Armed edit target: the eagle_time of OUR row the next send SUPERSEDES. The compose box prefills with the current body and the send button trades its arrowhead for a check mark. Cleared on send, Esc (which also clears the prefill), or conversation switch.
    compose_edit_of: Option<i64>,
    /// Armed custom-reaction target (the strip's circled "+"): the next send is a REACTION row carrying whatever short string is typed — the system keyboard is the emoji picker. Cleared like the reply/edit arms.
    compose_react_to: Option<i64>,
    /// Base hit id for the details-strip reaction row: glyph pills 0..=8, the circled "+" at 9.
    react_strip_base: HitId,
    /// The glyphs drawn on the reaction row last frame, in ranked order — the tap handler maps pill slot → glyph thru this (ranking can shift as counts change).
    react_strip_glyphs: Vec<String>,
    /// Conversation top-bar slide-off in PIXELS (0 = fully shown): the "‹ Contacts" strip slides out/in WITH the scroll gesture, browser-toolbar style — pure scroll-delta accumulation, no timers, clamped to the bar height in the wheel arm and at render. Reset on conversation open.
    conv_topbar_off: f32,
    /// Word-wrap cache for the conversation's message list: key (contact idx, message count, avail_w bits, msg_size bits) + the wrapped line STRINGS per visible message (chronological, probes excluded) + the total line count. Rebuilt only when the key changes (resize / zoom / new message / conversation switch). Caching the STRINGS (not just counts) means scroll frames do ZERO text shaping — the per-frame re-wrap of drawn messages was the "glitches and sticks" scroll regression.
    msg_wrap: Option<((usize, usize, usize, u32, u32), Vec<Vec<String>>, usize)>,
    /// Last IME inset applied to the layout (Android) — the tick diffs the JNI mirror against this and relayouts on change, since the keyboard no longer produces resize events.
    #[cfg(target_os = "android")]
    last_ime_inset: i32,
    /// One-shot per session: the settings + roster re-push once hp/keypair/fleet-key are ALL available. Saves made before the keys settled bailed silently (see spawn_settings_push), and nothing ever retried — so the fleet slot could sit stale/empty forever (the empty-profile restore, 2026-07-26).
    settings_repushed: bool,
    /// Rate limit for the pong-seal reseed triggered by unopenable tails (self-heal for ordering races on a fresh device).
    last_seal_reseed: Option<Instant>,
    /// Last periodic fleet-history-sweep kick. The edge-triggered kicks (roster merge, sibling-online) cover the common cases, but edges get missed (presence flaps, app-in-background misses); this jittered ~5 min re-arm is the convergence backstop — cheap because a complete conversation early-stops after ONE page.
    last_fleet_sweep: Option<Instant>,
    /// Fleet chain-replication bookkeeping: per-friendship, the `mutated_osc` we last PUSHED to siblings (or last ADOPTED from one — recording the adopted stamp stops the echo). The per-tick `drive_chain_replication` sweep pushes any chain whose live stamp is newer; comparing stamps instead of hooking every mutation site coalesces bursts and covers every path (send, ACK, receive, ceremony completion, reset) for free.
    chain_pushed_osc: std::collections::HashMap<[u8; 32], i64>,
    /// Tokens we fired a `chain_pull` for (Complete-without-chains, siblings exist) — once per session, so the fleet gets ONE ask before any re-key verdict. RAM-only.
    chain_pull_sent: std::collections::HashSet<[u8; 32]>,
    /// Per-token miss answers: sibling device pubkeys that replied `chain_pull_miss`. When every live sibling contact has a device in the set, the fleet truly holds nothing and the re-key runs. RAM-only.
    chain_pull_misses: std::collections::HashMap<[u8; 32], std::collections::HashSet<[u8; 32]>>,
    /// Per-LANE replication bookkeeping: (friendship_id ‖ lane_label) → the lane position we last pushed to siblings. `drive_chain_replication` sends ONLY the lanes whose position advanced past this, as a per-lane checkpoint subset — so a mutation on one lane no longer re-transmits every other lane's 16KB chain (the 85KB whole-blob frame that stalled the render thread every tick).
    lane_pushed_pos: std::collections::HashMap<[u8; 64], u64>,
    /// Base hit id for the settings stub action pills (immediate-mode Buttons — Add device, Lock, Shred, Snapshot, …). Each page draws its pills over a small contiguous slice of this range; clicks land here and log a stub line. Allocated in `init` with a fixed span.
    settings_btn_base: HitId,
    /// Appearance-page theme selector — a real fluor `Dropdown`. Only in the widget walk while the Settings/Appearance page is up.
    settings_theme_dropdown: Option<fluor::widgets::Dropdown>,
    /// Appearance-page zoom / text-size control — a real fluor `Slider`.
    settings_zoom_slider: Option<fluor::widgets::Slider>,
    /// Live PT transfer progress (peer, done, total, outbound) — throttled push from the status thread; drives the pill progress bar.
    attach_progress: Vec<(std::net::SocketAddr, u32, u32, bool)>,
    /// Blob pushes confirmed landed (attach_have), this session.
    attach_confirmed: std::collections::HashSet<[u8; 32]>,
    /// Android: set when the paperclip asks for the system file picker; drained by nativePollAttachPicker.
    pending_attach_picker: bool,
    /// Bridge executor channels (host side): commands go to the off-thread dispatcher that routes to one worker+shell per sibling device; FINAL outputs come back here for `drain_bridge_output` to reply with. Lazily created on the first command so a fleet that never bridges spawns nothing. Desktop-unix only (the shell host).
    #[cfg(all(unix, not(target_os = "android"), not(target_os = "redox")))]
    bridge_cmd_tx: Option<std::sync::mpsc::Sender<bridge::BridgeJob>>,
    #[cfg(all(unix, not(target_os = "android"), not(target_os = "redox")))]
    bridge_out_rx: Option<std::sync::mpsc::Receiver<bridge::BridgeEmit>>,
    /// Latest-wins streamed-snapshot slot per contact (host side) — the UI drain paces the wire, a burst collapses to the newest snapshot, no clock anywhere.
    #[cfg(all(unix, not(target_os = "android"), not(target_os = "redox")))]
    bridge_partials: Option<
        std::sync::Arc<std::sync::Mutex<std::collections::HashMap<usize, bridge::BridgeEmit>>>,
    >,
    /// Last wire-send instant per conversation for streamed PARTIALS — the ONE granted timer (Nick 2026-08-31): at most one partial per second per conversation; the latest-wins slot holds anything faster. Finals never consult it.
    #[cfg(all(unix, not(target_os = "android"), not(target_os = "redox")))]
    bridge_partial_sent: std::collections::HashMap<usize, std::time::Instant>,
    /// The interrupt registry (host side): device → (foreground-job pgid handle, bash pid), so a Stop arriving while a worker is blocked draining output can signal the command's own process group directly.
    #[cfg(all(unix, not(target_os = "android"), not(target_os = "redox")))]
    bridge_fg: Option<bridge::BridgeFgMap>,
    /// CLIENT side, any platform: the latest locus the bridge host reported — (device, host name, cwd) — rendered as the strip above the compose box so the operator is never blind to where commands land (field 2026-08-23: a pull meant for photon ran in keys/).
    bridge_locus: Option<([u8; 32], String, String)>,
    /// CLIENT side: Stop-press escalation for the in-flight command — (command eagle_time, presses so far); each press walks SIGINT → SIGTERM → SIGKILL, reset when a new command starts.
    bridge_int: Option<(i64, u8)>,
    /// Recovery-page "be a custodian" opt-in — a custom `Checkbox`.
    settings_custodian_check: Option<fluor::widgets::Checkbox>,
    /// Notifications-page global chime on/off — a custom `Checkbox`.
    settings_chime_check: Option<fluor::widgets::Checkbox>,
    /// About-page "Dozenal numbers" toggle — fleet-wide (`display.dozenal`, linked). Mirrors into the render-edge `crate::DOZENAL_UI` static on flip + settings load.
    settings_dozenal_check: Option<fluor::widgets::Checkbox>,
    /// Notifications: vibrate on new message (`notify.vibrate_msg`) + the call pair (`notify.ring_call` / `notify.vibrate_call`). Persisted fleet-wide; enforcement is the alert paths' to honor (Android vibration rides Kotlin — follow-up).
    settings_vibrate_msg_check: Option<fluor::widgets::Checkbox>,
    settings_ring_call_check: Option<fluor::widgets::Checkbox>,
    settings_vibrate_call_check: Option<fluor::widgets::Checkbox>,
    /// Notifications-page presence-visibility toggle — a custom `Checkbox`.
    settings_presence_check: Option<fluor::widgets::Checkbox>,
    /// Updates-page auto-update on/off — a custom `Checkbox`.
    settings_autoupdate_check: Option<fluor::widgets::Checkbox>,
    /// Diagnostics "Hard logs" toggle (`logs.hard`): ON = every record writes thru to disk; OFF (default) = RAM-batched with edge flushes — see lib.rs LOG_HARD.
    settings_hardlogs_check: Option<fluor::widgets::Checkbox>,
    /// Desktop "Run in background" toggle (Notifications page): the OS autostart artifact IS the stored state (`platform::autostart` — no vault setting to desync), and `resident_mode` follows it live. Never built on Android (the OS owns app lifecycle there).
    settings_background_check: Option<fluor::widgets::Checkbox>,
    /// Security-page "Auto-attest on reboot" toggle — DANGEROUS, off by default. Marker file (`unattended_reboot`) + a device-bound reboot capsule; defeats the deliberate reboot-death of the session for remote failsafe boxes. See `set_unattended`.
    settings_unattended_check: Option<fluor::widgets::Checkbox>,
    /// Handle-confirmation modal for the unattended toggle: `Some(target_on)` while the operator must re-type their handle to arm (true) or disarm (false) unattended mode. Arming/disarming this device-becomes-you switch from an already-unlocked session must still prove the operator — not just whoever walked up to the screen. `None` = no modal.
    unattended_confirm: Option<bool>,
    /// The handle-entry box for the unattended-confirm modal (built lazily on first open).
    unattended_confirm_tb: Option<Textbox>,
    /// Hit-id base for the confirm modal pills (confirm / cancel). Allocated in init.
    unattended_confirm_base: HitId,
    /// Last confirm attempt mismatched — shows a red "handle didn't match" line until the next edit.
    unattended_confirm_failed: bool,
    /// In-flight arm/disarm handle-proof check: the ~1s MEMORY-HARD proof runs OFF the UI thread (spawned on the confirm click), and this carries (verdict receiver, target_on) until `tick` drains it. `Some` = verifying (re-clicks ignored). Matching the microsecond identity SEED here — as it did before — made "make this box become you" a cheap brute-force oracle; the proof is memory-hard on purpose.
    unattended_verify: Option<(std::sync::mpsc::Receiver<bool>, bool)>,
    /// Auto-attest-on-reboot is armed — cached at construction and moved by the settings toggle, because the truth lives in a device-vault flag and the banner renders every frame (Nick 2026-08-25: a box that attests without a handle must SAY so on screen, always, or the arming gets forgotten).
    unattended_on: bool,
    /// Desktop resident mode: close hides the window instead of exiting (`FluorApp::on_close_requested`), the process keeps serving the network, and a second launch (or a future tray click) surfaces it via the control channel. True when launched `--background` or when the autostart artifact exists; the settings toggle moves it live.
    resident_mode: bool,
    /// The tray icon exists (once per process — a re-spawn would park a second orb). Set on the resident-at-launch spawn or the first toggle-on; toggle-off leaves the icon until exit (v1 — despawn needs a service handle plumb-thru).
    #[cfg(not(target_os = "android"))]
    tray_spawned: bool,
    /// The bell string this session last published to the worker (Android: `fcm:<project>:<token>`), so the ping-cycle publish is a no-op until the token rotates. `None` = nothing published yet. Read+written only on Android (the doorbell publish is `#[cfg(target_os = "android")]`); the field exists on every platform to keep the struct shape uniform.
    #[allow(dead_code)]
    published_bell: Option<String>,
    /// Launched with `--background` (the login-item invocation): the host creates the window invisible (`FluorApp::start_hidden`) and nothing shows until a ShowWindow surfaces it.
    start_in_background: bool,
    /// Diagnostics log viewer: `true` = the page body is the scrollable decoded-record list instead of the Clear/Snapshot/Submit controls. Toggled by the "View"/"Hide" pill.
    diag_log_view: bool,
    /// Decoded records currently held by the viewer (shared decode with the `photonlog` bin — [`crate::parse_log_records`]), capped to the newest [`DIAG_LOG_MAX_ROWS`].
    diag_log_rows: Vec<crate::LogRecord>,
    /// Byte offset into the on-disk log of the last COMPLETE record decoded — the tail-follow cursor. A shrink (rotation/Clear) re-syncs from zero.
    diag_log_consumed: u64,
    /// Off-thread initial decode in flight (a full 16 MiB parse must not stall a frame); the result lands here and is drained in tick.
    diag_log_rx: Option<std::sync::mpsc::Receiver<(Vec<crate::LogRecord>, usize)>>,
    /// `true` = the view is pinned to the newest record (scroll rides the extent as records append). Scrolling up unpins; scrolling back to the bottom re-pins.
    diag_log_follow: bool,
    /// Earliest eagle time the next tail-follow poll may run (the on-disk size probe is an atomic, but the seek+decode shouldn't run every frame).
    diag_log_next_poll_osc: i64,
    /// INSPECT drill-down: a tapped record's index + its pretty-printed VSF as coloured span-lines (parsed once from `vsf::inspect_vsf`'s ANSI output). `None` = the text list. Back returns to the list.
    diag_log_inspect: Option<(usize, Vec<Vec<(String, u32)>>)>,
    /// Diagnostics-page optional-note field — a real fluor `Textbox` (distinct from the launch / contacts / compose boxes so content never bleeds).
    settings_note_textbox: Option<Textbox>,
    /// You-page profile editor: one box per field (display name, first, email, custom fields, …), grouped by taxonomy tier and prefilled from the fleet `profile.<id>` settings on page-open. "Save profile" writes every changed field in one batched push. HitId is scarce (u16) so this is built ONCE (lazily, on first open) and never rebuilt — custom fields append.
    you_fields: Vec<ProfileField>,
    /// The "add a custom field" entry box: type a label (e.g. "Address 2") → Add registers it in `profile._custom` and appends a new field box.
    you_add_textbox: Option<Textbox>,
    /// Reset to false on each entry to the You page; the layout pass reloads every field box from the current settings (so a fleet-synced edit shows) and flips it true. Prevents the per-frame reload from clobbering in-progress typing.
    you_fields_loaded: bool,
    /// Fleet-page device management: the device pubkey the user tapped to select (highlighted row). `None` = nothing selected. Only OUR OTHER devices (siblings) are selectable — never this device. Remove-other retired (sovereign records: self-signed departure only; eviction = withholding at the key layer, arriving with the device-trust bundle) — selection currently feeds only the future rename.
    settings_fleet_selected: Option<[u8; 32]>,
    /// Fleet-page retired inventory (identity never dies): devices the chain shows signed OUT but whose hardware brand this identity still holds — brands survive departure; freeing one takes the owner's member-signed `device_release`. Refreshed synchronously on each Fleet-page entry; rows render "retired — still yours" with a per-row Release pill.
    fleet_retired: Vec<[u8; 32]>,
    /// The retired pubkey whose Release pill is two-tap armed (`None` = disarmed). Cleared on page switch like every destructive arm.
    fleet_release_armed: Option<[u8; 32]>,
    /// The sibling pubkey whose "Lock out" pill is two-tap armed (treat-as-stolen). Cleared on page switch like every destructive arm.
    /// Has-cached-avatar probe results by party id — the avatar sweep's answer, remembered. The sweep runs every tick and its per-contact `read_addr` probe takes the VAULT MUTEX, which background persist workers hold thru whole encrypted-table writes — so the innocent per-tick read turned every writer hold into a UI stall (the 2026-08-15 400-1794ms "status pre-loop" field log). One probe per contact per session; cleared wholesale on any avatar install / pin adoption (rare edges), costing one re-probe each.
    avatar_probe_cache: std::collections::HashMap<[u8; 32], bool>,
    /// Egged-status probe results by sibling device key — fleet_device_rows' fanout_pairs::load answer, remembered. The Fleet page gathers rows every FRAME, so the per-sibling vault read was a per-frame librarian round trip; a pair secret changes only at ceremony completion, which invalidates its entry there. Same doctrine as avatar_probe_cache: the UI thread's steady state touches the vault zero times.
    egged_cache: std::collections::HashMap<[u8; 32], bool>,
    /// One-shot window-geometry restore — a fluor `window_rect` (x, y, w, h in GLOBAL desktop units), armed with the zoom at settings load; the host applies it thru its maximize machinery, clamped into live surfaces.
    pending_geometry_restore: Option<(i32, i32, u32, u32)>,
    fleet_lock_armed: Option<[u8; 32]>,
    /// Two-tap arm state for the Unlock pill (the lock pill's mirror).
    fleet_unlock_armed: Option<[u8; 32]>,
    /// An armed UNLOCK awaiting handle confirmation: (device to unlock, armer's handle_proof, display name). Fires in the attest-success path exactly like `pending_lock` — the owner re-proves the handle, then the reversal executes.
    pending_unlock: Option<([u8; 32], [u8; 32], String)>,
    /// Hit id for the Locked dead-end screen's retry pill ("unlocked from another device? tap to retry") — returns to the normal bound-resume launch entry; the handle is only ever typed on the standard attest screen, never on the dead-end itself.
    locked_retry_hit: HitId,
    /// A lock-out confirmed on the pill but NOT yet executed: (target device pk, the arming identity's handle_proof, display name). The confirm de-attests THIS device and the lock fires only inside the next successful attest — so the lock is gated on KNOWING the handle: a thief holding an unlocked device just logs themselves out (Nick's design, 2026-08-05). Runtime-only, deliberately never persisted — a restart discards the armed lock, so a thief can't park one for the owner's next sign-in to trip.
    pending_lock: Option<([u8; 32], [u8; 32], String)>,
    /// Self-update state (docs/updates.md): off-thread check/apply results drain here. tx kept so both channel checks + an apply share ONE receiver.
    update_rx: Option<std::sync::mpsc::Receiver<UpdateEvent>>,
    update_tx: Option<std::sync::mpsc::Sender<UpdateEvent>>,
    /// Keep-transcode results (worker → UI): a finished N-channel recording posts here for the `call.audio` row mint. Lazily created on first keep (see `call_keep_sender`).
    call_keep_rx: Option<std::sync::mpsc::Receiver<call_ui::CallKeepResult>>,
    call_keep_tx: Option<std::sync::mpsc::Sender<call_ui::CallKeepResult>>,
    /// Per-channel manifest state, populated by the auto-check on each Updates-page open — drives each button's label (target version, dozenal), colour, and enabled-ness.
    update_release: ChannelCheck,
    update_dev: ChannelCheck,
    /// True once the current Updates-page visit kicked its auto-check (reset on page-enter, like `you_fields_loaded`).
    update_checked: bool,
    /// Latest APPLY outcome (download/install) for the status line — event-shown, interaction-cleared.
    update_status: Option<String>,
    /// An APPLY (install) is in flight — both buttons freeze.
    update_busy: bool,
    /// Desktop: a verified binary swap completed — re-exec into it on the next tick (from the main thread, outside all borrows).
    update_reexec: Option<std::path::PathBuf>,
    /// In-flight download progress (bytes done, total; total 0 = unknown length): the Updates page renders the bar from this. `None` = no download running.
    update_progress: Option<(u64, u64)>,
    /// Next automatic release-channel check, eagle time. 0 = not yet scheduled (the driver arms a short post-launch delay, then ~6–8h jittered). The AUTOMATIC path (docs/updates.md): desktop release builds self-apply thru the stamp window; dev builds and Android only surface a toast — dev updates stay manual by mandate, Android package installs belong to the OS.
    next_update_check_osc: i64,
    /// Session dedup for the "update available" toast — the version already announced, so a 6-hourly re-check doesn't re-toast the same release.
    update_toasted: Option<(usize, usize, usize)>,
    /// Devices already sent the boot ANNOUNCE (the unsolicited pong whose sealed tail carries our About — see PingRequest::announce). Runtime-only, so every launch announces once per sibling: the peers' fleet pages read the new build within seconds instead of a ping-backoff cycle later (field 2026-08-31: 71.0 shown for a host running 71.1).
    announced_devices: std::collections::HashSet<[u8; 32]>,
    /// Android: a hash-verified APK is staged — the JNI poll hands this path to Kotlin, which fires the system installer (the second click).
    #[cfg(target_os = "android")]
    pub pending_apk_install: Option<String>,
    /// One-shot clipboard hand-off to the Android shell (`nativePollClipboardCopy`): the Choreographer frame poll drains it into ClipboardManager. Desktop copies via arboard directly and never touches this.
    pub pending_clipboard_copy: Option<String>,
    /// OFF-GRID add-a-friend (docs/offgrid.md open house): the typed handle + its derived handle_proof, armed by an add submitted while the registry is unreachable. The pt_disc beacon that matches this proof CREATES the contact (device key + address from the beacon) — the registry's job, done by proximity. Cleared on match or on leaving the flow.
    pub pending_woods_add: Option<(String, [u8; 32])>,
    /// Ring-panel avatar cache: the caller's avatar (or gradient) pre-scaled to the full-screen panel diameter — the list-size `avatar_scaled` is far too small to blit large. (diameter, pixels); rebuilt when the panel diameter changes, dropped when no call is ringing.
    pub ring_avatar_scaled: Option<(usize, Vec<u8>)>,
    /// The off-thread handle_proof derivation for the pending woods add (the ~1s memory-hard PoW never runs on the UI thread).
    pub woods_add_rx: Option<std::sync::mpsc::Receiver<(String, [u8; 32])>>,
    /// Rubber-band scroll extents, measured by the last render (the extents live in render-side geometry — text metrics, dynamic row counts — so render publishes them and the wheel handler + tick() read last frame's value; geometry is stable frame-to-frame). `tick()` relaxes any out-of-range scroll back to [0, extent] thru these.
    settings_rail_extent: f32,
    settings_content_extent: f32,
    /// The active conversation's message-scroll ceiling, captured each render (content height − viewport height). The render clamps a LOCAL copy for drawing but the stored `message_scroll_offset` must be clamped too — else it drifts past the ceiling when the viewport height changes (the soft keyboard opening/closing, exaggerated now that the width-pinned scale keeps line height constant while the surface height swings), and the list sticks at the top until you scroll back past the excess. The tick clamps the stored offset against this.
    msg_max_scroll: f32,
    contacts_scroll_extent: isize,
    settings_shred_armed: bool,
    /// Two-tap confirm armed for the Security page's "Remove this device from fleet" (self-departure WITHOUT the wipe — vault stays, claims dormant). Mutually exclusive with the two wipers; cleared on any page switch.
    settings_remove_armed: bool,
    /// Our outstanding departure request's consent stamp (bilateral removal): Some = we asked the fleet to sign us out and await a sibling's approval. Completion = observing our own key de-folded from the adopted member set. RAM-only; a relaunch just re-requests.
    depart_request_t: Option<i64>,
    /// The pending departure completes as a WIPE (Remove & shred) instead of the keep-vault de-attest. Dies with the process — a relaunch mid-ceremony safely degrades to keep-vault (the user can Shred manually).
    depart_wipe_after: bool,
    /// A sibling's inbound departure request awaiting THIS user's approval: (leaving device pubkey, consent_t, consent_sig). One at a time — a second request overwrites (latest wins; the earlier requester just re-taps).
    pending_depart_req: Option<([u8; 32], i64, Vec<u8>)>,
    /// Two-tap arm for the fleet page's "Approve sign-out" pill, keyed by the leaving device pubkey.
    fleet_approve_armed: Option<[u8; 32]>,
    /// Two-tap confirm armed for the Security page's "Remove & shred" (self-departure from the fleet chain, then crypto-wipe). Mutually exclusive with `settings_shred_armed`; cleared on any page switch, like every destructive arm.
    settings_removeshred_armed: bool,
    /// About page: false = show the version as dozenal GLYPHS (the default — proper rendered dozenal, never arabic); true = the version tapped, spell it out in voca words. Toggles on each tap of the version row.
    about_version_spelled: bool,
    /// The running audio calibration's worker handle (Settings→Audio) — drop = stop. One at a time; the drain stores the posted profile + clears this.
    audio_cal_handle: Option<crate::call::calibrate::CalHandle>,
    /// One tap on the version reveals the dozenal index; ONE tap within the index reveals the custodian riddle easter egg beneath it (session-permanent once found, hidden with the index when the version collapses).
    about_riddle_revealed: bool,

    /// This node's own reflexive (public) address, learned via peer-echoed reflection (see [`crate::network::traverse::reflexive`]). `None` until the first signed pong / `ReflectResponse` echo. Fed forward to candidate gathering and the FGTW announce so our published address is the one seen on the live UDP data socket — not fgtw.org's TLS-flow `cf-connecting-ip`, which is only right for cone NATs.
    our_reflexive: Option<std::net::SocketAddr>,
    /// This node's own LAN address, learned from our OWN looped-back discovery beacon (its source is kernel truth for the interface the beacon left on). The LAN counterpart of `our_reflexive`, and the record's LAN slot prefers it over `get_local_ip` — the routing trick asks which interface reaches the INTERNET, which on a phone routing internet over cellular names the CLAT/CGNAT interface while the Wi-Fi holds the real LAN address (the published record then carried no LAN entry and the peer parked on relay, 2026-08-11).
    our_lan_ip: Option<std::net::Ipv4Addr>,
    /// The address we last published a signed self-record for. Differs from `our_reflexive` exactly when the record peers hold for us is stale — on first learn, on a network change, or when the first echo beat attestation and there was no `handle_proof` to sign against yet.
    self_record_published_for: Option<std::net::SocketAddr>,
    /// Whether the persisted phonebook has been merged into the live store this session. One-shot: also stops an unreadable vault entry being retried every tick.
    peer_store_loaded: bool,
    /// The last OWN fold the primary registry was converged onto — the fold-change edge detector (same fold again = no converge spawn; the plan would be empty anyway, this just saves the round trip).
    registry_converged_fold: Vec<[u8; 32]>,
    /// Store size at the last vault persist — gossip merges land on the checker thread, so the tick that OBSERVES growth writes the store down. Before this, only our own address change persisted: a session that merged rows but never moved address lost them at exit.
    peer_store_persisted_len: usize,
}

impl PhotonApp {
    /// Construct an empty app shell. Real state (chrome, network handles, app state machine) initializes in [`FluorApp::init`] once the viewport is known.
    pub fn new() -> Self {
        // Desktop resident mode is ON BY DEFAULT (user mandate): residency + the login item enroll automatically unless the user's explicit opt-out marker exists (the settings toggle writes it). `--background` (the login-item launch) additionally starts hidden.
        #[cfg(not(target_os = "android"))]
        let (start_in_background, resident_mode) = {
            let bg = std::env::args().any(|a| a == "--background");
            if bg {
                crate::platform::desktop_notify::set_window_visible(false);
            }
            crate::platform::autostart::ensure_enrolled();
            (bg, bg || crate::platform::autostart::background_desired())
        };
        #[cfg(target_os = "android")]
        let (start_in_background, resident_mode) = (false, false);
        Self {
            start_in_background,
            resident_mode,
            published_bell: None,
            #[cfg(not(target_os = "android"))]
            tray_spawned: false,
            settings_background_check: None,
            settings_unattended_check: None,
            unattended_confirm: None,
            unattended_confirm_tb: None,
            unattended_confirm_base: HIT_NONE,
            unattended_confirm_failed: false,
            unattended_verify: None,
            unattended_on: Self::unattended_enabled(),
            chrome: None,
            hit_counter: 0,
            event_proxy: None,
            our_reflexive: None,
            our_lan_ip: None,
            self_record_published_for: None,
            peer_store_loaded: false,
            registry_converged_fold: Vec::new(),
            peer_store_persisted_len: 0,
            zoom_saved_ru: 1.0,
            persist_tx: None,
            persist_done: std::sync::mpsc::channel(),
            chains_persist_tx: None,
            conv_state_persist_tx: None,
            peer_persist_tx: None,
            peer_persist_dirty: false,
            last_peer_persist: None,
            our_lan_ips: std::collections::HashMap::new(),
            tick_serial: 0,
            pending_chain_sends: Vec::new(),
            seed_identity_count: 0,
            pb_resolve_rx: None,
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
            last_screen: AppState::default(),
            last_presence_ping: None,
            last_interaction: None,
            last_fleet_refold: None,
            last_stalled_refetch: None,
            peer_store: None,
            handle_query: None,
            status_checker: None,
            contact_pubkeys: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
            sync_records: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
            pong_seal_keys: std::sync::Arc::new(std::sync::Mutex::new(
                std::collections::HashMap::new(),
            )),
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
            clutch_kem_decap_tx: {
                let (tx, _) = std::sync::mpsc::channel();
                tx
            },
            clutch_kem_decap_rx: std::sync::mpsc::channel().1,
            avatar_dl_tx: {
                let (tx, _) = std::sync::mpsc::channel();
                tx
            },
            avatar_dl_rx: std::sync::mpsc::channel().1,
            attach_installed_tx: {
                let (tx, _) = std::sync::mpsc::channel();
                tx
            },
            attach_installed_rx: std::sync::mpsc::channel().1,
            hist_opened_tx: {
                let (tx, _) = std::sync::mpsc::channel();
                tx
            },
            hist_opened_rx: std::sync::mpsc::channel().1,
            chain_sync_opened_tx: {
                let (tx, _) = std::sync::mpsc::channel();
                tx
            },
            chain_sync_opened_rx: std::sync::mpsc::channel().1,
            braid_rx_tx: {
                let (tx, _) = std::sync::mpsc::channel();
                tx
            },
            braid_rx_rx: std::sync::mpsc::channel().1,
            chat_replay_queue: std::collections::VecDeque::new(),
            braid_tx_tx: {
                let (tx, _) = std::sync::mpsc::channel();
                tx
            },
            braid_tx_rx: std::sync::mpsc::channel().1,
            send_encrypt_busy: std::collections::HashSet::new(),
            seal_job_tx: spawn_job_worker("photon-seal"),
            braid_job_tx: spawn_job_worker("photon-braid"),
            fleet_heal_busy: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
            fanout_grow_pending: false,
            self_avatar_recover_pending: None,
            scoped_regrant_pending: Vec::new(),
            fleet_rotated_tx: {
                let (tx, _) = std::sync::mpsc::channel();
                tx
            },
            fleet_rotated_rx: std::sync::mpsc::channel().1,
            avatar_dl_started: std::collections::HashSet::new(),
            avatar_req_pending: std::collections::HashMap::new(),
            hist_rid_map: std::collections::HashMap::new(),
            keygen_fleet_gate_holding: false,
            blind_flip: std::collections::HashMap::new(),
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
            ready_toast_screen: None,
            clock_check_tx: {
                let (tx, _) = std::sync::mpsc::channel();
                tx
            },
            clock_check_rx: std::sync::mpsc::channel().1,
            clock_off: None,
            clock_consensus: None,
            // ~1 hour of unexplained wall-vs-monotonic skew triggers a re-check (loose enough to ignore NTP steps and short sleeps, tight enough to catch a day-scale set or long sleep).
            clock_jump: crate::network::ClockJumpDetector::new(3600),
            inbox_check_tx: {
                let (tx, _) = std::sync::mpsc::channel();
                tx
            },
            inbox_check_rx: std::sync::mpsc::channel().1,
            online: false,
            photon_orb: None,
            orb_contact: None,
            orb_key: None,
            painted_ring_tiers: Vec::new(),
            orb_had_avatar: false,
            contacts_textbox: None,
            message_textbox: None,
            contacts_plus_btn: None,
            message_send_btn: None,
            storage: None,
            contacts: Vec::new(),
            conversations: Vec::new(),
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
            lan_heard: Vec::new(),
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
            successor_rx: None,
            successor_tx: None,
            successor_inflight: std::collections::HashSet::new(),
            launch_add_mode: false,
            add_join_handle: None,
            probed_session: None,
            probed_handle: None,
            add_join_status: String::new(),
            add_join_words: None,
            add_join_rx: None,
            pending_fleet_key: None,
            fleet_epoch: None,
            fleet_epoch_prev: None,
            fleet_focus_claim: None,
            fleet_attention: None,
            active_call: None,
            call_status_btn: None,
            call_start_btn: None,
            call_action_btn: None,
            call_decline_btn: None,
            call_speaker_btn: None,
            call_addhandle_btn: None,
            call_back_btn: None,
            call_play_btn: None,
            call_minimized: false,
            call_speaker_on: false,
            call_playback: None,
            lane_rearm_cycles: std::collections::HashMap::new(),
            lane_reserve_bursts: std::collections::HashMap::new(),
            pb_resolve_cursor: 0,
            ckpt_mint_due: false,
            ckpt_spineless_holds: 0,
            ckpt_rows_base: None,
            ckpt_busy: false,
            ckpt_rx: None,
            ckpt_loaded: false,
            ckpt_last_attempt: None,
            roster_pull_rx: None,
            roster_push_rx: None,
            roster_push_queued: false,
            fleet_settings: None,
            fleet_key_ram: std::sync::Arc::new(std::sync::Mutex::new(None)),
            needs_initial_roster_pull: false,
            roster_pull_retries_left: 0,
            roster_pull_parked_under: None,
            roster_pull_exhausted: false,
            device_avatar_pixels: None,
            device_avatar_scaled: None,
            device_avatar_scaled_diameter: 0,
            avatar_hit_id: HIT_NONE,
            known_pick_hit: HIT_NONE,
            known_mine_hit: HIT_NONE,
            joiner_selected: false,
            pending_zoom_restore: None,
            zoom_restored: false,
            avatar_set_rx: None,
            active_conversation: None,
            contact_hit_base: HIT_NONE,
            back_btn_hit_id: HIT_NONE,
            join_startfresh_hit_id: HIT_NONE,
            join_copywords_hit_id: HIT_NONE,
            join_words_copied: false,
            join_startfresh_armed: false,
            pending_picker_request: false,
            pending_broadcast_signal: 0,
            next_session_broadcast: None,
            contacts_scroll: 0,
            settings_rail_scroll: 0.0,
            settings_content_scroll: 0.0,
            hints_dismissed: false,
            avatar_hovered: false,
            hover_hit: HIT_NONE,
            hover_is_textbox: false,
            pointer_down: false,
            drag_select_hit: HIT_NONE,
            pan_grab_x: 0.0,
            pan_grab_scroll: 0.0,
            last_click_hit: HIT_NONE,
            last_click_time: None,
            click_streak: 0,
            settings_nav_base: HIT_NONE,
            contact_panel_btn_base: HIT_NONE,
            contact_nav_base: HIT_NONE,
            contact_boot_armed: false,
            exit_requested: false,
            shift_held: false,
            msg_hit_base: HIT_NONE,
            msg_copy_id: HIT_NONE,
            msg_action_base: HIT_NONE,
            msg_hit_rows: Vec::new(),
            msg_view_h: 0.0,
            painted_compose_lines: 1,
            selected_msg: None,
            selected_msg_copied: false,
            pending_delete: None,
            compose_reply_to: None,
            compose_edit_of: None,
            compose_react_to: None,
            react_strip_base: HIT_NONE,
            react_strip_glyphs: Vec::new(),
            conv_topbar_off: 0.0,
            msg_wrap: None,
            #[cfg(target_os = "android")]
            last_ime_inset: 0,
            settings_repushed: false,
            last_seal_reseed: None,
            last_fleet_sweep: None,
            chain_pushed_osc: std::collections::HashMap::new(),
            chain_pull_sent: std::collections::HashSet::new(),
            chain_pull_misses: std::collections::HashMap::new(),
            lane_pushed_pos: std::collections::HashMap::new(),
            settings_btn_base: HIT_NONE,
            settings_theme_dropdown: None,
            settings_zoom_slider: None,
            attach_progress: Vec::new(),
            attach_confirmed: std::collections::HashSet::new(),
            pending_attach_picker: false,
            #[cfg(all(unix, not(target_os = "android"), not(target_os = "redox")))]
            bridge_cmd_tx: None,
            #[cfg(all(unix, not(target_os = "android"), not(target_os = "redox")))]
            bridge_out_rx: None,
            #[cfg(all(unix, not(target_os = "android"), not(target_os = "redox")))]
            bridge_partials: None,
            #[cfg(all(unix, not(target_os = "android"), not(target_os = "redox")))]
            bridge_partial_sent: std::collections::HashMap::new(),
            #[cfg(all(unix, not(target_os = "android"), not(target_os = "redox")))]
            bridge_fg: None,
            bridge_locus: None,
            bridge_int: None,
            settings_custodian_check: None,
            settings_chime_check: None,
            settings_dozenal_check: None,
            settings_vibrate_msg_check: None,
            settings_ring_call_check: None,
            settings_vibrate_call_check: None,
            settings_presence_check: None,
            settings_autoupdate_check: None,
            settings_hardlogs_check: None,
            diag_log_view: false,
            diag_log_rows: Vec::new(),
            diag_log_consumed: 0,
            diag_log_rx: None,
            diag_log_follow: true,
            diag_log_next_poll_osc: 0,
            diag_log_inspect: None,
            settings_note_textbox: None,
            you_fields: Vec::new(),
            you_add_textbox: None,
            you_fields_loaded: false,
            settings_fleet_selected: None,
            fleet_retired: Vec::new(),
            fleet_release_armed: None,
            avatar_probe_cache: std::collections::HashMap::new(),
            egged_cache: std::collections::HashMap::new(),
            pending_geometry_restore: None,
            fleet_lock_armed: None,
            fleet_unlock_armed: None,
            pending_unlock: None,
            locked_retry_hit: HIT_NONE,
            pending_lock: None,
            update_rx: None,
            update_tx: None,
            call_keep_rx: None,
            call_keep_tx: None,
            update_release: ChannelCheck::Idle,
            update_dev: ChannelCheck::Idle,
            update_checked: false,
            update_status: None,
            update_busy: false,
            update_reexec: None,
            update_progress: None,
            next_update_check_osc: 0,
            update_toasted: None,
            announced_devices: std::collections::HashSet::new(),
            #[cfg(target_os = "android")]
            pending_apk_install: None,
            pending_clipboard_copy: None,
            pending_woods_add: None,
            woods_add_rx: None,
            ring_avatar_scaled: None,
            settings_rail_extent: 0.0,
            settings_content_extent: 0.0,
            msg_max_scroll: 0.0,
            contacts_scroll_extent: 0,
            settings_shred_armed: false,
            settings_remove_armed: false,
            depart_request_t: None,
            depart_wipe_after: false,
            pending_depart_req: None,
            fleet_approve_armed: None,
            settings_removeshred_armed: false,
            about_version_spelled: false,
            audio_cal_handle: None,
            about_riddle_revealed: false,
        }
    }

    /// Inject the device keypair before `init` runs. Used by the Android JNI shim to pass thru the keypair that `PhotonConnectionService` derives from the OS-provided device fingerprint — that fingerprint lives in Java (`Build.FINGERPRINT` / `Settings.Secure.ANDROID_ID`) and reaches the native side via `NetworkContext`. On desktop this stays unset; `init` falls back to `get_machine_fingerprint` (which reads `/etc/machine-id` etc.) and derives the keypair internally.
    pub fn set_device_keypair(&mut self, keypair: crate::network::fgtw::Keypair) {
        self.device_keypair = Some(keypair);
    }

    /// Take the one-shot image-picker request. JNI shim polls this once per frame; returns `true` exactly on the frame the user taps the avatar so the Activity launches `ACTION_GET_CONTENT` once per tap.
    /// Android: drain the paperclip's system-file-picker request (mirrors [`Self::take_picker_request`]).
    pub fn take_attach_picker_request(&mut self) -> bool {
        let req = self.pending_attach_picker;
        self.pending_attach_picker = false;
        req
    }

    /// Android picker result: a file's name + bytes for the ACTIVE conversation (no-op outside one).
    pub fn attach_picked(&mut self, name: String, bytes: Vec<u8>) {
        if matches!(self.state, AppState::Conversation) {
            if let Some(ci) = self.active_contact() {
                self.send_attachment_from_bytes(ci, name, bytes);
            }
        }
    }

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
        crate::logf!("avatar picker: processing {} bytes", image_bytes.len());
        let storage = match self.storage.clone() {
            Some(s) => s,
            None => {
                crate::log("avatar picker: ignored — storage not initialized yet");
                return;
            }
        };
        // NOTHING runs on the UI thread (a 50MP photo's decode + Lanczos is hundreds of ms — no half of this pipeline is frame-safe): one LOW-PRIORITY worker decodes, sends the display pixels back thru `avatar_set_rx` (installed next tick, typically a frame or two later), then grinds thru the rav1e encode, vault save, and upload behind everything else.
        // Pin + upload material gathered here first (ensure_avatar_pin may mint + persist, needs &mut self); the pong slot updates now so friends' next ping already carries the pin.
        let proof = self.our_handle_proof();
        // ROTATE the pin on every set: a fresh random key ‖ lookup per avatar change. Two birds — friends detect the change (the pin rides every pong, a new pin = refetch, closing the stale-avatar-until-next-session gap), and any cross-identity pin pollution heals on the next set (the old slot is deleted after the new upload lands).
        let old_pin = self
            .fleet_settings
            .as_ref()
            .and_then(|fs| fs.effective("profile.avatar_pin"))
            .and_then(crate::storage::fleet_settings::as_bytes)
            .filter(|v| v.len() == 64)
            .map(|v| {
                let mut p = [0u8; 64];
                p.copy_from_slice(&v);
                p
            });
        let mut new_pin = [0u8; 64];
        {
            use rand::RngCore;
            rand::thread_rng().fill_bytes(&mut new_pin);
        }
        self.settings_set("profile.avatar_pin", vsf::VsfType::hR(new_pin.to_vec()));
        self.publish_avatar_pin();
        let avatar_pin = Some(new_pin);
        // Sibling ding: bump the fleet-synced avatar stamp so the fstate event wakes the fleet and their next avatar sync pulls the fresh copy. Bumped at SET time — a sibling racing the upload just gets the old copy once and heals on the next sync (newest-wins).
        self.settings_set(
            "profile.avatar_ts",
            vsf::VsfType::e(vsf::types::EtType::e6(vsf::eagle_time_oscillations())),
        );
        let kp = self.device_keypair.clone();
        // The scoped-blob reader set (docs/scoped-blobs.md), gathered here because it needs `&self`: our own fleet key so every sibling can read, plus the CLUTCH pair secret for each friend. A friend with no pair secret yet is simply not a reader this round — their slot appears the next time we publish after their ceremony completes.
        let scoped_readers: Vec<[u8; 32]> = {
            let mut r = Vec::new();
            if let Some(fk) = self.fleet_key_cached() {
                r.push(fk);
            }
            if let (Some(ours), Some(storage)) = (
                self.device_keypair.as_ref().map(|k| *k.public.as_bytes()),
                self.storage.as_ref(),
            ) {
                for c in self.contacts.iter().filter(|c| !c.is_sibling) {
                    let Some(their_dev) = c.device_key() else {
                        continue;
                    };
                    if let Some(ps) = crate::storage::fanout_pairs::load(
                        &ours,
                        &their_dev,
                        storage,
                    ) {
                        r.push(ps);
                    }
                }
            }
            r
        };
        let (px_tx, px_rx) = std::sync::mpsc::channel();
        self.avatar_set_rx = Some(px_rx);
        // The fresh wall blob carries the preferred name beside the new pixels (same pin).
        let our_name = self
            .fleet_settings
            .as_ref()
            .and_then(|fs| fs.effective("profile.name"))
            .and_then(crate::storage::fleet_settings::as_text)
            .filter(|n| !n.is_empty());
        let wake = self.event_proxy.clone();
        std::thread::spawn(move || {
            #[cfg(not(target_os = "redox"))]
            let _ =
                thread_priority::set_current_thread_priority(thread_priority::ThreadPriority::Min);
            let rgb_f32 = match crate::ui::avatar::image_to_avatar_rgb_f32(&image_bytes) {
                Ok(p) => p,
                Err(e) => {
                    crate::logf!("avatar picker: decode failed: {}", e);
                    return;
                }
            };
            // Display pixels first — the UI installs them the next tick; the grind continues below.
            let vsf_rgb = crate::ui::avatar::avatar_rgb_f32_to_u8(&rgb_f32);
            let _ = px_tx.send(crate::ui::colour_convert::vsf_rgb_to_bt2020(&vsf_rgb));
            if let Some(w) = wake.as_ref() {
                let _ = w.send(crate::ui::PhotonEvent::NetworkUpdate);
            }
            let av1_data = match crate::ui::avatar::encode_avatar_rgb_f32(&rgb_f32) {
                Ok(d) => d,
                Err(e) => {
                    crate::logf!("avatar picker: encode failed (display keeps the picked image this session): {}", e);
                    return;
                }
            };
            if let Err(e) =
                crate::ui::avatar::save_avatar_from_seed(&av1_data, &identity_seed, &storage)
            {
                crate::logf!("avatar picker: save failed: {}", e);
                return;
            }
            crate::log("avatar picker: encoded + saved");
            match (kp, proof, avatar_pin) {
                (Some(kp), Some(hp), Some(pin)) => {
                    match crate::ui::avatar::upload_avatar_from_seed(
                        &kp.secret,
                        &identity_seed,
                        &pin,
                        &hp,
                        our_name.as_deref(),
                        &storage,
                    ) {
                        Ok(_) => {
                            crate::log("avatar picker: FGTW upload ok");
                            // Scoped publish: one ciphertext plus a private slot per reader. This is what friends actually read now — the pin upload above stays only until every avatar in the fleet has been re-set under the new scheme.
                            if scoped_readers.is_empty() {
                                crate::log("SCOPED: no readers yet (no fleet key, no egged friends) — slots follow the next publish");
                            } else {
                                // WE are the publisher, so the purpose carries OUR pid — the direction bind that keeps our slot and a friend's slot apart on the one pair secret they share.
                                let purpose = crate::ui::avatar_scoped::avatar_purpose(
                                    &crate::crypto::clutch::identity_party_id(&identity_seed),
                                );
                                match crate::ui::avatar_scoped::publish_blocking(
                                    &av1_data,
                                    &scoped_readers,
                                    &purpose,
                                    &kp,
                                    &hp,
                                ) {
                                    // Remember the blob id and its key: a friend whose pair secret is re-minted later needs a slot at their NEW address, and re-uploading the image to give it to them would be absurd.
                                    Ok((_, contents)) => {
                                        crate::ui::avatar_scoped::remember_published(
                                            &purpose, &contents, &storage,
                                        )
                                    }
                                    Err(e) => crate::logf!("SCOPED: avatar publish failed: {}", e),
                                }
                            }
                            // The rotation's second half: the OLD slot dies once the new one is live — no orphan blobs, and a polluted/shared slot stops serving this identity's history.
                            if let Some(op) = old_pin.filter(|op| *op != pin) {
                                let sk =
                                    ed25519_dalek::SigningKey::from_bytes(kp.secret.as_bytes());
                                match crate::ui::avatar::delete_avatar_blocking(&sk, &identity_seed, &op) {
                                    Ok(()) => crate::log("avatar picker: old wall slot deleted (pin rotated)"),
                                    Err(e) => crate::logf!("avatar picker: old slot delete failed (orphan blob remains): {}", e),
                                }
                            }
                        }
                        Err(e) => crate::logf!("avatar picker: FGTW upload failed: {}", e),
                    }
                }
                _ => crate::log(
                    "avatar picker: skipping FGTW upload — keypair / proof / pin unavailable",
                ),
            }
        });
    }
}

/// DEV-ONLY orb: a per-build random gradient, so a freshly built + uploaded debug build is instantly recognizable (did the upload actually take?). Seeded from a hash of THIS executable's tail — the appended signature is regenerated every build — so it's identical across a run and different across builds. Each channel (R, G, B) gets its OWN random plane `z = a·x + b·y` with `a, b ∈ [-1, 1]` over normalized `x, y ∈ [0, 1]`; the raw z, clamped to [0, 1], IS the channel value — NO normalization, so different slopes yield different intensity ranges and every build looks distinct. `draw_app_icon` masks it to the circle.
#[cfg(feature = "development")]
fn dev_gradient_orb() -> fluor::host::icon::Icon {
    fn splitmix(state: &mut u64) -> u64 {
        *state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = *state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
    // Per-build seed: hash the executable's tail (its appended 64-byte signature changes every build).
    let mut seed = (|| -> Option<u64> {
        use std::io::{Read, Seek, SeekFrom};
        let mut f = std::fs::File::open(std::env::current_exe().ok()?).ok()?;
        let len = f.metadata().ok()?.len();
        let tail = 4096u64.min(len) as usize;
        f.seek(SeekFrom::End(-(tail as i64))).ok()?;
        let mut buf = vec![0u8; tail];
        f.read_exact(&mut buf).ok()?;
        Some(u64::from_le_bytes(
            blake3::hash(&buf).as_bytes()[..8].try_into().unwrap(),
        ))
    })()
    .unwrap_or(0xDEAD_BEEF_CAFE_F00D);
    let unit = |s: &mut u64| (splitmix(s) >> 11) as f64 / (1u64 << 53) as f64; // [0,1)
                                                                               // Per-channel slopes a,b ∈ [-1,1): red, green, blue each get their own gradient plane.
                                                                               // Slopes CUBED: still [-1,1] (extremes still reach the full [-4pi,4pi] → 4 periods) but concentrated near zero, so most channels are gentle low-frequency gradients — not a busy 4x plaid.
    let (r_a, r_b) = (
        (unit(&mut seed) * 2.0 - 1.0).powi(3),
        (unit(&mut seed) * 2.0 - 1.0).powi(3),
    );
    let (g_a, g_b) = (
        (unit(&mut seed) * 2.0 - 1.0).powi(3),
        (unit(&mut seed) * 2.0 - 1.0).powi(3),
    );
    let (b_a, b_b) = (
        (unit(&mut seed) * 2.0 - 1.0).powi(3),
        (unit(&mut seed) * 2.0 - 1.0).powi(3),
    );
    // Each channel is a sine plane wave: map the disk to [-4pi, 4pi] per axis, z = a*x + b*y (slopes in [-1,1]), then (sin(z)+1)/2 -> [0,1] with NO normalization and NO clipping (sine is bounded). Circle vignette darkens the rim over the top.
    const N: u32 = 256;
    let s = 4.0 * std::f64::consts::PI;
    let mut pixels = Vec::with_capacity((N * N) as usize);
    for py in 0..N {
        for px in 0..N {
            let xc = 2.0 * (px as f64 / (N - 1) as f64) - 1.0;
            let yc = 2.0 * (py as f64 / (N - 1) as f64) - 1.0;
            let (x, y) = (xc * s, yc * s);
            let vignette = (1.0 - xc * xc - yc * yc).max(0.0).sqrt();
            let ch =
                |a: f64, b: f64| ((((a * x + b * y).sin() + 1.0) / 2.0) * vignette * 255.0) as u32;
            let (cr, cg, cb) = (ch(r_a, r_b), ch(g_a, g_b), ch(b_a, b_b));
            pixels.push(0xFF00_0000 | ((255 - cr) << 16) | ((255 - cg) << 8) | (255 - cb));
        }
    }
    fluor::host::icon::Icon {
        width: N,
        height: N,
        pixels,
    }
}

/// The default UNSET avatar: a deterministic per-identity gradient (seeded from the public proof), spherical-shaded like the dev orb, as a `diam×diam×3` visible-RGB buffer for `draw_avatar`. Replaces the flat grey placeholder — everyone without a set avatar shows a distinct little lit orb keyed to their identity, identical on every device that knows the proof. Each channel is its own plane `z = a·x + b·y` (`a,b ∈ [-1,1]`, raw clamped z, no normalization) × the dome `√(1−x_c²−y_c²)`.
fn gradient_avatar_rgb(mut seed: u64, diam: usize) -> Vec<u8> {
    fn splitmix(state: &mut u64) -> u64 {
        *state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = *state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
    let unit = |s: &mut u64| (splitmix(s) >> 11) as f64 / (1u64 << 53) as f64; // [0,1)
                                                                               // Slopes CUBED: still [-1,1] (extremes still reach the full [-4pi,4pi] → 4 periods) but concentrated near zero, so most channels are gentle low-frequency gradients — not a busy 4x plaid.
    let (r_a, r_b) = (
        (unit(&mut seed) * 2.0 - 1.0).powi(3),
        (unit(&mut seed) * 2.0 - 1.0).powi(3),
    );
    let (g_a, g_b) = (
        (unit(&mut seed) * 2.0 - 1.0).powi(3),
        (unit(&mut seed) * 2.0 - 1.0).powi(3),
    );
    let (b_a, b_b) = (
        (unit(&mut seed) * 2.0 - 1.0).powi(3),
        (unit(&mut seed) * 2.0 - 1.0).powi(3),
    );
    // Each channel is a sine plane wave: disk mapped to [-4pi, 4pi] per axis, z = a*x + b*y, then (sin(z)+1)/2 -> [0,1] (bounded, no clip); circle vignette over the top.
    let denom = diam.saturating_sub(1).max(1) as f64;
    let s = 4.0 * std::f64::consts::PI;
    let mut out = vec![0u8; diam * diam * 3];
    for py in 0..diam {
        for px in 0..diam {
            let xc = 2.0 * (px as f64 / denom) - 1.0;
            let yc = 2.0 * (py as f64 / denom) - 1.0;
            let (x, y) = (xc * s, yc * s);
            let vignette = (1.0 - xc * xc - yc * yc).max(0.0).sqrt();
            let ch =
                |a: f64, b: f64| ((((a * x + b * y).sin() + 1.0) / 2.0) * vignette * 255.0) as u8;
            let i = (py * diam + px) * 3;
            out[i] = ch(r_a, r_b);
            out[i + 1] = ch(g_a, g_b);
            out[i + 2] = ch(b_a, b_b);
        }
    }
    out
}

/// Seed a gradient from a 32-byte public proof (`handle_proof`) — deterministic per identity.
fn proof_gradient_seed(proof: &[u8; 32]) -> u64 {
    u64::from_le_bytes(proof[..8].try_into().unwrap())
}

/// Map a connectivity bool to the chrome orb tint. Offline = red disk, online = green disk. Visible RGB chosen for high contrast in either light or dark chrome themes; brighten=true on the online state for the eventual icon-overlay case (no-icon today just renders as a solid coloured circle).
fn orb_tint_for(online: bool) -> fluor::host::chrome::OrbTint {
    // Visible RGB(64, 224, 64) green: darkness = (0xBF, 0x1F, 0xBF); packed α=0xFF. Visible RGB(224, 64, 64) red:   darkness = (0x1F, 0xBF, 0xBF); packed α=0xFF.
    // These are hand-authored in darkness-space (pre-inverted, so no `dark()`), but they STILL need `fmt()` — the platform channel-order pass (identity on desktop, R↔B swap on Android). Every other photon colour rides `fmt`; the orb ring skipping it was the Android "red-blue swapped ring". `fmt` only reorders RGB and preserves the α byte, so it's correct on the already-darkened constants.
    fluor::host::chrome::OrbTint::Custom {
        ring: fluor::theme::fmt(if online {
            crate::ui::theme::ORB_ONLINE
        } else {
            crate::ui::theme::ORB_OFFLINE
        }),
        brighten: online,
    }
}

/// How long a LAN discovery hearing keeps a candidate marked "nearby" — comfortably past the joining device's ~8s announce cadence, short enough that a device that left the network stops reading as present.
const LAN_HEARD_FRESH: std::time::Duration = std::time::Duration::from_secs(30);

/// One matcher candidate on the AddDevice screen: a verified binding request plus its precomputed expected word tokens (23, lowercase — `masked_device_words` split) and keyed display name. Precomputing keeps the per-keystroke match a plain string walk.
struct AddCandidate {
    req: crate::network::fgtw::fleet::BindRequest,
    name: String,
    tokens: Vec<String>,
    /// This candidate's device pubkey is currently being heard over the BLE announce beacon — proximity confirmation (docs/pairing-v2.md, BLE transport). The candidate list marks these "nearby"; tapping any candidate binds it (BLE/tap select), typing its words still works too.
    heard_ble: bool,
    /// Same proximity confirmation over the LAN: the candidate's device pubkey is currently broadcasting UDP discovery under OUR handle on this network (the joining device announces during its join loop). This is the "see local devices, add, done" path — the words remain the remote/no-LAN fallback.
    heard_lan: bool,
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
/// Off-thread checkpoint work reporting home: a spine advance (minted here or adopted from custody), or "nothing landed, re-arm on the next edge".
enum CkptOutcome {
    Advanced {
        k: u64,
        epoch: [u8; 32],
        prev: Option<(u64, [u8; 32])>,
        root: Option<[u8; 32]>,
        fanout_epoch: u64,
        minted_here: bool,
    },
    /// The chain carries a Checkpoint but custody didn't open and we hold no spine — the drain asks siblings for ckpt_state and counts the dry sweeps toward the supersession breaker.
    SpinelessHold,
    Idle,
}

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
        // Dispatch (click / key / focus / tab), hover, damage-tracking, and the cursor icon ALL walk widgets through here. App widgets first (screen-gated), then chrome. Registering a widget in `visit_app_widgets` is the ONLY place it must be added — everything else iterates that one walk, so a new textbox inherits hover + damage + I-beam + gestures for free. (Hand-maintained per-concern widget lists were the recurring "new box misses hover/damage" bug.)
        self.visit_app_widgets(f);
        if let Some(chrome) = self.chrome.as_mut() {
            chrome.visit(f);
        }
    }
}

impl PhotonApp {
    /// Every APP widget (NOT chrome) active on the current screen, yielded to `f` — the single per-widget registry (see [`Container::visit`]). Screen-gated: an off-screen widget is neither dispatched to, tab-focusable, hover-lit, nor damage-claimed. An inherent method (not part of `Container`) so hover/damage passes can call it directly.
    fn visit_app_widgets(&mut self, f: &mut dyn FnMut(&mut dyn Widget)) {
        // Call controls ride EVERY screen (a ring must be answerable from wherever the user is — docs/calls.md), so they're yielded BEFORE the per-state matches — hover/press/dispatch/apply_pressed/overlay-tint all walk this one registry. The status chip is NOT yielded (non-interactive label). Visibility mirrors the render gate exactly: a live call yields the action (+ decline in ringing/ended); an open callable conversation with no call yields the ☎ start pill. A dimmed (not-yet-reachable) start pill is drawn but withheld here so a dead tap can't dispatch.
        if let Some(phase) = self.active_call.as_ref().map(|c| c.phase) {
            use crate::call::CallPhase;
            // The action button is live in EVERY phase (Answer / End call / Hang up / Keep).
            if let Some(b) = self.call_action_btn.as_mut() {
                f(b);
            }
            match phase {
                CallPhase::Ringing => {
                    if let Some(b) = self.call_decline_btn.as_mut() {
                        f(b);
                    }
                }
                CallPhase::Ended => {
                    if let Some(b) = self.call_decline_btn.as_mut() {
                        f(b);
                    }
                    if let Some(b) = self.call_play_btn.as_mut() {
                        f(b);
                    }
                }
                // Active full-screen in-call controls; a minimized Active call yields only the action (the strip / compact bar's End).
                CallPhase::Active if !self.call_minimized => {
                    if let Some(b) = self.call_speaker_btn.as_mut() {
                        f(b);
                    }
                    if let Some(b) = self.call_addhandle_btn.as_mut() {
                        f(b);
                    }
                    if let Some(b) = self.call_back_btn.as_mut() {
                        f(b);
                    }
                }
                _ => {}
            }
        } else if matches!(self.state, AppState::Conversation) {
            let callable = self
                .active_contact()
                .and_then(|ci| self.contacts.get(ci))
                .map_or(false, |c| {
                    !c.is_sibling && c.is_online && (c.chain_woven || c.friendship_id.is_some())
                });
            if callable {
                if let Some(b) = self.call_start_btn.as_mut() {
                    f(b);
                }
            }
        }
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
            // The compose box is the only focusable widget in a conversation; yielding it here wires click-to-focus, Tab, and key dispatch. Same `compose_ready` the render reads — one definition, so the walk and the paint can never disagree again.
            let compose_ready = self.compose_ready();
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
                SettingsPage::Security => {
                    // Confirm state shows the handle box; normal state shows the checkbox — only one is live.
                    if self.unattended_confirm.is_some() {
                        if let Some(tb) = self.unattended_confirm_tb.as_mut() {
                            f(tb);
                        }
                    } else if let Some(cb) = self.settings_unattended_check.as_mut() {
                        f(cb);
                    }
                }
                SettingsPage::Notifications => {
                    if let Some(cb) = self.settings_chime_check.as_mut() {
                        f(cb);
                    }
                    if let Some(cb) = self.settings_vibrate_msg_check.as_mut() {
                        f(cb);
                    }
                    if let Some(cb) = self.settings_ring_call_check.as_mut() {
                        f(cb);
                    }
                    if let Some(cb) = self.settings_vibrate_call_check.as_mut() {
                        f(cb);
                    }
                    // presence COMMENTED OUT (Nick 2026-09-01) — restore alongside the render + layout rows.
                    if let Some(cb) = self.settings_background_check.as_mut() {
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
                    if let Some(cb) = self.settings_hardlogs_check.as_mut() {
                        f(cb);
                    }
                }
                SettingsPage::You => {
                    for pf in self.you_fields.iter_mut() {
                        f(&mut pf.tb);
                        if let Some(tag) = pf.tag_tb.as_mut() {
                            f(tag);
                        }
                        if let Some(cb) = pf.share_cb.as_mut() {
                            f(cb);
                        }
                    }
                    if let Some(tb) = self.you_add_textbox.as_mut() {
                        f(tb);
                    }
                }
                SettingsPage::About => {
                    if let Some(cb) = self.settings_dozenal_check.as_mut() {
                        f(cb);
                    }
                }
                _ => {}
            }
        }
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

/// True if `ip` is a private / non-routable address that must NOT be stored as a contact's public (`ip`) address — it belongs in `local_ip` instead. Covers IPv4 RFC1918 (10/8, 172.16/12, 192.168/16), link-local (169.254/16), loopback; IPv6 loopback, link-local (fe80::/10), unique-local (fc00::/7); and IPv4-mapped IPv6 (`::ffff:a.b.c.d`) by unwrapping to the embedded v4 (the ping/pong path reports LAN sources in exactly this mapped form, e.g. `::ffff:a.b.c.d`).
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
/// Per-channel manifest-check state for the Updates page (one each for release + dev). Drives the button label + colour.
#[derive(Clone)]
enum ChannelCheck {
    /// Not checked this page-visit yet.
    Idle,
    /// Manifest fetch in flight.
    Checking,
    /// Fetched: the row for our platform (`None` = manifest carries no build for us).
    Ready(Option<crate::network::updates::ManifestRow>),
    /// Fetch/verify failed (reason is logged at the point it's set).
    Failed,
}

/// Off-thread self-update results (docs/updates.md), drained in tick.
enum UpdateEvent {
    /// A manifest check finished: our platform's row (None = manifest has no row for us) or the error.
    Checked(
        crate::network::updates::Channel,
        Result<Option<crate::network::updates::ManifestRow>, String>,
    ),
    /// The AUTOMATIC release-channel check finished: the signed manifest's creation stamp (the window's `t`) + our platform's row. Errors land as a log line in the worker, not here — the cadence just retries later.
    AutoChecked(i64, Option<crate::network::updates::ManifestRow>),
    /// Download progress for the in-flight apply: (bytes done, total bytes; total 0 = length unknown). Throttled to whole-percent changes by the sender.
    Progress(u64, u64),
    /// Desktop: the binary swap completed and verified — re-exec into this path.
    #[cfg(not(target_os = "android"))]
    Applied(std::path::PathBuf),
    /// Android: the APK downloaded + hash-verified — hand to the system installer.
    #[cfg(target_os = "android")]
    ApkReady(String),
    /// A download/verify/apply step failed; the running version is untouched.
    ApplyFailed(String),
}

/// Rubber-band wheel step: full-strength inside `[0, hi]`, asymptotically resisted past either end — the overshoot can never exceed `reach`, and `tick()` eases it back once input stops. The resistance factor `(reach/(reach+over))²` is 1 at the boundary (C¹ join with in-range scrolling — the hard clamp this replaces was a C⁰ kink, banned by the GUI-continuity rule) and falls smoothly toward 0, integrating to a `reach·over/(reach+over)` saturation. `hi = f32::INFINITY` rubber-bands only the 0 end (conversation history).
fn rubber_step(cur: f32, step: f32, hi: f32, reach: f32) -> f32 {
    let over = if cur < 0.0 {
        -cur
    } else if cur > hi {
        cur - hi
    } else {
        0.0
    };
    let f = (reach / (reach + over)) * (reach / (reach + over));
    cur + step * f
}

fn settings_page_rows(page: SettingsPage) -> usize {
    match page {
        SettingsPage::You => 7,
        SettingsPage::Diagnostics => 10,
        SettingsPage::Security => 15,
        SettingsPage::Wave => 10,
        _ => 8,
    }
}

/// Natural row count per contact-panel page — the scroll-extent input, mirroring [`settings_page_rows`]. About's first rows are consumed by the avatar block (drawn at row height ×N, not text).
fn contact_page_rows(page: ContactPage) -> usize {
    match page {
        ContactPage::About => 12,
        ContactPage::Stats => 9,
        ContactPage::Manage => 6,
    }
}

/// Snapshot of one fleet sibling's presence for §4.2 parking decisions: (device pubkey, is_online, presence_probed).
type SiblingPresence = ([u8; 32], bool, bool);

/// Collect every sibling's presence snapshot — taken BEFORE a mutable walk over `self.contacts` so [`ceremony_parked_by`] can be consulted mid-loop without a second borrow.
fn sibling_presence_snapshot(contacts: &[crate::types::Contact]) -> Vec<SiblingPresence> {
    contacts
        .iter()
        .filter(|c| c.is_sibling)
        .filter_map(|c| c.device_key().map(|k| (k, c.is_online, c.presence_probed)))
        .collect()
}

impl PhotonApp {
    /// OUR identity handle proof, session-first: a sticky-broadcast resume can reach Ready with HandleQuery's worker-populated `last_handle_proof` still unset, and every fleet-plane gate that read only the cache sat silently dead for the whole session — no fstate pull, no roster push, no fleet-key sync, so §4.2 ownership never converged between siblings (a resumed phone, 36 minutes, 2026-08-04). Signed in ⇒ session ⇒ handle_proof; the cache covers the pre-session attest window.
    fn our_handle_proof(&self) -> Option<[u8; 32]> {
        self.session.as_ref().map(|s| s.handle_proof).or_else(|| {
            self.handle_query
                .as_ref()
                .and_then(|hq| hq.get_handle_proof())
        })
    }
}

/// The relay list a send toward this contact should carry, judged by whether the DIRECT path can actually be trusted right now. The old idiom — relay only when `validated_path.is_none()` — treated any Some() as "direct works", but a validated path is HELD state: a phone that left the LAN it validated on still holds the house's address, and suppressing the relay on that stale Some() black-holed every frame (a phone on cellular still aiming at the peer's home LAN, the weave probe undelivered, the ceremony stuck "testing the secure channel", 2026-08-03). A validated path earns relay-suppression only when it is NOT a foreign-LAN leftover; anything less rides the pipe too — the relay is cheap and the receiver dedups.
fn relay_unless_direct_trusted(
    c: &crate::types::Contact,
    our_lan_v4: Option<std::net::Ipv4Addr>,
) -> Vec<[u8; 32]> {
    use crate::network::traverse::gather::is_foreign_peer_lan;
    let direct_trusted = c
        .validated_path
        .map_or(false, |(a, _)| !is_foreign_peer_lan(&a, our_lan_v4));
    if direct_trusted {
        Vec::new()
    } else {
        c.relay_device_list()
    }
}

/// The conversation a contact row stands for, found by participant-set id. A FREE function on the conversations slice, not a `&self` method, so render scopes holding a `&mut chrome` field borrow can still resolve it thru disjoint field paths.
fn dm_conversation<'a>(
    conversations: &'a [crate::types::Conversation],
    us: &[u8; 32],
    c: &crate::types::Contact,
) -> Option<&'a crate::types::Conversation> {
    let id = c.conversation(us).id();
    conversations.iter().find(|v| v.id() == id)
}

/// §4.2: true when this FRIEND contact's ceremony belongs to another fleet device and this device must not run — or keep alive — a round toward it. Encodes the takeover-hardening rules:
/// - sibling contacts are never parked (sibling weaves are per-device-pair by design);
/// - unclaimed or self-owned → not parked (the winner proceeds; claim-on-pickup handles None);
/// - `owner_woven` → ALWAYS parked — never re-clutch a completed friendship: the chain lives on the owner, and a takeover would clobber the friend's chain (the live-diagnosed fork);
/// - owner sibling present → parked (its ceremony is the fleet's);
/// - owner sibling NOT YET PROBED this session → parked (boot race: every sibling starts is_online=false before the first sweep, which made a freshly-booted device "take over" from a live owner);
/// - owner probed-offline, or owner device unknown (revoked) → NOT parked — takeover-eligible.
fn ceremony_parked_by(
    c: &crate::types::Contact,
    our_device: Option<[u8; 32]>,
    siblings: &[SiblingPresence],
) -> bool {
    if c.is_sibling {
        return false;
    }
    let Some(owner) = c.ceremony_owner else {
        return false;
    };
    if Some(owner) == our_device {
        return false;
    }
    if c.owner_woven {
        return true;
    }
    match siblings.iter().find(|(pk, _, _)| *pk == owner) {
        Some((_, online, probed)) => *online || !*probed,
        None => false,
    }
}

/// The status line for a friend's ceremony, fleet-aware: if ANOTHER of our devices owns the ceremony (§4.2 claim), say so — "weaving on <device>…" / "secured on <device>" — instead of showing our own deliberately-parked round. Falls thru to the contact's own step detail otherwise. Free function (not a method) so render arms can call it while `chrome` holds the &mut self borrow.
fn contact_status_line(
    c: &crate::types::Contact,
    our_device: Option<[u8; 32]>,
    identity_seed: Option<&[u8; 32]>,
) -> String {
    if c.clutch_proof_gave_up {
        return "can\u{2019}t complete \u{2014} they answer as a different identity; remove & re-add".to_string();
    }
    if !c.is_sibling && !c.chain_woven {
        match c.ceremony_owner {
            Some(owner) if Some(owner) != our_device => {
                let name = identity_seed
                    .map(|seed| crate::network::fgtw::fleet::device_name_default(&owner, seed))
                    .unwrap_or_else(|| "another device".to_string());
                return if c.owner_woven {
                    format!("secured on {name} \u{2014} replies visible here; send from there (for now)")
                } else {
                    format!("securing on {name}\u{2026}")
                };
            }
            // A sibling's roster says the friendship is woven but no owner claim survived (pre-§4.2 ceremony) — still HONEST: it is secured elsewhere, not "weaving" here. The owner backfill in seal_chain_if_ready names the device once the owner pushes a fresh roster.
            None if c.owner_woven => {
                return "secured on another of your devices \u{2014} replies visible here; send from there (for now)".to_string();
            }
            _ => {}
        }
    }
    c.clutch_status_detail()
}

/// One connectivity classification for ANY counterparty device row — the ring and the path dot both map from this, and the self row folds it over the fleet siblings (same rule, different counterparty set — self and bob are both people).
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum ConnTier {
    Lan,
    /// Wi-Fi Direct group path (192.168.49/24) — radio-direct with no infrastructure at all; outranks even LAN in the display because it means the phones are talking to each other, not to a router.
    Wfd,
    Wan,
    Relay,
    Offline,
}

/// The tier of one contact row's live connectivity. A LIVE validated direct path is authoritative and wins over reached_via_relay — that flag tracks how the LAST frame happened to arrive, and a peer reachable BOTH ways (good direct path + the relay pipe still delivering redundant copies) flips it every cycle (the amber/green flicker). validated_path is held state with a TTL, so it doesn't flap; relay only colours the tier when there is genuinely no direct path.
pub(crate) fn contact_conn_tier(c: &crate::types::Contact) -> ConnTier {
    if !c.is_online {
        return ConnTier::Offline;
    }
    if let Some((addr, _)) = c.validated_path {
        // WFD subnet check IS address-shape-safe here (unlike the LAN verdict below): 192.168.49/24 is the P2P group's fixed subnet, carrier CGNAT never hands it out.
        if let std::net::IpAddr::V4(v4) = addr.ip() {
            if crate::network::traverse::gather::is_wfd_subnet(v4) {
                return ConnTier::Wfd;
            }
        }
        // The same-LAN verdict was judged ONCE at the validation edge (status.rs punch-ack arm: private AND on our own subnet / WFD group). Re-deriving from the address here called every RFC-private address "same room" — and carrier CGNAT hands cellular devices 10.x, so a Verizon-internal path to a peer hundreds of miles away rang cyan (field 2026-08-30). Private-but-foreign is a real direct path: WAN.
        return if c.validated_path_lan {
            ConnTier::Lan
        } else {
            ConnTier::Wan
        };
    }
    if c.reached_via_relay {
        ConnTier::Relay
    } else {
        // Online with no proven direct path yet: the punch is still in flight.
        ConnTier::Wan
    }
}

/// Presence-ring tier (user spec, VSF-authored in theme.rs): cyan = direct in the same room (LAN), green = direct across the WAN, amber = relay-only, grey = offline. LAN = the validated direct path is a private / link-local / ULA address; a same-site GLOBAL v6 path (e.g. two phones on one home /64) still reads green — refining that needs a same-prefix check against our own addresses, later.
pub(crate) fn ring_tier_colour(c: &crate::types::Contact, has_remote: bool) -> u32 {
    // Zero-remote rows must come thru row_ring_tier (the sibling fold); a direct call with has_remote=false is the single-device degenerate answer.
    let tier = if has_remote {
        contact_conn_tier(c)
    } else {
        ConnTier::Lan
    };
    ring_colour_of(tier)
}

/// A row's HONEST ring tier over an explicit contacts slice — the free-fn form for render scopes where `chrome.as_mut()` pins self (contacts is a disjoint field). For a friend row the tier is the friend's own classification; for the self/notes row it is the best tier over the fleet SIBLINGS — same classifier, different counterparty set, so a dead sync partner shows grey instead of a hardcoded always-LAN lie. No siblings = single-device fleet = LAN.
pub(crate) fn row_ring_tier_in(
    contacts: &[crate::types::Contact],
    c: &crate::types::Contact,
    has_remote: bool,
) -> u32 {
    if has_remote {
        return ring_tier_colour(c, true);
    }
    match contacts
        .iter()
        .filter(|s| s.is_sibling && !s.locked_out)
        .map(contact_conn_tier)
        .min()
    {
        Some(t) => ring_colour_of(t),
        None => ring_tier_colour(c, false),
    }
}

pub(crate) fn ring_colour_of(tier: ConnTier) -> u32 {
    match tier {
        ConnTier::Lan => *theme::RING_LAN_COLOUR,
        ConnTier::Wfd => *theme::RING_WFD_COLOUR,
        ConnTier::Wan => *theme::RING_ONLINE_COLOUR,
        ConnTier::Relay => *theme::RING_RELAY_COLOUR,
        ConnTier::Offline => *theme::RING_OFFLINE_COLOUR,
    }
}

/// The transport tier of a live path as a DOT colour: LAN green (same subnet — no NAT, nothing in the middle), WAN cyan (a punched or routable direct path across the internet), relay orange (no direct path — frames ride the seed's pipe). `None` for a device that isn't reachable at all, which renders no dot.
/// Same held-state rule the avatar ring uses: a live `validated_path` is authoritative and outranks `reached_via_relay`, because that flag tracks how the LAST frame happened to arrive and flaps every cycle for a peer reachable both ways.
fn path_tier_colour(c: &crate::types::Contact, has_remote: bool) -> Option<u32> {
    let tier = if has_remote {
        contact_conn_tier(c)
    } else {
        ConnTier::Lan
    };
    match tier {
        ConnTier::Lan => Some(*theme::PATH_LAN_COLOUR),
        // One display language, ring and dot alike: WFD = the VSF primary blue.
        ConnTier::Wfd => Some(*theme::RING_WFD_COLOUR),
        ConnTier::Wan => {
            // A validated path earns WAN; online with no proven direct path yet rides the relay — say so rather than promising a direct path we don't have.
            if c.validated_path.is_some() {
                Some(*theme::PATH_WAN_COLOUR)
            } else {
                Some(*theme::PATH_RELAY_COLOUR)
            }
        }
        ConnTier::Relay => Some(*theme::PATH_RELAY_COLOUR),
        ConnTier::Offline => None,
    }
}

/// How many doublings a contact's presence cadence may take: 1 minute at level 0 up to roughly an hour. Beyond that a contact is effectively "checked hourly", which is the floor Nick asked for on battery grounds.
const PING_BACKOFF_MAX: u8 = 6;
/// The base cadence a contact is polled at when something is actively going on with them.
const PING_BASE: std::time::Duration = std::time::Duration::from_secs(60);

/// Is this contact due for a presence ping?
///
/// Backoff is PER CONTACT and doubles with each quiet round — 1, 2, 4 … up to ~an hour — so the friend you have not spoken to since March costs a request an hour, not one every twenty seconds. Anything that means "this contact matters right now" resets it to the floor: they spoke, we opened their conversation, we sent to them.
///
/// The one exception is a contact holding a VALIDATED direct path AND recent traffic: that path exists only because something keeps its NAT mapping warm, so letting it taper would drop us to the relay mid-conversation. A path with no recent traffic is allowed to lapse — holding every path open forever was the old global clamp, and it is what made the taper cosmetic.
fn contact_ping_due(c: &crate::types::Contact, now: std::time::Instant) -> bool {
    let Some(last) = c.last_pinged else {
        return true; // never pinged — always reach a contact at least once
    };
    // FLEET-FIRST probe acceleration: an UNPROBED sibling gates every friend keygen (spawn_next_pending_keygen), so its verdict must land in seconds — re-ping just past the 5s expiry so the 3 offline strikes accrue back-to-back (~18s to a dead-sibling verdict) instead of riding the 60s backoff (~2min). Ends at the verdict edge; the normal cadence owns the row from there.
    if c.is_sibling && !c.presence_probed {
        return now.duration_since(last) >= std::time::Duration::from_secs(6);
    }
    let recent_traffic = c
        .last_heard
        .is_some_and(|t| now.duration_since(t) < std::time::Duration::from_secs(120));
    if c.validated_path.is_some() && recent_traffic {
        return now.duration_since(last) >= VALIDATED_PATH_KEEPALIVE;
    }
    let interval = PING_BASE * (1u32 << c.ping_backoff.min(PING_BACKOFF_MAX));
    now.duration_since(last) >= crate::jitter_dur(interval)
}

/// Bytes as something a person can read at a glance. Deliberately coarse — one decimal past a megabyte is noise on a figure that exists to answer "is there much in there?".
fn human_bytes(n: u64) -> String {
    const KIB: u64 = 1024;
    const MIB: u64 = KIB * 1024;
    if n >= MIB {
        format!("{:.1} MiB", n as f64 / MIB as f64)
    } else if n >= KIB {
        format!("{} KiB", n / KIB)
    } else {
        format!("{} B", n)
    }
}

/// The About slab geometry: the ATTEST screen's spectrum/wordmark proportions (LaunchLayout portrait slices — 0.75u top air, 6u spectrum, wordmark 3.5u overlapping the spectrum's bottom by 2u, of a 22.75u window) shrunk UNIFORMLY by the pane/window width ratio — same shape, pane-sized, zoom-independent (Nick 2026-09-02: the first slab was "too short and wide with no air up top"). Returns (unit_px, total_slab_h); band offsets in units: air 0..0.75, wave 0.75..6.75, wordmark 4.75..8.25. One formula, shared by the bg-pass draw and the card's cursor advance so they can't drift.
pub(super) fn about_slab(buf_w: usize, buf_h: usize, pane_w: f32) -> (f32, f32) {
    let unit = (buf_h as f32 / 22.75) * (pane_w / buf_w as f32).min(1.0);
    (unit, unit * 8.25)
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
    text.draw_text_left(
        canvas,
        s,
        row.x + size * 0.3,
        row.center_y(),
        &TextStyle::new(size, colour).weight(weight).font("Oxanium"),
        None,
        None,
    );
}

/// FLOW layout for settings pages (Nick 2026-09-02, "option 2"): a y-cursor every element advances by its own height — so EVERY text element wraps at the pane edge by construction, and the page's true height falls out of the final cursor (measured scroll extent, no hand-counted row estimates). The About card proved the pattern; this is it, named. Pages convert one at a time; toka-rendered documents later ride this same substrate.
pub(super) struct Flow {
    pub x: Coord,
    pub y: Coord,
    pub w: Coord,
    start_y: Coord,
}

impl Flow {
    /// Start a flow at the content inset, offset by the page scroll.
    pub(super) fn new(inset: fluor::region::Region, scroll: Coord) -> Self {
        Flow { x: inset.x, y: inset.y - scroll, w: inset.w, start_y: inset.y - scroll }
    }

    /// One line that WRAPS if it must (titles, statuses — anything). Advances by the drawn height.
    pub(super) fn line(
        &mut self,
        canvas: &mut Canvas,
        text: &mut fluor::text::TextRenderer,
        s: &str,
        size: Coord,
        colour: u32,
        weight: u16,
    ) {
        let region = fluor::region::Region::new(self.x, self.y, self.w, size * 1.6);
        let n = settings_prose(canvas, text, region, s, size, colour, weight);
        self.y += (n.max(1) as Coord) * size * 1.25 + size * 0.35;
    }

    /// A paragraph — same as [`line`] (everything wraps here); named for call-site intent.
    pub(super) fn prose(
        &mut self,
        canvas: &mut Canvas,
        text: &mut fluor::text::TextRenderer,
        s: &str,
        size: Coord,
        colour: u32,
        weight: u16,
    ) {
        self.line(canvas, text, s, size, colour, weight);
    }

    /// Vertical breathing room.
    pub(super) fn gap(&mut self, h: Coord) {
        self.y += h;
    }

    /// Claim a fixed-height band (pills, checkboxes, custom draws) and advance past it.
    pub(super) fn band(&mut self, h: Coord) -> fluor::region::Region {
        let r = fluor::region::Region::new(self.x, self.y, self.w, h);
        self.y += h;
        r
    }

    /// Total content height flowed so far — the MEASURED scroll extent input (content_h; caller subtracts the pane height).
    pub(super) fn used(&self) -> Coord {
        self.y - self.start_y
    }
}

/// Centred wrapped line (the About card, Nick 2026-09-02: stanza lines clipped at the pane edge when zoomed big — "they just dissa"). Each authored line stays its own stanza but WRAPS to as many centred rows as it needs at the current width/zoom; returns the advanced y. `step` is the per-row advance (the caller's rhythm).
#[allow(clippy::too_many_arguments)]
pub(super) fn centered_wrapped(
    canvas: &mut Canvas,
    text: &mut fluor::text::TextRenderer,
    cx: Coord,
    max_w: Coord,
    y: Coord,
    s: &str,
    style: &TextStyle,
    step: Coord,
    clip: Option<fluor::paint::Clip>,
) -> Coord {
    let lines = wrap_text_lines(text, s, style, max_w.max(8.0));
    let mut yy = y;
    for l in &lines {
        text.draw_text_center(canvas, l, cx, yy + step * 0.5, style, clip, None);
        yy += step;
    }
    yy
}

/// Flow-aware action pills (Nick 2026-09-02: "have button A and button B be on distinct lines if the width starts to clamp on the inside text"): each pill sizes to its MEASURED label at the natural font (matching draw_pill_immediate's h×0.5 + 1.6-em padding, so the fit-to-slot shrink never engages); pills lay side-by-side while the pane holds them and WRAP onto a fresh band when it can't — a label never squeezes. Returns nothing; the flow cursor ends past the last band.
#[allow(clippy::too_many_arguments)]
pub(super) fn flow_pills(
    flow: &mut Flow,
    canvas: &mut Canvas,
    text: &mut fluor::text::TextRenderer,
    hit_map: &mut [HitId],
    buf_w: usize,
    buf_h: usize,
    pressed_hit: HitId,
    size: Coord,
    pills: &[(&str, HitId, bool)],
) {
    let pill_h = size * 2.0;
    let band_h = pill_h + size * 0.5;
    let gap = size * 0.8;
    let margin = size * 0.3;
    let font = size; // draw_pill_immediate uses rect.h × 0.5 = size — measure at the same size so widths agree
    let mut x = flow.x + margin;
    let right = flow.x + flow.w - margin;
    let mut band: Option<fluor::region::Region> = None;
    for (label, hit_id, enabled) in pills {
        let style = TextStyle::new(font, 0).weight(500).font("Open Sans");
        let w = (text.measure_text(label, &style) + font * 1.6 + size * 0.4).min(right - flow.x - margin);
        if band.is_none() || x + w > right {
            let b = flow.band(band_h);
            x = flow.x + margin;
            band = Some(b);
        }
        let b = band.unwrap();
        let rect = fluor::region::Region::new(x, b.y + (b.h - pill_h) * 0.5, w, pill_h);
        if *enabled {
            draw_stub_pill(canvas, text, hit_map, buf_w, buf_h, rect, label, *hit_id, pressed_hit);
        } else {
            draw_stub_pill_disabled(canvas, text, hit_map, buf_w, buf_h, rect, label, *hit_id, pressed_hit);
        }
        x += w + gap;
    }
}

/// Word-wrapped settings prose (Nick 2026-09-02: settings text ran off the screen edge mid-word) — the conversation screen's greedy wrapper reused, drawn into `region` (usually several stacked rows). Lines stack at `size × 1.25` from the region's top; returns the line count so a caller can flow content below. Prose that outgrows the region is still drawn (the page scroll owns overflow) — never silently truncated.
fn settings_prose(
    canvas: &mut Canvas,
    text: &mut fluor::text::TextRenderer,
    region: fluor::region::Region,
    s: &str,
    size: Coord,
    colour: u32,
    weight: u16,
) -> usize {
    let style = TextStyle::new(size, colour).weight(weight).font("Oxanium");
    let max_w = (region.w - size * 0.6).max(size);
    let lines = wrap_text_lines(text, s, &style, max_w);
    let step = size * 1.25;
    let mut y = region.y + size * 0.9;
    for line in &lines {
        text.draw_text_left(canvas, line, region.x + size * 0.3, y, &style, None, None);
        y += step;
    }
    lines.len()
}

thread_local! {
    /// The hovered hit id for the CURRENT frame's immediate-mode action pills. Set once per render from `self.hover_hit` (see [`set_stub_hover`]) so [`draw_stub_pill_filled`] can tell fluor's pill renderer which pill is hovered WITHOUT threading `hover_hit` through every wrapper and all ~30 call sites. UI thread only; a stale value between frames is harmless (the next render overwrites it before any pill draws).
    static STUB_HOVER_HIT: std::cell::Cell<HitId> = const { std::cell::Cell::new(HIT_NONE) };
}

/// Publish the frame's hovered hit id for the immediate-mode pills. Call once at the top of render, before any `draw_stub_pill*`.
pub(super) fn set_stub_hover(hit: HitId) {
    STUB_HOVER_HIT.with(|c| c.set(hit));
}

fn stub_hover() -> HitId {
    STUB_HOVER_HIT.with(|c| c.get())
}

/// Draw an inert stub action pill filling `rect`: a Button-family squircle (fill + two-tone raised edge) with a centred label, hit-stamped with `hit_id`. STUB only — clicks land in the settings dispatch range and log a line; nothing functional fires. Kept immediate-mode (not a persistent `Button`) because the panel has many one-off action pills and a stub doesn't need each to carry click-counter state. Rendering is delegated to [`fluor::widgets::Button::draw_pill_immediate`] — the SAME code a retained Button paints with, so pills and buttons are visually identical and hover/press come from fluor, not a hand-rolled tint.
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
    draw_stub_pill_styled(
        canvas,
        text,
        hit_map,
        buf_w,
        buf_h,
        rect,
        label,
        hit_id,
        pressed_hit,
        true,
    );
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
    draw_stub_pill_styled(
        canvas,
        text,
        hit_map,
        buf_w,
        buf_h,
        rect,
        label,
        hit_id,
        pressed_hit,
        false,
    );
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
    draw_stub_pill_filled(
        canvas,
        text,
        hit_map,
        buf_w,
        buf_h,
        rect,
        label,
        hit_id,
        pressed_hit,
        enabled,
        None,
        "Open Sans",
    );
}

/// [`draw_stub_pill_styled`] with an optional custom fill pair `(idle, held)` — the Security page's destructiveness ramp (green → yellow → orange → red). `None` = the standard BUTTON_FILL/HELD navy.
#[allow(clippy::too_many_arguments)]
fn draw_stub_pill_filled(
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
    fill: Option<(u32, u32)>,
    label_font: &'static str,
) {
    // Rendering + fit-to-slot + hit stamp all live in fluor now (ONE pill renderer, shared with retained Buttons). Hover comes from the frame's `set_stub_hover` publish; press from `pressed_hit`; a disabled pill passes `None` for the hit map so it stamps nowhere (a dead tap dispatches to HIT_NONE). `label_font` is normally "Open Sans"; the version buttons pass "Oxanium" so the dozenal control-block glyphs resolve to its +glyphs face.
    let hovered = hit_id != HIT_NONE && stub_hover() == hit_id;
    let pressed = hit_id != HIT_NONE && hit_id == pressed_hit;
    fluor::widgets::Button::draw_pill_immediate(
        canvas,
        text,
        if enabled { Some(hit_map) } else { None },
        buf_w,
        buf_h,
        rect,
        label,
        label_font,
        hit_id,
        hovered,
        pressed,
        enabled,
        fill,
    );
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

#[cfg(test)]
mod compose_codec_tests {
    /// The reaction-recency settings blob: typed x/e6 pairs round-trip in order, and garbage decodes as empty rather than erroring (the strip falls back to defaults).
    #[test]
    fn react_recency_blob_round_trips_typed() {
        let stamps = vec![
            ("\u{1F44D}".to_string(), 9_000_000_000i64),
            ("Z".to_string(), 42),
        ];
        let blob = super::PhotonApp::encode_react_recent(&stamps);
        assert_eq!(super::PhotonApp::decode_react_recent(&blob), stamps);
        assert!(super::PhotonApp::decode_react_recent(b"not a field").is_empty());
    }
}
