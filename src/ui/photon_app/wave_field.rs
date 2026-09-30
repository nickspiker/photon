//! THE WAVE FIELD (Nick 2026-09-28): on the active wave screen, every 5 ms frame of audio ripples out of the avatar that made it.
//!
//! Both avatars sit in a square at the top of the screen, theirs a third in from the top-right, ours a third in from the bottom-left, each a quarter of the square across; the square is 24 layout units (ru) wide, so the whole layout scales with the interface like everything else.
//! Each ripple reaches exactly as far as the OTHER avatar (Nick 2026-09-28): from my avatar's edge to theirs is ONE SECOND of audio, whatever the screen, and nothing is painted past it.
//! Each pixel's AUDIO AGE is its SQUARED distance from the avatar over a constant: no square root anywhere, and every frame of audio gets the same screen AREA, so the newest audio sits wide near the avatar and older frames pack toward the reach.
//! The ages are precomputed per layout in 8.8 fixed point (integer, doubled coordinates), and a pixel samples BETWEEN its two neighbouring frames by the fraction — no frame-edge steps however few pixels a frame gets.
//! A frame's colour is the kept card's colour (`agb_bytes`, the same function) at full brightness, scaled 1:1 by its AMPLITUDE in linear light (Nick 2026-09-28: no log scale, the γ encode makes displayed brightness match amplitude), times a LINEAR fade to nothing at the reach; the two sides ADD in linear light where their ripples overlap.
//! Each pixel is one translucent layer: the colour at FULL brightness, its darkness premultiplied by α, and α the brightness γ-encoded — so silence is transparent. It composites UNDER everything painted before it (fluor paints front to back), so the avatars, rings, text and buttons stay on top, and the speckled background, painted after it, lands under it.

use super::*;
use crate::wave::live::{FrameEnv, FIELD_FRAMES};

/// Linear-light quantization of the per-age tables (Q12).
const Q: usize = 4096;
/// Fraction bits of a stored age.
const FRAC: u32 = 8;
/// The stored age of "no audio here": inside an avatar or past the reach. PROOF: FIELD_FRAMES = 200, so 200·256 = 51 200 < 2^16.
const NONE: u16 = (FIELD_FRAMES << FRAC) as u16;
/// The square's width in layout units: avatar radius 3 units, the two centres 8 units apart on each axis.
const SQUARE_UNITS: f32 = 24.0;

/// Where the field sits, in buffer pixels.
#[derive(Clone, Copy, Debug)]
pub(super) struct FieldGeom {
    pub w: usize,
    pub h: usize,
    pub y0: usize,
    pub side: usize,
    /// Avatar radius (a quarter of the side across).
    pub r: f32,
    pub theirs: (f32, f32),
    pub ours: (f32, f32),
}

/// The avatars' square: SQUARE_UNITS layout units under the top inset, centred — narrowed only when the screen cannot hold it above the three text lines and the wave's button rows (`unit` = the layout unit the wave screen scales from).
pub(super) fn field_geom(buf_w: usize, buf_h: usize, unit: f32) -> FieldGeom {
    // Under the status bar (max rule: the margin or the bar, whichever is taller), and clear of the navigation bar at the bottom.
    let top = (unit * 0.4).max(crate::ui::safe_top_px() as f32);
    // The wave screen's bottom stack (the diagonal button row) and the name and status lines take about 14.5 units, plus whatever of the navigation bar the button margin does not already cover.
    let nav_extra = (crate::ui::safe_bottom_px() as f32 - unit * 1.5).max(0.0);
    let room = buf_h as f32 - top - unit * 14.5 - nav_extra;
    // WHY/PROOF: a square wider than the buffer, or taller than the room above the text and buttons, would put an avatar off screen; the ru size stands wherever it fits, and the floor keeps a degenerate window drawable.
    let side = (unit * SQUARE_UNITS).min(buf_w as f32).min(room).max(16.0).floor() as usize;
    let x0 = buf_w.saturating_sub(side) / 2;
    let y0 = top as usize;
    let s = side as f32;
    FieldGeom {
        w: buf_w,
        h: buf_h,
        y0,
        side,
        r: s / 8.0,
        theirs: (x0 as f32 + s * 2.0 / 3.0, y0 as f32 + s / 3.0),
        ours: (x0 as f32 + s / 3.0, y0 as f32 + s * 2.0 / 3.0),
    }
}

