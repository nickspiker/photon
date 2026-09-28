//! THE WAVE FIELD (Nick 2026-09-28): on the active wave screen, every 5 ms frame of audio ripples out of the avatar that made it.
//!
//! A square at the top of the screen holds both avatars, theirs a third in from the top-right, ours a third in from the bottom-left, each a quarter of the square across; the ripples fill the WHOLE screen (Nick 2026-09-28), under the text and the wave's buttons.
//! Each pixel's AUDIO AGE is its SQUARED distance from an avatar's edge over a constant: no square root anywhere, and every frame of audio gets the same screen AREA, so the newest seconds sit wide near the avatar and older ones pack toward the screen's far corner.
//! The ages are precomputed per layout (integer, doubled coordinates); a paint is two table lookups per pixel.
//! A frame's colour is the kept card's colour (`agb_bytes`, the same function) at full brightness, scaled 1:1 by its AMPLITUDE in linear light (Nick 2026-09-28: no log scale, the γ encode makes displayed brightness match amplitude); the two sides ADD in linear light where their ripples overlap.
//! Each pixel is one translucent layer: the colour at FULL brightness, its darkness premultiplied by α, and α the brightness γ-encoded — so silence is transparent. It composites UNDER everything painted before it (fluor paints front to back), so the avatars, rings, text and buttons stay on top, and the speckled background, painted after it, lands under it.

use super::*;
use crate::wave::live::{FrameEnv, FIELD_FRAMES};

/// Linear-light quantization of the per-age tables (Q12).
const Q: usize = 4096;
/// Table index of the always-empty entry: a pixel inside an avatar, or past the history, looks this up.
const NONE: u16 = FIELD_FRAMES as u16;

/// Where the field sits, in buffer pixels: the avatars' square, inside a field as big as the buffer.
#[derive(Clone, Copy, Debug)]
pub(super) struct FieldGeom {
    /// The whole buffer: the ripples cover all of it.
    pub w: usize,
    pub h: usize,
    pub y0: usize,
    pub side: usize,
    /// Avatar radius (a quarter of the side across).
    pub r: f32,
    pub theirs: (f32, f32),
    pub ours: (f32, f32),
}

