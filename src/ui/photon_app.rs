//! [`PhotonApp`]: the [`fluor::host::app::FluorApp`] impl that hosts Photon on desktop. Owns the app state machine (`AppState`), network handles, contact list, and the per-screen widgets (Launch / Ready / Searching / Conversation), drawing the chrome (perimeter, shadow, window buttons, app-icon orb) plus each screen's content, and routing cross-thread wake-ups thru `FluorApp::on_user_event` with the [`super::PhotonEvent`] payload.

use super::chromatic_wave::chromatic_wave;
use super::launch_layout::{AttestBlockLayout, LaunchLayout};
use super::photon_logo::paint_photon_logo;
use super::ready_layout::ReadyLayout;
use super::state::{AppState, LaunchState};
use super::PhotonEvent;
use crate::network::fgtw::{derive_device_keypair, PeerStore};
#[cfg(not(target_os = "android"))]
use crate::network::fgtw::get_machine_fingerprint;
use crate::network::{HandleQuery, QueryResult};
use fluor::canvas::{Canvas, PixelRect};
use fluor::coord::Coord;
use fluor::geom::Viewport;
use fluor::host::app::{Context, EventResponse, FluorApp};
use fluor::host::chrome::{self, ResizeEdge};
use fluor::host::chrome_widget::DefaultChrome;
use fluor::event::{
    CursorIcon, ElementState, Event, Ime, Key, MouseButton, MouseScrollDelta, NamedKey,
};
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
const RING_OFFLINE_COLOUR: u32 = fluor::theme::dark(fluor::theme::fmt(0x00_50_50_50));
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
    ("H", "Hit-mask overlay"),
    ("P", "Skip premultiply"),
    ("A", "Show alpha (cycle)"),
    ("C", "Skip chrome"),
    ("L", "Skip controls"),
    ("R", "Force redraw"),
    ("F", "FPS / per-stage timings strip"),
    ("W", "Damage rect outline (Where)"),
    ("D", "Screen-buffer decay (fade)"),
    ("B", "Finalize copy-pass blue tint"),
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
                    Err(e) => {
                        crate::log(&format!("avatar picker: FGTW upload failed: {e}"))
                    }
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
        db.load_font_data(include_bytes!("../../assets/Oxanium/Oxanium-Regular+glyphs.ttf").to_vec());
        db.load_font_data(include_bytes!("../../assets/Oxanium/Oxanium-Medium.ttf").to_vec());
        db.load_font_data(include_bytes!("../../assets/Oxanium/Oxanium-SemiBold.ttf").to_vec());
        db.load_font_data(include_bytes!("../../assets/Oxanium/Oxanium-Bold.ttf").to_vec());
        db.load_font_data(include_bytes!("../../assets/Oxanium/Oxanium-ExtraBold.ttf").to_vec());

        // Chrome owns its own hit-test map sized to the viewport, allocates four hit-ids for its buttons via the threaded counter, and stamps the perimeter + button rasters in `rasterize_chrome`. The Photon orb (chromatic starburst — same brand mark as the OS-level app icon) ships as a VSF image and decodes into the chrome's app_icon slot.
        let orb_icon = fluor::host::icon::Icon::from_vsf_bytes(include_bytes!(
            "../../assets/photon-orb.vsf"
        ))
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
            fluor::paint::DEBUG_SKIP_CONTROLS
                .store(true, std::sync::atomic::Ordering::Relaxed);
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
        self.contacts_plus_btn = Some(Button::new(
            &mut self.hit_counter,
            0.,
            0.,
            1.,
            1.,
            12.,
            "+",
        ));
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
        self.handle_query = Some(hq);

        // Auto-resume from the remembered session roots. If tohu has this login's roots (persisted on a prior, FGTW-confirmed attest), paint Ready IMMEDIATELY from local state — we already own this identity, so there is no reason to block the first frame on the network. The avatar comes from a local cache file (no vault, no network); contacts + peer presence + cloud-merge arrive a beat later via the background `query_resume` and merge in through `on_query_result`. A rejection (handle claimed by another device) bails back to the attest screen; a transient network error leaves the local session on Ready untouched.
        // None (first run / post-logout) falls through to the normal typed-attest flow.
        if let Some(remembered) = tohu::session() {
            self.session = Some(remembered);
            self.device_avatar_pixels = crate::ui::avatar::load_avatar_from_seed(&remembered.identity_seed)
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
                        self.storage = Some(s);
                    }
                    Err(e) => crate::log(&format!("STORAGE: init failed on resume: {}", e)),
                }
            }
            self.state = AppState::Ready;
            if let Some(hq) = self.handle_query.as_ref() {
                crate::log("UI: resumed to Ready from local session roots (tohu) — FGTW announce + presence run in background");
                hq.query_resume(remembered);
            }
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
                // While attesting, the launch textbox + attest button are inert: force their hover state off regardless of cursor position so neither lights up under the pointer during the frozen wait.
                let launch_locked = matches!(self.state, AppState::Launch(ref s) if !s.can_edit_handle());
                let mut changed = false;
                if let Some(chrome) = self.chrome.as_mut() {
                    changed |= chrome.set_hover(new_hit);
                }
                if let Some(tb) = self.textbox.as_mut() {
                    let want = !launch_locked && new_hit == tb.hit_id();
                    if tb.is_hovered() != want {
                        tb.set_hovered(want);
                        changed = true;
                    }
                }
                if let Some(btn) = self.attest_btn.as_mut() {
                    let want = !launch_locked && new_hit == btn.hit_id();
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
                // While attesting, the launch screen is frozen: a click on the handle textbox OR the attest button is swallowed without effect. For the textbox this prevents refocus / cursor placement / drag-select arm; for the button it kills the press visual and the submit (which would no-op anyway). The submit path already dropped focus; this keeps a stray click from grabbing it back until the query resolves or the user escapes back to Fresh.
                let launch_locked = matches!(self.state, AppState::Launch(ref s) if !s.can_edit_handle());
                let locked_launch_widget = launch_locked
                    && (self
                        .textbox
                        .as_ref()
                        .map(|t| t.hit_id() == hit_id)
                        .unwrap_or(false)
                        || self
                            .attest_btn
                            .as_ref()
                            .map(|b| b.hit_id() == hit_id)
                            .unwrap_or(false));
                if hit_is_focusable && !locked_launch_widget && self.change_focus(Some(hit_id)) {
                    ctx.window.request_redraw();
                }

                // Frozen launch widget during attesting: swallow the click without dispatching, so neither `Textbox::on_click` (cursor/selection) nor `Button::on_click` (press counter) fires.
                if locked_launch_widget {
                    return EventResponse::Handled;
                }

                // Dispatch the click via the fluor widget helper. Walks the tree once, finds the widget with `hit_id`, calls its `Click::on_click`. Returns `EventResponse::Pass` if the widget has no Click capability — covers chrome's app-icon orb (no action wired yet).
                let response = widget::dispatch_click(
                    self,
                    hit_id,
                    ctx.cursor_x,
                    ctx.cursor_y,
                    ctx.modifiers,
                );

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
                let clicked = self.attest_btn.as_mut().map(|b| b.take_click()).unwrap_or(false);
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
                            let resp = widget::dispatch_key(self, focus_id, kev, ctx.modifiers, ctx.text);
                            // Either button can activate on Enter; poll both and route to the matching submit.
                            let attest_clicked = self.attest_btn.as_mut().map(|b| b.take_click()).unwrap_or(false);
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
                            if attest_clicked || plus_clicked || matches!(resp, EventResponse::Handled) {
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
            if let Some(tb) = self.textbox.as_mut() {
                if tb.flip_blinkey() {
                    needs_redraw = true;
                }
            }
            if let Some(tb) = self.contacts_textbox.as_mut() {
                if tb.flip_blinkey() {
                    needs_redraw = true;
                }
            }
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
        let version_size = (attest_for_version.hint.y1 - attest_for_version.hint.y0) as f32 * 0.7 * 0.5;
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
                LaunchState::Error(msg) if !msg.is_empty() => Some((msg.as_str(), ERROR_TEXT_COLOUR)),
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
                    self.device_avatar_scaled = Some(crate::ui::avatar_render::update_avatar_scaled(
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
                restamp_hit_rect(&mut chrome.hit_test_map, buf_w, buf_h, x0, y0, x1, y1, btn_id);
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
                .filter(|(_, c)| filter.is_empty() || c.handle.as_str().to_lowercase().contains(&filter))
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
            let ring_thickness = (avatar_r * 0.15).max(1.0);
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
                        &mut canvas, avatar_cx, cy, avatar_r, scaled, diam, Some(rows_clip),
                    );
                } else {
                    paint::draw_circle(&mut canvas, avatar_cx, cy, avatar_r, AVATAR_PLACEHOLDER, Some(rows_clip));
                }
                let ring = if online { RING_ONLINE_COLOUR } else { RING_OFFLINE_COLOUR };
                paint::draw_circle(&mut canvas, avatar_cx, cy, avatar_r + ring_thickness, ring, Some(rows_clip));

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
                let w_sec = ctx.text.measure_text_width("Sec", label_size, 500, "Oxanium");
                let w_rec = ctx.text.measure_text_width("Rec", label_size, 500, "Oxanium");
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
                        None, None, None,
                    );
                    // Stamp the back button hit rect.
                    let back_w = ctx.text.measure_text_width(back_text, back_size, 500, "Oxanium");
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
                        None, None, None,
                    );

                    // Avatar
                    let avatar_y = name_y + unit * 3.0;
                    let avatar_diam = (unit * 3.0) as usize;
                    let avatar_r = avatar_diam as f32 * 0.5;
                    let avatar_cx = buf_w as f32 * 0.5;
                    if let Some(scaled) = contact.avatar_scaled.as_ref() {
                        crate::ui::avatar_render::draw_avatar(
                            &mut canvas, avatar_cx, avatar_y, avatar_r, scaled, avatar_diam, None,
                        );
                    } else {
                        paint::draw_circle(&mut canvas, avatar_cx, avatar_y, avatar_r, AVATAR_PLACEHOLDER, None);
                    }
                    let ring = if contact.is_online { RING_ONLINE_COLOUR } else { RING_OFFLINE_COLOUR };
                    let ring_thick = (avatar_r * 0.15).max(1.0);
                    paint::draw_circle(&mut canvas, avatar_cx, avatar_y, avatar_r + ring_thick, ring, None);

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
                        None, None, None,
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
                let device_pubkey = self.device_keypair.as_ref()
                    .map(|kp| crate::types::DevicePubkey::from_bytes(*kp.public.as_bytes()))
                    .unwrap_or_else(|| crate::types::DevicePubkey::from_bytes([0u8; 32]));
                let mut contact = crate::types::Contact::new(
                    handle_text,
                    session.handle_proof,
                    device_pubkey,
                );
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
        // While attesting the handle field is frozen — no cut/paste mutating the in-flight handle (copy is harmless but we lock the whole field for consistency).
        if on_launch && matches!(self.state, AppState::Launch(ref s) if !s.can_edit_handle()) {
            return EventResponse::Handled;
        }
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
                for incoming in &data.contacts {
                    let dominated = self.contacts.iter().any(|c| c.handle_proof == incoming.handle_proof);
                    if !dominated {
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
                }
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
                    self.search_status = Some((format!("{handle} already added"), SEARCH_FOUND_COLOUR));
                    return;
                }
                let mut contact = crate::types::Contact::new(
                    peer.handle.clone(),
                    peer.handle_proof,
                    peer.device_pubkey.clone(),
                )
                .with_ip(peer.ip);
                // Self-contact: same identity, no key exchange needed.
                if self.session.as_ref().map(|s| s.identity_seed) == Some(contact.handle_hash) {
                    contact.clutch_state = crate::types::ClutchState::Complete;
                }
                crate::log(&format!(
                    "search-result: added contact '{}' (total: {})",
                    contact.handle.as_str(),
                    self.contacts.len() + 1
                ));
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

    /// True iff `id` belongs to one of photon's textboxes (launch handle textbox or contacts search textbox). Used by `change_focus` to detect focus transitions into / out of a text-input target so the Android IME show/hide signal can be triggered.
    fn is_textbox(&self, id: Option<HitId>) -> bool {
        let Some(id) = id else {
            return false;
        };
        self.textbox.as_ref().map(|t| t.hit_id()) == Some(id)
            || self
                .contacts_textbox
                .as_ref()
                .map(|t| t.hit_id())
                == Some(id)
    }

    /// The textbox that currently holds focus (launch handle field or contacts search box), or `None`. The mutable counterpart to [`is_textbox`] — the Android IME commit path routes the committed string here, since (unlike desktop keys) it has no focus-generic dispatcher. Keep this and `is_textbox` in lockstep: a future textbox (the Conversation input bar) must be added to both.
    fn focused_textbox_mut(&mut self) -> Option<&mut Textbox> {
        let focused = self.focused?;
        if self.textbox.as_ref().map(|t| t.hit_id()) == Some(focused) {
            return self.textbox.as_mut();
        }
        if self.contacts_textbox.as_ref().map(|t| t.hit_id()) == Some(focused) {
            return self.contacts_textbox.as_mut();
        }
        None
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
