//! [`PhotonApp`]: the [`fluor::host::app::FluorApp`] impl that hosts Photon on desktop. Owns the app state machine (`AppState`), network handles, contact list, and the per-screen widgets (Launch / Ready / Searching / Conversation), drawing the chrome (perimeter, shadow, window buttons, app-icon orb) plus each screen's content, and routing cross-thread wake-ups thru `FluorApp::on_user_event` with the [`super::PhotonEvent`] payload.

use super::chromatic_wave::chromatic_wave;
use super::launch_layout::{AttestBlockLayout, LaunchLayout};
use super::photon_logo::paint_photon_logo;
use super::ready_layout::ReadyLayout;
use super::state::{AppState, LaunchState};
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

/// Contact name text on the Ready list — near-white. α+darkness (the format fluor's text/shape rasterizers expect; the legacy `theme::CONTACT_NAME` is visible-RGB and not interchangeable here).
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
/// Grey placeholder circle for contacts/avatars without a loaded image.
const AVATAR_PLACEHOLDER: u32 = 0xFF_C5_C5_C5;

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

/// Security and Recovery posture for the current identity — each a count of filled pips out of [`POSTURE_PIPS`]. Two orthogonal axes, surfaced on the Ready-screen bottom strip:
///   * Security — how hard it is for an attacker to steal or forge this identity. Bounded by the device root. Today every platform derives `device_secret` from a *readable* fingerprint (Linux machine-id, Windows MachineGuid, macOS IOPlatformUUID), so same-privilege code can lift it: 1 pip everywhere. A root-gated firmware fact would be 2; a hardware enclave or PIPE, 3.
///   * Recovery — how hard it is for the *owner* to lose this identity for good. For a single device it is whether the root survives a factory reset: macOS's IOPlatformUUID is firmware and re-derives after a wipe (2); Linux machine-id, Windows MachineGuid and Android's ANDROID_ID are software / reset-volatile (1). Device redundancy (Mirrored), a durable anchor (desktop/PIPE) and social vouching raise this toward 3.
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

/// Status-message colour for the "Attesting…" indicator that occupies the error slot while a handle query is in flight. Pure visible white, fully opaque — same slot as `ERROR_TEXT_COLOUR` but white instead of red so the user reads it as "neutral status" rather than "something went wrong".
const STATUS_TEXT_COLOUR: u32 = fluor::theme::dark(fluor::theme::fmt(0x00_FF_FF_FF));

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
];

/// Bounding rect the chord hint panel covers — matches `paint::draw_chord_hint`'s positioning math so `damage_rect` can union it when both brackets are held. Pulled out of the panes example with the same math; if fluor's hint geometry changes, this needs updating in lockstep.
fn chord_hint_bbox(viewport: Viewport, vw: usize, vh: usize) -> PixelRect {
    let span = viewport.effective_span();
    let font_size = (span * 0.014).max(11.0);
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
    /// `true` while a left-mouse-button drag is extending the textbox selection (set on left-press over a focused textbox, cleared on left-release). `CursorMoved` consults this to decide whether to grow the selection toward the cursor — otherwise hover updates are the only thing CursorMoved touches.
    is_dragging_select: bool,
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
    /// The device's session identity (register-shaped roots), set on `QueryResult::Success`. `None` while the user is still on Launch. Replaces the handle string — Photon never holds the plaintext handle past first attest; an optional "show my handle" label would re-prompt rather than store it.
    session: Option<tohu::SessionIdentity>,
    /// True when the dual-ring vault flagged a damaged ring on open this session. Drives the persistent amber banner on the Ready screen. Sticky for the session.
    vault_degraded: bool,
    /// FGTW connectivity state — flipped by `HandleQuery::try_recv_online`. Drives the top-left chrome orb's colour (red offline / green online). Starts false; the background worker reports the first real status within the first second of launch.
    online: bool,
    /// Contacts-page handle search/add textbox (Ready state). Distinct from `textbox` so content doesn't bleed between Launch (handle being attested) and Ready (handle being added as a contact).
    contacts_textbox: Option<Textbox>,
    /// Plus button to the right of `contacts_textbox` — clicking it (or pressing Enter in the textbox) triggers the add-contact flow (`HandleQuery::search`). Will eventually carry an idle "+" glyph and an in-progress rotating-hourglass animation (legacy port from `compositing.rs`); that lands when `ProgressButton` gets extracted to fluor.
    contacts_plus_btn: Option<Button>,
    /// Encrypted local storage — initialized after attestation success with the device secret + handle.
    storage: Option<crate::storage::FlatStorage>,
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
    /// Contact-list scroll offset in pixels (Ready screen). 0 = top; grows as the user scrolls down. The user section (avatar/search) stays fixed; only the rows below the separator scroll. Re-clamped to the list extent each render.
    contacts_scroll: isize,
    /// `true` once the user has interacted (any click or keystroke) since the last transition into `Ready` — hides the standing avatar prompt. Hints are event-shown and interaction-cleared, never hover- or time-driven; reset to `false` on each `Ready` entry. See [`clear_hints`].
    hints_dismissed: bool,
    /// `true` while the cursor is over the Ready-screen avatar circle. Drives the "drag/drop to update avatar" hover hint.
    avatar_hovered: bool,
}

