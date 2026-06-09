//! New [`PhotonApp`] under construction: a [`fluor::host::app::FluorApp`] impl that hosts Photon-desktop. Lives in a separate module from `super::app::PhotonApp` (the legacy 6 081-line owner of the per-platform renderer + 5 837-line `compositing.rs`) which is cfg-gated to Android as of Phase 0b+0e+0f. After the migration completes the legacy module deletes entirely.
//!
//! Phase 0c milestone: chrome only — perimeter, drop shadow, three buttons, app-icon orb slot. No app state, no widgets, no background, no screens. Phase 1+ rebuilds Launch / Ready / Searching / Conversation as widgets attached to this struct.
//!
//! Subsequent phases (1+) port Photon's state machine (`AppState`, network handles, contact list) into this struct's fields, add per-screen widgets, and wire cross-thread wake-ups through `FluorApp::on_user_event` using the [`super::PhotonEvent`] payload type.

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

/// Error-state message colour for the Launch screen's error slot — visible RGB (255, 80, 80), bright red, fully opaque. Packed as α + darkness: `α=0xFF | (0xFF − 255)<<16 | (0xFF − 80)<<8 | (0xFF − 80) = 0xFF_00_AF_AF`. Matches the legacy `theme::STATUS_TEXT_ERROR` intent (red error text) in fluor's storage convention.
const ERROR_TEXT_COLOUR: u32 = 0xFF_00_AF_AF;

/// Hint-label colour for the static "handle" prompt under the textbox — visible RGB (160, 160, 160), soft grey, fully opaque. Quieter than `theme::TEXTBOX_TEXT` (visible 224/224/220) because the hint is contextual, not content — dim enough that the eye reads the textbox first and the hint only on attention. Packed: `α=0xFF | (0xFF − 160) per channel = 0xFF_5F_5F_5F`.
const HINT_TEXT_COLOUR: u32 = 0xFF_5F_5F_5F;

/// Status-message colour for the "Attesting…" indicator that occupies the error slot while a handle query is in flight. Pure visible white (255, 255, 255), fully opaque — same slot as `ERROR_TEXT_COLOUR` but white instead of red so the user reads it as "neutral status" rather than "something went wrong". Packed: `α=0xFF | darkness=0x00_00_00 = 0xFF_00_00_00`.
const STATUS_TEXT_COLOUR: u32 = 0xFF_00_00_00;

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
    /// Blinkey timer for the focused textbox cursor. `tick()` polls it and writes `textbox.blinkey_visible` accordingly; resets on every keystroke so the cursor stays solid through typing instead of strobing.
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
    /// "Were both brackets held last frame?" — read in `damage_rect` so the frame following a release still includes the chord-hint bbox (one extra paint to clear stale hint pixels), and the toggle is debounced through a full frame.
    last_chord_held: bool,
    /// Attested handle, set on `QueryResult::Success`. Used by the Ready screen for the optional handle label below the avatar (gated by user settings — defaults off for security). `None` while the user is still on Launch.
    attested_handle: Option<String>,
    /// True when the dual-ring vault flagged a damaged ring on open this session. Drives the persistent amber banner on the Ready screen. Sticky for the session.
    vault_degraded: bool,
    /// FGTW connectivity state — flipped by `HandleQuery::try_recv_online`. Drives the top-left chrome orb's colour (red offline / green online). Starts false; the background worker reports the first real status within the first second of launch.
    online: bool,
    /// Contacts-page handle search/add textbox (Ready state). Distinct from `textbox` so content doesn't bleed between Launch (handle being attested) and Ready (handle being added as a contact).
    contacts_textbox: Option<Textbox>,
    /// Plus button to the right of `contacts_textbox` — clicking it (or pressing Enter in the textbox) triggers the add-contact flow (`HandleQuery::search`). Will eventually carry an idle "+" glyph and an in-progress rotating-hourglass animation (legacy port from `compositing.rs`); that lands when `ProgressButton` gets extracted to fluor.
    contacts_plus_btn: Option<Button>,
    /// In-memory contact list. Populated from `AttestationData.contacts` on attestation success and grown by `submit_add_friend` → `HandleQuery::search` results. Persistence (FlatStorage write on add) + rendering as scrollable rows below the search box land in subsequent slices.
    contacts: Vec<crate::types::Contact>,
    /// Device keypair injected externally (Android: from `NetworkContext` via `set_device_keypair` before `init`). When `Some`, `init` uses it directly; when `None`, `init` derives a fresh keypair from `get_machine_fingerprint` (desktop path). Android MUST set this before `init` runs — leaving it `None` on Android would silently downgrade to a zeroed placeholder keypair, which would be a critical key-derivation failure.
    device_keypair: Option<crate::network::fgtw::Keypair>,
    /// One-shot Android soft-keyboard request. `change_focus` sets `Some(true)` when focus enters a textbox and `Some(false)` when it leaves; `wants_keyboard` returns and clears the value. The Activity reads the JNI signal after each touch and calls `InputMethodManager.show/hide` accordingly. Stays `None` on idle frames so the Activity doesn't churn the IME.
    pending_keyboard_request: Option<bool>,
}