/// Per-pixel audio ages for both avatars over the box their reaches cover, with each row's covered span, built once per layout.
pub(super) struct FieldMap {
    /// The layout it was built for: (buffer width, height, square side, square top).
    pub key: (usize, usize, usize, usize),
    /// The box both reaches cover, clipped to the buffer.
    x0: usize,
    y0: usize,
    bw: usize,
    bh: usize,
    /// Per box row, the x span [lo, hi) (buffer columns) where either ripple can land; lo ≥ hi = none.
    spans: Vec<(u32, u32)>,
    age_theirs: Vec<u16>,
    age_ours: Vec<u16>,
}

impl FieldMap {
    pub fn build(g: &FieldGeom) -> Self {
        let (w, h) = (g.w as i64, g.h as i64);
        // Doubled integer coordinates: a pixel centre is 2x+1, so every distance below is exact integer arithmetic.
        let dbl = |v: f32| (2.0 * v).round() as i64;
        let (t, o) = ((dbl(g.theirs.0), dbl(g.theirs.1)), (dbl(g.ours.0), dbl(g.ours.1)));
        let rd = dbl(g.r);
        let r2 = rd * rd;
        // THE REACH: from one centre to the far avatar's near edge — centre distance minus a radius. One second of audio spans avatar edge to avatar edge.
        let (ex, ey) = (t.0 - o.0, t.1 - o.1);
        let cd = ((ex * ex + ey * ey) as f64).sqrt().round() as i64; // once per layout, never per pixel
        let reach = (cd - rd).max(rd + 1);
        let reach2 = reach * reach;
        // Equal AREA per frame, in 8.8 fixed point: age = (d² − r²)·FIELD_FRAMES·256 / (reach² − r²). PROOF: reach > r, so the divisor is ≥ 1.
        let span2 = reach2 - r2;
        let full = (FIELD_FRAMES as i64) << FRAC;
        // The box both reaches cover (in buffer pixels), clipped to the buffer.
        let px = |c: i64, dir: i64| (c + dir * reach).div_euclid(2);
        let x0 = px(t.0, -1).min(px(o.0, -1)).clamp(0, w);
        let x1 = (px(t.0, 1).max(px(o.0, 1)) + 1).clamp(0, w);
        let y0 = px(t.1, -1).min(px(o.1, -1)).clamp(0, h);
        let y1 = (px(t.1, 1).max(px(o.1, 1)) + 1).clamp(0, h);
        let (bw, bh) = ((x1 - x0) as usize, (y1 - y0) as usize);
        let mut age_theirs = Vec::with_capacity(bw * bh);
        let mut age_ours = Vec::with_capacity(bw * bh);
        let mut spans = Vec::with_capacity(bh);
        let age = |(cx, cy): (i64, i64), x: i64, y: i64| -> u16 {
            let (dx, dy) = (2 * x + 1 - cx, 2 * y + 1 - cy);
            let d2 = dx * dx + dy * dy;
            // PROOF: r² ≤ d² < reach² ⇒ 0 ≤ age < full = NONE < 2^16.
            if d2 < r2 || d2 >= reach2 { NONE } else { ((d2 - r2) * full / span2) as u16 }
        };
        for y in y0..y1 {
            let (mut lo, mut hi) = (u32::MAX, 0u32);
            for x in x0..x1 {
                let (a, b) = (age(t, x, y), age(o, x, y));
                if a != NONE || b != NONE {
                    lo = lo.min(x as u32);
                    hi = x as u32 + 1;
                }
                age_theirs.push(a);
                age_ours.push(b);
            }
            spans.push((lo, hi));
        }
        FieldMap { key: (g.w, g.h, g.side, g.y0), x0: x0 as usize, y0: y0 as usize, bw, bh, spans, age_theirs, age_ours }
    }
}

/// Colour-cache ring size: a frame's colour is computed ONCE, when it first appears, into the slot its frame number masks to (Nick 2026-09-28: "write to it like a ring buffer so only our new samples overwrite the old"); a power of two past the shown second.
const CACHE: usize = 256;

/// One paint's colour tables (linear light, display primaries, Q12) and the two live levels.
#[derive(Default)]
pub(super) struct FieldTables {
    theirs: Vec<[u16; 3]>,
    ours: Vec<[u16; 3]>,
    /// The colour cache per side: (frame number, colour) by frame number masked to CACHE.
    cache_theirs: Vec<(i64, [u16; 3])>,
    cache_ours: Vec<(i64, [u16; 3])>,
}