impl PhotonApp {
    /// Construct an empty app shell. Real state (chrome, network handles, app state machine) initializes in [`FluorApp::init`] once the viewport is known.
    pub fn new() -> Self {
        Self {
            chrome: None,
            hit_counter: 0,
            event_proxy: None,
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
            is_dragging_select: false,
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
            friendship_chains: Vec::new(),
            chord_lb_press: None,
            chord_lb_release: None,
            chord_rb_press: None,
            chord_rb_release: None,
            show_hitmask: false,
            debug_hit_colours: Vec::new(),
            last_chord_held: false,
            session: None,
            vault_degraded: false,
            online: false,
            contacts_textbox: None,
            contacts_plus_btn: None,
            storage: None,
            contacts: Vec::new(),
            add_in_flight: false,
            hourglass_angle: 0.0,
            hourglass_rng: 0x9E37_79B9_7F4A_7C15,
            search_status: None,
            device_keypair: None,
            pending_keyboard_request: None,
            device_avatar_pixels: None,
            device_avatar_scaled: None,
            device_avatar_scaled_diameter: 0,
            avatar_hit_id: HIT_NONE,
            active_contact: None,
            contact_hit_base: HIT_NONE,
            back_btn_hit_id: HIT_NONE,
            pending_picker_request: false,
            pending_broadcast_signal: 0,
            contacts_scroll: 0,
            hints_dismissed: false,
            avatar_hovered: false,
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
        if let Err(e) = crate::ui::avatar::save_avatar_from_seed(&av1_data, &identity_seed) {
            crate::log(&format!("avatar picker: save failed: {e}"));
            return;
        }
        let Some((_, vsf_rgb)) = crate::ui::avatar::load_avatar_from_seed(&identity_seed) else {
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
                match crate::ui::avatar::upload_avatar_from_seed(&kp.secret, &identity_seed, &hp) {
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
    const ORB_ONLINE: u32 = 0xFF_BF_1F_BF;
    const ORB_OFFLINE: u32 = 0xFF_1F_BF_BF;
    fluor::host::chrome::OrbTint::Custom {
        ring: if online { ORB_ONLINE } else { ORB_OFFLINE },
        brighten: online,
    }
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
            // The attest button is only part of the tree when there's a handle to attest — same reveal as the render gate. An empty field yields just the textbox, so Tab can't land focus on a button that isn't drawn and a hit-test can't dispatch to it.
            let handle_entered = self
                .textbox
                .as_ref()
                .map(|tb| !tb.chars.is_empty())
                .unwrap_or(false);
            if let Some(tb) = self.textbox.as_mut() {
                f(tb);
            }
            if handle_entered {
                if let Some(btn) = self.attest_btn.as_mut() {
                    f(btn);
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

        // Chrome owns its own hit-test map sized to the viewport, allocates four hit-ids for its buttons via the threaded counter, and stamps the perimeter + button rasters in `rasterize_chrome`. The Photon orb (chromatic starburst — same brand mark as the OS-level app icon) ships as a VSF image and decodes into the chrome's app_icon slot.
        let orb_icon =
            fluor::host::icon::Icon::from_vsf_bytes(include_bytes!("../../assets/photon-orb.vsf"))
                .ok();
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
        }

        // Spawn the presence + CLUTCH status checker on HandleQuery's shared socket. Desktop only — Android's checker takes no wake sender (its redraws come thru the JNI/Choreographer path). Done BEFORE `hq` is moved into the field so we can take its socket.
        #[cfg(not(target_os = "android"))]
        {
            match crate::network::status::StatusChecker::new(
                hq.socket(),
                self.device_keypair
                    .clone()
                    .expect("device_keypair set above"),
                self.contact_pubkeys.clone(),
                self.sync_records.clone(),
                proxy.clone(),
            ) {
                Ok(c) => {
                    self.status_checker = Some(c);
                    crate::log("UI: status checker started (presence + CLUTCH)");
                }
                Err(e) => crate::log(&format!("UI: status checker failed to start: {e}")),
            }
        }

        self.handle_query = Some(hq);

        // Auto-resume from the remembered session roots. If tohu has this login's roots (persisted on a prior, FGTW-confirmed attest), paint Ready IMMEDIATELY from local state — we already own this identity, so there is no reason to block the first frame on the network. The avatar comes from a local cache file (no vault, no network); contacts + peer presence + cloud-merge arrive a beat later via the background `query_resume` and merge in through `on_query_result`. A rejection (handle claimed by another device) bails back to the attest screen; a transient network error leaves the local session on Ready untouched.
        // None (first run / post-logout) falls through to the normal typed-attest flow.
        if let Some(remembered) = tohu::session() {
            self.session = Some(remembered);
            self.device_avatar_pixels =
                crate::ui::avatar::load_avatar_from_seed(&remembered.identity_seed)
                    .map(|(_, vsf_rgb)| crate::ui::colour_convert::vsf_rgb_to_bt2020(&vsf_rgb));
            self.hints_dismissed = false; // fresh Ready entry → the avatar prompt gets a chance until first interaction
                                          // Initialize local storage and load contacts immediately so the contact list is visible before the FGTW round-trip completes.
            if let Some(kp) = &self.device_keypair {
                let device_secret = *kp.secret.as_bytes();
                match crate::storage::FlatStorage::new(
                    crate::storage::APP,
                    remembered.vault_seed,
                    device_secret,
                ) {
                    Ok(s) => {
                        self.contacts = crate::storage::contacts::load_all_contacts(&s);
                        crate::log(&format!(
                            "UI: loaded {} contact(s) from local vault on resume",
                            self.contacts.len()
                        ));
                        // Seed the checker's answerable-pubkey set with every loaded contact so pongs from them are honoured.
                        if let Ok(mut pks) = self.contact_pubkeys.lock() {
                            for c in &self.contacts {
                                if !pks.contains(&c.public_identity) {
                                    pks.push(c.public_identity.clone());
                                }
                            }
                        }
                        // Rehydrate each contact's saved ephemeral keypairs from disk (~588KB each).
                        // load_contact_state deliberately doesn't pull these (they're huge and live
                        // in a separate vault key), so without this every resume re-runs the
                        // McEliece-heavy keygen below — which is what froze the UI on launch. Loading
                        // the persisted keypairs makes the re-key filter a no-op for contacts that
                        // already have them, so keygen only fires for genuinely keyless Pending ones.
                        for contact in self.contacts.iter_mut() {
                            if contact.clutch_our_keypairs.is_none() {
                                match crate::storage::contacts::load_clutch_keypairs(
                                    contact.handle.as_str(),
                                    &s,
                                ) {
                                    Ok(Some(keypairs)) => {
                                        contact.clutch_our_keypairs = Some(keypairs);
                                    }
                                    Ok(None) => {}
                                    Err(e) => crate::log(&format!(
                                        "CLUTCH: failed to rehydrate keypairs for {}: {}",
                                        contact.handle, e
                                    )),
                                }
                            }
                        }
                        self.storage = Some(s);
                        // Force any self-contact Complete before re-keying so it's excluded below (a self-contact has no peer to key with).
                        self.settle_self_contacts();
                        // Re-key only Pending contacts that still have no keypairs after the rehydrate above — without keypairs a Pending contact can never send its offer (it sticks on Pending forever). This is the fallback for contacts added before the keypairs were ever persisted. Self-contact excluded (same identity, no exchange).
                        let our_handle_hash = remembered.identity_seed;
                        let to_rekey: Vec<(ContactId, [u8; 32])> = self
                            .contacts
                            .iter_mut()
                            .filter(|c| {
                                c.handle_hash != our_handle_hash
                                    && c.clutch_state == crate::types::ClutchState::Pending
                                    && c.clutch_our_keypairs.is_none()
                                    && !c.clutch_keygen_in_progress
                            })
                            .map(|c| {
                                c.clutch_keygen_in_progress = true;
                                (c.id.clone(), c.handle_hash)
                            })
                            .collect();
                        for (cid, their_hh) in to_rekey {
                            crate::log("CLUTCH: resume re-keygen for Pending contact without keypairs");
                            self.spawn_clutch_keygen(cid, our_handle_hash, their_hh);
                        }
                    }
                    Err(e) => crate::log(&format!("STORAGE: init failed on resume: {}", e)),
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

    fn on_event(&mut self, event: &Event, ctx: &mut Context) -> EventResponse {
        match event {
            Event::CursorMoved { .. } => {
                // Drag-select extension takes precedence over hover updates. Active iff we're inside a left-press-then-move sequence over the focused textbox; on first move during the drag we set the anchor to the cursor's pre-drag position (the click landed there via Textbox::on_click), then update `cursor` to the character nearest the live cursor X. `cursor_index_from_x` saturates internally — X past text bounds returns the first/last character index — so no clamp here.
                if self.is_dragging_select {
                    if let Some(tb) = self.textbox.as_mut() {
                        if tb.selection_anchor.is_none() {
                            tb.selection_anchor = Some(tb.cursor);
                        }
                        tb.cursor = tb.cursor_index_from_x(ctx.cursor_x);
                    }
                    ctx.window.request_redraw();
                    return EventResponse::Handled;
                }

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
                {
                    let want = self.avatar_hit_id != HIT_NONE && new_hit == self.avatar_hit_id;
                    if self.avatar_hovered != want {
                        self.avatar_hovered = want;
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
                        // On the contacts screen the wheel scrolls the list, not the background. Down-scroll (negative dy) moves the list up (reveals lower contacts), so subtract; the render pass clamps to the list extent.
                        self.contacts_scroll = (self.contacts_scroll - dy).max(0);
                    } else {
                        self.bg_scroll = self.bg_scroll.wrapping_add(dy);
                    }
                    if let Some(chrome) = self.chrome.as_mut() {
                        chrome.invalidate_bg();
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

                if hit_id == HIT_NONE {
                    // No widget under the cursor — clear focus, then fall back to resize-edge / title-bar drag. Resize edge takes precedence; clicks anywhere else inside the visible window start a move-drag (which the host promotes to an actual drag once the cursor passes the dead-zone threshold).
                    if self.change_focus(None) {
                        ctx.window.request_redraw();
                    }
                    let edge = chrome::get_resize_edge(
                        ctx.viewport.width_px,
                        ctx.viewport.height_px,
                        ctx.cursor_x,
                        ctx.cursor_y,
                    );
                    if edge != ResizeEdge::None {
                        return EventResponse::StartResize(edge);
                    }
                    return EventResponse::StartWindowDrag;
                }

                // Avatar tap on Ready dispatches to the image picker — not a Widget, just a hit-stamp in chrome.hit_test_map. Intercepted BEFORE `widget::dispatch_click` so the walk doesn't waste a pass looking for a non-existent widget. Drops focus first because the picker overlays the whole UI.
                if hit_id == self.avatar_hit_id
                    && matches!(self.state, AppState::Ready)
                    && self.avatar_hit_id != HIT_NONE
                {
                    self.change_focus(None);
                    // Android: a tap opens the system image picker directly (the picker IS the update mechanism — tapping the grey circle is self-evident, so no on-screen prompt). Desktop: no picker — the avatar updates by drag/drop — the click just dismisses hints (above) and is swallowed here.
                    #[cfg(target_os = "android")]
                    {
                        self.pending_picker_request = true;
                    }
                    ctx.window.request_redraw();
                    return EventResponse::Handled;
                }

                // Back button on conversation screen.
                if hit_id == self.back_btn_hit_id
                    && matches!(self.state, AppState::Conversation)
                    && self.back_btn_hit_id != HIT_NONE
                {
                    self.state = AppState::Ready;
                    self.active_contact = None;
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
                            self.contacts[ci].handle.as_str()
                        ));
                        self.active_contact = Some(ci);
                        self.state = AppState::Conversation;
                        self.change_focus(None);
                        // Refresh this contact's presence on conversation-enter so the header reflects reality promptly.
                        self.ping_contact(ci);
                        ctx.window.request_redraw();
                        return EventResponse::Handled;
                    }
                }

                // Focus follows click for focusable widgets (textbox + attest button). Chrome buttons aren't focusable (their `Widget::focus()` returns None) so a click on close/min/max leaves the prior focus intact — matches GNOME / macOS convention. We can't borrow `self` mutably twice in one walk, so determine "is this id focusable" via a pre-walk, then change_focus before the dispatch.
                let mut hit_is_focusable = false;
                self.visit(&mut |w| {
                    if w.id() == hit_id && w.focus().is_some() {
                        hit_is_focusable = true;
                    }
                });
                // A busy (frozen) field/button is already invisible to this path: its `focus()` accessor returns `None` (so `hit_is_focusable` is false — no refocus) and its `click()` accessor returns `None` (so `dispatch_click` below no-ops to `Pass`). No explicit launch-locked swallow needed anymore.
                if hit_is_focusable && self.change_focus(Some(hit_id)) {
                    ctx.window.request_redraw();
                }

                // Dispatch the click via the fluor widget helper. Walks the tree once, finds the widget with `hit_id`, calls its `Click::on_click`. Returns `EventResponse::Pass` if the widget has no Click capability — covers chrome's app-icon orb (no action wired yet).
                let response =
                    widget::dispatch_click(self, hit_id, ctx.cursor_x, ctx.cursor_y, ctx.modifiers);

                // Arm drag-select if the click landed on the (now-focused) textbox. CursorMoved consults `is_dragging_select` to grow the selection; release clears it. Set AFTER dispatch so Textbox::on_click has placed the cursor at the click position first — drag then extends from there.
                let textbox_focused = self
                    .textbox
                    .as_ref()
                    .map(|t| Some(t.hit_id()) == self.focused)
                    .unwrap_or(false);
                if textbox_focused {
                    self.is_dragging_select = true;
                }

                // Fluor's host doesn't auto-redraw on `Handled` (app.rs:712); a click that moved a textbox cursor or armed drag-select needs an explicit redraw or the visual update waits for the next tick (perceived as input lag).
                if matches!(response, EventResponse::Handled) || textbox_focused {
                    ctx.window.request_redraw();
                }

                response
            }
            Event::MouseInput {
                state: ElementState::Released,
                button: MouseButton::Left,
                ..
            } => {
                // Drag-select end: if the drag never moved (anchor == cursor) the selection range is empty; clear the anchor so subsequent keyboard navigation behaves as "no selection" rather than "0-length selection".
                if self.is_dragging_select {
                    self.is_dragging_select = false;
                    if let Some(tb) = self.textbox.as_mut() {
                        if tb.selection_anchor == Some(tb.cursor) {
                            tb.selection_anchor = None;
                        }
                    }
                    self.blink_timer.start(Instant::now());
                    ctx.window.request_redraw();
                }

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
                            self.submit_handle();
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
                            if attest_clicked
                                || plus_clicked
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
                        if let Some(focus_id) = self.focused {
                            let resp =
                                widget::dispatch_key(self, focus_id, kev, ctx.modifiers, ctx.text);
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
                if let Some(tb) = self.focused_textbox_mut() {
                    for c in s.chars() {
                        if c == '\u{0008}' {
                            tb.backspace(ctx.text);
                        } else {
                            tb.insert_char(c, ctx.text);
                        }
                    }
                    handled = true;
                }
                if handled {
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
        // Schedule the next wakeup at the soonest of:
        //   * `blink_timer.next_tick()` — drives the focused-textbox cursor pulse (random 0-300ms intervals); `None` while no textbox is focused.
        //   * `now` when an attestation is in flight — `tick()` advances `attest_anim_phase` at 1 cycle/sec for the "query in flight" wave shift; we need a wakeup every frame to keep it animating smoothly. Without this, the host blocks waiting for input and the animation stalls.
        let blink = self.blink_timer.next_tick();
        // An attestation OR an in-flight add-friend search both need a wakeup every frame to animate (the spectrum wave / the hourglass wobble).
        let animating = matches!(
            self.state,
            AppState::Launch(LaunchState::Attesting) | AppState::Searching
        ) || self.add_in_flight;
        let anim = animating.then(Instant::now);
        match (blink, anim) {
            (Some(b), Some(a)) => Some(b.min(a)),
            (Some(b), None) => Some(b),
            (None, Some(a)) => Some(a),
            (None, None) => None,
        }
    }

    fn tick(&mut self, ctx: &mut Context) -> bool {
        let now = Instant::now();
        let mut needs_redraw = false;

        // Freeze / unfreeze the busy widgets (attest field+button while attesting, search box+plus while adding) before anything else this frame — disabled widgets drop out of dispatch via their fluor accessors.
        self.sync_busy_freeze();

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

        // Drive the blinkey on the focused textbox. `BlinkTimer::poll(now)` returns `true` ONLY on the rising edge of each fire (then schedules the next random 0-300ms interval and returns false the rest of the time). On each fire, toggle the focused textbox's blinkey via `flip_blinkey` — which is a no-op on an unfocused textbox, so we can call it on every textbox without gating.
        if self.blink_timer.poll(now) {
            for (_, tb) in self.textboxes_mut() {
                if tb.flip_blinkey() {
                    needs_redraw = true;
                }
            }
        }

        // Drain per-contact presence + CLUTCH ceremony updates (pongs → is_online/ip; offers/KEM/complete → ceremony progress), plus the three background-job result channels (keygen / KEM-encap / ceremony-expand).
        if self.check_status_updates() {
            needs_redraw = true;
        }
        if self.check_clutch_keygens() {
            needs_redraw = true;
        }
        if self.check_clutch_kem_encaps() {
            needs_redraw = true;
        }
        if self.check_clutch_ceremonies() {
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
            self.on_query_result(result);
            needs_redraw = true;
        }
        for search in drained_searches {
            self.on_search_result(search);
            needs_redraw = true;
        }

        if needs_redraw {
            ctx.window.request_redraw();
        }
        needs_redraw
    }

    fn damage_rect(&self, viewport: Viewport) -> Option<PixelRect> {
        // Default = full viewport. Override here ONLY to ensure the chord hint bbox is included in the frame following a bracket release — without that, stale hint pixels survive one frame because the host's damage rect wouldn't otherwise cover it. (Full-viewport default already covers the chord hint, so this is mostly future-proofing for when we narrow damage.)
        let vw = viewport.width_px as usize;
        let vh = viewport.height_px as usize;
        let mut combined = PixelRect::new(0, 0, vw, vh);
        if self.last_chord_held || self.brackets_held(Instant::now()) {
            combined = combined.union(chord_hint_bbox(viewport, vw, vh));
        }
        Some(combined)
    }

    fn render(&mut self, target: &mut [u32], ctx: &mut Context) {
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
        // Dev-only: the zoom-% readout is a debugging aid, not a shipped affordance.
        let show_zoom = self.zoom_hint && cfg!(feature = "development");

        // Title-bar text by screen, computed BEFORE the chrome borrow (peer count reads `self.handle_query` / `self.online`). Launch/attest shows the "← Network" affordance; once attested (Ready) it shows the live connection count — FGTW seed (counted when online) plus every distinct peer device in the store. `set_title` only re-rasterizes chrome when the string actually changes, so this is cheap to recompute each frame.
        let title_text: String = if matches!(self.state, AppState::Conversation) {
            self.active_contact
                .and_then(|ci| self.contacts.get(ci))
                .map(|c| c.handle.as_str().to_string())
                .unwrap_or_else(|| "Conversation".to_string())
        } else if matches!(self.state, AppState::Ready) {
            let store_peers = self
                .handle_query
                .as_ref()
                .and_then(|hq| hq.get_transport())
                .map(|t| t.lock().map(|s| s.peer_count()).unwrap_or(0))
                .unwrap_or(0);
            let n = store_peers + if self.online { 1 } else { 0 };
            format!("{n} peers")
        } else {
            "\u{2190} Network".to_string()
        };

        let Some(chrome) = self.chrome.as_mut() else {
            return;
        };
        chrome.set_title(title_text);

        // Bg noise. `shimmer` is driven by `bg_scroll` and mixes into each row's starting colour — so the noise colour bias cycles as you scroll without changing the underlying pattern topology. `scroll_offset` is per-screen: Launch/Attest gets `0` (no vertical movement on the attest screen — shimmer only); future screens (Ready, Searching, Conversation) will pass `bg_scroll` so the noise pattern also translates with their page-scroll content. Phase 2+ branches on AppState to pick which.
        let bg_scroll = self.bg_scroll;
        let shimmer = bg_scroll as usize;
        let scroll_offset = 0; // Launch only for now.
                               // Launch layout: faithful proportional slicing port from legacy `Layout::new` — spectrum near the top, logo wordmark overlapping its bottom, attest block (textbox + hint + button) below. Compute every frame; cheap and lets resize flow thru without a separate cache.
        let layout = LaunchLayout::compute(buf_w, buf_h, ctx.viewport.ru);
        // Chromatic wave phase has two summands:
        //   * Scroll-driven base (`bg_scroll * 1/128 rad/scroll-unit`) — one wheel-notch ≈ 8 units → ~1/16 rad shift; user-tunable by changing the shift exponent.
        //   * `attest_anim_phase` (advanced in `tick()` while `LaunchState::Attesting`) — the "query in flight" cue, 1 cycle/sec.
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
        let version_cy = buf_h as f32 - version_size;
        // Zoom watermark, top-centre: current `ru` zoom factor as a decimal percentage ("100%", "103%"), twice the version size, at 1/4 opacity. Mirrors the version's bottom-centre placement (one font-size in from the edge). Integer percent — the ~3%/step zoom granularity makes decimals noise.
        let zoom_size = version_size * 2.0;
        let zoom_text = format!("{}%", (ctx.viewport.ru * 100.0).round() as i64);
        let zoom_cx = buf_w as f32 * 0.5;
        let zoom_cy = zoom_size;
        // Split-borrow `ctx.damage` (consumed by rasterize_bg's first arg) and `ctx.text` (captured by the closure for the logo's text rendering). These are disjoint fields of `Context` so the borrow checker allows both reborrows simultaneously. The closure is non-`move` so the text reborrow ends when rasterize_bg returns, leaving `ctx.text` available for `rasterize_chrome` on the next line.
        let text = &mut *ctx.text;
        // Bg-first compose chain: noise paints opaque, the wave reads it for the `sqrt(c*scale + c_bg²)` blend, then the logo (glow / body / highlight) paints over both via legacy visible-RGB ops. Each step preserves α on the pixels it touches. The wave + logo are Launch-screen chrome — once attested the user shouldn't be staring at the wordmark every time they open the app, so Ready / Searching / Conversation get just the background noise and let their own widgets own the canvas.
        let on_launch = matches!(self.state, AppState::Launch(_));
        // Version watermark shows ONLY on the attest screen (Launch) and the contacts screen (Ready) — not conversations or other screens. (Settings, when it lands, spells the version out in words rather than glyphs; that's its own render path.)
        let show_version = on_launch || matches!(self.state, AppState::Ready);
        // Swap the noise base colour to BG_BASE_WARNING when the dual-ring vault flagged degraded this session — the noise pass already runs every frame so this changes a colour, not the pass count. None on the happy path keeps the default green-dark base from theme.rs.
        let bg_base = if self.vault_degraded {
            Some(crate::ui::theme::BG_BASE_WARNING)
        } else {
            None
        };
        // The 1-px noise inset exists ONLY to clear the window perimeter hairline / shadow band — so gate it on whether that perimeter is actually drawn, which is exactly `!chrome.full_edge`. A windowed desktop draws the perimeter → inset. A maximized/fullscreen desktop goes full_edge (no perimeter) and Android forces full_edge too → paint to the screen edge, else a 1-px unpainted border shows. (Earlier this was hardcoded per-OS, so desktop-maximized still inset for a perimeter that wasn't there.) `|| cfg!(android)` keeps the Android always-fullscreen guarantee even on a transient pre-resize frame where full_edge hasn't synced yet.
        let bg_fullscreen = chrome.full_edge || cfg!(target_os = "android");
        chrome.rasterize_bg(ctx.damage, |canvas| {
            // Version watermark FIRST — fluor's `under()` is topmost-first, and `background_noise` composes UNDER existing content, so the version must be painted before the noise to survive. At α≈1/32 white the noise blends thru ~97%, leaving it as a faint bottom-of-screen watermark. (This is what "before the background gets painted" meant — paint last and the opaque noise buries it.)
            if show_version {
                text.draw_text_left_u32(
                    canvas,
                    &version_glyphs,
                    version_x,
                    version_cy,
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
            paint::background_noise(canvas, shimmer, bg_fullscreen, scroll_offset, None, bg_base);
            if on_launch {
                chromatic_wave(canvas, spectrum_rect, phase, period_scale);
                paint_photon_logo(canvas, text, logo_rect);
            }
        });
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
            let status: Option<(&str, u32)> = match launch_state {
                LaunchState::Attesting => Some(("Attesting\u{2026}", STATUS_TEXT_COLOUR)),
                LaunchState::Error(msg) if !msg.is_empty() => {
                    Some((msg.as_str(), ERROR_TEXT_COLOUR))
                }
                _ => None,
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

            // Hint slot — static "handle" label below the textbox, always shown on the Launch screen. Tells the user what to type; doesn't change with sub-state.
            let hint_rect = attest.hint;
            if !hint_rect.is_empty() {
                let region_h = (hint_rect.y1 - hint_rect.y0) as f32;
                let cx = (hint_rect.x0 + hint_rect.x1) as f32 * 0.5;
                let cy = (hint_rect.y0 + hint_rect.y1) as f32 * 0.5;
                ctx.text.draw_text_center_u32(
                    &mut canvas,
                    "handle",
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

            // Dormant infinity centred IN the handle textbox — it sits where the typed handle will appear, a half-brightness grey placeholder for the resting field, shown only while the field is empty AND unfocused. Painted BEFORE the textbox: fluor's under-blend is "topmost paints first; later opaque dst wins", so the glyph must precede the textbox's empty-pill fill to survive (same ordering the contacts plus-button uses). The instant the user focuses (cursor in) or a character lands, the gate goes false and the textbox owns the slot alone.
            // Anchor and size come straight off the textbox (`center_x/center_y/font_size`), so the glyph lands pixel-identical to where a typed character would — the textbox draws its own glyphs via `draw_text_center_u32` at the same anchor, so matching it here keeps the ∞ from sitting high or scaling differently.
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

        // Ready screen — slice-based layout matching legacy ContactsUnifiedLayout. Today only the avatar circle is painted; the layout already carries rects for handle / hint / textbox / separator / contact rows so subsequent slices drop into named slots without re-computing geometry.
        if matches!(self.state, AppState::Ready) {
            let mut canvas = Canvas::new(target, buf_w, buf_h, ctx.damage);
            let ready_layout = ReadyLayout::compute(buf_w, buf_h, ctx.viewport.ru);

            // Clear the contacts textbox slot in the shared hit_test_map before re-stamping. Same reason as the launch screen: chrome only wipes the map on its own dirty cycles, but the textbox + overlaid plus-button re-stamp every frame, and the plus only renders when the field is non-empty. Without this, clearing the search field to empty on a chrome-clean frame would leave the plus-button's old hit-rect dispatching pointer + hitmask. The plus lives inside the textbox slot, so clearing that slot covers both.
            restamp_hit_rect(
                &mut chrome.hit_test_map,
                buf_w,
                buf_h,
                ready_layout.textbox.x0 as isize,
                ready_layout.textbox.y0 as isize,
                ready_layout.textbox.x1 as isize,
                ready_layout.textbox.y1 as isize,
                HIT_NONE,
            );

            let (cx, cy, radius) = ready_layout.avatar_center_radius();
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
            // Under-blend semantics ("topmost paints first; later opaque dst wins"): paint the button/hourglass FIRST so it's visually topmost, then the textbox under it. Textbox::render_content_into stamps hit_test_map unconditionally over its entire bbox, so after the textbox runs we re-stamp the button's bbox with the button's hit_id to recover correct click dispatch in the overlap.
            let plus_visible = self
                .contacts_textbox
                .as_ref()
                .map(|tb| !tb.chars.is_empty())
                .unwrap_or(false);
            let plus_bbox: Option<(isize, isize, isize, isize, HitId)> = if self.add_in_flight {
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
                None
            } else if plus_visible {
                self.contacts_plus_btn.as_mut().map(|btn| {
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
                    let bbox = button_bbox(btn);
                    (bbox.0, bbox.1, bbox.2, bbox.3, id)
                })
            } else {
                None
            };
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
            if let Some((x0, y0, x1, y1, btn_id)) = plus_bbox {
                restamp_hit_rect(
                    &mut chrome.hit_test_map,
                    buf_w,
                    buf_h,
                    x0,
                    y0,
                    x1,
                    y1,
                    btn_id,
                );
            }

            // Add-friend result text in the hint slot above the search box: green "added {h}", red "not found" / "error: …". Stays until the next search starts (cleared in `submit_add_friend`).
            if let Some((text, colour)) = self.search_status.as_ref() {
                let hint = ready_layout.hint;
                if !hint.is_empty() {
                    let region_h = (hint.y1 - hint.y0) as f32;
                    let scx = (hint.x0 + hint.x1) as f32 * 0.5;
                    let scy = (hint.y0 + hint.y1) as f32 * 0.5;
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

            // ───────── Separator + scrollable contact list ─────────
            // 1-pixel hairline centred in the separator slot (height 0 = hairline; the slot itself is just reserved breathing room around the line).
            let sep = ready_layout.separator;
            paint::fill_rect(
                &mut canvas,
                sep.x0 as isize,
                ((sep.y0 + sep.y1) / 2) as isize,
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
            let rows_clip = fluor::paint::Clip::new(rows.x0, rows.y0, rows.x1, buf_h);

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
                    filter.is_empty() || c.handle.as_str().to_lowercase().contains(&filter)
                })
                .map(|(i, _)| i)
                .collect();

            // Clamp scroll to the list extent (a wheel can't push past the last row).
            let view_h = (buf_h as isize - rows.y0 as isize).max(0);
            let max_scroll = (matching.len() as isize * row_h - view_h).max(0);
            if self.contacts_scroll > max_scroll {
                self.contacts_scroll = max_scroll;
            }

            // Row geometry: avatar on the left with a half-radius margin, name to its right.
            let avatar_cx = rows.x0 as f32 + avatar_r * 1.5;
            let text_x = avatar_cx + avatar_r * 1.5;
            let text_size = row_h as f32 * 0.5;
            let ring_thickness = (avatar_r * 0.0375).max(1.0);
            for (vis, &ci) in matching.iter().enumerate() {
                let row_top = rows.y0 as isize + vis as isize * row_h - self.contacts_scroll;
                if row_top + row_h <= rows.y0 as isize || row_top >= buf_h as isize {
                    continue; // fully outside the visible list region
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

                // Handle name, vertically centred in the row, clipped to the list region.
                ctx.text.draw_text_left_u32(
                    &mut canvas,
                    self.contacts[ci].handle.as_str(),
                    text_x,
                    cy,
                    text_size,
                    500,
                    CONTACT_NAME_COLOUR,
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
                        row_top.max(rows.y0 as isize),
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
                let band_h = (buf_h / 24).max(20);
                let cx = buf_w as f32 * 0.5;
                let cy = buf_h as f32 - band_h as f32 * 0.5;
                let font_size = band_h as f32 * 0.6;
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
                let mut x = buf_w as f32 - version_size - total; // right margin mirrors the version's left margin
                let strip_cy = version_cy;
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
                    let contact = &self.contacts[ci];
                    let ru = ctx.viewport.ru;
                    let unit = (buf_h as f32 * 0.04 * ru).max(12.0);

                    // Back arrow (top-left) — below the chrome title bar area.
                    let back_y = buf_h as f32 * 0.06 + unit;
                    let back_size = unit * 0.8;
                    let back_text = "\u{2190} Contacts";
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

                    // Contact name, centred
                    let name_y = back_y + unit * 2.5;
                    let name_size = unit * 1.2;
                    ctx.text.draw_text_center_u32(
                        &mut canvas,
                        contact.handle.as_str(),
                        buf_w as f32 * 0.5,
                        name_y,
                        name_size,
                        600,
                        CONTACT_NAME_COLOUR,
                        "Oxanium",
                        None,
                        None,
                        None,
                    );

                    // Avatar
                    let avatar_y = name_y + unit * 3.0;
                    let avatar_diam = (unit * 3.0) as usize;
                    let avatar_r = avatar_diam as f32 * 0.5;
                    let avatar_cx = buf_w as f32 * 0.5;
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

                    // CLUTCH state
                    let clutch_y = avatar_y + avatar_r + unit * 2.0;
                    let clutch_label = match contact.clutch_state {
                        crate::types::ClutchState::Pending => "CLUTCH: pending",
                        crate::types::ClutchState::AwaitingProof => "CLUTCH: awaiting proof",
                        crate::types::ClutchState::Complete => "CLUTCH: complete",
                    };
                    let clutch_colour = match contact.clutch_state {
                        crate::types::ClutchState::Complete => SEARCH_FOUND_COLOUR,
                        _ => HOURGLASS_COLOUR,
                    };
                    ctx.text.draw_text_center_u32(
                        &mut canvas,
                        clutch_label,
                        buf_w as f32 * 0.5,
                        clutch_y,
                        unit * 0.7,
                        500,
                        clutch_colour,
                        "Oxanium",
                        None,
                        None,
                        None,
                    );
                }
            }
        }

        chrome.flatten_into(target, buf_w, buf_h, None);

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

    fn cursor_for(&self, x: Coord, y: Coord, ctx: &Context) -> CursorIcon {
        let hit = self
            .chrome
            .as_ref()
            .map(|c| c.hit_at(x, y))
            .unwrap_or(HIT_NONE);
        if let Some(chrome) = self.chrome.as_ref() {
            // App-icon orb has no click action yet; everything else (close / min / max) is pressable so the pointer cursor is the visual cue.
            if chrome.owns_hit(hit) && hit != chrome.app_icon_btn.id() {
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
        match chrome::get_resize_edge(ctx.viewport.width_px, ctx.viewport.height_px, x, y) {
            ResizeEdge::Top | ResizeEdge::Bottom => CursorIcon::NsResize,
            ResizeEdge::Left | ResizeEdge::Right => CursorIcon::EwResize,
            ResizeEdge::TopLeft | ResizeEdge::BottomRight => CursorIcon::NwseResize,
            ResizeEdge::TopRight | ResizeEdge::BottomLeft => CursorIcon::NeswResize,
            ResizeEdge::None => CursorIcon::Default,
        }
    }
}

impl PhotonApp {
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
        let tb_cx = slot_x0 + slot_w * 0.5;
        let tb_cy = slot_y0 + slot_h * 0.5;
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

        if self
            .contacts
            .iter()
            .any(|c| c.handle.as_str().eq_ignore_ascii_case(&handle))
        {
            crate::log(&format!("add-friend: '{}' already in contacts", handle));
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
            crate::log(&format!("add-friend: searching FGTW for '{}'", handle));
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

    /// A launch-handle edit invalidates any prior attestation error — drop `Error` back to `Fresh` so the red message clears and the user can resubmit. No-op off the launch screen or when not in an error state.
    fn clear_launch_error(&mut self) {
        if matches!(self.state, AppState::Launch(LaunchState::Error(_))) {
            self.state = AppState::Launch(LaunchState::Fresh);
        }
    }

    /// Send the current textbox contents as an attestation query and transition Launch → Attesting. Called from Enter in the textbox path and from clicking the Attest button — same submit path. No-op if the textbox is empty, HandleQuery wasn't constructed (init failure path), or the launch sub-state forbids submission (`LaunchState::Attesting` — query already in flight; second submit would double-spend the ~5s memory-hard proof).
    fn submit_handle(&mut self) {
        if let AppState::Launch(s) = &self.state {
            if !s.can_edit_handle() {
                return;
            }
        }
        let handle: String = match self.textbox.as_ref() {
            Some(tb) => tb.chars.iter().collect(),
            None => return,
        };
        if handle.is_empty() {
            return;
        }
        if let Some(hq) = self.handle_query.as_ref() {
            hq.query(handle);
            self.state = AppState::Launch(LaunchState::Attesting);
            // Drop focus off the textbox during the attesting wait so further IME / key input doesn't mutate the handle being attested. `change_focus(None)` also flips the Android soft-IME signal to "hide" via `pending_keyboard_request`, which the next touch dispatch picks up — keeps the keyboard from hovering over a frozen textbox.
            self.change_focus(None);
        }
    }

    /// Handle a [`QueryResult`] arriving from HandleQuery's background worker. On success, stashes the proof, loads the device avatar + contacts, and transitions to the Ready screen; on rejection/error, drops to `LaunchState::Error` and refocuses the handle field.
    fn on_query_result(&mut self, result: QueryResult) {
        use num_bigint::BigUint;
        // Resume painted Ready optimistically from local state, so a result arriving while we're already past Launch is a background refresh (presence / contacts / cloud-merge), NOT a first attest. This gates the bailouts below: a transient network error must not knock a valid local session off Ready.
        let in_app = !matches!(self.state, AppState::Launch(_));
        match result {
            QueryResult::Success(data) => {
                if let Some(hq) = self.handle_query.as_ref() {
                    hq.set_handle_proof(data.handle_proof);
                }
                // Pubkey emitted as voca-encoded camelCase so a user reading the log can double-click + paste the value as a single word (matches `Development:` key lines from handle_query.rs). The handle is deliberately NOT logged — Photon never surfaces the plaintext handle.
                eprintln!(
                    "attestation success: pubkey = {}",
                    voca::encode(BigUint::from_bytes_be(&data.handle_proof))
                );
                // Adopt the session roots the worker just derived + persisted (register-shaped, no handle string). Shared across the user's TOKEN apps, gone at logout; a close/reopen resumes from these without re-typing or recomputing the proof.
                self.session = tohu::session();
                self.pending_broadcast_signal = 1;
                self.vault_degraded = data.vault_degraded;
                // The worker already loaded this device's avatar (keyed on identity_seed) into `data.avatar_pixels`; colour-convert it to BT.2020 γ=2.0 for the Ready screen. `None` = storage-miss → grey placeholder.
                if let Some(vsf_rgb) = &data.avatar_pixels {
                    self.device_avatar_pixels =
                        Some(crate::ui::colour_convert::vsf_rgb_to_bt2020(vsf_rgb));
                    self.device_avatar_scaled = None;
                    self.device_avatar_scaled_diameter = 0;
                }
                // Initialize local encrypted storage from the session's vault_seed + device secret.
                if let Some(session) = &self.session {
                    if let Some(kp) = &self.device_keypair {
                        let device_secret = *kp.secret.as_bytes();
                        match crate::storage::FlatStorage::new(
                            crate::storage::APP,
                            session.vault_seed,
                            device_secret,
                        ) {
                            Ok(s) => self.storage = Some(s),
                            Err(e) => crate::log(&format!("STORAGE: init failed: {}", e)),
                        }
                    }
                }
                // Merge incoming contacts with any already loaded locally — union by handle_proof so contacts added on another device (via FGTW/cloud) appear without losing locally-added ones.
                let mut added = 0usize;
                let mut merged_ids: Vec<(ContactId, [u8; 32])> = Vec::new();
                for incoming in &data.contacts {
                    let dominated = self
                        .contacts
                        .iter()
                        .any(|c| c.handle_proof == incoming.handle_proof);
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
                    // Register the merged contacts' pubkeys so the checker answers their pings, and
                    // kick CLUTCH keygen for any that arrived Pending without keypairs. The resume
                    // path (load_all_contacts) already does this for locally-stored contacts, but
                    // cloud/FGTW-merged contacts land here AFTER that ran — without this they sit
                    // Pending forever with no keypairs, no offer, no connection (exactly what broke
                    // after a []n nuke wiped the local vault and contacts came back only via cloud).
                    if let Ok(mut pks) = self.contact_pubkeys.lock() {
                        for c in &self.contacts {
                            if !pks.contains(&c.public_identity) {
                                pks.push(c.public_identity.clone());
                            }
                        }
                    }
                    // A merged self-contact (notes-to-self) needs no key exchange — force it Complete so it's skipped by the keygen filter below.
                    self.settle_self_contacts();
                    let our_handle_hash =
                        self.session.as_ref().map(|s| s.identity_seed).unwrap_or([0u8; 32]);
                    let to_keygen: Vec<(ContactId, [u8; 32])> = self
                        .contacts
                        .iter_mut()
                        .filter(|c| {
                            c.handle_hash != our_handle_hash
                                && merged_ids.iter().any(|(id, _)| *id == c.id)
                                && c.clutch_state == crate::types::ClutchState::Pending
                                && c.clutch_our_keypairs.is_none()
                                && !c.clutch_keygen_in_progress
                        })
                        .map(|c| {
                            c.clutch_keygen_in_progress = true;
                            (c.id.clone(), c.handle_hash)
                        })
                        .collect();
                    for (cid, their_hh) in to_keygen {
                        crate::log("CLUTCH: keygen for merged cloud/FGTW contact without keypairs");
                        self.spawn_clutch_keygen(cid, our_handle_hash, their_hh);
                    }
                }
                // Refresh existing contacts' WAN + LAN addresses from the FGTW peer list.
                // FGTW reports both a public and a same-LAN address per device; pulling the LAN address in lets the offer/KEM send race the LAN path against the WAN path right away, instead of waiting for LAN multicast (which routers often drop) or a pong.
                // This is what unblocks a same-router peer whose stored WAN IPv6 says "No route to host" — the case where m never received an offer.
                self.refresh_contact_addrs_from_peers(&data.peers);
                self.hints_dismissed = false;
                self.state = AppState::Ready;
            }
            QueryResult::AlreadyAttested(peer) => {
                let msg = format!(
                    "handle already attested by another device (pubkey {})",
                    voca::encode(BigUint::from_bytes_be(peer.device_pubkey.as_bytes()))
                );
                eprintln!("attestation rejected: {msg}");
                // The handle is owned by another device — our stored roots are contested. Clear them so the next launch can't auto-resume into the same rejection, and bail to the attest screen (even from an optimistic Ready: this is the genuine takeover case).
                tohu::clear_session();
                self.session = None;
                self.state = AppState::Launch(LaunchState::Error(msg));
                self.refocus_handle_select_all();
            }
            QueryResult::Error(e) => {
                eprintln!("attestation error: {e}");
                if in_app {
                    // Transient network failure on a resume refresh — the local session is still valid. Stay on Ready; the next presence cycle retries. Do NOT drop the user back to the attest screen.
                    crate::log("UI: background refresh failed (network); staying on local session");
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
                let handle = peer.handle.as_str().to_string();
                let already = self
                    .contacts
                    .iter()
                    .any(|c| c.handle.as_str().eq_ignore_ascii_case(&handle));
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
                let is_self = self.session.as_ref().map(|s| s.identity_seed) == Some(contact.handle_hash);
                if is_self {
                    contact.clutch_state = crate::types::ClutchState::Complete;
                }
                let contact_id = contact.id.clone();
                let their_handle_hash = contact.handle_hash;
                let their_pubkey = contact.public_identity.clone();
                // Mark keygen in flight BEFORE spawning (race guard) for non-self contacts.
                if !is_self {
                    contact.clutch_keygen_in_progress = true;
                }
                crate::log(&format!(
                    "search-result: added contact '{}' (total: {})",
                    contact.handle.as_str(),
                    self.contacts.len() + 1
                ));
                self.contacts.push(contact);
                // Register the contact's pubkey so the checker answers its pings, and kick CLUTCH keypair generation so the contact becomes offer-ready when it comes online.
                if let Ok(mut pks) = self.contact_pubkeys.lock() {
                    if !pks.contains(&their_pubkey) {
                        pks.push(their_pubkey);
                    }
                }
                if !is_self {
                    let our_handle_hash = self.session.as_ref().map(|s| s.identity_seed).unwrap_or([0u8; 32]);
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

    /// Apply a focus change: update `self.focused`, then walk the widget tree via `apply_focus_change` so the old + new widgets fire `set_focused(false/true)` and mark their caches dirty. Returns `true` if anything changed (caller decides whether to request a redraw — most callers do). Also drops a one-shot Android keyboard-show/hide request when focus enters or leaves a textbox; the Activity reads it via `FluorApp::wants_keyboard` after each touch and raises / dismisses the soft IME accordingly.
    /// Dismiss the standing hints (the desktop avatar prompt) and clear the transient search status. Called on any click or keystroke: hints are event-shown and interaction-cleared — never hover- or time-driven. The avatar prompt's dismissal is reset on each `Ready` entry.
    fn clear_hints(&mut self) {
        self.hints_dismissed = true;
        self.search_status = None;
    }

    fn change_focus(&mut self, new: Option<HitId>) -> bool {
        if new == self.focused {
            return false;
        }
        let old = self.focused;
        let was_textbox = self.is_textbox(old);
        let is_textbox = self.is_textbox(new);
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

        // Keypair generation includes McEliece460896 — very CPU-heavy (large matrix build).
        // On resume every Pending contact re-keys at once (two contacts = two McEliece keygens in
        // parallel), so this MUST run at Min priority or it starves the UI render thread and the
        // window freezes until keygen finishes — the "GUI loads but you can't do anything until it
        // syncs" symptom. Matches the Min-priority KEM-encap and ceremony-expand threads.
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
            if let Some(p) = proxy.as_ref() { let _ = p.send(crate::ui::PhotonEvent::ClutchKeygenComplete); }
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
            if let Some(p) = proxy.as_ref() { let _ = p.send(crate::ui::PhotonEvent::ClutchKemEncapComplete); }
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
            if let Some(p) = proxy.as_ref() { let _ = p.send(crate::ui::PhotonEvent::ClutchCeremonyComplete); }
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
            derive_conversation_token, ClutchKemSharedSecrets,
            ClutchOfferPayload,
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

        // Get our handle_hash for CLUTCH (PRIVATE identity seed)
        let our_handle_hash = match self.session.as_ref().map(|s| s.identity_seed) {
            Some(h) => h,
            None => return changed,
        };
        let device_pubkey = *self.device_keypair.as_ref().expect("device_keypair set in init").public.as_bytes();
        let device_secret = *self.device_keypair.as_ref().expect("device_keypair set in init").secret.as_bytes();

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

                    // Clear the in-progress flag now that keygen is complete
                    contact.clutch_keygen_in_progress = false;

                    // Store keypairs (ceremony_id computed on-demand when provenances available)
                    contact.clutch_our_keypairs = Some(result.keypairs);
                    changed = true;

                    // Persist keypairs to disk immediately (crash recovery)
                    if let (Some(ref keypairs), Some(storage)) = (&contact.clutch_our_keypairs, self.storage.as_ref()) {
                        if let Err(e) = crate::storage::contacts::save_clutch_keypairs(
                            keypairs,
                            contact.handle.as_str(),
                            storage,
                        ) {
                            crate::log(&format!(
                                "CLUTCH: Failed to save keypairs for {}: {}",
                                contact.handle, e
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
                                contact.handle
                            ));
                        } else {
                            crate::log(&format!(
                                "CLUTCH: Could not find local slot for {} - handle_hash mismatch?",
                                contact.handle
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
                                            if let Err(e) = crate::storage::contacts::save_clutch_slots(
                                                &contact.clutch_slots,
                                                &contact.offer_provenances,
                                                contact.ceremony_id,
                                                contact.handle.as_str(),
                                                storage,
                                            ) {
                                                crate::log(&format!(
                                                    "Failed to persist CLUTCH provenance: {}",
                                                    e
                                                ));
                                            }
                                        }

                                        if let Some(ref checker) = self.status_checker {
                                            let (primary, alt) = contact
                                                .race_addrs()
                                                .unwrap_or((ip, None));
                                            checker.send_offer(ClutchOfferRequest {
                                                peer_addr: primary,
                                                alt_addr: alt,
                                                vsf_bytes,
                                            });
                                            contact.clutch_offer_sent = true;
                                            crate::log(&format!(
                                                "CLUTCH: Sent offer to {} (prov={}...)",
                                                contact.handle,
                                                hex::encode(&our_offer_provenance[..4])
                                            ));
                                        }
                                    }
                                    Err(e) => {
                                        crate::log(&format!(
                                            "CLUTCH: Failed to build offer VSF for {}: {}",
                                            contact.handle, e
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
                            contact.handle,
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
                                            contact.handle
                                        ));
                                    }
                                }
                            } else {
                                crate::log(&format!(
                                    "CLUTCH: Keypairs ready for {} - need ceremony_id for KEM response (have {} offer provenances)",
                                    contact.handle,
                                    contact.offer_provenances.len()
                                ));
                            }
                        }
                    }

                    // Process any pending KEM response that arrived before keygen completed.
                    // Also compute ceremony_id here if provenances are ready — the KEM may have arrived in the network thread between when we added our provenance and when the main loop got here to run the ceremony_id derivation above.
                    if contact.clutch_pending_kem.is_some() {
                        if contact.ceremony_id.is_none()
                            && contact.offer_provenances.len() >= 2
                        {
                            let ceremony_id = *CeremonyId::derive(
                                &[our_handle_hash, contact.handle_hash],
                                &contact.offer_provenances,
                            )
                            .as_bytes();
                            contact.ceremony_id = Some(ceremony_id);
                            crate::log(&format!(
                                "CLUTCH: Computed ceremony_id for {} while draining queued KEM",
                                contact.handle
                            ));
                        }
                    }

                    if let Some(pending_kem) = contact.clutch_pending_kem.take() {
                        crate::log(&format!(
                            "CLUTCH: Processing queued KEM response from {}",
                            contact.handle
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
                                    contact.handle
                                ));
                            }

                            // If we haven't sent our own KEM encap yet, do it now.
                            // This covers the case where their KEM arrived before we had ceremony_id, so the normal encap-trigger was skipped.
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
                                                contact.handle
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
                                    contact.handle.as_str(),
                                    storage,
                                ) {
                                    crate::log(&format!(
                                        "CLUTCH: Failed to save slots for {}: {}",
                                        contact.handle, e
                                    ));
                                }
                            }
                        }
                    }

                    // Check if ceremony can complete
                    if contact.all_slots_complete() {
                        crate::log(&format!(
                            "CLUTCH: All slots complete for {} after keygen - triggering ceremony completion",
                            contact.handle
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
            self.complete_clutch_ceremony_by_idx(idx, our_handle_hash);
            changed = true;
        }

        if changed {
        }
        changed
    }

    /// Process background CLUTCH KEM encapsulation results. When KEM encap completes, store the secrets and send the KEM response.
    pub fn check_clutch_kem_encaps(&mut self) -> bool {
        use crate::network::status::ClutchKemResponseRequest;

        let mut changed = false;
        let mut ceremony_completions: Vec<usize> = Vec::new();
        let our_handle_hash = match self.session.as_ref().map(|s| s.identity_seed) {
            Some(h) => h,
            None => return changed,
        };
        let device_pubkey = *self.device_keypair.as_ref().expect("device_keypair set in init").public.as_bytes();
        let device_secret = *self.device_keypair.as_ref().expect("device_keypair set in init").secret.as_bytes();

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
                            contact.handle.as_str(),
                            storage,
                        ) {
                            crate::log(&format!(
                                "CLUTCH: Failed to save slots for {}: {}",
                                contact.handle, e
                            ));
                        }
                    }

                    // Send the KEM response
                    if let Some(ref checker) = self.status_checker {
                        let (primary, alt) = contact
                            .race_addrs()
                            .unwrap_or((result.peer_addr, None));
                        checker.send_kem_response(ClutchKemResponseRequest {
                            peer_addr: primary,
                            alt_addr: alt,
                            conversation_token: result.conversation_token,
                            ceremony_id: result.ceremony_id,
                            payload: result.kem_response,
                            device_pubkey,
                            device_secret,
                        });
                        crate::log(&format!("CLUTCH: Sent KEM response to {}", contact.handle));
                    }

                    // Check if all slots are complete after storing our KEM encap secrets
                    if contact.all_slots_complete() {
                        crate::log(&format!(
                            "CLUTCH: All slots complete for {} after KEM encap - triggering ceremony",
                            contact.handle
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
            self.complete_clutch_ceremony_by_idx(idx, our_handle_hash);
            changed = true;
        }

        if changed {
        }
        changed
    }

    /// Process background CLUTCH ceremony completion results. When ceremony completes, store the friendship chains and send proof.
    pub fn check_clutch_ceremonies(&mut self) -> bool {
        use crate::crypto::clutch::ClutchCompletePayload;
        use crate::network::status::ClutchCompleteRequest;
        use crate::types::ClutchState;

        let mut changed = false;
        let _our_handle_hash = match self.session.as_ref().map(|s| s.identity_seed) {
            Some(h) => h,
            None => return changed,
        };
        let device_pubkey = *self.device_keypair.as_ref().expect("device_keypair set in init").public.as_bytes();
        let device_secret = *self.device_keypair.as_ref().expect("device_keypair set in init").secret.as_bytes();

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
                entry.1 = result.friendship_chains;
            } else {
                self.friendship_chains
                    .push((friendship_id, result.friendship_chains));
            }

            // Update sync records for new friendship
            self.update_sync_records();

            // Find the contact and update state
            if let Some(contact) = self.contacts.iter_mut().find(|c| c.id == result.contact_id) {
                let contact_handle = contact.handle.clone();
                contact.clutch_ceremony_in_progress = false;
                contact.friendship_id = Some(friendship_id);

                crate::log(&format!(
                    "CLUTCH: Eggs computed with {}! (proof: {}...)",
                    contact_handle,
                    hex::encode(&result.eggs_proof[..8])
                ));

                // Store our proof for later verification
                contact.clutch_our_eggs_proof = Some(result.eggs_proof);
                // Budget a handful of proof retransmits — the proof is a single unreliable UDP
                // packet, so ping_contacts re-sends it until this drains, guaranteeing the peer
                // gets it even on a lossy or freshly-changed path.
                contact.clutch_proof_resends_left = 5;

                // Check if we already received their proof (fast party case)
                let their_early_proof = contact.clutch_their_eggs_proof;

                // Send ClutchComplete proof to peer
                if let Some(ref checker) = self.status_checker {
                    let payload = ClutchCompletePayload {
                        eggs_proof: result.eggs_proof,
                    };

                    let (primary, alt) = contact
                        .race_addrs()
                        .unwrap_or((result.peer_addr, None));
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
                        // Store their HQC pub prefix to detect stale offers after restart
                        contact.completed_their_hqc_prefix = Some(result.their_hqc_prefix);
                        // We're Complete, but the peer may not have our proof yet — we got theirs
                        // first, and our single send (just above) might have dropped. Keep the proof
                        // and the resend budget so ping_contacts keeps delivering it for a few more
                        // cycles; that's exactly what stops the peer from hanging in AwaitingProof.
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
                    if let Err(e) = crate::storage::contacts::save_contact(
                        contact,
                        storage,
                    ) {
                        crate::log(&format!("Failed to save contact after CLUTCH: {}", e));
                    } else {
                        #[cfg(feature = "development")]
                        #[cfg(feature = "development")]
                        crate::log(&format!("CLUTCH: Saved {} state to disk", contact_handle));
                    }

                    // Delete slots file - ceremony is complete, slots no longer needed
                    if let Err(e) =
                        crate::storage::contacts::delete_clutch_slots(contact_handle.as_str(), storage)
                    {
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
        }

        if changed {
        }
        changed
    }

    /// Spawn background CLUTCH ceremony completion when all slots are filled. Extracts data from contact and spawns background thread for heavy crypto.
    ///
    /// Takes contact index to avoid borrow conflicts in the event loop.
    fn complete_clutch_ceremony_by_idx(&mut self, contact_idx: usize, our_handle_hash: [u8; 32]) {
        use crate::crypto::clutch::{derive_conversation_token, ClutchSharedSecrets};

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
                contact.handle
            ));
            return;
        }

        // Get their slot (the other party)
        let their_handle_hash = contact.handle_hash;
        let contact_id = contact.id.clone();
        let contact_handle = contact.handle.to_string();
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
                crate::log(&format!("CLUTCH: No IP for {}", contact_handle));
                return;
            }
        };
        let ceremony_id = match contact.ceremony_id {
            Some(c) => c,
            None => {
                #[cfg(feature = "development")]
                #[cfg(feature = "development")]
                crate::log(&format!("CLUTCH: No ceremony_id for {}", contact_handle));
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

        let our_device_pub = *self.device_keypair.as_ref().expect("device_keypair set in init").public.as_bytes();
        self.spawn_clutch_ceremony(
            contact_id,
            our_handle_hash,
            their_handle_hash,
            our_device_pub,
            their_device_pub,
            secrets,
            ceremony_id,
            conversation_token,
            peer_addr,
            their_hqc_prefix,
        );
    }

    /// Cross-reference the FGTW peer list into existing contacts, updating each matched contact's public address (`ip`) and same-LAN address (`local_ip`/`local_port`).
    /// Matched by handle_proof + device_pubkey so the right device's record updates the right contact. Only IPv4 LAN addresses are stored (the hairpin case the `local_ip` field is typed for); a v6-only peer just refreshes the WAN address. The send path races both (see [`crate::types::Contact::race_addrs`]).
    fn refresh_contact_addrs_from_peers(
        &mut self,
        peers: &[crate::network::fgtw::PeerRecord],
    ) {
        // Addresses whose transfers must be cancelled because they went stale (collected here so
        // the checker borrow stays out of the contact-iter loop).
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
                            contact.handle, peer.ip, v4, peer.ip.port()
                        ));
                    }
                    // If the address actually moved while a CLUTCH offer was already sent, that
                    // offer is in flight to a now-dead address (the "No route to host" retries we
                    // kept hammering). Cancel the stale transfer and reset clutch_offer_sent so the
                    // contact's next online pong re-sends the offer to the fresh address, with the
                    // LAN path now raced alongside. Without this the one-shot flag blocks re-send
                    // and the ceremony stalls forever on the dead path.
                    let addr_changed =
                        old_ip != contact.ip || old_local != contact.local_ip;
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
                            contact.handle
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

    /// True if `handle_hash` is our own identity — i.e. this contact is the user's self-contact
    /// (notes to self / future multi-device sync). A self-contact shares our single seed, so there
    /// is no peer to exchange keys with: CLUTCH must be forced Complete and keygen/offer/ceremony
    /// skipped entirely. Without this a self-contact runs a pointless CLUTCH loop against its own
    /// device and never settles.
    fn is_self_contact(&self, handle_hash: &[u8; 32]) -> bool {
        self.session
            .as_ref()
            .is_some_and(|s| s.identity_seed == *handle_hash)
    }

    /// Force every self-contact in the list to CLUTCH-Complete and clear any in-flight CLUTCH work.
    /// Applied after contacts load on resume and after cloud/FGTW merges, since those paths build
    /// contacts as Pending by default. Returns true if any contact changed.
    fn settle_self_contacts(&mut self) -> bool {
        let Some(our_seed) = self.session.as_ref().map(|s| s.identity_seed) else {
            return false;
        };
        let mut changed = false;
        for contact in self.contacts.iter_mut() {
            if contact.handle_hash == our_seed
                && contact.clutch_state != crate::types::ClutchState::Complete
            {
                contact.clutch_state = crate::types::ClutchState::Complete;
                contact.clutch_keygen_in_progress = false;
                changed = true;
                crate::log(&format!(
                    "CLUTCH: self-contact '{}' auto-completed (no key exchange with self)",
                    contact.handle
                ));
                if let Some(storage) = self.storage.as_ref() {
                    let _ = crate::storage::contacts::save_contact(contact, storage);
                }
            }
        }
        changed
    }

    /// Ping all contacts that have IP addresses (call periodically)
    fn ping_contacts(&mut self) {
        let Some(checker) = self.status_checker.as_ref() else {
            return;
        };
        let mut pinged = 0;
        for contact in &self.contacts {
            let addr = match (contact.local_ip, contact.local_port) {
                (Some(ip), Some(port)) => {
                    Some(std::net::SocketAddr::new(std::net::IpAddr::V4(ip), port))
                }
                _ => contact.ip,
            };
            if let Some(ip) = addr {
                checker.ping(ip, contact.public_identity.clone());
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

        // Retransmit the ClutchComplete proof for any contact with budget left. The proof is a lone
        // unreliable UDP packet, so a single drop (or a send to a since-refreshed address) would
        // strand the peer in AwaitingProof. Re-sending it for a few ping cycles converges both
        // sides regardless of which completed first or which packet was lost. Self-terminates as the
        // budget drains; a peer already Complete just ignores the duplicate.
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
            let conv_token =
                derive_conversation_token(&[our_handle_hash, contact.handle_hash]);
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
                contact.handle, contact.clutch_proof_resends_left
            ));
            // Budget exhausted — stop holding the proof.
            if contact.clutch_proof_resends_left == 0
                && contact.clutch_state == crate::types::ClutchState::Complete
            {
                contact.clutch_our_eggs_proof = None;
            }
        }
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
            checker.ping(ip, contact.public_identity.clone());
        }
    }

    /// Drain `StatusUpdate`s from the checker and apply them to contacts. v1 (presence checkpoint) handles only `Online`: match the pong's pubkey to a contact, update its `ip` from the source address, and flip `is_online`. Returns true if any contact changed (→ redraw the list ring). The CLUTCH arms (offer/KEM/complete) land in the follow-up commit. Chat/ack/PT arms are intentionally ignored (messaging not yet ported).
    pub fn check_status_updates(&mut self) -> bool {
        use crate::crypto::clutch;
        use crate::network::status::StatusUpdate;
        // NOTE: ClutchRequest and ClutchRequestType imports removed - legacy v1 CLUTCH no longer used
        use crate::types::ClutchState;

        let checker = match &self.status_checker {
            Some(c) => c,
            None => return false,
        };

        // Get our handle_hash for CLUTCH (PRIVATE identity seed, used in VSF messages) Formula: BLAKE3(VsfType::x(handle).flatten()) - VSF normalized for Unicode safety SECURITY: This IS sent in CLUTCH offers for contact matching, but only parties who already know our handle can compute it to match us
        let our_handle_hash = match self.session.as_ref().map(|s| s.identity_seed) {
            Some(h) => h,
            None => return false, // Can't do CLUTCH without our handle_hash
        };

        // Also need our_identity_seed alias for keygen spawning (same value)
        let our_identity_seed = our_handle_hash;

        let mut changed = false;
        let mut ceremony_completions: Vec<usize> = Vec::new(); // Contact indices to complete after loop
        let mut lan_ping_indices: Vec<usize> = Vec::new(); // Contact indices to ping immediately on new LAN discovery
                                                               // Collect pending message retransmit requests (friendship_id, ip, handle, device_pubkey, last_received_ef6) to process after loop last_received_ef6 from pong tells us what they already have - only retransmit newer
        let mut retransmit_requests: Vec<(
            crate::types::FriendshipId,
            std::net::SocketAddr,
            String,
            [u8; 32], // Recipient device pubkey for relay fallback
            Option<i64>,
        )> = Vec::new();
        // Flag to update sync records after the loop (when borrows are released)
        let mut need_sync_update = false;

        while let Some(update) = checker.try_recv() {
            match update {
                StatusUpdate::Online {
                    peer_pubkey,
                    is_online,
                    peer_addr,
                    sync_records,
                } => {
                    // Find matching contact and update status
                    for contact in &mut self.contacts {
                        if contact.public_identity == peer_pubkey {
                            // Note: ceremony_id is now computed from offer_provenances, not ping provenances. Offer provenances are collected when ClutchOfferReceived messages arrive.

                            // Update IP from the ping/pong source address
                            if let Some(addr) = peer_addr {
                                if contact.ip != Some(addr) {
                                    crate::log(&format!(
                                        "Status: Updated {} IP from ping/pong: {:?} -> {}",
                                        contact.handle, contact.ip, addr
                                    ));
                                    contact.ip = Some(addr);
                                }
                            }

                            if contact.is_online != is_online {
                                contact.is_online = is_online;
                                changed = true;
                                crate::log(&format!(
                                    "Status: {} is now {}",
                                    contact.handle,
                                    if is_online { "ONLINE" } else { "offline" }
                                ));
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
                                            self.device_keypair.as_ref().expect("device_keypair set in init").public.as_bytes(),
                                            self.device_keypair.as_ref().expect("device_keypair set in init").secret.as_bytes(),
                                        ) {
                                            Ok((vsf_bytes, our_offer_provenance)) => {
                                                crate::log(&format!(
                                                    "CLUTCH: Sending full offer to {} (prov={}...)",
                                                    contact.handle,
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
                                                            contact.handle.as_str(),
                                                            storage,
                                                        )
                                                    {
                                                        crate::log(&format!(
                                                            "Failed to persist CLUTCH provenance: {}",
                                                            e
                                                        ));
                                                    }
                                                }

                                                let (primary, alt) = contact
                                                    .race_addrs()
                                                    .unwrap_or((ip, None));
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

                            // Queue retransmit of pending messages when contact comes online
                            if is_online {
                                if let (Some(fid), Some(ip)) = (contact.friendship_id, contact.ip) {
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
                                        ip,
                                        contact.handle.as_str().to_string(),
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

                    // Find friendship by conversation_token
                    let chains_result = self
                        .friendship_chains
                        .iter_mut()
                        .find(|(_, c)| c.conversation_token == conversation_token);

                    let mut need_sync_records_update = false;
                    if let Some((fid, chains)) = chains_result {
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
                                Some((idx, c.handle.to_string()))
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

                        // Deduplication: skip if we've already processed this exact message (UDP duplicates have identical eagle_time) Note: Sender learns our state via last_received_hp in ping/pong - no ACK needed for dupes
                        if chains.is_duplicate(&from_handle_hash, timestamp) {
                            crate::log(&format!(
                                "CHAT: Skipping duplicate message from {} (eagle_time {})",
                                handle, timestamp
                            ));
                            continue;
                        }

                        // Hash chain verification: check prev_msg_hp matches expected If mismatch: either out-of-order or missing messages
                        if let Err(expected) =
                            chains.verify_chain_link(&from_handle_hash, &prev_msg_hp)
                        {
                            crate::log(&format!(
                                "CHAT: Hash chain mismatch from {} - expected {}..., got {}... (may need resync)",
                                handle,
                                hex::encode(&expected[..8]),
                                hex::encode(&prev_msg_hp[..8])
                            ));
                            // For now, continue with decryption anyway (soft verification) TODO: Request resync if gap detected
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
                                crate::log(&format!("CHAT: Sender chain not found for {}", handle));
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

                        crate::log(&format!(
                            "CHAT: Decrypted message from {}: \"{}\" (incorporated_hp={}...)",
                            handle,
                            message_text,
                            hex::encode(&incorporated_hp[..8])
                        ));

                        // Compute plaintext hash for ACK
                        let plaintext_hash = *blake3::hash(&plaintext).as_bytes();

                        // Derive this message's hash pointer (for bidirectional tracking)
                        use crate::types::friendship::derive_msg_hp;
                        let msg_hp = derive_msg_hp(&prev_msg_hp, &plaintext_hash, timestamp);

                        // Update their last_plaintext for next message's salt
                        chains.set_last_plaintext(&from_handle_hash, plaintext.clone());

                        // Update bidirectional entropy state (derive weave hash from full message context)
                        chains.update_received_for_mixing(timestamp, msg_hp, &plaintext);

                        // Look up OUR plaintext that they incorporated (for bidirectional weave) If incorporated_hp is all zeros, they didn't incorporate any of our messages Clone to avoid borrow issues with advance()
                        let our_incorporated_plaintext: Option<Vec<u8>> =
                            if incorporated_hp != [0u8; 32] {
                                chains
                                    .get_pending_plaintext_by_hp(&incorporated_hp)
                                    .map(|p| p.to_vec())
                            } else {
                                None
                            };

                        // Advance their chain with bidirectional weave
                        let eagle_time_for_advance =
                            vsf::EagleTime::from_oscillations(timestamp);
                        chains.advance(
                            &from_handle_hash,
                            &eagle_time_for_advance,
                            &plaintext,
                            our_incorporated_plaintext.as_deref(),
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

                        // CRASH SAFETY: Persist to disk BEFORE sending ACK If we crash after ACK but before disk, sender thinks we have it but we don't. Disk write is the commit point - ACK is just notification. If chain save fails, DO NOT send ACK. Sender will retransmit and we can try again, preventing permanent desync.
                        if let Some(storage) = self.storage.as_ref() {
                            if let Err(e) = crate::storage::friendship::save_friendship_chains(
                                chains,
                                storage,
                            ) {
                                crate::log(&format!(
                                    "STORAGE CRITICAL: Failed to save chains after recv, skipping ACK: {}",
                                    e
                                ));
                                continue;
                            }
                            // Flag to update sync records after borrow ends
                            need_sync_records_update = true;
                        }

                        // Add message to contact's message list and persist
                        if let Some(contact) = self.contacts.get_mut(contact_idx) {
                            // Use actual eagle_time and sorted insert for correct chronological order
                            contact.insert_message_sorted(ChatMessage::new_with_timestamp(
                                message_text,
                                false,     // is_outgoing = false (received)
                                timestamp, // Use message's actual eagle_time, not current time
                            ));
                            contact.message_scroll_offset = 0.0; // Scroll to show new message
                            changed = true;

                            // Persist messages for UI
                            if let Some(storage) = self.storage.as_ref() {
                                if let Err(e) = crate::storage::contacts::save_messages(
                                    contact,
                                    storage,
                                ) {
                                    crate::log(&format!("STORAGE: Failed to save messages: {}", e));
                                }
                            }
                        }

                        // *** THEN send ACK - if we crash here, sender will resend, we can dedup *** Get recipient pubkey for relay fallback
                        let recipient_pubkey = self.contacts.get(contact_idx)
                            .map(|c| *c.public_identity.as_bytes())
                            .unwrap_or([0u8; 32]);
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

                    // Find friendship by conversation_token
                    let chains_result = self
                        .friendship_chains
                        .iter_mut()
                        .find(|(_, c)| c.conversation_token == conversation_token);

                    if let Some((_, chains)) = chains_result {
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
                                Some((idx, c.handle.to_string()))
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

                            // First ACK confirms both sides have working chains - safe to zeroize CLUTCH keypairs
                            if let Some(contact) = self.contacts.get_mut(contact_idx) {
                                if contact.clutch_our_keypairs.is_some() {
                                    let handle_str = contact.handle.as_str().to_string();
                                    crate::log(&format!(
                                        "CLUTCH: First ACK from {} - zeroizing ephemeral keypairs",
                                        contact.handle
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
                                        if let Err(e) = crate::storage::contacts::delete_clutch_keypairs(
                                            &handle_str,
                                            storage,
                                        ) {
                                            crate::log(&format!(
                                                "CLUTCH: Failed to delete keypairs file for {}: {}",
                                                handle_str, e
                                            ));
                                        }
                                    }
                                }
                            }

                            // Persist chains (AGENT.md: every change hits disk)
                            if let Some(storage) = self.storage.as_ref() {
                                if let Err(e) = crate::storage::friendship::save_friendship_chains(
                                    chains,
                                    storage,
                                ) {
                                    crate::log(&format!(
                                        "STORAGE CRITICAL: Failed to save chains after ACK: {}",
                                        e
                                    ));
                                }
                            }
                        } else {
                            crate::log(&format!(
                                "CHAT: ACK verification failed for {} (no matching pending message)",
                                handle
                            ));
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
                                    if let Err(e) = crate::storage::contacts::save_messages(
                                        contact,
                                        storage,
                                    ) {
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
                        derive_conversation_token,
                        ClutchKemSharedSecrets, ClutchOfferPayload,
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

                    // Find contact by conversation_token (compute token for each contact and match)
                    let their_handle_hash = match self
                        .contacts
                        .iter()
                        .find(|c| {
                            derive_conversation_token(&[our_handle_hash, c.handle_hash])
                                == conversation_token
                        })
                        .map(|c| c.handle_hash)
                    {
                        Some(h) => h,
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

                    // Verify sender's device pubkey matches the contact's known identity
                    let contact_pubkey = self
                        .contacts
                        .iter()
                        .find(|c| c.handle_hash == their_handle_hash)
                        .map(|c| c.public_identity.key);

                    match contact_pubkey {
                        None => {
                            #[cfg(feature = "development")]
                            #[cfg(feature = "development")]
                            crate::log("CLUTCH: Received offer from unknown contact");
                            continue;
                        }
                        Some(expected) if expected != sender_pubkey => {
                            crate::log(&format!(
                                "CLUTCH: Device pubkey mismatch! Expected {}, got {}",
                                hex::encode(&expected[..8]),
                                hex::encode(&sender_pubkey[..8])
                            ));
                            continue;
                        }
                        Some(_) => {} // Match - proceed
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
                                            contact.handle
                                        ));
                                        // Don't continue - fall thru to re-send KEM below
                                    } else {
                                        // Same keys but no KEM sent yet - truly duplicate, ignore
                                        crate::log(&format!(
                                            "CLUTCH: Ignoring duplicate offer from {} (same keys, no KEM sent yet)",
                                            contact.handle
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
                                    // Only nuke if we're COMPLETE and they're sending fresh keys (meaning they lost their chains and need full re-key)
                                    if contact.clutch_state == ClutchState::Complete {
                                        crate::log(&format!(
                                            "CLUTCH: Re-key from {} - we're Complete, they have new keys, nuking for fresh ceremony",
                                            contact.handle
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
                                                contact.handle
                                            ));
                                            chains_to_remove.push(old_friendship_id);
                                        }
                                        rekey_request =
                                            Some((contact.id.clone(), contact.handle_hash));
                                    } else {
                                        // Not Complete - just update their offer, don't regenerate our keys
                                        crate::log(&format!(
                                            "CLUTCH: {} sent new keys but we're mid-ceremony (state={:?}) - updating their offer, keeping our keys",
                                            contact.handle, contact.clutch_state
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
                                    contact.handle
                                ));
                            }

                            // Store their offer_provenance for ceremony_id derivation
                            if !contact.offer_provenances.contains(&offer_provenance) {
                                contact.offer_provenances.push(offer_provenance);
                                crate::log(&format!(
                                    "CLUTCH: Stored offer_provenance from {} (now have {})",
                                    contact.handle,
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
                                        contact.handle
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
                                                contact.handle
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
                                    contact.handle.as_str(),
                                    storage,
                                ) {
                                    crate::log(&format!(
                                        "CLUTCH: Failed to save slots for {}: {}",
                                        contact.handle, e
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
                                        self.device_keypair.as_ref().expect("device_keypair set in init").public.as_bytes(),
                                        self.device_keypair.as_ref().expect("device_keypair set in init").secret.as_bytes(),
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
                                                contact.handle,
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
                                                        contact.handle.as_str(),
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
                                            device_pubkey: *self.device_keypair.as_ref().expect("device_keypair set in init").public.as_bytes(),
                                            device_secret: *self.device_keypair.as_ref().expect("device_keypair set in init").secret.as_bytes(),
                                        });
                                        crate::log(&format!(
                                            "CLUTCH: Re-sent KEM response to {}",
                                            contact.handle
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
                                            contact.handle
                                        ));
                                        changed = true;
                                    } else {
                                        crate::log(&format!(
                                            "CLUTCH: Deferring KEM response to {} - waiting for ceremony_id",
                                            contact.handle
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
                                        contact.handle
                                    ));
                                } else {
                                    // No keypairs - need to respond (whether Complete or not) If Complete: peer lost their chains, accept re-key If not Complete: restart mid-ceremony or fresh re-key
                                    if contact.clutch_state == ClutchState::Complete {
                                        // Peer is sending an offer while we think we're Complete. This means either:
                                        // 1. Same HQC prefix: peer missed our KEM response (can't re-send without keypairs)
                                        // 2. Different HQC prefix: peer lost chains, wants re-key
                                        //
                                        // Since we have NO keypairs here (we're in the is_none branch), we can't re-respond even to the same offer. Accept as re-key.
                                        //
                                        // Note: If peer keeps re-sending same offer, both sides will eventually converge on a fresh ceremony (peer will regenerate keys after timeout).
                                        crate::log(&format!(
                                            "CLUTCH: Received offer from {} while Complete - peer lost chains, accepting re-key",
                                            contact.handle
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
                                            if let Err(e) = crate::storage::contacts::save_clutch_slots(
                                                &contact.clutch_slots,
                                                &contact.offer_provenances,
                                                contact.ceremony_id,
                                                contact.handle.as_str(),
                                                storage,
                                            ) {
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
                                                contact.handle
                                            ));
                                            break;
                                        }

                                        // Different keys = peer reset. Clear their slot and reset to Pending.
                                        crate::log(&format!(
                                            "CLUTCH: Peer {} reset while we were AwaitingProof - resetting",
                                            contact.handle
                                        ));
                                        if let Some(slot) = contact.get_slot_mut(&their_handle_hash) {
                                            slot.offer = None;
                                            slot.kem_secrets_from_them = None;
                                        }
                                        contact.clutch_state = ClutchState::Pending;
                                        contact.clutch_offer_sent = false;
                                        contact.ceremony_id = None;
                                        contact.clutch_our_eggs_proof = None;
                                        contact.clutch_their_eggs_proof = None;
                                        // Remove their old provenance (keep ours)
                                        contact.offer_provenances.retain(|p| p != &offer_provenance);
                                        // Fall thru - normal flow will store new offer and trigger keygen
                                    } else {
                                        crate::log(&format!(
                                            "CLUTCH: Received offer from {} but no keypairs (state={:?}) - triggering keygen",
                                            contact.handle, contact.clutch_state
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
                        self.friendship_chains.retain(|(id, _)| *id != old_id);
                        // Delete from disk
                        if let Some(storage) = self.storage.as_ref() {
                            if let Err(e) =
                                crate::storage::friendship::delete_friendship_chains(&old_id, storage)
                            {
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

                    // Find contact by conversation_token
                    let their_handle_hash = match self
                        .contacts
                        .iter()
                        .find(|c| {
                            derive_conversation_token(&[our_handle_hash, c.handle_hash])
                                == conversation_token
                        })
                        .map(|c| c.handle_hash)
                    {
                        Some(h) => h,
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

                    // Verify sender's device pubkey matches the contact's known identity
                    let contact_pubkey = self
                        .contacts
                        .iter()
                        .find(|c| c.handle_hash == their_handle_hash)
                        .map(|c| c.public_identity.key);

                    match contact_pubkey {
                        None => {
                            #[cfg(feature = "development")]
                            #[cfg(feature = "development")]
                            crate::log("CLUTCH: Received KEM response from unknown contact");
                            continue;
                        }
                        Some(expected) if expected != sender_pubkey => {
                            crate::log(&format!(
                                "CLUTCH: KEM device pubkey mismatch! Expected {}, got {}",
                                hex::encode(&expected[..8]),
                                hex::encode(&sender_pubkey[..8])
                            ));
                            continue;
                        }
                        Some(_) => {} // Match - proceed
                    }

                    // The payload is already parsed
                    let their_kem = payload;

                    // Find contact by handle_hash
                    for (idx, contact) in self.contacts.iter_mut().enumerate() {
                        if contact.handle_hash == their_handle_hash {
                            contact.ip = Some(sender_addr);

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
                                            contact.handle,
                                            hex::encode(&their_kem.target_hqc_pub_prefix)
                                        ));
                                        break;
                                    }
                                    // KEM targets our current keypairs. The other side already had both offer provenances and computed ceremony_id. Adopt it directly and decapsulate immediately — waiting for their offer to re-arrive over lossy UDP would deadlock indefinitely.
                                    crate::log(&format!(
                                        "CLUTCH: KEM response from {} arrived before ceremony_id - adopting peer's ceremony_id and decapsulating now",
                                        contact.handle
                                    ));
                                    contact.ceremony_id = Some(received_ceremony_id);
                                    // Initialize slots if needed
                                    if contact.clutch_slots.is_empty() {
                                        contact.init_clutch_slots(our_handle_hash);
                                    }
                                    use crate::crypto::clutch::ClutchKemSharedSecrets;
                                    let remote_secrets =
                                        ClutchKemSharedSecrets::decapsulate_from_peer(
                                            &their_kem,
                                            &our_keys_cloned,
                                        );
                                    if let Some(remote_slot) =
                                        contact.get_slot_mut(&their_handle_hash)
                                    {
                                        remote_slot.kem_secrets_from_them =
                                            Some(remote_secrets);
                                        crate::log(&format!(
                                            "CLUTCH: Decapsulated KEM from {} (ceremony_id adopted from peer)",
                                            contact.handle
                                        ));
                                    }
                                    break;
                                } else {
                                    // No keypairs at all - stale KEM encrypted to unknown keys
                                    crate::log(&format!(
                                        "CLUTCH: KEM response from {} arrived before keygen - discarding (encrypted to old keys)",
                                        contact.handle
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
                                        contact.handle,
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
                                        contact.handle
                                    ));
                                }

                                // Persist slot state after receiving KEM
                                if let Some(storage) = self.storage.as_ref() {
                                    if let Err(e) = crate::storage::contacts::save_clutch_slots(
                                        &contact.clutch_slots,
                                        &contact.offer_provenances,
                                        contact.ceremony_id,
                                        contact.handle.as_str(),
                                        storage,
                                    ) {
                                        crate::log(&format!(
                                            "CLUTCH: Failed to save slots for {}: {}",
                                            contact.handle, e
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
                                        contact.handle
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
                                    contact.handle
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

                    // Find contact by conversation_token
                    let their_handle_hash = match self
                        .contacts
                        .iter()
                        .find(|c| {
                            derive_conversation_token(&[our_handle_hash, c.handle_hash])
                                == conversation_token
                        })
                        .map(|c| c.handle_hash)
                    {
                        Some(h) => h,
                        None => {
                            crate::log(&format!(
                                "CLUTCH: Received complete proof with unknown conversation_token {}",
                                hex::encode(&conversation_token[..8])
                            ));
                            continue;
                        }
                    };

                    // Verify sender's device pubkey matches the contact's known identity
                    let contact_pubkey = self
                        .contacts
                        .iter()
                        .find(|c| c.handle_hash == their_handle_hash)
                        .map(|c| c.public_identity.key);

                    match contact_pubkey {
                        None => {
                            #[cfg(feature = "development")]
                            #[cfg(feature = "development")]
                            crate::log("CLUTCH: Received proof from unknown contact");
                            continue;
                        }
                        Some(expected) if expected != sender_pubkey => {
                            crate::log(&format!(
                                "CLUTCH: Proof device pubkey mismatch! Expected {}, got {}",
                                hex::encode(&expected[..8]),
                                hex::encode(&sender_pubkey[..8])
                            ));
                            continue;
                        }
                        Some(_) => {} // Match - proceed
                    }

                    // Find contact and process proof
                    for contact in &mut self.contacts {
                        if contact.handle_hash == their_handle_hash {
                            contact.ip = Some(sender_addr);

                            match contact.clutch_state {
                                ClutchState::AwaitingProof => {
                                    // We have our proof - verify theirs matches
                                    if let Some(our_proof) = contact.clutch_our_eggs_proof {
                                        if payload.eggs_proof == our_proof {
                                            // SUCCESS! Both parties computed same eggs
                                            crate::log(&format!(
                                                "CLUTCH: Proof verified with {}! ✓ proof={}...",
                                                contact.handle,
                                                hex::encode(&our_proof[..8])
                                            ));
                                            contact.clutch_state = ClutchState::Complete;
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
                                            // Keep our proof + resend budget: we just verified
                                            // theirs, but ours may still be in flight or dropped.
                                            // ping_contacts drains the budget over the next few
                                            // cycles, then clears it — so neither side strands.
                                            contact.clutch_their_eggs_proof = None;
                                            changed = true;

                                            // NOTE: Don't clear PT sends here - our ClutchComplete proof might still be in flight to them. Let it finish.

                                            // Save Complete state to disk immediately
                                            if let Some(storage) = self.storage.as_ref() {
                                                if let Err(e) =
                                                    crate::storage::contacts::save_contact(
                                                        contact,
                                                        storage,
                                                    )
                                                {
                                                    crate::log(&format!(
                                                        "Failed to save Complete state: {}",
                                                        e
                                                    ));
                                                } else {
                                                    crate::log(&format!(
                                                        "CLUTCH: Saved {} Complete state to disk",
                                                        contact.handle
                                                    ));
                                                }
                                            }
                                        } else {
                                            // CRYPTOGRAPHIC FAILURE - proofs don't match! This should NEVER happen unless:
                                            // 1. MITM attack
                                            // 2. Bug in ceremony
                                            // 3. Corruption
                                            let our_hex = hex::encode(&our_proof);
                                            let their_hex = hex::encode(&payload.eggs_proof);
                                            crate::log(&format!(
                                                "CLUTCH PROOF MISMATCH! This is a critical error.\n\
                                                Our proof:   {}\n\
                                                Their proof: {}",
                                                our_hex, their_hex
                                            ));
                                            panic!(
                                                "CLUTCH PROOF MISMATCH with {}! \
                                                This indicates MITM, bug, or corruption. \
                                                Our: {}... Their: {}...",
                                                contact.handle,
                                                &our_hex[..16],
                                                &their_hex[..16]
                                            );
                                        }
                                    } else {
                                        // Race condition: proof arrived before check_clutch_ceremonies processed our ceremony result. Store theirs for when we're ready.
                                        crate::log(&format!(
                                            "CLUTCH: Storing early proof from {} (AwaitingProof but our result not processed yet)",
                                            contact.handle
                                        ));
                                        contact.clutch_their_eggs_proof = Some(payload.eggs_proof);
                                        changed = true;
                                    }
                                }
                                ClutchState::Pending => {
                                    // We haven't computed our proof yet - store theirs for later
                                    crate::log(&format!(
                                        "CLUTCH: Storing early proof from {} (we're still in Pending)",
                                        contact.handle
                                    ));
                                    contact.clutch_their_eggs_proof = Some(payload.eggs_proof);
                                    changed = true;
                                }
                                ClutchState::Complete => {
                                    // Already complete - ignore duplicate
                                    crate::log(&format!(
                                        "CLUTCH: Ignoring duplicate proof from {} (already Complete)",
                                        contact.handle
                                    ));
                                }
                            }
                            break;
                        }
                    }
                }

                // LAN peer discovered via broadcast (NAT hairpinning workaround)
                StatusUpdate::LanPeerDiscovered {
                    handle_proof,
                    local_ip,
                    port,
                } => {
                    // Find contact by handle_proof and store their LAN IP + port
                    for (idx, contact) in self.contacts.iter_mut().enumerate() {
                        if contact.handle_proof == handle_proof {
                            let old_local = contact.local_ip;
                            let old_port = contact.local_port;
                            contact.local_ip = Some(local_ip);
                            contact.local_port = Some(port);
                            if old_local != Some(local_ip) || old_port != Some(port) {
                                crate::log(&format!(
                                    "LAN: Discovered {} at local {}:{}",
                                    contact.handle, local_ip, port
                                ));
                                // Ping immediately so we don't wait for next scheduled cycle
                                lan_ping_indices.push(idx);
                                changed = true;
                            }
                            break;
                        }
                    }
                }
            }
        }

        // Process deferred ceremony completions (after releasing checker borrow)
        for idx in ceremony_completions {
            self.complete_clutch_ceremony_by_idx(idx, our_handle_hash);
            changed = true;
        }

        // Ping contacts immediately when a new LAN address is discovered Fixes timing gap: startup ping fires before first LAN discovery arrives
        for idx in lan_ping_indices {
            self.ping_contact(idx);
        }

        // Retransmit pending messages to contacts that just came online Use last_received_ef6 from pong to only retransmit messages they don't have
        for (fid, peer_addr, handle, recipient_pubkey, last_received_ef6) in retransmit_requests {
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
                let mut count = 0usize;
                let wipe_dir = |dir: Option<std::path::PathBuf>, count: &mut usize| {
                    let Some(base) = dir else { return };
                    let app_dir = base.join(crate::storage::APP.dir);
                    let rd = match std::fs::read_dir(&app_dir) {
                        Ok(rd) => rd,
                        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return,
                        Err(e) => { eprintln!("[]n WARN: read_dir {}: {}", app_dir.display(), e); return }
                    };
                    for entry in rd.flatten() {
                        let p = entry.path();
                        if p.extension().map_or(false, |e| e == "vsf") {
                            match std::fs::remove_file(&p) {
                                Ok(()) => { eprintln!("[]n deleted {}", p.display()); *count += 1; }
                                Err(e) => eprintln!("[]n WARN: could not delete {}: {}", p.display(), e),
                            }
                        }
                    }
                };
                wipe_dir(dirs::config_dir(), &mut count);
                wipe_dir(dirs::data_dir(), &mut count);
                // Drop the in-memory vault state so the UI reflects the wipe immediately. Keep the
                // session + a live FlatStorage handle: it points at the now-empty dir and recreates
                // files lazily on the next write, so the app stays usable without a relaunch.
                self.contacts.clear();
                self.friendship_chains.clear();
                if let Ok(mut pks) = self.contact_pubkeys.lock() {
                    pks.clear();
                }
                eprintln!("[]n nuked {} vault file(s); session kept (still attested)", count);
            }
            'u' => {
                // De-attest — clear the tohu session (identity_seed/vault_seed/handle_proof) and drop back to the attest screen, leaving the vault on disk intact. The identity is deterministic from the handle, so re-typing it re-derives the same roots. Mirror of []n: []u forgets WHO you are, []n forgets WHAT you've stored. Only fires in development builds.
                tohu::clear_session();
                self.session = None;
                self.pending_broadcast_signal = -1; // Android: drop the sticky session broadcast.
                self.state = AppState::Launch(LaunchState::Fresh);
                self.refocus_handle_select_all();
                eprintln!("[]u de-attested; session cleared — re-type handle to re-attest");
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
fn button_bbox(btn: &Button) -> (isize, isize, isize, isize) {
    let half_w = btn.width * 0.5;
    let half_h = btn.height * 0.5;
    let x0 = (btn.center_x - half_w) as isize;
    let y0 = (btn.center_y - half_h) as isize;
    let x1 = (btn.center_x + half_w) as isize;
    let y1 = (btn.center_y + half_h) as isize;
    (x0, y0, x1, y1)
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