/// The square: as wide as the screen allows under the top inset, above the three text lines and the wave's button rows (`unit` = the layout unit the wave screen scales from).
pub(super) fn field_geom(buf_w: usize, buf_h: usize, unit: f32) -> FieldGeom {
    let top = unit * 0.4;
    // The wave screen's bottom stack (primary, secondary and route rows) takes about 10 units; the name, status and stats lines about 4 more.
    let room = buf_h as f32 - top - unit * 14.5;
    let side = (buf_w as f32).min(room).max(16.0).floor() as usize;
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

/// Per-pixel audio ages for both avatars over the whole buffer, built once per layout.
pub(super) struct FieldMap {
    /// The layout it was built for: (buffer width, height, square side).
    pub key: (usize, usize, usize),
    age_theirs: Vec<u16>,
    age_ours: Vec<u16>,
}

impl FieldMap {
    pub fn build(g: &FieldGeom) -> Self {
        let (w, h) = (g.w, g.h);
        // Doubled integer coordinates: a pixel centre is 2x+1, so every distance below is exact integer arithmetic.
        let dbl = |v: f32| (2.0 * v).round() as i64;
        let (t, o) = ((dbl(g.theirs.0), dbl(g.theirs.1)), (dbl(g.ours.0), dbl(g.ours.1)));
        let r2 = dbl(g.r) * dbl(g.r);
        let (w2, h2) = (2 * w as i64, 2 * h as i64);
        // The farthest buffer corner from either centre sets the history scale, so the oldest frame just reaches it.
        let far = |(cx, cy): (i64, i64)| [(0, 0), (w2, 0), (0, h2), (w2, h2)].iter().map(|&(x, y)| (x - cx) * (x - cx) + (y - cy) * (y - cy)).max().unwrap_or(0);
        let dmax2 = far(t).max(far(o));
        // Equal AREA per frame: age = (d² − r²) / k. PROOF: k ≥ 1 keeps the division defined whatever the layout.
        let k = ((dmax2 - r2) / FIELD_FRAMES as i64).max(1);
        let ages = |(cx, cy): (i64, i64)| -> Vec<u16> {
            let mut v = Vec::with_capacity(w * h);
            for y in 0..h as i64 {
                let dy = 2 * y + 1 - cy;
                for x in 0..w as i64 {
                    let dx = 2 * x + 1 - cx;
                    let d2 = dx * dx + dy * dy;
                    // PROOF: 0 ≤ age ≤ FIELD_FRAMES = 1024 = NONE < 2^16.
                    v.push(if d2 < r2 { NONE } else { ((d2 - r2) / k).min(NONE as i64) as u16 });
                }
            }
            v
        };
        FieldMap { key: (w, h, g.side), age_theirs: ages(t), age_ours: ages(o) }
    }
}

/// Colour-cache ring size: a frame's colour is computed ONCE, when it first appears, into the slot its frame number masks to (Nick 2026-09-28: "write to it like a ring buffer so only our new samples overwrite the old"); a power of two past the shown history.
const CACHE: usize = FIELD_FRAMES * 2;

/// One paint's colour tables (linear light, display primaries, Q12) and the two live levels.
#[derive(Default)]
pub(super) struct FieldTables {
    theirs: Vec<[u16; 3]>,
    ours: Vec<[u16; 3]>,
    /// The colour cache per side: (frame number, colour) by frame number masked to CACHE.
    cache_theirs: Vec<(i64, [u16; 3])>,
    cache_ours: Vec<(i64, [u16; 3])>,
    /// Newest amplitude, linear fraction of full scale (RMS of the frame).
    pub level_theirs: f32,
    pub level_ours: f32,
}

impl FieldTables {
    /// Fill from a live snapshot: RX is theirs, TX is ours.
    pub fn fill(&mut self, rx: &[FrameEnv], tx: &[FrameEnv]) {
        fill_side(&mut self.theirs, &mut self.cache_theirs, rx);
        fill_side(&mut self.ours, &mut self.cache_ours, tx);
        self.level_theirs = rx.first().map_or(0.0, amplitude);
        self.level_ours = tx.first().map_or(0.0, amplitude);
    }
}

fn amplitude(e: &FrameEnv) -> f32 {
    if e.fno == i64::MIN { 0.0 } else { e.p[0].max(0.0).sqrt() }
}

/// Lay one side's history out in age order for the per-pixel lookup. Each frame's colour comes from the cache ring — computed only the first time its frame number is seen — so a paint costs a copy of FIELD_FRAMES entries, not FIELD_FRAMES colour conversions; the per-pixel read stays one plain index (cheaper than an add-and-wrap at every pixel).
fn fill_side(out: &mut Vec<[u16; 3]>, cache: &mut Vec<(i64, [u16; 3])>, frames: &[FrameEnv]) {
    if cache.len() != CACHE {
        *cache = vec![(i64::MIN, [0; 3]); CACHE];
    }
    out.clear();
    out.extend(frames.iter().take(FIELD_FRAMES).map(|e| {
        if e.fno == i64::MIN {
            return [0; 3];
        }
        // PROOF: CACHE is a power of two and rem_euclid is non-negative, so the slot is in range for any frame number.
        let slot = &mut cache[e.fno.rem_euclid(CACHE as i64) as usize];
        if slot.0 != e.fno {
            *slot = (e.fno, frame_lin(e));
        }
        slot.1
    }));
    out.resize(FIELD_FRAMES + 1, [0; 3]); // the last entry is NONE's: always empty
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
        crate::logf!("WAVE: field paint — {:.2} ms per paint over the last {} ({}² px, {} threads)", ns as f64 / PAINT_LOG_EVERY as f64 / 1e6, PAINT_LOG_EVERY, g.side, rayon::current_num_threads());
        PAINT_NS.store(0, Ordering::Relaxed);
    }
}

fn paint_field_inner(canvas: &mut Canvas, g: &FieldGeom, map: &FieldMap, t: &FieldTables) {
    use rayon::prelude::*;
    let (w, h) = (canvas.width, canvas.height);
    if map.key != (w, h, g.side) || t.theirs.len() != FIELD_FRAMES + 1 || t.ours.len() != FIELD_FRAMES + 1 {
        return;
    }
    use fluor::pixel::Blend;
    let enc = &*ENC;
    let lim = (Q - 1) as u16;
    // One task per row, each pixel composited in place (field 2026-09-28: a per-row buffer plus a nested parallel flatten per row cost ~20 ms a paint on a phone).
    // Fast paths: an opaque pixel above hides the field; silence adds nothing; nothing above (0) takes the field pixel as is; only the anti-aliased edges of what is above run the full under-blend.
    canvas.pixels[..w * h].par_chunks_mut(w).enumerate().for_each(|(y, line)| {
        let base = y * w;
        let ages_t = &map.age_theirs[base..base + w];
        let ages_o = &map.age_ours[base..base + w];
        for ((dst, &at), &ao) in line.iter_mut().zip(ages_t).zip(ages_o) {
            let d = *dst;
            if d >= 0xFF00_0000 {
                continue;
            }
            let a = t.theirs[at as usize];
            let b = t.ours[ao as usize];
            let (r, gr, bl) = ((a[0] + b[0]).min(lim), (a[1] + b[1]).min(lim), (a[2] + b[2]).min(lim));
            if r | gr | bl == 0 {
                continue;
            }
            let src = field_pixel(enc, r, gr, bl);
            *dst = if d == 0 { src } else { d.under(src, fluor::BlendMode::Normal) };
        }
    });
    canvas.damage.add_bounds(0, 0, w, h);
}

