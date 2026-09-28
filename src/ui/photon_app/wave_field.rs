//! THE WAVE FIELD (Nick 2026-09-28): on the active wave screen, every 5 ms frame of audio ripples out of the avatar that made it.
//!
//! A square at the top of the screen holds both avatars, theirs a third in from the top-right, ours a third in from the bottom-left, each a quarter of the square across.
//! Each pixel's AUDIO AGE is its SQUARED distance from an avatar's edge over a constant: no square root anywhere, and every frame of audio gets the same screen AREA, so the newest seconds sit wide near the avatar and older ones pack toward the edges.
//! The ages are precomputed per layout (integer, doubled coordinates); a paint is two table lookups per pixel.
//! A frame's colour is the kept card's colour (`agb_bytes`, the same function), its brightness is its level in stops below full scale; the two sides ADD in linear light where their ripples overlap.
//! The field composites UNDER everything painted before it (fluor paints front to back), so the avatars, rings and text drawn first stay on top.

use super::*;
use crate::wave::live::{FrameEnv, FIELD_FRAMES};

/// The brightness scale's range: a frame this many stops below full scale is dark (a dozen stops).
const FIELD_STOPS: f32 = 12.0;
/// Linear-light quantization of the per-age tables (Q12).
const Q: usize = 4096;
/// Table index of the always-empty entry: a pixel inside an avatar, or past the history, looks this up.
const NONE: u16 = FIELD_FRAMES as u16;

/// Where the field sits, in buffer pixels.
#[derive(Clone, Copy, Debug)]
pub(super) struct FieldGeom {
    pub x0: usize,
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
        x0,
        y0,
        side,
        r: s / 8.0,
        theirs: (x0 as f32 + s * 2.0 / 3.0, y0 as f32 + s / 3.0),
        ours: (x0 as f32 + s / 3.0, y0 as f32 + s * 2.0 / 3.0),
    }
}

/// Per-pixel audio ages for both avatars, built once per square size.
pub(super) struct FieldMap {
    pub side: usize,
    age_theirs: Vec<u16>,
    age_ours: Vec<u16>,
}

impl FieldMap {
    pub fn build(side: usize) -> Self {
        // Doubled integer coordinates: a pixel centre is 2x+1, so every distance below is exact integer arithmetic.
        let s2 = 2 * side as i64;
        let (a, b) = ((2 * s2 + 1) / 3, (s2 + 1) / 3); // 2/3 and 1/3 of the doubled side, rounded
        let r2 = (s2 / 8) * (s2 / 8); // radius s/8 → doubled s/4 = s2/8, squared
        // The farthest corner from either centre sets the history scale; the layout is symmetric, so one scale serves both.
        let far = |cx: i64, cy: i64| [(0, 0), (s2, 0), (0, s2), (s2, s2)].iter().map(|&(x, y)| (x - cx) * (x - cx) + (y - cy) * (y - cy)).max().unwrap_or(0);
        let dmax2 = far(a, b).max(far(b, a));
        // Equal AREA per frame: age = (d² − r²) / k. PROOF: dmax2 > r2 for any side ≥ 16, and k ≥ 1 keeps the division defined.
        let k = ((dmax2 - r2) / FIELD_FRAMES as i64).max(1);
        let ages = |cx: i64, cy: i64| -> Vec<u16> {
            let mut v = Vec::with_capacity(side * side);
            for y in 0..side as i64 {
                let dy = 2 * y + 1 - cy;
                for x in 0..side as i64 {
                    let dx = 2 * x + 1 - cx;
                    let d2 = dx * dx + dy * dy;
                    // PROOF: 0 ≤ age ≤ FIELD_FRAMES = 1024 = NONE < 2^16.
                    v.push(if d2 < r2 { NONE } else { ((d2 - r2) / k).min(NONE as i64) as u16 });
                }
            }
            v
        };
        FieldMap { side, age_theirs: ages(a, b), age_ours: ages(b, a) }
    }
}

/// One paint's colour tables (linear light, display primaries, Q12) and the two live levels.
#[derive(Default)]
pub(super) struct FieldTables {
    theirs: Vec<[u16; 3]>,
    ours: Vec<[u16; 3]>,
    /// Newest amplitude, linear fraction of full scale (RMS of the frame).
    pub level_theirs: f32,
    pub level_ours: f32,
}

impl FieldTables {
    /// Fill from a live snapshot: RX is theirs, TX is ours.
    pub fn fill(&mut self, rx: &[FrameEnv], tx: &[FrameEnv]) {
        fill_side(&mut self.theirs, rx);
        fill_side(&mut self.ours, tx);
        self.level_theirs = rx.first().map_or(0.0, amplitude);
        self.level_ours = tx.first().map_or(0.0, amplitude);
    }
}

fn amplitude(e: &FrameEnv) -> f32 {
    if e.fno == i64::MIN { 0.0 } else { e.p[0].max(0.0).sqrt() }
}

fn fill_side(out: &mut Vec<[u16; 3]>, frames: &[FrameEnv]) {
    out.clear();
    out.extend(frames.iter().take(FIELD_FRAMES).map(frame_lin));
    out.resize(FIELD_FRAMES + 1, [0; 3]); // the last entry is NONE's: always empty
}

