//! The beam's receive-side colour chain (docs/beams.md §4), integer end to end — no float in the pixel path. The floats belong to the one-time matrix fold at the top of a frame.
//!
//! ```text
//! Y Cb Cr (8-bit, 4:2:0, 601 full range)
//!   │ 3×3 Q12 + offsets, truncate → R'G'B' γ2 (8-bit)
//!   │ square → linear, 16-bit (full scale 255² = 65025)
//!   │ ONE folded 3×3 in Q10 — (VSF RGB → Rec.2020) × (camera → VSF RGB) × diag(1/gains) — signed, clamp after
//!   │ √ by a 65536-entry table → γ2 Rec.2020 at 16 bits
//!   │ + blue-noise tile (x,y mod 64, origin hashed per frame), truncate → 8-bit
//!   ▼
//! R G B triples, γ2 Rec.2020 — the shape `colour_convert::vsf_rgb_to_bt2020` already hands the blit
//! ```
//!
//! Headroom accounting (i32 only, never isize): 8-bit × Q12 × 3 terms = 22 bits; 16-bit × Q10 × 3 terms with coefficients under 4.0 = 30 bits. A `srgb` transfer (a desktop camera's `creative`/`assumed` entry) swaps the square for a 256-entry inverse table; a `gamma2` entry (the phone) is the square.
//!
//! The dither: unbiased truncation — the mean of ⌊x + u⌋ over uniform u is x — with a void-and-cluster blue-noise tile (all its energy high-frequency, the least visible noise for its amplitude) whose origin walks per frame so a still scene never shows a fixed pattern. Applied at the ONE place it belongs, the final 8-bit output. The 16-bit intermediate needs none. It hides OUR steps only; the codec's artifacts sit many codes high (docs/beams.md §5).

use super::beam::Gains;
use super::h264::I420;
use std::sync::OnceLock;

/// The transfer the encoded samples carry (vsf::spectral_image::Transfer, the two a beam uses).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Transfer {
    Gamma2,
    Srgb,
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
            _ => Transfer::Gamma2,
        };
        Characterization { matrix: e.matrix, transfer }
    }
}

const Q12: i32 = 4096;
const Q10: i32 = 1024;
/// Full scale of the 16-bit linear intermediate: 255².
const LIN_FULL: i32 = 255 * 255;
pub const TILE: usize = 64;

/// 601 full-range YCbCr → R'G'B', Q12: R = Y + 1.402 Cr; G = Y − 0.344136 Cb − 0.714136 Cr; B = Y + 1.772 Cb (Cb, Cr centred on 128).
const CR_R: i32 = 5743;
const CB_G: i32 = 1410;
const CR_G: i32 = 2925;
const CB_B: i32 = 7258;

/// √ table: index = 16-bit linear (0..=65025), value = √(lin / 65025) × 255 × 256 — γ2 encode at 16 bits, so the 8-bit truncation below it has 8 bits of dither room.
fn sqrt_lut() -> &'static [u16] {
    static LUT: OnceLock<Vec<u16>> = OnceLock::new();
    LUT.get_or_init(|| {
        // Built once with f64; the pixel path only indexes it.
        (0..=LIN_FULL as usize).map(|lin| ((lin as f64 / LIN_FULL as f64).sqrt() * 255.0 * 256.0).round().min(65280.0) as u16).collect()
    })
}

/// sRGB → linear, 8-bit code → 16-bit linear (full scale 65025).
fn srgb_lut() -> &'static [u16; 256] {
    static LUT: OnceLock<[u16; 256]> = OnceLock::new();
    LUT.get_or_init(|| {
        let mut t = [0u16; 256];
        for (i, v) in t.iter_mut().enumerate() {
            let c = i as f64 / 255.0;
            let lin = if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) };
            *v = (lin * LIN_FULL as f64).round() as u16;
        }
        t
    })
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

