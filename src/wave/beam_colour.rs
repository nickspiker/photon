//! The beam's receive-side colour chain (docs/beams.md §4), integer end to end — no float in the pixel path. The floats belong to the one-time matrix fold at the top of a frame.
//!
//! ```text
//! Y Cb Cr (8-bit, 4:2:0, 601 full range)
//!   │ 3×3 Q12 + offsets, truncate → R'G'B' codes (8-bit)
//!   │ the entry's transfer, a 256-entry table → light, Q16 (diffuse white = 65536, fluor's scale)
//!   │ ONE folded 3×3 in Q10 — (VSF RGB → Rec.2020) × (camera → VSF RGB) × diag(1/gains) — signed, i64, clamp after:
//!   │   at white for an SDR entry (γ2, sRGB), at the buffer's 9.7× for a g2t one (an HDR beam's light past white is kept)
//!   │ light → g2t code × 256 by table (fractional, for the dither)
//!   │ + blue-noise tile (x,y mod 64, origin hashed per frame), truncate → 8-bit
//!   ▼
//! R G B triples, g2t Rec.2020 — fluor's own buffer encoding (theme.rs), blitted as they are
//! ```
//!
//! Headroom: 8-bit × Q12 × 3 terms = 22 bits; light up to 9.7 × 2^16 (20 bits) × Q10 coefficients under 4.0 × 3 terms = 34 bits, so the matrix runs in i64. A `gamma2` entry (the phone's straight path) decodes by the square, `srgb` (a desktop camera, the phone's own colour) by the sRGB curve, `g2t` (an HDR beam, docs/beams.md) by fluor's `G2T_LIGHT_Q16`.
//!
//! The dither: unbiased truncation — the mean of ⌊x + u⌋ over uniform u is x — with a void-and-cluster blue-noise tile (all its energy high-frequency, the least visible noise for its amplitude) whose origin walks per frame so a still scene never shows a fixed pattern. Applied at the ONE place it belongs, the final 8-bit output. The 16-bit intermediate needs none. It hides OUR steps only; the codec's artifacts sit many codes high (docs/beams.md §5).

use super::beam::Gains;
use super::h264::I420;
use std::sync::OnceLock;

/// The transfer the encoded samples carry (vsf::spectral_image::Transfer, the three a beam uses).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Transfer {
    Gamma2,
    Srgb,
    /// γ2 with headroom (an HDR beam): white at code 194, light past it kept.
    G2t,
}

/// The camera's characterization as the receiver needs it: the camera→VSF-RGB matrix (row-major, linear in, linear out, unscaled) and the transfer its samples carry. Built from a `vsf::spectral_image::ProfileEntry`; identity + γ2 when a beam carries none yet.
#[derive(Debug, Clone, PartialEq)]
pub struct Characterization {
    pub matrix: [f32; 9],
    pub transfer: Transfer,
}

impl Characterization {
    pub const IDENTITY_GAMMA2: Characterization =
        Characterization { matrix: [1., 0., 0., 0., 1., 0., 0., 0., 1.], transfer: Transfer::Gamma2 };

    /// From VSF's own entry (the first entry of a `ColourProfile` is the best one).
    pub fn from_entry(e: &vsf::spectral_image::ProfileEntry) -> Self {
        let transfer = match e.transfer {
            vsf::spectral_image::Transfer::Srgb => Transfer::Srgb,
            vsf::spectral_image::Transfer::Gamma2Tanh => Transfer::G2t,
            _ => Transfer::Gamma2,
        };
        Characterization { matrix: e.matrix, transfer }
    }
}

const Q12: i32 = 4096;
const Q10: i32 = 1024;
/// Light, Q16: diffuse white (fluor's scale, `theme::G2T_LIGHT_Q16`).
const WHITE: i64 = 65536;
/// The most light the buffer holds: code 255's (9.7× white).
const PEAK: i64 = fluor::theme::G2T_LIGHT_Q16[255] as i64;
/// Above white the encode table steps 16 Q16 units at a time (g2t's codes there are hundreds of units apart).
const ABOVE_SHIFT: u32 = 4;
pub const TILE: usize = 64;

/// 601 full-range YCbCr → R'G'B', Q12: R = Y + 1.402 Cr; G = Y − 0.344136 Cb − 0.714136 Cr; B = Y + 1.772 Cb (Cb, Cr centred on 128).
const CR_R: i32 = 5743;
const CB_G: i32 = 1410;
const CR_G: i32 = 2925;
const CB_B: i32 = 7258;

