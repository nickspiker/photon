//! A desktop render rig at the Pro's panel (1344 × 2992 under the safe circle): a fling in a long conversation and down the contacts and a conversation left at rest, each frame drawn as fluor's ferros host draws it (the app's rects, the scroll hint taken unless the frame is one whole rect, the rects cleared, the render told the hint it rode), counted, and held to the frame drawn whole at the same state.
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
    /// Each drawn frame printed with its rects and the lines that asked for it.
    verbose: bool,
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
    /// Frames whose rows' tap bookkeeping (hit rows, link targets, wave bands, attachment visuals) was not the frame drawn whole's.
    stale: u32,
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
        let mut r = Rig { app: PhotonApp::new(), text: fluor::text::TextRenderer::new(), clip: vec![255; W * H], damage: Damage::new(), scratch: vec![Px::default(); W * H], verbose: false };
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
            // Every third row's second word a link, so the link targets of a frame drawn in part are held to the whole frame's too.
            if i % 3 == 0 {
                let m = conv.messages.last_mut().unwrap();
                let s = m.content.find(' ').unwrap() + 1;
                let len = m.content[s..].find(' ').unwrap_or(m.content.len() - s);
                m.marks.push(crate::types::MessageMark { kind: 1, start: s, len, dest: "https://example.org".to_string() });
            }
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
        if self.verbose {
            println!("  frame of {:?}, asks {:?}, dirty {} from {:?} line {}", rects.as_slice(), self.app.frame_why, self.app.scene_dirty, self.app.dirty_event, self.app.dirty_blame_line);
        }
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

    /// The conversation rows' tap bookkeeping the last frame left, as text.
    fn bookkeeping(&self) -> String {
        format!("{:?} {:?} {:?} {:?}", self.app.msg_hit_rows, self.app.msg_link_hits, self.app.msg_wave_bands, self.app.msg_attach_visuals)
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
            let kept = self.bookkeeping();
            if !self.matches_whole() {
                t.wrong += 1;
            }
            if kept != self.bookkeeping() {
                t.stale += 1;
            }
        }
        t
    }
}

fn report(what: &str, t: &Tally) {
    let n = t.frames.max(1) as u64;
    println!(
        "{what}: {} frames, {} on the memmove, {} drawn whole; drawn {} % of the frame a frame; render {} us a frame, worst {} us; {} frames not the frame drawn whole, {} with stale row bookkeeping",
        t.frames,
        t.hinted,
        t.whole,
        t.drawn_px * 100 / ((W * H) as u64 * n),
        t.render_us / n as u128,
        t.worst_us,
        t.wrong,
        t.stale
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

/// A conversation left alone, its window attended, at sixty frames a second for `secs` seconds, its newest row `age_secs` old and the ages in arabic (the Pro's base): the frames it drew, each held to the frame drawn whole.
fn idle(age_secs: i64, secs: u64) -> Tally {
    crate::platform::desktop_notify::set_window_visible(true);
    crate::platform::desktop_notify::set_window_focused(true);
    crate::set_num_base(crate::NumBase::Arabic);
    let mut r = Rig::new(AppState::Conversation);
    // The newest row stamped `age_secs` ago, the strip's unasked row.
    let now = vsf::eagle_time_oscillations();
    let conv = r.app.conversations.last_mut().unwrap();
    let n = conv.messages.len() as i64;
    for (i, m) in conv.messages.iter_mut().enumerate() {
        m.timestamp = now - (age_secs + (n - 1 - i as i64) * 60) * crate::OSC_PER_SEC;
    }
    let mut t = Tally::default();
    for _ in 0..8 {
        r.frame(&mut t);
    }
    let mut t = Tally::default();
    r.verbose = true;
    let t0 = std::time::Instant::now();
    while t0.elapsed() < std::time::Duration::from_secs(secs) {
        let before = t.frames;
        r.frame(&mut t);
        if t.frames != before {
            let links = r.app.msg_link_hits.clone();
            if !r.matches_whole() {
                t.wrong += 1;
            }
            // Each link's tap target where the whole frame puts it, to within a pixel (a culled run is measured, a drawn one reports its width).
            let near = |a: &(f32, f32, f32, f32, String), b: &(f32, f32, f32, f32, String)| (a.0 - b.0).abs() < 1. && (a.1 - b.1).abs() < 1. && (a.2 - b.2).abs() < 1. && (a.3 - b.3).abs() < 1. && a.4 == b.4;
            if links.is_empty() || links.len() != r.app.msg_link_hits.len() || !links.iter().zip(&r.app.msg_link_hits).all(|(a, b)| near(a, b)) {
                println!("  link targets differ: {} drawn in part, {} whole", links.len(), r.app.msg_link_hits.len());
                t.wrong += 1;
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(16));
    }
    t
}

/// A conversation whose newest row is a day old, left alone for three seconds: nothing on it changes, and it draws nothing.
/// Until 2026-11-22 the live age the tick watched was the row's age in whole seconds while the strip shows the largest whole unit ("1 d ago"), so on the Pro a conversation drew a whole frame every second at rest.
#[test]
#[ignore]
fn conversation_idle_at_the_pros_size() {
    let t = idle(86_400 + 7, 3);
    report("conversation idle, its newest row a day old", &t);
    assert_eq!(t.frames, 0, "an idle conversation draws nothing");
}

/// A conversation whose newest row is ten seconds old: its strip's age counts the seconds, one frame a second, each only the strip's box, and each the frame drawn whole.
#[test]
#[ignore]
fn conversation_live_age_at_the_pros_size() {
    let t = idle(10, 3);
    report("conversation idle, its newest row ten seconds old", &t);
    assert!(t.frames <= 4, "a frame a second at most");
    assert_eq!(t.whole, 0, "the age alone draws only its own box");
    assert_eq!(t.wrong, 0, "each frame is the frame drawn whole");
}