/// Converts decoded frames to γ2 Rec.2020 RGB triples for the blit. One per beam; `set_characterization` on the info edge.
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
        self.fold = mat_mul(&vsf::colour::VSF_RGB2REC2020, &chr.matrix);
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

    /// One frame: `out` becomes w×h×3 bytes, γ2 Rec.2020, dithered at the final truncation.
    pub fn convert(&self, pic: &I420, gains: Gains, frame_no: u32, out: &mut Vec<u8>) {
        let (w, h) = (pic.w, pic.h);
        out.clear();
        out.resize(w * h * 3, 0);
        let m = self.q10(gains);
        let tile = blue_noise_tile();
        let sq = sqrt_lut();
        let srgb = srgb_lut();
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
                let (r8, g8, b8) = (r8.clamp(0, 255), g8.clamp(0, 255), b8.clamp(0, 255));
                // Transfer → 16-bit linear.
                let (rl, gl, bl) = match self.chr.transfer {
                    Transfer::Gamma2 => (r8 * r8, g8 * g8, b8 * b8),
                    Transfer::Srgb => (srgb[r8 as usize] as i32, srgb[g8 as usize] as i32, srgb[b8 as usize] as i32),
                };
                // The folded matrix, Q10, signed; clamp AFTER (where out-of-gamut colours finally clip).
                let ro = ((m[0] * rl + m[1] * gl + m[2] * bl) >> 10).clamp(0, LIN_FULL);
                let go = ((m[3] * rl + m[4] * gl + m[5] * bl) >> 10).clamp(0, LIN_FULL);
                let bo = ((m[6] * rl + m[7] * gl + m[8] * bl) >> 10).clamp(0, LIN_FULL);
                // √ at 16 bits, + the tile, truncate to 8.
                let d = tile[trow + (x + ox) % TILE] as i32;
                let o = (y * w + x) * 3;
                out[o] = ((sq[ro as usize] as i32 + d) >> 8) as u8;
                out[o + 1] = ((sq[go as usize] as i32 + d) >> 8) as u8;
                out[o + 2] = ((sq[bo as usize] as i32 + d) >> 8) as u8;
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

    /// A flat grey through the identity chain lands on itself: the dither is unbiased, so the MEAN over the area is the exact value and no sample is more than one code off.
    #[test]
    fn a_flat_grey_is_unbiased_through_the_chain() {
        let conv = Converter::new(Characterization { matrix: inverse_rec2020(), transfer: Transfer::Gamma2 });
        let (w, h) = (64, 64);
        let mut pic = I420::new(w, h);
        pic.y.fill(140); // Cb = Cr = 128: R' = G' = B' = 140
        let mut out = Vec::new();
        conv.convert(&pic, Gains::NONE, 7, &mut out);
        let mean = out.iter().map(|&v| v as f64).sum::<f64>() / out.len() as f64;
        assert!((mean - 140.0).abs() < 0.6, "mean {mean:.2}");
        assert!(out.iter().all(|&v| (v as i32 - 140).abs() <= 1));
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
        let mb = |v: &Vec<u8>| v.chunks(3).map(|p| p[2] as f64).sum::<f64>() / (w * h) as f64;
        let (pb, hb) = (mb(&plain), mb(&halved));
        // Linear blue halves → γ2 blue × 1/√2.
        assert!((hb / pb - 0.7071).abs() < 0.03, "plain {pb:.1} halved {hb:.1}");
    }

    #[test]
    fn srgb_entries_linearize_by_the_curve() {
        let s = srgb_lut();
        assert_eq!(s[0], 0);
        assert_eq!(s[255], LIN_FULL as u16);
        // sRGB 128 → 0.2158 linear.
        assert!((s[128] as f64 / LIN_FULL as f64 - 0.2158).abs() < 0.002);
    }

    /// The inverse of VSF RGB→Rec.2020, so the fold becomes identity: samples that are already Rec.2020 pass straight through.
    fn inverse_rec2020() -> [f32; 9] {
        let m = vsf::colour::VSF_RGB2REC2020;
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