/// Each transfer's decode: 8-bit code → light, Q16 (white = 65536). Built once with f64; the pixel path only indexes.
fn decode_lut(t: Transfer) -> &'static [u32; 256] {
    static GAMMA2: OnceLock<[u32; 256]> = OnceLock::new();
    static SRGB: OnceLock<[u32; 256]> = OnceLock::new();
    let build = |f: fn(f64) -> f64| {
        let mut t = [0u32; 256];
        for (i, v) in t.iter_mut().enumerate() {
            *v = (f(i as f64 / 255.0) * WHITE as f64).round() as u32;
        }
        t
    };
    match t {
        Transfer::Gamma2 => GAMMA2.get_or_init(|| build(|c| c * c)),
        Transfer::Srgb => SRGB.get_or_init(|| build(|c| if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) })),
        Transfer::G2t => &fluor::theme::G2T_LIGHT_Q16,
    }
}

/// Light → g2t code × 256 (the fraction the dither spends), nearest: code = 256·tanh(√light), VSF's g2t. Index = light Q16 up to white, then white + (i << [`ABOVE_SHIFT`]) up to [`PEAK`], the last entry exactly the peak so the top light is the top code.
fn encode_lut() -> &'static [u16] {
    static LUT: OnceLock<Vec<u16>> = OnceLock::new();
    LUT.get_or_init(|| {
        let code = |l: i64| ((l as f64 / WHITE as f64).sqrt().tanh() * 256.0 * 256.0).round().min(65535.0) as u16;
        let above = ((PEAK - WHITE + (1 << ABOVE_SHIFT) - 1) >> ABOVE_SHIFT) as usize;
        (0..=WHITE).map(code).chain((1..=above).map(|i| code((WHITE + ((i as i64) << ABOVE_SHIFT)).min(PEAK)))).collect()
    })
}

/// The encode table's entry for a light already clamped to 0..=PEAK (above white, the nearest step).
#[inline]
fn encode_at(lut: &[u16], l: i64) -> i32 {
    let i = if l <= WHITE { l as usize } else { WHITE as usize + ((l - WHITE + (1 << (ABOVE_SHIFT - 1))) >> ABOVE_SHIFT) as usize };
    lut[i] as i32
}

// ───────────────────────── blue noise ─────────────────────────

fn splitmix64(x: &mut u64) -> u64 {
    *x = x.wrapping_add(0x9E3779B97F4A7C15);
    let mut z = *x;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58476D1CE4E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D049BB133111EB);
    z ^ (z >> 31)
}

/// The 64×64 blue-noise tile, void-and-cluster (Ulichney), deterministic — the same tile on every device and in every test. Values 0..=255, each appearing exactly 16 times.
pub fn blue_noise_tile() -> &'static [u8; TILE * TILE] {
    static T: OnceLock<[u8; TILE * TILE]> = OnceLock::new();
    T.get_or_init(build_blue_noise)
}

/// The toroidal Gaussian energy kernel (σ = 1.5) over the tile, precomputed.
fn kernel() -> Vec<f32> {
    let mut k = vec![0f32; TILE * TILE];
    let sigma2 = 2.0 * 1.5f32 * 1.5;
    for dy in 0..TILE {
        for dx in 0..TILE {
            let ex = (dx.min(TILE - dx)) as f32;
            let ey = (dy.min(TILE - dy)) as f32;
            k[dy * TILE + dx] = (-(ex * ex + ey * ey) / sigma2).exp();
        }
    }
    k
}

struct Field {
    k: Vec<f32>,
    e: Vec<f32>,
}

impl Field {
    fn new() -> Self {
        Self { k: kernel(), e: vec![0.0; TILE * TILE] }
    }
    fn add(&mut self, idx: usize, sign: f32) {
        let (px, py) = (idx % TILE, idx / TILE);
        for y in 0..TILE {
            let dy = (y + TILE - py) % TILE;
            for x in 0..TILE {
                let dx = (x + TILE - px) % TILE;
                self.e[y * TILE + x] += sign * self.k[dy * TILE + dx];
            }
        }
    }
    /// The set pixel with the highest energy (tightest cluster) / the clear pixel with the lowest (largest void).
    fn extreme(&self, bits: &[bool], set: bool, highest: bool) -> usize {
        let mut best = usize::MAX;
        let mut bv = if highest { f32::MIN } else { f32::MAX };
        for i in 0..TILE * TILE {
            if bits[i] != set {
                continue;
            }
            let v = self.e[i];
            if (highest && v > bv) || (!highest && v < bv) {
                bv = v;
                best = i;
            }
        }
        best
    }
}

