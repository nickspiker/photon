//! New [`PhotonApp`] under construction: a [`fluor::host::app::FluorApp`] impl that hosts Photon-desktop. Lives in a separate module from `super::app::PhotonApp` (the legacy 6 081-line owner of the per-platform renderer + 5 837-line `compositing.rs`) which is cfg-gated to Android as of Phase 0b+0e+0f. After the migration completes the legacy module deletes entirely.
//!
//! Phase 0c milestone: chrome only — perimeter, drop shadow, three buttons, app-icon orb slot. No app state, no widgets, no background, no screens. Phase 1+ rebuilds Launch / Ready / Searching / Conversation as widgets attached to this struct.
//!
//! Subsequent phases (1+) port Photon's state machine (`AppState`, network handles, contact list) into this struct's fields, add per-screen widgets, and wire cross-thread wake-ups through `FluorApp::on_user_event` using the [`super::PhotonEvent`] payload type.

use super::PhotonEvent;
use fluor::coord::Coord;
use fluor::geom::Viewport;
use fluor::host::app::{Context, EventResponse, FluorApp};
use fluor::host::chrome::{self, ResizeEdge};
use fluor::host::chrome_widget::DefaultChrome;
use fluor::host::widget::{self, Container, Widget};
use fluor::paint::{self, HIT_NONE, HitId};
use winit::event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::EventLoopProxy;
use winit::window::CursorIcon;

/// Photon-desktop as a `FluorApp`. Owns fluor's `DefaultChrome` (window frame), the dense hit-id counter for widget allocation, and an optional event-loop proxy clone for waking from background tasks.
///
/// `chrome` is `Option` because [`DefaultChrome::new`] needs the actual viewport size, which the host doesn't hand the app until [`FluorApp::init`] fires. `new()` is parameterless; everything else allocates in `init`.
pub struct PhotonApp {
    chrome: Option<DefaultChrome>,
    hit_counter: HitId,
    event_proxy: Option<EventLoopProxy<PhotonEvent>>,
    /// Vertical scroll offset for the background noise — drives `paint::background_noise`'s `scroll_offset` (visually translates the noise pattern up/down) AND, on the home screen, gets fed into the `speckle` parameter so the speckle density modulates as you scroll. MouseWheel events in `on_event` mutate this; everything else reads it.
    bg_scroll: isize,
}

impl PhotonApp {
    /// Construct an empty app shell. Real state (chrome, network handles, app state machine) initializes in [`FluorApp::init`] once the viewport is known.
    pub fn new() -> Self {
        Self {
            chrome: None,
            hit_counter: 0,
            event_proxy: None,
            bg_scroll: 0,
        }
    }
}

impl Default for PhotonApp {
    fn default() -> Self {
        Self::new()
    }
}