impl PhotonApp {
    /// Construct an empty app shell. Real state (chrome, network handles, app state machine) initializes in [`FluorApp::init`] once the viewport is known.
    pub fn new() -> Self {
        Self {
            chrome: None,
            hit_counter: 0,
            event_proxy: None,
            bg_scroll: 0,
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
            attested_handle: None,
            vault_degraded: false,
            online: false,
            contacts_textbox: None,
            contacts_plus_btn: None,
            contacts: Vec::new(),
            device_keypair: None,
            pending_keyboard_request: None,
        }
    }

    /// Inject the device keypair before `init` runs. Used by the Android JNI shim to pass through the keypair that `PhotonConnectionService` derives from the OS-provided device fingerprint — that fingerprint lives in Java (`Build.FINGERPRINT` / `Settings.Secure.ANDROID_ID`) and reaches the native side via `NetworkContext`. On desktop this stays unset; `init` falls back to `get_machine_fingerprint` (which reads `/etc/machine-id` etc.) and derives the keypair internally.
    pub fn set_device_keypair(&mut self, keypair: crate::network::fgtw::Keypair) {
        self.device_keypair = Some(keypair);
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

/// Walk the (currently chrome-only) widget tree. Once Phase 1+ adds the launch screen widgets (logo, spectrograph, handle textbox), they yield BEFORE chrome — same ordering convention as fluor's panes example: content first, chrome last, matching macOS / GNOME tab traversal. Widget tree visit order: launch-screen content (textbox → attest button) FIRST, then chrome's four buttons. Matches macOS / GNOME convention where Tab traverses form fields before window-frame controls. `linear_tab_next` reads this order off the visit walk; `dispatch_click` / `dispatch_key` use it to route events by id. Container-impl on widgets the Launch screen doesn't own (Ready/Searching/Conversation widgets, when they land) gates on `state` so off-screen widgets neither hit-test nor cycle.
impl Container for PhotonApp {
    fn visit(&mut self, f: &mut dyn FnMut(&mut dyn Widget)) {
        if matches!(self.state, AppState::Launch(_)) {
            if let Some(tb) = self.textbox.as_mut() {
                f(tb);
            }
            if let Some(btn) = self.attest_btn.as_mut() {
                f(btn);
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
        // Register Photon's Oxanium font weights with fluor's shared `TextRenderer` so the logo wordmark can resolve `Family::Name("Oxanium")`. Seven weights matches the legacy Photon font set; ExtraLight/Light/Regular/Medium/SemiBold/Bold/ExtraBold = numeric weights 200/300/400/500/600/700/800. The logo uses weight 800.
        let db = ctx.text.font_system_mut().db_mut();
        db.load_font_data(include_bytes!("../../assets/Oxanium/Oxanium-ExtraLight.ttf").to_vec());
        db.load_font_data(include_bytes!("../../assets/Oxanium/Oxanium-Light.ttf").to_vec());
        db.load_font_data(include_bytes!("../../assets/Oxanium/Oxanium-Regular.ttf").to_vec());
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
        // Top-left orb's ring doubles as the FGTW connectivity indicator (port of the legacy compositing.rs connectivity dot). Initialize red/offline; `try_recv_online` flips to green once the FGTW reports the device is reachable.
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
        self.update_widget_layout(ctx);

        // HandleQuery: device keypair is derived deterministically from the machine fingerprint (NEVER stored to disk per legacy convention — same machine yields the same keypair so attestations are reproducible across restarts). HandleQuery owns the UDP socket + sends/receives FGTW packets; an empty PeerStore wires the transport so query packets have somewhere to fan out to. Status checker, peer-update client, contact pubkeys etc. are deferred — they belong to later migration slices (Phase 2+). The proxy expect is structurally safe: fluor's host calls `set_event_proxy` BEFORE `init` (see `run_app` in fluor/src/host/app.rs), so `event_proxy` is always `Some` here.
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
                         the JNI shim wires through the keypair derived from the OS fingerprint \
                         in PhotonConnectionService; a missing keypair here means the wiring was \
                         skipped and would produce a zeroed/insecure key derivation"
                    );
                }
            }
        };
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
    }