fn build_blue_noise() -> [u8; TILE * TILE] {
    let n = TILE * TILE;
    let mut seed = 0x6265616d_626c7565u64; // "beamblue"
    // Initial binary pattern: 1/10 of the pixels set, at random.
    let mut bits = vec![false; n];
    let mut field = Field::new();
    let ones = n / 10;
    let mut placed = 0;
    while placed < ones {
        let i = (splitmix64(&mut seed) % n as u64) as usize;
        if !bits[i] {
            bits[i] = true;
            field.add(i, 1.0);
            placed += 1;
        }
    }
    // Relax: move the tightest cluster into the largest void until they coincide.
    loop {
        let c = field.extreme(&bits, true, true);
        bits[c] = false;
        field.add(c, -1.0);
        let v = field.extreme(&bits, false, false);
        if v == c {
            bits[c] = true;
            field.add(c, 1.0);
            break;
        }
        bits[v] = true;
        field.add(v, 1.0);
    }
    let mut rank = vec![0u32; n];
    // Phase 1: remove the tightest cluster repeatedly, ranks counting down from ones−1.
    let mut work = bits.clone();
    let mut f1 = Field::new();
    for i in 0..n {
        if work[i] {
            f1.add(i, 1.0);
        }
    }
    let mut r = ones;
    while r > 0 {
        let c = f1.extreme(&work, true, true);
        work[c] = false;
        f1.add(c, -1.0);
        r -= 1;
        rank[c] = r as u32;
    }
    // Phase 2: fill the largest void repeatedly from the initial pattern, ranks counting up from ones.
    let mut work = bits.clone();
    let mut f2 = Field::new();
    for i in 0..n {
        if work[i] {
            f2.add(i, 1.0);
        }
    }
    let mut r = ones;
    while r < n / 2 {
        let v = f2.extreme(&work, false, false);
        work[v] = true;
        f2.add(v, 1.0);
        rank[v] = r as u32;
        r += 1;
    }
    // Phase 3: past the half, the roles invert — the clear pixels are the minority; keep filling the largest void of the inverted field.
    let mut f3 = Field::new();
    for i in 0..n {
        if !work[i] {
            f3.add(i, 1.0);
        }
    }
    while r < n {
        let v = f3.extreme(&work, false, true);
        work[v] = true;
        f3.add(v, -1.0);
        rank[v] = r as u32;
        r += 1;
    }
    let mut out = [0u8; TILE * TILE];
    for i in 0..n {
        out[i] = (rank[i] * 256 / n as u32) as u8;
    }
    out
}

// ───────────────────────── the converter ─────────────────────────

/// VSF's colour constants (`vsf::colour::*2*`) are stored COLUMN-major; this chain, a `ProfileEntry`'s matrix, lumis and opsin are all row-major. Every VSF constant enters the beam thru here (field 2026-10-14: read raw, sRGB white came out (0.65, 1.35, 1.00) — the green on every `assumed` beam, Samsung and Mac).
pub const fn vsf_rows(m: [f32; 9]) -> [f32; 9] {
    [m[0], m[3], m[6], m[1], m[4], m[7], m[2], m[5], m[8]]
}

/// Converts decoded frames to g2t Rec.2020 RGB triples for the blit (fluor's buffer encoding). One per beam; `set_characterization` on the info edge.
pub struct Converter {
    chr: Characterization,
    /// The camera→VSF-RGB matrix already folded with VSF RGB→Rec.2020 (f32, gains not yet in).
    fold: [f32; 9],
}

impl Converter {
    pub fn new(chr: Characterization) -> Self {
        let mut c = Self { chr: Characterization::IDENTITY_GAMMA2, fold: [0.0; 9] };
        c.set_characterization(chr);
        c
    }

    pub fn set_characterization(&mut self, chr: Characterization) {
        self.fold = mat_mul(&vsf_rows(vsf::colour::VSF_RGB2REC2020), &chr.matrix);
        self.chr = chr;
    }

    /// The per-frame Q10 matrix: the fold with the reciprocal gains on its columns (R, G = mean of the two greens, B).
    fn q10(&self, gains: Gains) -> [i32; 9] {
        let g = gains.0;
        let inv = [
            Q12 as f32 / g[0].max(1) as f32,
            Q12 as f32 * 2.0 / (g[1].max(1) as f32 + g[2].max(1) as f32),
            Q12 as f32 / g[3].max(1) as f32,
        ];
        let mut m = [0i32; 9];
        for r in 0..3 {
            for c in 0..3 {
                m[r * 3 + c] = (self.fold[r * 3 + c] * inv[c] * Q10 as f32).round() as i32;
            }
        }
        m
    }

