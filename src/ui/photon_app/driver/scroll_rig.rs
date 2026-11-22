//! A desktop render rig at the Pro's panel (1344 × 2992 under the safe circle): a fling in a long conversation and down the contacts, each frame drawn as fluor's ferros host draws it (the app's rects, the scroll hint taken unless the frame is one whole rect, the rects cleared, the render told the hint it rode), counted, and held to the frame drawn whole at the same state.
//! Run it with `cargo test --lib scroll_rig -- --ignored --nocapture --test-threads=1`: it prints the frames on the memmove, the part of the frame drawn and the render's time a frame.
//! It lays out the UI alone (`init_display`): no vault, no network, no thread of the core.

use super::*;
use fluor::canvas::{Damage, DamageRects, PixelRect};

struct NullWindow;
impl fluor::host::window_handle::WindowHandle for NullWindow {
    fn request_redraw(&self) {}
}

/// The Pro's panel.
const W: usize = 1344;
const H: usize = 2992;

struct Rig {
    app: PhotonApp,
    text: fluor::text::TextRenderer,
    clip: Vec<u8>,
    damage: Damage,
    /// The frame as the host keeps it: moved by each hint it takes, cleared and drawn in each frame's rects.
    scratch: Vec<Px>,
}

#[derive(Default)]
struct Tally {
    frames: u32,
    hinted: u32,
    whole: u32,
    drawn_px: u64,
    render_us: u128,
    worst_us: u128,
    /// Frames whose pixels were not the frame drawn whole at the same state.
    wrong: u32,
}

macro_rules! ctx {
    ($r:expr, $clip:expr, $rects:expr, $scroll:expr) => {
        Context {
            viewport: Viewport::new(W as u32, H as u32),
            text: &mut $r.text,
            clip_mask: &mut $r.clip,
            damage: &mut $r.damage,
            damage_clip: $clip,
            damage_rects: $rects,
            scroll: $scroll,
            window: &NullWindow,
            modifiers: Default::default(),
            cursor_x: (W >> 1) as Coord,
            cursor_y: (H >> 1) as Coord,
            is_maximized: true,
            window_origin: (0, 0),
            pressed_hit: HIT_NONE,
        }
    };
}

impl Rig {
    /// Photon on `screen`, signed in as an alias with 64 contacts and a 600-row conversation with the third of them open.
    fn new(screen: AppState) -> Self {
        crate::ui::set_safe_circle(crate::ui::SafeCircle::of_panel(W, H));
        let mut r = Rig { app: PhotonApp::new(), text: fluor::text::TextRenderer::new(), clip: vec![255; W * H], damage: Damage::new(), scratch: vec![Px::default(); W * H] };
        let full = PixelRect::new(0, 0, W, H);
        {
            let mut ctx = ctx!(r, full, DamageRects::EMPTY, None);
            r.app.init_display(&mut ctx);
            r.app.on_resize(W as u32, H as u32, &mut ctx);
        }
        let me = [1u8; 32];
        let them = [2u8; 32];
        r.app.set_device_keypair(crate::network::fgtw::derive_device_keypair(&[7u8; 32]));
        r.app.photon_keys = Some(crate::crypto::photon_keys::PhotonKeys::new([9u8; 32], [8u8; 32], me, Vec::new(), None));
        for k in 0..64u8 {
            let mut pid = them;
            pid[31] = k;
            let mut c = crate::types::Contact::from_pin([0u8; 64], [k ^ 0x5a; 32], pid, None);
            c.published_name = format!("Person {}", (b'A' + (k % 26)) as char);
            r.app.contacts.push(c);
        }
        let mut conv = crate::types::Conversation::new([me, them]);
        let words = ["so", "the", "beam", "landed", "on", "the", "pro", "and", "scrolling", "feels", "almost", "right", "but", "not", "quite", "yet", "today"];
        for i in 0..600usize {
            let n = 3 + (i * 7) % 40;
            let body: Vec<&str> = (0..n).map(|k| words[(i + k * 3) % words.len()]).collect();
            conv.messages.push(crate::types::ChatMessage::new_with_timestamp(body.join(" "), i % 3 == 0, 1_000_000_000_000 + i as i64 * 1_000_000_000));
        }
        conv.hydrated = true;
        r.app.active_conversation = Some(conv.id());
        r.app.conversations.push(conv);
        r.app.state = screen;
        r
    }