    fn on_resize(&mut self, _width: u32, _height: u32, ctx: &mut Context) {
        if let Some(chrome) = self.chrome.as_mut() {
            // Use `ctx.viewport` directly — it carries the current `ru` (zoom factor) that fluor's host has already updated from Ctrl/Cmd +/-/0/scroll. Building a fresh `Viewport::new(w, h)` here would reset ru to 1.0 every resize/zoom event and silently strip the user's zoom state. Width/height are redundant with `ctx.viewport.{width_px, height_px}` for the same reason.
            chrome.resize(ctx.viewport);
            // Maximize toggles always change size between user-sized and screen-sized, so on_resize is the natural sync point for full_edge mode (no perimeter hairline / corner cutout / shadow when the window fills the screen). User-tweakable later if someone wants the bordered look when maximized.
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
                if changed {
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
                    self.bg_scroll = self.bg_scroll.wrapping_add(dy);
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

                // Focus follows click for focusable widgets (textbox + attest button). Chrome buttons aren't focusable (their `Widget::focus()` returns None) so a click on close/min/max leaves the prior focus intact — matches GNOME / macOS convention. We can't borrow `self` mutably twice in one walk, so determine "is this id focusable" via a pre-walk, then change_focus before the dispatch.
                let mut hit_is_focusable = false;
                self.visit(&mut |w| {
                    if w.id() == hit_id && w.focus().is_some() {
                        hit_is_focusable = true;
                    }
                });
                if hit_is_focusable && self.change_focus(Some(hit_id)) {
                    ctx.window.request_redraw();
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

                match &kev.logical_key {
                    // Tab cycles focus through the widget tree in registration order (launch widgets first, then chrome). Intercepted BEFORE delivery so textbox can't swallow it as "\t" insertion.
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
                    // Esc clears focus.
                    Key::Named(NamedKey::Escape) => {
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
                                // Reset blink so the cursor stays solid through fast typing instead of blinking mid-keystroke.
                                self.blink_timer.start(Instant::now());
                            }
                            return resp;
                        }
                        EventResponse::Pass
                    }
                }
            }
            Event::Ime(Ime::Commit(s)) => {
                // Android: soft IME committed `s` (typing, swipe, autocomplete). Deliver into the focused textbox via `insert_str`. Backspace arrives as the literal "\b" character from PhotonSurfaceView's deleteSurroundingText / composing-text replacement path, so peel those off and route to `backspace`. Anything else gets inserted verbatim. No-op when no textbox is focused (focus might sit on the attest button via Tab).
                let focused = self.focused;
                let tb_focused = self
                    .textbox
                    .as_ref()
                    .map(|t| Some(t.hit_id()) == focused)
                    .unwrap_or(false);
                if tb_focused {
                    if let Some(tb) = self.textbox.as_mut() {
                        for c in s.chars() {
                            if c == '\u{0008}' {
                                tb.backspace(ctx.text);
                            } else {
                                tb.insert_char(c, ctx.text);
                            }
                        }
                        self.blink_timer.start(Instant::now());
                        ctx.window.request_redraw();
                    }
                    return EventResponse::Handled;
                }
                EventResponse::Pass
            }
            _ => EventResponse::Pass,
        }
    }

    fn wake_at(&self) -> Option<Instant> {
        // Schedule the next wakeup at the soonest of:
        //   * `blink_timer.next_tick()` — drives the focused-textbox cursor pulse (random 0-300ms intervals); `None` while no textbox is focused.
        //   * `now` when an attestation is in flight — `tick()` advances `attest_anim_phase` at 1 cycle/sec for the "query in flight" wave shift; we need a wakeup every frame to keep it animating smoothly. Without this, the host blocks waiting for input and the animation stalls.
        let blink = self.blink_timer.next_tick();
        let attest = matches!(
            self.state,
            AppState::Launch(LaunchState::Attesting) | AppState::Searching
        )
        .then(Instant::now);
        match (blink, attest) {
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

        // Spectrum animation while attesting (port of legacy `compositing.rs:58-79`): wave phase advances at 2π rad/sec = 1 cycle/sec. Provides the visual "query in flight" cue the legacy build had — the bar slowly slides while we wait for FGTW to answer. Idle / Fresh / Error states leave the phase frozen so the screen stays calm.
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
        // Compute chord-held state BEFORE taking the mutable `chrome` borrow — `brackets_held` reads `&self` and the chrome borrow lives through the entire render. Update `last_chord_held` here too so the next frame's `damage_rect` knows whether to include the hint bbox for the one-frame clear.
        let held_now = self.brackets_held(Instant::now());
        self.last_chord_held = held_now;
        let show_hitmask = self.show_hitmask;
        // Snapshot the colour table so the post-flatten hitmask overlay can read it after the chrome borrow ends.
        let buf_w = ctx.viewport.width_px as usize;
        let buf_h = ctx.viewport.height_px as usize;

        let Some(chrome) = self.chrome.as_mut() else {
            return;
        };

        // Bg noise. `shimmer` is driven by `bg_scroll` and mixes into each row's starting colour — so the noise colour bias cycles as you scroll without changing the underlying pattern topology. `scroll_offset` is per-screen: Launch/Attest gets `0` (no vertical movement on the attest screen — shimmer only); future screens (Ready, Searching, Conversation) will pass `bg_scroll` so the noise pattern also translates with their page-scroll content. Phase 2+ branches on AppState to pick which.
        let bg_scroll = self.bg_scroll;
        let shimmer = bg_scroll as usize;
        let scroll_offset = 0; // Launch only for now.
        // Launch layout: faithful proportional slicing port from legacy `Layout::new` — spectrum near the top, logo wordmark overlapping its bottom, attest block (textbox + hint + button) below. Compute every frame; cheap and lets resize flow through without a separate cache.
        let layout = LaunchLayout::compute(buf_w, buf_h, ctx.viewport.ru);
        // Chromatic wave phase has two summands:
        //   * Scroll-driven base (`bg_scroll * 1/128 rad/scroll-unit`) — one wheel-notch ≈ 8 units → ~1/16 rad shift; user-tunable by changing the shift exponent.
        //   * `attest_anim_phase` (advanced in `tick()` while `LaunchState::Attesting`) — the legacy "query in flight" cue, 1 cycle/sec.
        // Summing them means the wave responds to BOTH inputs simultaneously: a user scrolling during an attestation still nudges the phase on top of the animation.
        let phase = bg_scroll as f32 * (1. / ((1 << 7) as f32)) + self.attest_anim_phase;
        let period_scale = 1.;
        let spectrum_rect = layout.spectrum;
        let logo_rect = layout.photon_text;
        // Split-borrow `ctx.damage` (consumed by rasterize_bg's first arg) and `ctx.text` (captured by the closure for the logo's text rendering). These are disjoint fields of `Context` so the borrow checker allows both reborrows simultaneously. The closure is non-`move` so the text reborrow ends when rasterize_bg returns, leaving `ctx.text` available for `rasterize_chrome` on the next line.
        let text = &mut *ctx.text;
        // Bg-first compose chain (matches legacy `compositing.rs` exactly): noise paints opaque, the wave reads it for the `sqrt(c*scale + c_bg²)` blend, then the logo (glow / body / highlight) paints over both via legacy visible-RGB ops. Each step preserves α on the pixels it touches. The wave + logo are Launch-screen chrome — once attested the user shouldn't be staring at the wordmark every time they open the app, so Ready / Searching / Conversation get just the background noise and let their own widgets own the canvas.
        let on_launch = matches!(self.state, AppState::Launch(_));
        // Swap the noise base colour to BG_BASE_WARNING when the dual-ring vault flagged degraded this session — the noise pass already runs every frame so this changes a colour, not the pass count. None on the happy path keeps the default green-dark base from theme.rs.
        let bg_base = if self.vault_degraded {
            Some(crate::ui::theme::BG_BASE_WARNING)
        } else {
            None
        };
        // `fullscreen` controls the 1-pixel inset around the noise: on desktop we leave a 1-px ring for the window perimeter / shadow band so the noise doesn't draw under the chrome's edge stroke; on Android (fullscreen surface, no chrome edges, no shadow) we paint right to the screen edge or you get a 1-px white frame.
        #[cfg(target_os = "android")]
        let bg_fullscreen = true;
        #[cfg(not(target_os = "android"))]
        let bg_fullscreen = false;
        chrome.rasterize_bg(ctx.damage, |canvas| {
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
                    HINT_TEXT_COLOUR,
                    "Oxanium",
                    None,
                    None,
                    None,
                );
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

        // Ready screen — slice-based layout matching legacy ContactsUnifiedLayout. Today only the avatar circle is painted; the layout already carries rects for handle / hint / textbox / separator / contact rows so subsequent slices drop into named slots without re-computing geometry.
        if matches!(self.state, AppState::Ready) {
            let mut canvas = Canvas::new(target, buf_w, buf_h, ctx.damage);
            let ready_layout = ReadyLayout::compute(buf_w, buf_h, ctx.viewport.ru);
            let (cx, cy, radius) = ready_layout.avatar_center_radius();
            // 0xFFC5C5C5 in fluor's α+darkness format = α 0xFF, darkness 0xC5 each channel = visible RGB(0x3A, 0x3A, 0x3A) ≈ 22% brightness. Standalone constant (no theme.rs entry yet) — promote when Ready chrome gets a proper palette pass.
            const AVATAR_PLACEHOLDER: u32 = 0xFF_C5_C5_C5;
            paint::draw_circle(&mut canvas, cx, cy, radius, AVATAR_PLACEHOLDER, None);

            // Contacts-page textbox + plus button. The plus button is OVERLAID inside the textbox right edge (legacy pattern) and ONLY rendered when the textbox has content — empty textbox shows no button.
            //
            // Under-blend semantics ("topmost paints first; later opaque dst wins"): paint the button FIRST so it's visually topmost, then the textbox under it. Textbox::render_content_into stamps hit_test_map unconditionally over its entire bbox, so after the textbox runs we re-stamp the button's bbox with the button's hit_id to recover correct click dispatch in the overlap.
            let plus_visible = self
                .contacts_textbox
                .as_ref()
                .map(|tb| !tb.chars.is_empty())
                .unwrap_or(false);
            let plus_bbox: Option<(isize, isize, isize, isize, HitId)> = if plus_visible {
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
    /// Send a [`PhotonEvent`] through the event-loop proxy. Returns `false` if the proxy hasn't been set yet (host hasn't called `set_event_proxy`) or if the event loop has closed. Background tasks clone the proxy once at startup and call this; UI-thread code should mutate state directly + return `true` from `tick` or `on_event` instead of going through the proxy.
    #[allow(dead_code)] // Used by background tasks once Phase 1+ wires them in.
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
        // Font size = span/24 — small enough to fit the legacy attest-block textbox ratio (height ≈ 2 units of the 9.25-unit slice), large enough to remain legible across zoom range. Same scalar drives the button so they read as a matched pair.
        let span = ctx.viewport.effective_span();
        let font_size = span / 24.;

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

        // Contacts-page widgets: textbox takes the full ReadyLayout textbox slot; the plus button is OVERLAID inside the textbox's right edge (legacy compositing.rs pattern). Button size = 7/8 textbox height, inset from the right by 1/16 of the textbox height — same proportions as the legacy `tl.box_height * 7/8` / `tl.box_height / 16`. Same font_size as the launch widgets so zoom feels consistent across screens.
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
        if self.attested_handle.as_deref() == Some(handle.as_str()) {
            crate::log(&format!(
                "add-friend: refusing to add own handle '{}'",
                handle
            ));
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
        if let Some(hq) = self.handle_query.as_ref() {
            crate::log(&format!("add-friend: searching FGTW for '{}'", handle));
            hq.search(handle);
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
        }
    }

    /// Handle a [`QueryResult`] arriving from HandleQuery's background worker. Transitions the launch state and stashes the proof on success — Phase 2 (Ready screen) is where the proof gets consumed by contact + storage init. For now success leaves the user on Launch with a "ready to advance" log line; persistence + screen transition land next slice.
    fn on_query_result(&mut self, result: QueryResult) {
        use num_bigint::BigUint;
        match result {
            QueryResult::Success(data) => {
                if let Some(hq) = self.handle_query.as_ref() {
                    hq.set_handle_proof(data.handle_proof, &data.handle);
                }
                // Pubkey emitted as voca-encoded camelCase so a user reading the log can double-click + paste the value as a single word (matches `Development:` key lines from handle_query.rs).
                eprintln!(
                    "attestation success: handle = {}  pubkey = {}",
                    data.handle,
                    voca::encode(BigUint::from_bytes_be(&data.handle_proof))
                );
                // Stash the handle for the Ready screen (the optional label below the avatar). Settings persistence + the actual gate on whether to show it land in a later slice — for now Ready always renders the placeholder without text.
                self.attested_handle = Some(data.handle.clone());
                self.vault_degraded = data.vault_degraded;
                self.contacts = data.contacts.clone();
                crate::log(&format!(
                    "UI: loaded {} contact(s) into Ready state",
                    self.contacts.len()
                ));
                self.state = AppState::Ready;
            }
            QueryResult::AlreadyAttested(peer) => {
                let msg = format!(
                    "handle already attested by another device (pubkey {})",
                    voca::encode(BigUint::from_bytes_be(peer.device_pubkey.as_bytes()))
                );
                eprintln!("attestation rejected: {msg}");
                self.state = AppState::Launch(LaunchState::Error(msg));
            }
            QueryResult::Error(e) => {
                eprintln!("attestation error: {e}");
                self.state = AppState::Launch(LaunchState::Error(e));
            }
        }
    }

    /// Handle a [`SearchResult`] from `HandleQuery::search`. On `Found`, build a `Contact` from the peer and append to `self.contacts` (skip if a contact with the same handle already exists; should be rare given `submit_add_friend` pre-checks, but the search races against attestation worker's contact load). Clears the textbox on success so the user can immediately search the next handle. On `NotFound`/`Error`, log only — UI search-result rendering into the hint slot lands in a follow-up.
    fn on_search_result(&mut self, result: crate::ui::state::SearchResult) {
        use crate::ui::state::SearchResult;
        match result {
            SearchResult::Found(peer) => {
                let already = self
                    .contacts
                    .iter()
                    .any(|c| c.handle.as_str().eq_ignore_ascii_case(peer.handle.as_str()));
                if already {
                    crate::log(&format!(
                        "search-result: '{}' already in contacts — skipping add",
                        peer.handle.as_str()
                    ));
                    return;
                }
                let contact = crate::types::Contact::new(
                    peer.handle.clone(),
                    peer.handle_proof,
                    peer.device_pubkey.clone(),
                )
                .with_ip(peer.ip);
                crate::log(&format!(
                    "search-result: added contact '{}' (total: {})",
                    contact.handle.as_str(),
                    self.contacts.len() + 1
                ));
                self.contacts.push(contact);
                // Textbox clearing post-search would be nice UX but Textbox has no public `clear` method yet — the user can select-all+delete or backspace. Punted to a fluor follow-up: either add `Textbox::clear` or a "consume submit" option that auto-clears on successful submit.
            }
            SearchResult::NotFound => {
                crate::log("search-result: handle not found on FGTW");
            }
            SearchResult::Error(e) => {
                crate::log(&format!("search-result: error '{}'", e));
            }
        }
    }

    /// Apply a focus change: update `self.focused`, then walk the widget tree via `apply_focus_change` so the old + new widgets fire `set_focused(false/true)` and mark their caches dirty. Returns `true` if anything changed (caller decides whether to request a redraw — most callers do). Also drops a one-shot Android keyboard-show/hide request when focus enters or leaves a textbox; the Activity reads it via `FluorApp::wants_keyboard` after each touch and raises / dismisses the soft IME accordingly.
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