    /// One frame: `out` becomes w×h×3 bytes, g2t Rec.2020, dithered at the final truncation.
    pub fn convert(&self, pic: &I420, gains: Gains, frame_no: u32, out: &mut Vec<u8>) {
        let (w, h) = (pic.w, pic.h);
        out.clear();
        out.resize(w * h * 3, 0);
        let m = self.q10(gains);
        let tile = blue_noise_tile();
        let dec = decode_lut(self.chr.transfer);
        let enc = encode_lut();
        // An SDR entry's light stops at white (a saturated colour can still overshoot thru the matrix, and clips there as before); an HDR entry keeps what it carries, up to the buffer's top.
        let top = if self.chr.transfer == Transfer::G2t { PEAK } else { WHITE };
        let m: [i64; 9] = m.map(|v| v as i64);
        // The tile's origin walks per frame: a hash of the frame number, never the same offset twice in a row.
        let mut hs = frame_no as u64 ^ 0xB10E_0A15E;
        let hv = splitmix64(&mut hs);
        let (ox, oy) = ((hv & 63) as usize, ((hv >> 6) & 63) as usize);
        let cw = w / 2;
        for y in 0..h {
            let yrow = &pic.y[y * w..y * w + w];
            let crow = (y / 2) * cw;
            let trow = ((y + oy) % TILE) * TILE;
            for x in 0..w {
                let yy = yrow[x] as i32;
                let cb = pic.u[crow + x / 2] as i32 - 128;
                let cr = pic.v[crow + x / 2] as i32 - 128;
                // YCbCr → R'G'B', Q12, truncating; clamp to the 8-bit range.
                let r8 = ((yy << 12) + CR_R * cr) >> 12;
                let g8 = ((yy << 12) - CB_G * cb - CR_G * cr) >> 12;
                let b8 = ((yy << 12) + CB_B * cb) >> 12;
                // WHY/PROOF: the peer's decoded Y/Cb/Cr can name a colour outside the cube, and the code indexes the 256-entry decode table just below — an unclipped code reads past it.
                let (r8, g8, b8) = (r8.clamp(0, 255), g8.clamp(0, 255), b8.clamp(0, 255));
                // Transfer → light.
                let (rl, gl, bl) = (dec[r8 as usize] as i64, dec[g8 as usize] as i64, dec[b8 as usize] as i64);
                // The folded matrix, Q10, signed; clamp AFTER (where out-of-gamut colours finally clip).
                // WHY/PROOF: a saturated colour leaves the target gamut thru the matrix, and the clipped light indexes the encode table sized for 0..=PEAK.
                let ro = ((m[0] * rl + m[1] * gl + m[2] * bl) >> 10).clamp(0, top);
                let go = ((m[3] * rl + m[4] * gl + m[5] * bl) >> 10).clamp(0, top);
                let bo = ((m[6] * rl + m[7] * gl + m[8] * bl) >> 10).clamp(0, top);
                // g2t at 16 bits, + the tile, truncate to 8 (the top code holds whatever passes it).
                let d = tile[trow + (x + ox) % TILE] as i32;
                let o = (y * w + x) * 3;
                out[o] = ((encode_at(enc, ro) + d) >> 8).min(255) as u8;
                out[o + 1] = ((encode_at(enc, go) + d) >> 8).min(255) as u8;
                out[o + 2] = ((encode_at(enc, bo) + d) >> 8).min(255) as u8;
            }
        }
    }
}

/// Turn an RGB-triple picture `q` quarter turns CLOCKWISE (0..=3); returns the new pixels and size. A pure index map — no resampling, no float.
pub fn rotate_rgb(rgb: &[u8], w: usize, h: usize, q: u8) -> (Vec<u8>, usize, usize) {
    rotate_px(rgb, w, h, q, 3)
}

