//! New [`PhotonApp`] under construction: a [`fluor::host::app::FluorApp`] impl that will host Photon-desktop. Lives in a separate module from `super::app::PhotonApp` (the legacy 6 081-line owner of the per-platform renderer + 5 837-line `compositing.rs`) while the migration runs. After Phase 0e the legacy module deletes and `ui/mod.rs` re-exports this `PhotonApp` instead.
//!
//! Phase 0c milestone: empty render — chrome only. No app state, no widgets, no event handling beyond defaults. `cargo run --bin photon-messenger` (after Phase 0d wires main.rs to this) opens a window with fluor's chrome (perimeter hairline, drop shadow, min/max/close buttons, app-icon orb slot) and a blank scratch buffer inside.
//!
//! Subsequent phases (0e–1+) port Photon's state machine (`AppState`, network handles, contact list) into this struct's fields, add per-screen widgets, and wire cross-thread wake-ups through `FluorApp::on_user_event` using the [`super::PhotonEvent`] payload type.

use super::PhotonEvent;
use fluor::coord::Coord;
use fluor::geom::Viewport;
use fluor::host::app::{Context, EventResponse, FluorApp};
use fluor::host::chrome_widget::DefaultChrome;
use fluor::paint::HitId;
use winit::event::WindowEvent;
use winit::event_loop::EventLoopProxy;
use winit::window::CursorIcon;

/// Photon-desktop as a `FluorApp`. Owns fluor's `DefaultChrome` (window frame), the dense hit-id counter for widget allocation, and an optional event-loop proxy clone for waking from background tasks.
///
/// `chrome` is `Option` because [`DefaultChrome::new`] needs the actual viewport size, which the host doesn't hand the app until [`FluorApp::init`] fires. `new()` is parameterless; everything else allocates in `init`.
pub struct PhotonApp {
    chrome: Option<DefaultChrome>,
    hit_counter: HitId,
    event_proxy: Option<EventLoopProxy<PhotonEvent>>,
}

impl PhotonApp {
    /// Construct an empty app shell. Real state (chrome, network handles, app state machine) initializes in [`FluorApp::init`] once the viewport is known.
    pub fn new() -> Self {
        Self {
            chrome: None,
            hit_counter: 0,
            event_proxy: None,
        }
    }
}

impl Default for PhotonApp {
    fn default() -> Self {
        Self::new()
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

    fn on_resize(&mut self, width: u32, height: u32, _ctx: &mut Context) {
        if let Some(chrome) = self.chrome.as_mut() {
            chrome.resize(Viewport::new(width, height));
        }
    }

    fn on_event(&mut self, _event: &WindowEvent, _ctx: &mut Context) -> EventResponse {
        // Event routing wires up screen-by-screen in Phase 1+. For now, every event passes through unhandled — chrome buttons still work because the host wires its own resize / drag / maximize handling internally.
        EventResponse::Pass
    }

    fn render(&mut self, target: &mut [u32], ctx: &mut Context) {
        let Some(chrome) = self.chrome.as_mut() else {
            return;
        };
        let buf_w = ctx.viewport.width_px as usize;
        let buf_h = ctx.viewport.height_px as usize;

        // Bg layer is left empty — once Photon's procedural background noise lands behind chrome, the closure paints it. For now a no-op gives us a transparent inside-the-chrome region.
        chrome.rasterize_bg(ctx.damage, |_canvas| {});
        chrome.rasterize_chrome(ctx.damage, ctx.text, ctx.clip_mask);
        chrome.flatten_into(target, buf_w, buf_h, None);
    }

    fn hit_test_map(&self) -> Option<(&[fluor::paint::HitId], usize, usize)> {
        let chrome = self.chrome.as_ref()?;
        let (w, h) = chrome.dims();
        Some((chrome.hit_test_map(), w, h))
    }

    fn overlay_deltas(&mut self) -> Vec<u32> {
        // Chrome's four buttons all impl Hover via fluor's widget tree, so once Photon registers `Container::visit` for this app we just call `widget::build_overlay_deltas(self, self.hit_counter as usize + 1)`. Until then, an empty vec means no hover tint at all — chrome buttons render with their bg colour regardless of cursor position.
        Vec::new()
    }

    fn cursor_for(&self, _x: Coord, _y: Coord, _ctx: &Context) -> CursorIcon {
        CursorIcon::Default
    }
}

impl PhotonApp {
    /// Send a [`PhotonEvent`] through the event-loop proxy. Returns `false` if the proxy hasn't been set yet (host hasn't called `set_event_proxy`) or if the event loop has closed. Background tasks clone the proxy once at startup and call this; UI-thread code should mutate state directly + return `true` from `tick` or `on_event` instead of going through the proxy.
    #[allow(dead_code)] // Used by background tasks once Phase 0g wires them in.
    pub fn send_event(&self, event: PhotonEvent) -> bool {
        match &self.event_proxy {
            Some(proxy) => proxy.send_event(event).is_ok(),
            None => false,
        }
    }
}
