//! THE HDR BEAM'S SEND SIDE (docs/beams.md "HDR", Nick 2026-10-26): a phone camera's 10-bit HLG frame (P010, BT.2020) → the coded size → 8-bit `g2t` codes in the same 601 full-range YCbCr the encoder always takes. An HDR beam is an ordinary H.264 stream whose colour entry says `g2t`; the receiver keeps its light past white (beam_colour.rs).
//!
//! Per frame: the three P010 planes are box-averaged at 16 bits to the effective size (the ladder's band and softness, as beam_scale.rs does for an 8-bit frame), each pixel goes 10-bit Y'CbCr (BT.2020, limited or full range as the camera tagged it) → HLG R'G'B' → light → g2t code by ONE 4096-entry table, blue-noise dithered to 8 bits, re-encoded as 601 YCbCr, and grown to the coded size.
//! HLG → light: the inverse OETF, then the OOTF's system gamma 1.2 per channel (panel-dominant, as fluor maps everything), scaled so HLG's reference white (signal 0.75) is diffuse white. HLG's top lands at 4.9× white, g2t code 250.
//!
//! Integer in the pixel path; the floats build the table once.

use super::beam_colour::{blue_noise_tile, TILE};
use super::beam_scale::{crop_to, resample_plane, Plane};
use std::sync::OnceLock;

/// BT.2020 Y'CbCr → R'G'B', Q12: R = Y + 1.4746 Cr; G = Y − 0.16455 Cb − 0.57135 Cr; B = Y + 1.8814 Cb.
const CR_R: i32 = 6040;
const CB_G: i32 = 674;
const CR_G: i32 = 2340;
const CB_B: i32 = 7706;
/// 601 R'G'B' → Y'CbCr for the encoder, Q8 luma weights (sum 256) and Q12 chroma scales: Cb = (B − Y)/1.772, Cr = (R − Y)/1.402 — the inverse of what the receiver decodes (beam_colour.rs).
const Y_R: i32 = 77;
const Y_G: i32 = 150;
const Y_B: i32 = 29;
const CB_S: i32 = 2312;
const CR_S: i32 = 2922;

/// HLG signal (Q12, 0..=4095) → g2t code × 256.
fn hlg_lut() -> &'static [u16; 4096] {
    static LUT: OnceLock<Box<[u16; 4096]>> = OnceLock::new();
    LUT.get_or_init(|| {
        let (a, b, c) = (0.17883277f64, 0.28466892f64, 0.55991073f64);
        let scene = |e: f64| if e <= 0.5 { e * e / 3.0 } else { (((e - c) / a).exp() + b) / 12.0 };
        let white = scene(0.75).powf(1.2);
        let mut t = Box::new([0u16; 4096]);
        for (i, v) in t.iter_mut().enumerate() {
            let light = scene(i as f64 / 4095.0).powf(1.2) / white;
            *v = (light.sqrt().tanh() * 256.0 * 256.0).round().min(65535.0) as u16;
        }
        t
    })
}

/// One P010 sample: little-endian u16, the 10 bits in the top.
#[inline]
fn p010(p: &Plane, x: usize, y: usize) -> u32 {
    let i = y * p.row + x * p.px;
    debug_assert!(i + 1 < p.len);
    // SAFETY: `fits16` checked the plane covers w×h two-byte samples at its strides.
    unsafe { (*p.ptr.add(i) as u32 | (*p.ptr.add(i + 1) as u32) << 8) >> 6 }
}

/// Does the buffer hold every two-byte sample the geometry names?
fn fits16(p: &Plane) -> bool {
    p.w > 0 && p.h > 0 && p.px >= 2 && (p.h - 1) * p.row + (p.w - 1) * p.px + 1 < p.len
}