impl FieldTables {
    /// Fill from a live snapshot: RX is theirs, TX is ours.
    pub fn fill(&mut self, rx: &[FrameEnv], tx: &[FrameEnv]) {
        fill_side(&mut self.theirs, &mut self.cache_theirs, rx);
        fill_side(&mut self.ours, &mut self.cache_ours, tx);
    }

    /// An avatar ring's colour (Nick 2026-09-28): the field's own colour at AGE 0 — the newest audio, as the ripple leaves the avatar — as a straight fluor colour, the hue at full brightness with α its γ-encoded brightness (the same pixel the field paints there, un-premultiplied for `draw_circle`). Silence is transparent.
    pub fn ring_colour(&self, theirs: bool) -> u32 {
        let Some(&[r, g, b]) = (if theirs { &self.theirs } else { &self.ours }).first() else {
            return 0;
        };
        let enc = &*ENC;
        let a = enc[r.max(g).max(b) as usize];
        if a == 0 {
            return 0;
        }
        // Full-brightness hue byte = ENC[c]·255/α (the brightest channel reads 255).
        let hue = |c: u16| (enc[c as usize] * 255 + a / 2) / a;
        let visible = (hue(r) << 16) | (hue(g) << 8) | hue(b);
        (a << 24) | (fluor::theme::fmt(visible ^ 0x00FF_FFFF) & 0x00FF_FFFF)
    }
}

fn amplitude(e: &FrameEnv) -> f32 {
    if e.fno == i64::MIN { 0.0 } else { e.p[0].max(0.0).sqrt() }
}

/// Lay one side's second out in age order for the per-pixel lookup, faded linearly to nothing at the reach (age i keeps (FIELD_FRAMES − i)/FIELD_FRAMES of its light). Each frame's colour comes from the cache ring — computed only the first time its frame number is seen — so a paint costs a copy of FIELD_FRAMES entries, not FIELD_FRAMES colour conversions; the per-pixel read stays two plain indexes (cheaper than an add-and-wrap at every pixel).
fn fill_side(out: &mut Vec<[u16; 3]>, cache: &mut Vec<(i64, [u16; 3])>, frames: &[FrameEnv]) {
    if cache.len() != CACHE {
        *cache = vec![(i64::MIN, [0; 3]); CACHE];
    }
    out.clear();
    out.extend(frames.iter().take(FIELD_FRAMES).enumerate().map(|(i, e)| {
        if e.fno == i64::MIN {
            return [0; 3];
        }
        // PROOF: CACHE is a power of two and rem_euclid is non-negative, so the slot is in range for any frame number.
        let slot = &mut cache[e.fno.rem_euclid(CACHE as i64) as usize];
        if slot.0 != e.fno {
            *slot = (e.fno, frame_lin(e));
        }
        let keep = (FIELD_FRAMES - i) as u32; // PROOF: i < FIELD_FRAMES, so keep ∈ [1, FIELD_FRAMES]
        slot.1.map(|c| (c as u32 * keep / FIELD_FRAMES as u32) as u16)
    }));
    out.resize(FIELD_FRAMES + 2, [0; 3]); // entries FIELD_FRAMES and past it are empty: the reach (NONE) and the interpolation's upper neighbour
}

/// One frame → linear light in display primaries, Q12: the card's hue at full brightness, times the frame's amplitude (linear, 1:1 — the γ2 encode at store time makes displayed brightness track amplitude).
fn frame_lin(e: &FrameEnv) -> [u16; 3] {
    let amp = amplitude(e).min(1.0);
    if amp <= 0.0 {
        return [0; 3];
    }
    let hue = super::render::agb_bytes([e.p[1], e.p[2], e.p[3]]);
    let lin = hue.map(|c| {
        let v = c as f32 / 255.0;
        v * v * amp // γ2 decode of the hue; amplitude is already linear
    });
    #[cfg(not(target_os = "macos"))]
    let lin = vsf::colour::convert::apply_matrix_3x3_f32(&vsf::colour::VSF_RGB2REC2020, &lin);
    lin.map(|v| (v.clamp(0.0, 1.0) * (Q - 1) as f32).round() as u16)
}

/// Linear Q12 → γ2-encoded byte (√): the visible level a linear light displays as.
static ENC: std::sync::LazyLock<Vec<u32>> = std::sync::LazyLock::new(|| (0..Q).map(|v| ((v as f32 / (Q - 1) as f32).sqrt() * 255.0).round() as u32).collect());