/// Walk the (currently chrome-only) widget tree. Once Phase 1+ adds the launch screen widgets (logo, spectrograph, handle textbox), they yield BEFORE chrome — same ordering convention as fluor's panes example: content first, chrome last, matching macOS / GNOME tab traversal.
impl Container for PhotonApp {
    fn visit(&mut self, f: &mut dyn FnMut(&mut dyn Widget)) {
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

    fn set_event_proxy(&mut self, proxy: EventLoopProxy<Self::UserEvent>) {
        self.event_proxy = Some(proxy);
    }

    fn init(&mut self, ctx: &mut Context) {
        // Chrome owns its own hit-test map sized to the viewport, allocates four hit-ids for its buttons via the threaded counter, and stamps the perimeter + button rasters in `rasterize_chrome`. No app icon yet — Photon's icon asset wires here in a follow-up commit.
        let chrome = DefaultChrome::new(
            ctx.viewport,
            "Photon",
            None,
            None,
            &mut self.hit_counter,
        );
        self.chrome = Some(chrome);
    }

    fn on_resize(&mut self, width: u32, height: u32, ctx: &mut Context) {
        if let Some(chrome) = self.chrome.as_mut() {
            chrome.resize(Viewport::new(width, height));
            // Maximize toggles always change size between user-sized and screen-sized, so on_resize is the natural sync point for full_edge mode (no perimeter hairline / corner cutout / shadow when the window fills the screen). User-tweakable later if someone wants the bordered look when maximized.
            chrome.set_full_edge(ctx.is_maximized);
        }
    }

    fn on_event(&mut self, event: &WindowEvent, ctx: &mut Context) -> EventResponse {
        match event {
            WindowEvent::CursorMoved { .. } => {
                // Hover tint follows the cursor across chrome buttons. Outside chrome the hit is HIT_NONE, which clears any prior hover.
                let new_hit = self
                    .chrome
                    .as_ref()
                    .map(|c| c.hit_at(ctx.cursor_x, ctx.cursor_y))
                    .unwrap_or(HIT_NONE);
                if let Some(chrome) = self.chrome.as_mut() {
                    if chrome.set_hover(new_hit) {
                        ctx.window.request_redraw();
                    }
                }
                EventResponse::Pass
            }
            WindowEvent::CursorLeft { .. } => {
                if let Some(chrome) = self.chrome.as_mut() {
                    if chrome.set_hover(HIT_NONE) {
                        ctx.window.request_redraw();
                    }
                }
                EventResponse::Pass
            }
            WindowEvent::MouseWheel { delta, .. } => {
                // Bg-noise scroll. Vertical-only for now — horizontal trackpad gestures and shift-modified wheel both fold into the same `bg_scroll` axis. LineDelta from a discrete mouse wheel gets multiplied to feel like a normal scroll step; PixelDelta (trackpad) is used directly. The scroll value feeds both `scroll_offset` (translates the noise pattern up/down) and the speckle-density modulation in `render`.
                let dy = match delta {
                    MouseScrollDelta::LineDelta(_, y) => (*y as isize) * 8,
                    MouseScrollDelta::PixelDelta(p) => p.y as isize,
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
            WindowEvent::MouseInput {
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
                    // No chrome button under the cursor — fall back to resize-edge / title-bar drag. Resize edge takes precedence; clicks anywhere else inside the visible window start a move-drag (which the host promotes to an actual drag once the cursor passes the dead-zone threshold).
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

                // Chrome button click → walk the container, find the widget with this id, ask its Click impl for the action's EventResponse. Chrome's ChromeButton::on_click returns Close / Minimize / ToggleMaximized based on the button's action.
                let x = ctx.cursor_x;
                let y = ctx.cursor_y;
                let mods = ctx.modifiers;
                let mut response = EventResponse::Pass;
                self.visit(&mut |w| {
                    if w.id() == hit_id {
                        if let Some(c) = w.click() {
                            response = c.on_click(x, y, mods);
                        }
                    }
                });
                response
            }
            _ => EventResponse::Pass,
        }
    }

    fn render(&mut self, target: &mut [u32], ctx: &mut Context) {
        let Some(chrome) = self.chrome.as_mut() else {
            return;
        };
        let buf_w = ctx.viewport.width_px as usize;
        let buf_h = ctx.viewport.height_px as usize;

        // Bg noise. `speckle` is driven by `bg_scroll` so the speckle density changes as you scroll — the multiplier picks the dynamic range. `scroll_offset` is per-screen: Launch/Attest gets `0` (no vertical movement on the attest screen — speckle only); future screens (Ready, Searching, Conversation) will pass `bg_scroll` so the noise pattern also translates with their page-scroll content. Phase 2+ branches on AppState to pick which.
        let bg_scroll = self.bg_scroll;
        let speckle = (bg_scroll as usize).wrapping_mul(0x0100_0000);
        let scroll_offset = 0; // Launch only for now.
        chrome.rasterize_bg(ctx.damage, move |canvas| {
            paint::background_noise(canvas, speckle, false, scroll_offset, None);
        });
        chrome.rasterize_chrome(ctx.damage, ctx.text, ctx.clip_mask);
        chrome.flatten_into(target, buf_w, buf_h, None);
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
            Some(proxy) => proxy.send_event(event).is_ok(),
            None => false,
        }
    }
}