/// One frame → linear light in display primaries, Q12: the card's hue at the frame's stop-scaled brightness.
fn frame_lin(e: &FrameEnv) -> [u16; 3] {
    if e.fno == i64::MIN || e.p[0] <= 0.0 {
        return [0; 3];
    }
    let hue = super::render::agb_bytes([e.p[1], e.p[2], e.p[3]]);
    // Stops below full scale: amplitude = √power, so stops = −½·log₂(power).
    let stops = -0.5 * e.p[0].log2();
    let b = (1.0 - stops / FIELD_STOPS).clamp(0.0, 1.0);
    let lin = hue.map(|c| {
        let v = c as f32 / 255.0;
        v * v * b * b // γ2 decode of hue and brightness alike
    });
    #[cfg(not(target_os = "macos"))]
    let lin = vsf::colour::convert::apply_matrix_3x3_f32(&vsf::colour::VSF_RGB2REC2020, &lin);
    lin.map(|v| (v.clamp(0.0, 1.0) * (Q - 1) as f32).round() as u16)
}

/// Linear Q12 → stored darkness byte, already in the platform's channel position: γ2 encode (√), then fluor's byte order and darkness flip. One table per channel so a pixel is three lookups and two ORs.
static ENC: std::sync::LazyLock<[Vec<u32>; 3]> = std::sync::LazyLock::new(|| {
    let byte = |v: usize| ((v as f32 / (Q - 1) as f32).sqrt() * 255.0).round() as u32;
    let chan = |shift: u32| (0..Q).map(|v| fluor::theme::fmt((255 - byte(v)) << shift) & 0x00FF_FFFF).collect::<Vec<u32>>();
    [chan(16), chan(8), chan(0)]
});

/// Paint the field under whatever is already on the canvas.
pub(super) fn paint_field(canvas: &mut Canvas, g: &FieldGeom, map: &FieldMap, t: &FieldTables) {
    use rayon::prelude::*;
    if map.side != g.side || t.theirs.len() != FIELD_FRAMES + 1 || t.ours.len() != FIELD_FRAMES + 1 {
        return;
    }
    let width = canvas.width;
    if g.x0 >= width || g.y0 >= canvas.height {
        return;
    }
    let cols = g.side.min(width - g.x0);
    let rows = g.side.min(canvas.height - g.y0);
    let enc = &*ENC;
    let lim = (Q - 1) as u16;
    canvas.pixels[g.y0 * width..(g.y0 + rows) * width].par_chunks_mut(width).enumerate().for_each(|(y, line)| {
        let mut row = vec![0u32; cols];
        let base = y * g.side;
        for (x, px) in row.iter_mut().enumerate() {
            let a = t.theirs[map.age_theirs[base + x] as usize];
            let b = t.ours[map.age_ours[base + x] as usize];
            let (r, gr, bl) = ((a[0] + b[0]).min(lim), (a[1] + b[1]).min(lim), (a[2] + b[2]).min(lim));
            // Silence is transparent: the background shows thru where nobody's sound reaches.
            if r | gr | bl != 0 {
                *px = 0xFF00_0000 | enc[0][r as usize] | enc[1][gr as usize] | enc[2][bl as usize];
            }
        }
        fluor::paint::flatten(&mut line[g.x0..g.x0 + cols], &row, fluor::BlendMode::Normal);
    });
    canvas.damage.add_bounds(g.x0, g.y0, g.x0 + cols, g.y0 + rows);
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
        let m = FieldMap::build(side);
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

    /// Cost of one paint of a phone-sized field (run with --release --ignored --nocapture).
    #[test]
    #[ignore]
    fn paint_cost_on_a_phone_sized_square() {
        let side = 1080usize;
        let t0 = std::time::Instant::now();
        let map = FieldMap::build(side);
        let build = t0.elapsed();
        let frames: Vec<FrameEnv> = (0..FIELD_FRAMES as i64).map(|i| FrameEnv { fno: 1000 - i, p: [0.01 * ((i % 7) as f32 + 1.0) / 8.0, 0.002, 0.001, 0.0005] }).collect();
        let mut t = FieldTables::default();
        t.fill(&frames, &frames);
        let (w, h) = (side, side + 900);
        let mut px = vec![0u32; w * h];
        let mut dmg = fluor::canvas::Damage::new();
        let g = FieldGeom { x0: 0, y0: 0, side, r: side as f32 / 8.0, theirs: (0.0, 0.0), ours: (0.0, 0.0) };
        let n = 60;
        let t1 = std::time::Instant::now();
        for _ in 0..n {
            px.iter_mut().for_each(|p| *p = 0);
            let mut c = Canvas::new(&mut px, w, h, &mut dmg);
            paint_field(&mut c, &g, &map, &t);
        }
        let per = t1.elapsed() / n;
        eprintln!("field {side}²: map build {build:?}, paint {per:?} per frame");
    }

    #[test]
    fn level_colour_follows_the_linear_rule() {
        assert_eq!(level_colour(0.25), theme::rgb_colour(0, 255, 0));
        assert_eq!(level_colour(0.5), theme::rgb_colour(255, 255, 0));
        assert_eq!(level_colour(1.0), theme::rgb_colour(255, 0, 0));
    }
}