/// One field pixel as a fluor layer (Nick 2026-09-28): the colour at FULL brightness with its darkness premultiplied by α, and α = the brightest channel γ-encoded.
/// PROOF of the shortcut: full-brightness hue byte = √(c/m)·255 and α = √m·255, so the premultiplied visible level hue·α/255 = √c·255 = ENC[c], and premultiplied darkness = α − ENC[c] — no division per pixel, and ENC[c] ≤ ENC[m] = α keeps every channel's darkness within its opacity.
#[inline]
fn field_pixel(enc: &[u32], r: u16, g: u16, b: u16) -> u32 {
    let a = enc[r.max(g).max(b) as usize];
    let dark = ((a - enc[r as usize]) << 16) | ((a - enc[g as usize]) << 8) | (a - enc[b as usize]);
    (a << 24) | (fluor::theme::fmt(dark) & 0x00FF_FFFF)
}

/// A premultiplied layer composited under the partial composite `top`: each byte gains the layer's byte scaled by the opacity still open above it. PROOF: every byte of `bot` ≤ its α ≤ 255 and (256 − top_α)·255 >> 8 ≤ 255 − top_α, so each result stays ≤ 255 (and darkness ≤ α holds wherever it held in both).
#[inline]
fn under_premult(top: u32, bot: u32) -> u32 {
    let open = 256 - (top >> 24);
    let ch = |s: u32| ((top >> s) & 0xFF) + ((((bot >> s) & 0xFF) * open) >> 8);
    (ch(24) << 24) | (ch(16) << 16) | (ch(8) << 8) | ch(0)
}

/// Field paint timing, logged every PAINT_LOG_EVERY paints (a count of paints, not a clock): what the field costs on this device.
static PAINT_NS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
static PAINTS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
const PAINT_LOG_EVERY: u64 = 256;

/// Paint the field under whatever is already on the canvas.
pub(super) fn paint_field(canvas: &mut Canvas, g: &FieldGeom, map: &FieldMap, t: &FieldTables) {
    use std::sync::atomic::Ordering;
    let t0 = std::time::Instant::now();
    paint_field_inner(canvas, g, map, t);
    let e = t0.elapsed().as_nanos() as u64;
    let ns = PAINT_NS.fetch_add(e, Ordering::Relaxed) + e;
    let n = PAINTS.fetch_add(1, Ordering::Relaxed) + 1;
    if n % PAINT_LOG_EVERY == 0 {
        crate::logf!("WAVE: field paint — {:.2} ms per paint over the last {} ({}×{} px box, {} threads)", ns as f64 / PAINT_LOG_EVERY as f64 / 1e6, PAINT_LOG_EVERY, map.bw, map.bh, rayon::current_num_threads());
        PAINT_NS.store(0, Ordering::Relaxed);
    }
}

/// One side's colour at a stored age: linear between the two neighbouring frames by the age's fraction. PROOF: a stored age < NONE indexes ≤ FIELD_FRAMES − 1, so i + 1 ≤ FIELD_FRAMES < the table's FIELD_FRAMES + 2 entries; NONE indexes FIELD_FRAMES (empty) with fraction 0.
#[inline]
fn sample(t: &[[u16; 3]], age: u16) -> [u32; 3] {
    let (i, f) = ((age >> FRAC) as usize, (age & ((1 << FRAC) - 1)) as u32);
    let (a, b) = (t[i], t[i + 1]);
    let mix = |c: usize| (a[c] as u32 * (256 - f) + b[c] as u32 * f) >> FRAC;
    [mix(0), mix(1), mix(2)]
}