/// The same turn for any bytes-per-pixel `n` (the beam's quarter-size hole mask is n = 1).
pub fn rotate_px(rgb: &[u8], w: usize, h: usize, q: u8, n: usize) -> (Vec<u8>, usize, usize) {
    let q = q & 3;
    if q == 0 {
        return (rgb.to_vec(), w, h);
    }
    let (nw, nh) = if q % 2 == 1 { (h, w) } else { (w, h) };
    let mut out = vec![0u8; w * h * n];
    for y in 0..h {
        for x in 0..w {
            // Where source (x, y) lands after q clockwise quarter turns.
            let (nx, ny) = match q {
                1 => (h - 1 - y, x),
                2 => (w - 1 - x, h - 1 - y),
                _ => (y, w - 1 - x),
            };
            let s = (y * w + x) * n;
            let d = (ny * nw + nx) * n;
            out[d..d + n].copy_from_slice(&rgb[s..s + n]);
        }
    }
    (out, nw, nh)
}

/// Mirror an RGB-triple picture left↔right in place (the self-view of a front camera, as a mirror shows you).
pub fn mirror_rgb(rgb: &mut [u8], w: usize, h: usize) {
    for y in 0..h {
        let row = &mut rgb[y * w * 3..(y + 1) * w * 3];
        for x in 0..w / 2 {
            let (a, b) = (x * 3, (w - 1 - x) * 3);
            for c in 0..3 {
                row.swap(a + c, b + c);
            }
        }
    }
}