/// Box-average the (x0,y0,cw,ch) window of a P010 plane into an ew×eh scratch of 10-bit samples (ew ≤ cw, eh ≤ ch).
fn shrink16(src: &Plane, (x0, y0, cw, ch): (usize, usize, usize, usize), ew: usize, eh: usize) -> Vec<u16> {
    let spans: Vec<(usize, usize)> = (0..ew)
        .map(|ox| {
            let a = x0 + ox * cw / ew;
            (a, (x0 + (ox + 1) * cw / ew).max(a + 1))
        })
        .collect();
    let mut out = vec![0u16; ew * eh];
    let mut acc = vec![0u32; ew];
    for oy in 0..eh {
        let sy0 = y0 + oy * ch / eh;
        let sy1 = (y0 + (oy + 1) * ch / eh).max(sy0 + 1);
        acc.iter_mut().for_each(|a| *a = 0);
        for sy in sy0..sy1 {
            for (a, &(sx0, sx1)) in acc.iter_mut().zip(&spans) {
                *a += (sx0..sx1).map(|sx| p010(src, sx, sy)).sum::<u32>();
            }
        }
        let rows = (sy1 - sy0) as u32;
        for (ox, (&a, &(sx0, sx1))) in acc.iter().zip(&spans).enumerate() {
            out[oy * ew + ox] = (a / (rows * (sx1 - sx0) as u32)) as u16;
        }
    }
    out
}

/// The window and effective size one plane takes on its way to `dst` at `soft` half-stops (beam_scale::resample_plane's rule).
fn effective(src: &Plane, dst: &Plane, soft: u8) -> ((usize, usize, usize, usize), usize, usize) {
    let (x0, y0, cw, ch) = crop_to(src.w, src.h, dst.w, dst.h);
    let (cw, ch) = (cw.max(1), ch.max(1));
    let scale = 2f64.powf(-(soft as f64) / 2.0);
    let ew = ((dst.w as f64 * scale).round() as usize).max(1).min(cw);
    let eh = ((dst.h as f64 * scale).round() as usize).max(1).min(ch);
    ((x0, y0, cw, ch), ew, eh)
}