fn paint_field_inner(canvas: &mut Canvas, g: &FieldGeom, map: &FieldMap, t: &FieldTables) {
    use rayon::prelude::*;
    let (w, h) = (canvas.width, canvas.height);
    if map.key != (w, h, g.side, g.y0) || t.theirs.len() != FIELD_FRAMES + 2 || t.ours.len() != FIELD_FRAMES + 2 || map.bw == 0 {
        return;
    }
    let enc = &*ENC;
    let lim = (Q - 1) as u32;
    let (bx0, by0, bw) = (map.x0, map.y0, map.bw);
    // One task per box row, each pixel composited in place (field 2026-09-28: a per-row buffer plus a nested parallel flatten per row cost ~20 ms a paint on a phone). Only the row's covered span is visited: past the reach the fade has reached zero, so there is nothing to paint.
    // Fast paths: an opaque pixel above hides the field; silence adds nothing; nothing above (0) takes the field pixel as is; only the anti-aliased edges of what is above run the full under-blend.
    canvas.pixels[by0 * w..(by0 + map.bh) * w].par_chunks_mut(w).enumerate().for_each(|(y, line)| {
        let (lo, hi) = map.spans[y];
        if lo >= hi {
            return;
        }
        let (lo, hi) = (lo as usize, hi as usize);
        // PROOF: every span lies inside the box, so bx0 ≤ lo ≤ x < hi ≤ bx0 + bw indexes this box row.
        let (ages_t, ages_o) = (&map.age_theirs[y * bw..(y + 1) * bw], &map.age_ours[y * bw..(y + 1) * bw]);
        for x in lo..hi {
            let dst = &mut line[x];
            let d = *dst;
            if d >= 0xFF00_0000 {
                continue;
            }
            let a = sample(&t.theirs, ages_t[x - bx0]);
            let b = sample(&t.ours, ages_o[x - bx0]);
            let (r, gr, bl) = ((a[0] + b[0]).min(lim), (a[1] + b[1]).min(lim), (a[2] + b[2]).min(lim));
            if r | gr | bl == 0 {
                continue;
            }
            let src = field_pixel(enc, r as u16, gr as u16, bl as u16);
            // `src` is already premultiplied, and `under` would multiply it by its α again (a light fringe at every anti-aliased edge above the field): deposit it into the opacity left, every channel α included.
            *dst = if d == 0 { src } else { under_premult(d, src) };
        }
    });
    canvas.damage.add_bounds(bx0, by0, bx0 + bw, by0 + map.bh);
}

/// Is this global IPv6 peer on our own /64? Asks the OS which source address it would send from — a connected UDP socket, no packet leaves — and compares prefixes. Cached per peer address, so a render reads it without a syscall.
fn v6_same_link(peer: std::net::Ipv6Addr) -> bool {
    static CACHE: std::sync::Mutex<Option<(std::net::Ipv6Addr, bool)>> = std::sync::Mutex::new(None);
    let mut c = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((p, v)) = *c {
        if p == peer {
            return v;
        }
    }
    let same = std::net::UdpSocket::bind("[::]:0")
        .and_then(|s| s.connect((peer, 9)).and_then(|_| s.local_addr()))
        .is_ok_and(|l| matches!(l.ip(), std::net::IpAddr::V6(ours) if ours.segments()[..4] == peer.segments()[..4]));
    *c = Some((peer, same));
    same
}

impl PhotonApp {
    /// THE PATH COLOUR (Nick 2026-09-11): the path the wave is actually on — cyan the same LAN, blue radio-direct, green across the internet, amber while the engine waits on the sentinel with no direct path — and the contact's own tier while it still rings. During an Active wave it fills the top-left orb (Nick 2026-09-28); the avatar rings carry the live level instead.
    pub(super) fn wave_path_colour(&self, pi: Option<usize>) -> u32 {
        match crate::wave::wave_tx_addr() {
            Some(a) if a != crate::network::status::RELAY_ADDR => super::ring_colour_of(match a.ip().to_canonical() {
                std::net::IpAddr::V4(v4) if crate::network::traverse::gather::is_wfd_subnet(v4) => super::ConnTier::Wfd,
                std::net::IpAddr::V4(v4) if crate::network::traverse::gather::is_private_ipv4(v4) => super::ConnTier::Lan,
                // An IPv6 peer on OUR /64 is the same LAN (field 2026-09-12: a same-room wave ran on the router's global v6 at 10 ms and read green). OUR /64 is the source address the OS would send from (field 2026-09-28: the learned reflexive was v4 on one phone, so the same wave read cyan one way and green the other).
                std::net::IpAddr::V6(v6) if v6_same_link(v6) || self.our_reflexive.map_or(false, |o| matches!(o.ip().to_canonical(), std::net::IpAddr::V6(ours) if ours.segments()[..4] == v6.segments()[..4])) => super::ConnTier::Lan,
                _ => super::ConnTier::Wan,
            }),
            Some(_) => super::ring_colour_of(super::ConnTier::Relay),
            None => pi.map(|i| super::ring_tier_colour(&self.contacts[i], true)).unwrap_or(super::ring_colour_of(super::ConnTier::Relay)),
        }
    }