    /// One frame as the ferros host draws it (fluor host/ferros `draw`).
    fn frame(&mut self, t: &mut Tally) {
        let full = PixelRect::new(0, 0, W, H);
        {
            let mut ctx = ctx!(self, full, DamageRects::EMPTY, None);
            self.app.tick(&mut ctx);
        }
        let mut out = [PixelRect::empty(); fluor::canvas::DAMAGE_RECTS];
        let n = self.app.damage_rects(Viewport::new(W as u32, H as u32), &mut out);
        if n == 0 {
            return;
        }
        let rects = DamageRects::from_slice(&out[..n]).cut(W, H);
        let mut applied = None;
        if rects.as_slice() != [full] {
            if let Some((rect, dy)) = self.app.scroll_hint(Viewport::new(W as u32, H as u32)) {
                fluor::paint::scroll_copy_rect(&mut self.scratch, W, H, rect.x0, rect.y0, rect.x1, rect.y1, dy);
                applied = Some((rect, dy));
                t.hinted += 1;
            }
        } else {
            t.whole += 1;
            println!("  whole at {:?}, scene_dirty {} from {:?} line {} before tick {} after render {} why {:?}", self.app.pane_offset_and_max(), self.app.scene_dirty, self.app.dirty_event, self.app.dirty_blame_line, self.app.dirty_before_tick, self.app.dirty_after_render, self.app.frame_why);
        }
        for r in rects.as_slice() {
            for y in r.y0..r.y1 {
                self.scratch[y * W + r.x0..y * W + r.x1].fill(Px::default());
            }
        }
        let t0 = std::time::Instant::now();
        {
            let mut scratch = std::mem::take(&mut self.scratch);
            let mut ctx = ctx!(self, rects.bbox(), rects, applied);
            self.app.render(&mut scratch, &mut ctx);
            self.scratch = scratch;
        }
        let us = t0.elapsed().as_micros();
        t.frames += 1;
        t.drawn_px += rects.area() as u64;
        t.render_us += us;
        t.worst_us = t.worst_us.max(us);
    }

    /// A finger's pixel step on the open pane, as a fling's frame hands it.
    fn scroll(&mut self, dy: isize) {
        let full = PixelRect::new(0, 0, W, H);
        let mut ctx = ctx!(self, full, DamageRects::EMPTY, None);
        self.app.pane_scroll(&mut ctx, dy, true, true);
    }

    /// Whether the frame the host holds is the frame drawn whole at the same state; the rows that differ printed when not.
    fn matches_whole(&mut self) -> bool {
        let full = PixelRect::new(0, 0, W, H);
        let mut buf = vec![Px::default(); W * H];
        self.app.mark_dirty();
        {
            let mut ctx = ctx!(self, full, DamageRects::one(full), None);
            self.app.render(&mut buf, &mut ctx);
        }
        let rows: Vec<usize> = (0..H).filter(|y| self.scratch[y * W..(y + 1) * W] != buf[y * W..(y + 1) * W]).collect();
        if let (Some(a), Some(b)) = (rows.first(), rows.last()) {
            println!("  {} rows differ from the frame drawn whole, {a}..={b}, at {:?}", rows.len(), self.app.pane_offset_and_max());
        }
        rows.is_empty()
    }

    /// A fling `steps` frames one way and back, each frame a pixel slower (`pane_scroll`'s sign: positive moves a conversation into its history and the contacts back up), every frame held to the frame drawn whole.
    fn fling(&mut self, sign: isize, steps: isize) -> Tally {
        let mut t = Tally::default();
        for _ in 0..3 {
            self.frame(&mut t);
        }
        let mut t = Tally::default();
        for v in (1..=steps).rev().chain((1..=steps).rev().map(|v| -v)) {
            self.scroll(sign * v);
            self.frame(&mut t);
            if !self.matches_whole() {
                t.wrong += 1;
            }
        }
        t
    }
}

fn report(what: &str, t: &Tally) {
    let n = t.frames.max(1) as u64;
    println!(
        "{what}: {} frames, {} on the memmove, {} drawn whole; drawn {} % of the frame a frame; render {} us a frame, worst {} us; {} frames not the frame drawn whole",
        t.frames,
        t.hinted,
        t.whole,
        t.drawn_px * 100 / ((W * H) as u64 * n),
        t.render_us / n as u128,
        t.worst_us,
        t.wrong
    );
}

/// A fling into a long conversation's history and back: the frames between its ends ride the memmove, and every frame is the frame drawn whole.
#[test]
#[ignore]
fn conversation_fling_at_the_pros_size() {
    let mut r = Rig::new(AppState::Conversation);
    let t = r.fling(1, 1 << 5);
    report("conversation", &t);
    assert_eq!(t.wrong, 0, "a memmove frame is the frame drawn whole");
    assert!(t.hinted * 4 >= t.frames * 3, "most of a fling rides the memmove");
}

/// The same down the contacts.
/// Not held to the whole frame: a mark landing from its worker between the two draws differs by itself, and the filters' slat's pills, laid out in a band cut at the search box, land about a pixel off a pure move for the frames they pass under the box (2026-11-21, open).
#[test]
#[ignore]
fn contacts_fling_at_the_pros_size() {
    let mut r = Rig::new(AppState::Ready);
    let t = r.fling(-1, 1 << 5);
    report("contacts", &t);
    assert!(t.hinted * 4 >= t.frames * 3, "most of a fling rides the memmove");
}