/// One HDR camera frame → the encoder's planes. `full_range` = the camera tagged its P010 full range (else video range, 64–940 luma). False when a plane does not cover its geometry.
pub fn fill_hlg_p010(src: [Plane; 3], full_range: bool, dst: [Plane; 3], soft: u8, frame_no: u32) -> bool {
    if !src.iter().all(fits16) || !dst.iter().all(|d| d.fits()) {
        return false;
    }
    let (wy, ew, eh) = effective(&src[0], &dst[0], soft);
    let (wc, ewc, ehc) = effective(&src[1], &dst[1], soft);
    let ys = shrink16(&src[0], wy, ew, eh);
    let cbs = shrink16(&src[1], wc, ewc, ehc);
    let crs = shrink16(&src[2], wc, ewc, ehc);
    let lut = hlg_lut();
    let tile = blue_noise_tile();
    let (ox, oy) = ((frame_no as usize * 23) % TILE, (frame_no as usize * 41) % TILE);
    // Range expansion to Q12: video range Y 64..940, C 64..960 about 512; full range over 0..1023.
    let (y_off, y_mul, c_mul) = if full_range { (0, 4096 * 64 / 1023, 4096 * 64 / 1023) } else { (64, 4096 * 64 / 876, 4096 * 64 / 896) };
    let mut y8 = vec![0u8; ew * eh];
    // Per chroma sample: summed R, G, B (code × 256) of its luma pixels and how many.
    let mut acc = vec![[0u32; 4]; ewc * ehc];
    for y in 0..eh {
        let cy = (y / 2).min(ehc - 1);
        let trow = ((y + oy) % TILE) * TILE;
        for x in 0..ew {
            let cx = (x / 2).min(ewc - 1);
            let ci = cy * ewc + cx;
            let yy = ((ys[y * ew + x] as i32 - y_off) * y_mul) >> 6;
            let cb = ((cbs[ci] as i32 - 512) * c_mul) >> 6;
            let cr = ((crs[ci] as i32 - 512) * c_mul) >> 6;
            // WHY/PROOF: a camera's video-range samples can sit outside 64..940 (footroom, headroom) and a chroma pair can name a colour outside the cube — each R'G'B' indexes the 4096-entry table.
            let r = (yy + ((CR_R * cr) >> 12)).clamp(0, 4095) as usize;
            let g = (yy - ((CB_G * cb + CR_G * cr) >> 12)).clamp(0, 4095) as usize;
            let b = (yy + ((CB_B * cb) >> 12)).clamp(0, 4095) as usize;
            let (r, g, b) = (lut[r] as i32, lut[g] as i32, lut[b] as i32);
            let luma = (Y_R * r + Y_G * g + Y_B * b) >> 8;
            let d = tile[trow + (x + ox) % TILE] as i32;
            y8[y * ew + x] = ((luma + d) >> 8).min(255) as u8;
            let a = &mut acc[ci];
            a[0] += r as u32;
            a[1] += g as u32;
            a[2] += b as u32;
            a[3] += 1;
        }
    }
    let mut cb8 = vec![128u8; ewc * ehc];
    let mut cr8 = vec![128u8; ewc * ehc];
    for (i, a) in acc.iter().enumerate() {
        if a[3] == 0 {
            continue;
        }
        let n = a[3];
        let (r, g, b) = ((a[0] / n) as i32, (a[1] / n) as i32, (a[2] / n) as i32);
        let luma = (Y_R * r + Y_G * g + Y_B * b) >> 8;
        let d = tile[((i / ewc + oy) % TILE) * TILE + (i % ewc + ox) % TILE] as i32;
        cb8[i] = ((128 * 256 + ((CB_S * (b - luma)) >> 12) + d) >> 8).clamp(0, 255) as u8;
        cr8[i] = ((128 * 256 + ((CR_S * (r - luma)) >> 12) + d) >> 8).clamp(0, 255) as u8;
    }
    resample_plane(&Plane::from_slice(&mut y8, ew, 1, ew, eh), &dst[0], 0);
    resample_plane(&Plane::from_slice(&mut cb8, ewc, 1, ewc, ehc), &dst[1], 0);
    resample_plane(&Plane::from_slice(&mut cr8, ewc, 1, ewc, ehc), &dst[2], 0);
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hlg_reference_white_is_g2t_white_and_its_top_passes_it() {
        let lut = hlg_lut();
        assert_eq!(lut[0], 0);
        let white = lut[(0.75 * 4095.0) as usize] as f64 / 256.0;
        assert!((white - 194.96).abs() < 0.4, "HLG 0.75 → g2t white, got {white:.2}");
        let top = lut[4095] as f64 / 256.0;
        assert!(top > 249.0 && top < 251.0, "HLG 1.0 is ~4.9× white, g2t ~250, got {top:.2}");
        assert!(lut.windows(2).all(|p| p[0] <= p[1]), "monotone");
    }

    /// A flat video-range P010 frame at HLG reference white comes out as g2t white luma and neutral chroma, at a smaller coded size.
    #[test]
    fn a_flat_white_p010_frame_lands_on_g2t_white() {
        let (sw, sh, dw, dh) = (64usize, 36usize, 32usize, 18usize);
        let y10 = (64.0 + 0.75 * 876.0) as u16;
        let mut yb: Vec<u8> = (0..sw * sh).flat_map(|_| (y10 << 6).to_le_bytes()).collect();
        // Interleaved chroma (Cb, Cr) at 512, four bytes per pair, as Android's P010 planes step.
        let mut cb: Vec<u8> = (0..(sw / 2) * (sh / 2)).flat_map(|_| [(512u16 << 6).to_le_bytes(), (512u16 << 6).to_le_bytes()].concat()).collect();
        let len = cb.len();
        let src = [
            Plane::from_slice(&mut yb, sw * 2, 2, sw, sh),
            Plane { ptr: cb.as_mut_ptr(), len, row: sw * 2, px: 4, w: sw / 2, h: sh / 2 },
            Plane { ptr: unsafe { cb.as_mut_ptr().add(2) }, len: len - 2, row: sw * 2, px: 4, w: sw / 2, h: sh / 2 },
        ];
        let (mut dy, mut du, mut dv) = (vec![0u8; dw * dh], vec![0u8; dw * dh / 4], vec![0u8; dw * dh / 4]);
        let dst = [Plane::from_slice(&mut dy, dw, 1, dw, dh), Plane::from_slice(&mut du, dw / 2, 1, dw / 2, dh / 2), Plane::from_slice(&mut dv, dw / 2, 1, dw / 2, dh / 2)];
        assert!(fill_hlg_p010(src, false, dst, 0, 1));
        let mean = |v: &[u8]| v.iter().map(|&x| x as f64).sum::<f64>() / v.len() as f64;
        assert!((mean(&dy) - 194.96).abs() < 1.0, "luma {:.2}", mean(&dy));
        assert!((mean(&du) - 128.0).abs() < 1.0 && (mean(&dv) - 128.0).abs() < 1.0, "neutral chroma");
    }
}