    /// Is anyone looking? The field (and its every-tick repaint) freezes when the window is unfocused or hidden, or the phone's display is off — the proximity blank at the ear included (Nick 2026-09-28: "freeze the animation to save CPU").
    pub(super) fn wave_field_watched(&self) -> bool {
        #[cfg(target_os = "android")]
        {
            crate::platform::jni_android::app_in_foreground() && crate::platform::jni_android::display_on()
        }
        #[cfg(not(target_os = "android"))]
        {
            crate::platform::desktop_notify::window_attended()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Ages grow outward from each avatar's edge to the reach (the other avatar's near edge), interiors and everything past the reach read NONE, and the equal-area law holds (frames thin with distance).
    #[test]
    fn ages_run_one_second_from_edge_to_the_other_avatar() {
        let g = field_geom_for(240, 240); // r = 30, theirs (160, 80), ours (80, 160): centres ≈ 113 apart, reach ≈ 83
        let m = FieldMap::build(&g);
        let at = |v: &Vec<u16>, x: usize, y: usize| v[(y - m.y0) * m.bw + (x - m.x0)];
        assert_eq!(at(&m.age_theirs, 160, 80), NONE, "inside the avatar");
        // Walk from their centre toward ours along the diagonal: past their edge the age rises from ~0 to ~a second, and it is gone before our centre.
        let walk: Vec<u16> = (22..=58).map(|k| at(&m.age_theirs, 160 - k, 80 + k)).collect();
        assert!(walk.windows(2).all(|w| w[1] >= w[0] || w[1] == NONE), "ages rise outward: {walk:?}");
        assert!(walk[0] < NONE / 16, "the ring at the edge is the newest audio: {}", walk[0]);
        assert!(walk.iter().rev().find(|&&a| a != NONE).is_some_and(|&a| a > NONE / 8 * 7), "the reach holds the second-old audio");
        assert_eq!(at(&m.age_theirs, 80, 160), NONE, "nothing reaches past the other avatar");
        // Equal area: one pixel outward crosses MORE age near the reach than near the edge.
        let step = |k: usize| at(&m.age_theirs, 160 - k - 1, 80 + k + 1) as i32 - at(&m.age_theirs, 160 - k, 80 + k) as i32;
        assert!(step(23) < step(55), "near {} vs far {}", step(23), step(55));
        // Spans cover exactly the reached pixels: a far corner is outside every span.
        let (lo, hi) = m.spans[m.bh - 1];
        assert!(lo >= hi || (lo as usize) < m.x0 + m.bw && hi as usize <= m.x0 + m.bw);
    }

    /// The avatars' square at the top of a `w` × `h` buffer, as `field_geom` lays it out.
    fn field_geom_for(w: usize, h: usize) -> FieldGeom {
        let s = w.min(h) as f32;
        FieldGeom { w, h, y0: 0, side: w.min(h), r: s / 8.0, theirs: (s * 2.0 / 3.0, s / 3.0), ours: (s / 3.0, s * 2.0 / 3.0) }
    }

    /// Between two frames the colour is the blend of both by the age's fraction; the fade leaves the newest frame whole and the oldest nearly gone.
    #[test]
    fn sampling_interpolates_and_fades() {
        let frames: Vec<FrameEnv> = (0..FIELD_FRAMES as i64).map(|i| FrameEnv { fno: 900 - i, p: [0.25, 0.3, 0.2, 0.1] }).collect();
        let mut t = FieldTables::default();
        t.fill(&frames, &frames);
        let (f0, f1) = (sample(&t.theirs, 0), sample(&t.theirs, 1 << FRAC));
        let half = sample(&t.theirs, 1 << (FRAC - 1));
        for c in 0..3 {
            assert!(half[c] + 1 >= (f0[c] + f1[c]) / 2 && half[c] <= (f0[c] + f1[c]) / 2 + 1);
        }
        let last = sample(&t.theirs, ((FIELD_FRAMES - 1) << FRAC) as u16);
        assert!(last[0] * 100 < f0[0].max(1) * 2, "the oldest frame keeps ~1/{FIELD_FRAMES} of its light");
        assert_eq!(sample(&t.theirs, NONE), [0; 3]);
    }

    /// Silence paints nothing, nothing lands past the reach, and every lit pixel is a valid premultiplied layer (darkness within its opacity) at the colour's full brightness.
    #[test]
    fn field_is_a_premultiplied_layer_within_the_reach() {
        let (w, h) = (120usize, 260usize);
        let g = field_geom_for(w, h);
        let m = FieldMap::build(&g);
        assert_eq!(m.age_theirs.len(), m.bw * m.bh);
        let paint = |t: &FieldTables| {
            let mut px = vec![0u32; w * h];
            let mut dmg = fluor::canvas::Damage::new();
            paint_field(&mut Canvas::new(&mut px, w, h, &mut dmg), &g, &m, t);
            px
        };
        let mut silent = FieldTables::default();
        silent.fill(&[], &[]);
        assert!(paint(&silent).iter().all(|&p| p == 0), "silence is transparent");
        let frames: Vec<FrameEnv> = (0..FIELD_FRAMES as i64).map(|i| FrameEnv { fno: 5000 - i, p: [0.25, 0.3, 0.2, 0.1] }).collect();
        let mut loud = FieldTables::default();
        loud.fill(&frames, &frames);
        let lit = paint(&loud);
        assert!(lit.iter().all(|&p| [16, 8, 0].iter().all(|&s| (p >> s) & 0xFF <= p >> 24)), "darkness never exceeds opacity");
        assert!(lit.iter().filter(|&&p| p != 0).count() > w * w / 4, "the ripples fill the reach");
        assert!(lit[(h - 1) * w..].iter().all(|&p| p == 0), "nothing below the reach");
        // Full brightness: the brightest channel of a lit pixel carries no darkness (premultiplied visible level = α).
        let enc = &*ENC;
        let p = field_pixel(enc, 2000, 500, 100);
        assert_eq!([16, 8, 0].iter().map(|&s| (p >> s) & 0xFF).min(), Some(0));
        // Under a half-opaque pixel, the field deposits its premultiplied bytes into the half left open — once, not α twice.
        let top = 0x8040_4040u32;
        let out = under_premult(top, p);
        let open = 256 - 0x80;
        assert_eq!(out >> 24, 0x80 + ((p >> 24) * open >> 8));
        assert_eq!(out & 0xFF, 0x40 + ((p & 0xFF) * open >> 8));
        // The rings take the age-0 colour: straight, full-brightness hue (one channel carries no darkness), α its brightness; silence is transparent.
        let ring = loud.ring_colour(true);
        assert!(ring >> 24 > 0);
        assert_eq!([16, 8, 0].iter().map(|&s| (ring >> s) & 0xFF).min(), Some(0));
        assert_eq!(silent.ring_colour(false), 0);
    }

    /// Brightness is linear in amplitude: half the amplitude, half the linear light.
    #[test]
    fn brightness_is_linear_in_amplitude() {
        let f = |amp: f32| frame_lin(&FrameEnv { fno: 1, p: [amp * amp, 1.0, 1.0, 1.0] });
        let (full, half) = (f(1.0), f(0.5));
        for c in 0..3 {
            assert!((half[c] as i32 * 2 - full[c] as i32).abs() <= 2, "{half:?} vs {full:?}");
        }
    }

    /// Cost of one paint of a phone-sized field (run with --release --ignored --nocapture).
    #[test]
    #[ignore]
    fn paint_cost_on_a_phone_sized_screen() {
        let side = 1080usize;
        let (w, h) = (side, side + 1300);
        let g = field_geom_for(w, h);
        let t0 = std::time::Instant::now();
        let map = FieldMap::build(&g);
        let build = t0.elapsed();
        let frames: Vec<FrameEnv> = (0..FIELD_FRAMES as i64).map(|i| FrameEnv { fno: 1000 - i, p: [0.01 * ((i % 7) as f32 + 1.0) / 8.0, 0.002, 0.001, 0.0005] }).collect();
        let mut t = FieldTables::default();
        t.fill(&frames, &frames);
        let mut px = vec![0u32; w * h];
        let mut dmg = fluor::canvas::Damage::new();
        let n = 60;
        let t1 = std::time::Instant::now();
        for _ in 0..n {
            px.iter_mut().for_each(|p| *p = 0);
            let mut c = Canvas::new(&mut px, w, h, &mut dmg);
            paint_field(&mut c, &g, &map, &t);
        }
        let per = t1.elapsed() / n;
        eprintln!("field {w}×{h}: map build {build:?}, paint {per:?} per frame");
    }
}