impl PhotonApp {
    /// THE PATH COLOUR (Nick 2026-09-11): the path the wave is actually on — cyan the same LAN, blue radio-direct, green across the internet, amber while the engine waits on the sentinel with no direct path — and the contact's own tier while it still rings. During an Active wave it fills the top-left orb (Nick 2026-09-28); the avatar rings carry the live level instead.
    pub(super) fn wave_path_colour(&self, pi: Option<usize>) -> u32 {
        match crate::wave::wave_tx_addr() {
            Some(a) if a != crate::network::status::RELAY_ADDR => super::ring_colour_of(match a.ip().to_canonical() {
                std::net::IpAddr::V4(v4) if crate::network::traverse::gather::is_wfd_subnet(v4) => super::ConnTier::Wfd,
                std::net::IpAddr::V4(v4) if crate::network::traverse::gather::is_private_ipv4(v4) => super::ConnTier::Lan,
                // An IPv6 peer on OUR /64 is the same LAN (field 2026-09-12: a same-room wave ran on the router's global v6 at 10 ms and read green).
                std::net::IpAddr::V6(v6) if self.our_reflexive.map_or(false, |o| matches!(o.ip().to_canonical(), std::net::IpAddr::V6(ours) if ours.segments()[..4] == v6.segments()[..4])) => super::ConnTier::Lan,
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

/// An avatar ring's colour from the live level (Nick): green below half of full scale, yellow at exactly half, blending linearly to red at full scale (clipping).
pub(super) fn level_colour(amp: f32) -> u32 {
    if amp < 0.5 {
        return theme::rgb_colour(0, 255, 0);
    }
    let t = ((amp - 0.5) * 2.0).clamp(0.0, 1.0);
    theme::rgb_colour(255, (255.0 * (1.0 - t)).round() as u8, 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Ages grow outward from each avatar, both avatars' interiors read NONE, the equal-area law holds (a frame's ring gets thinner farther out), and nothing reaches past the history.
    #[test]
    fn ages_grow_outward_in_equal_area_rings() {
        let side = 240usize;
        let m = FieldMap::build(&field_geom_for(side, side));
        let at = |v: &Vec<u16>, x: usize, y: usize| v[y * side + x];
        // Their centre (160, 80): inside reads NONE.
        assert_eq!(at(&m.age_theirs, 160, 80), NONE);
        assert_eq!(at(&m.age_ours, 80, 160), NONE);
        // Walking right from their avatar edge (r = 30): ages rise monotonically.
        let line: Vec<u16> = (191..240).map(|x| at(&m.age_theirs, x, 80)).collect();
        assert!(line.windows(2).all(|w| w[1] >= w[0]), "ages rise outward: {line:?}");
        assert!(line[0] < 5, "the ring touching the avatar is the newest audio");
        // Equal area: one pixel outward crosses MORE frames far from the avatar than near it (the rings thin with distance).
        let step = |x: usize| at(&m.age_theirs, x + 1, 80) as i32 - at(&m.age_theirs, x, 80) as i32;
        assert!(step(192) < step(237), "near {} frames/pixel vs far {}", step(192), step(237));
        assert!(m.age_theirs.iter().all(|&a| a <= NONE));
    }

    /// The avatars' square at the top of a `w` × `h` buffer, as `field_geom` lays it out.
    fn field_geom_for(w: usize, h: usize) -> FieldGeom {
        let s = w.min(h) as f32;
        FieldGeom { w, h, y0: 0, side: w.min(h), r: s / 8.0, theirs: (s * 2.0 / 3.0, s / 3.0), ours: (s / 3.0, s * 2.0 / 3.0) }
    }

    /// The field reaches the whole buffer (the far corner holds the oldest audio, never past it), silence paints nothing, and every lit pixel is a valid premultiplied layer (darkness within its opacity) at the colour's full brightness.
    #[test]
    fn full_screen_field_is_a_premultiplied_layer() {
        let (w, h) = (120usize, 260usize);
        let g = field_geom_for(w, h);
        let m = FieldMap::build(&g);
        assert_eq!(m.age_theirs.len(), w * h);
        let corner = m.age_ours[(h - 1) * w + (w - 1)].max(m.age_theirs[(h - 1) * w]);
        assert!(corner > (FIELD_FRAMES as u16) * 3 / 4 && corner <= NONE, "the far corner holds the oldest frames: {corner}");
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
        assert!(lit.iter().filter(|&&p| p != 0).count() > w * h / 2, "the ripples cover the screen");
        // Full brightness: the brightest channel of a lit pixel carries no darkness (premultiplied visible level = α).
        let enc = &*ENC;
        let p = field_pixel(enc, 2000, 500, 100);
        assert_eq!([16, 8, 0].iter().map(|&s| (p >> s) & 0xFF).min(), Some(0));
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

    #[test]
    fn level_colour_follows_the_linear_rule() {
        assert_eq!(level_colour(0.25), theme::rgb_colour(0, 255, 0));
        assert_eq!(level_colour(0.5), theme::rgb_colour(255, 255, 0));
        assert_eq!(level_colour(1.0), theme::rgb_colour(255, 0, 0));
    }
}