fn mat_mul(a: &[f32; 9], b: &[f32; 9]) -> [f32; 9] {
    let mut o = [0f32; 9];
    for r in 0..3 {
        for c in 0..3 {
            o[r * 3 + c] = (0..3).map(|k| a[r * 3 + k] * b[k * 3 + c]).sum();
        }
    }
    o
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_tile_is_a_flat_histogram_and_deterministic() {
        let t = blue_noise_tile();
        let mut hist = [0u32; 256];
        for &v in t.iter() {
            hist[v as usize] += 1;
        }
        assert!(hist.iter().all(|&c| c == 16), "each of the 256 values appears 16 times in 64×64");
        assert_eq!(t, blue_noise_tile());
        // Blue: neighbouring values are far apart on average (no low-frequency clumps) — adjacent differences well above a random tile's would be, and never zero.
        let mut adj = 0u64;
        for y in 0..TILE {
            for x in 0..TILE {
                let a = t[y * TILE + x] as i64;
                let b = t[y * TILE + (x + 1) % TILE] as i64;
                adj += (a - b).unsigned_abs();
            }
        }
        let mean = adj as f64 / (TILE * TILE) as f64;
        assert!(mean > 85.0, "mean adjacent difference {mean:.1} — white noise sits near 85, blue above it");
    }

    /// A flat grey through the identity chain lands on its own light in g2t: the dither is unbiased, so the MEAN over the area is the exact fractional code and no sample is more than one code off.
    #[test]
    fn a_flat_grey_is_unbiased_through_the_chain() {
        let conv = Converter::new(Characterization { matrix: inverse_rec2020(), transfer: Transfer::Gamma2 });
        let (w, h) = (64, 64);
        let mut pic = I420::new(w, h);
        pic.y.fill(140); // Cb = Cr = 128: R' = G' = B' = 140
        let mut out = Vec::new();
        conv.convert(&pic, Gains::NONE, 7, &mut out);
        // γ2 140 is light (140/255)²; its g2t code is 256·tanh(140/255).
        let want = 256.0 * (140.0f64 / 255.0).tanh();
        let mean = out.iter().map(|&v| v as f64).sum::<f64>() / out.len() as f64;
        assert!((mean - want).abs() < 0.6, "mean {mean:.2}, want {want:.2}");
        assert!(out.iter().all(|&v| (v as f64 - want).abs() <= 1.0));
    }

    /// Gains divide back out: a frame whose ISP doubled blue arrives with blue halved on the wire's linear side.
    #[test]
    fn gains_are_divided_back_out() {
        let conv = Converter::new(Characterization { matrix: inverse_rec2020(), transfer: Transfer::Gamma2 });
        let (w, h) = (16, 16);
        let mut pic = I420::new(w, h);
        pic.y.fill(200);
        let mut plain = Vec::new();
        conv.convert(&pic, Gains::NONE, 1, &mut plain);
        let mut halved = Vec::new();
        conv.convert(&pic, Gains([4096, 4096, 4096, 8192]), 1, &mut halved);
        // Blue's mean LIGHT (decoded from the g2t codes) halves.
        let mb = |v: &Vec<u8>| v.chunks(3).map(|p| fluor::theme::G2T_LIGHT_Q16[p[2] as usize] as f64).sum::<f64>() / (w * h) as f64;
        let (pb, hb) = (mb(&plain), mb(&halved));
        assert!((hb / pb - 0.5).abs() < 0.03, "plain {pb:.0} halved {hb:.0}");
    }

    #[test]
    fn rotation_and_mirror_are_exact_index_maps() {
        // A 3×2 picture whose pixel values name their source position.
        let (w, h) = (3, 2);
        let px: Vec<u8> = (0..(w * h) as u8).flat_map(|i| [i, i, i]).collect();
        let (r1, w1, h1) = rotate_rgb(&px, w, h, 1);
        assert_eq!((w1, h1), (2, 3));
        // Clockwise: the bottom-left source (index 3) lands top-left.
        let first: Vec<u8> = r1.chunks(3).map(|p| p[0]).collect();
        assert_eq!(first, vec![3, 0, 4, 1, 5, 2]);
        let (r2, w2, h2) = rotate_rgb(&r1, w1, h1, 1);
        let (r3, w3, h3) = rotate_rgb(&r2, w2, h2, 1);
        let (r4, _, _) = rotate_rgb(&r3, w3, h3, 1);
        assert_eq!(r4, px, "four quarter turns are the identity");
        let mut m = px.clone();
        mirror_rgb(&mut m, w, h);
        let mv: Vec<u8> = m.chunks(3).map(|p| p[0]).collect();
        assert_eq!(mv, vec![2, 1, 0, 5, 4, 3]);
    }

    #[test]
    fn srgb_entries_linearize_by_the_curve() {
        let s = decode_lut(Transfer::Srgb);
        assert_eq!(s[0], 0);
        assert_eq!(s[255], WHITE as u32);
        // sRGB 128 → 0.2158 linear.
        assert!((s[128] as f64 / WHITE as f64 - 0.2158).abs() < 0.002);
    }

    /// White lands on g2t's white code; an SDR entry stops there, an HDR (g2t) entry keeps the light past it — the whole point of an HDR beam.
    #[test]
    fn sdr_white_is_code_194_and_only_a_g2t_entry_passes_it() {
        let flat = |y: u8, t: Transfer| {
            let conv = Converter::new(Characterization { matrix: inverse_rec2020(), transfer: t });
            let mut pic = I420::new(32, 32);
            pic.y.fill(y);
            let mut out = Vec::new();
            conv.convert(&pic, Gains::NONE, 3, &mut out);
            out.iter().map(|&v| v as f64).sum::<f64>() / out.len() as f64
        };
        let sdr_white = flat(255, Transfer::Gamma2);
        assert!((sdr_white - 194.96).abs() < 0.6, "γ2 white → g2t 256·tanh(1) = 194.96, got {sdr_white:.2}");
        assert!((flat(255, Transfer::Srgb) - 194.96).abs() < 0.6, "sRGB white too");
        // A g2t stream's code 194 is white again (decode then encode, the dither unbiased); its code 240 is about 3× white and survives.
        assert!((flat(194, Transfer::G2t) - 194.0).abs() < 0.7);
        let bright = flat(240, Transfer::G2t);
        assert!((bright - 240.0).abs() < 0.7, "light past white kept: {bright:.2}");
        // Every encode entry is monotone and the top light is the top code.
        let enc = encode_lut();
        assert!(enc.windows(2).all(|p| p[0] <= p[1]));
        assert_eq!(encode_at(enc, PEAK) >> 8, 255);
    }

    /// The inverse of VSF RGB→Rec.2020, so the fold becomes identity: samples that are already Rec.2020 pass straight through.
    fn inverse_rec2020() -> [f32; 9] {
        let m = vsf_rows(vsf::colour::VSF_RGB2REC2020);
        let det = m[0] * (m[4] * m[8] - m[5] * m[7]) - m[1] * (m[3] * m[8] - m[5] * m[6]) + m[2] * (m[3] * m[7] - m[4] * m[6]);
        let mut inv = [0f32; 9];
        inv[0] = (m[4] * m[8] - m[5] * m[7]) / det;
        inv[1] = (m[2] * m[7] - m[1] * m[8]) / det;
        inv[2] = (m[1] * m[5] - m[2] * m[4]) / det;
        inv[3] = (m[5] * m[6] - m[3] * m[8]) / det;
        inv[4] = (m[0] * m[8] - m[2] * m[6]) / det;
        inv[5] = (m[2] * m[3] - m[0] * m[5]) / det;
        inv[6] = (m[3] * m[7] - m[4] * m[6]) / det;
        inv[7] = (m[1] * m[6] - m[0] * m[7]) / det;
        inv[8] = (m[0] * m[4] - m[1] * m[3]) / det;
        inv
    }
}
