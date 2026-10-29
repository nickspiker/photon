//! Chromatic wave: a sine-modulated visible-spectrum colour bar that paints over the existing background via legacy `sqrt(c*scale + c_bg²)` per-channel blend. Direct port of `compositing.rs::draw_spectrum` (compositing.rs:5092-5168), preserving the legacy quadrature-add interaction with the bg below.
//!
//! The wave reads the bg pixel's visible RGB (assumed opaque from a prior `paint::background_noise` pass), quadrature-adds the wave's per-channel intensity, writes back to α + darkness storage with α preserved. Render order is bg-first (noise → wave → logo) — the wave depends on the noise already being in the buffer to read.
//!
//! Colour at each x sweeps the 350-750 nm band of the visible spectrum (sampled via [`super::lms2006so::LMS2006SO`] and converted to RGB via the LMS → REC2020 magic 9 coefficients inlined below). The y modulation is a sine whose **frequency** ramps up toward the blue end (logarithmic) and whose **amplitude** rises toward blue too; `period_scale` multiplies the base `waves_per_region` so the same shape stretches/compresses globally.

use super::lms2006so::LMS2006SO;
use fluor::canvas::PixelRect;
use fluor::canvas::Canvas;
use fluor::pixel::Pixel;
use std::f32::consts::TAU;

/// Paint a chromatic wave covering `rect` of `canvas`. `phase` is the wave's horizontal phase shift in radians (advance over time to animate); `period_scale` multiplies the base waves-per-region (`1.` = legacy density, `>1.` = more waves, `<1.` = fewer waves).
pub fn chromatic_wave<P: Pixel>(canvas: &mut Canvas<P>, rect: PixelRect, phase: f32, period_scale: f32) {
    let h = canvas.height;
    chromatic_wave_clipped(canvas, rect.x0, rect.x1, rect.y0 as isize, rect.y1 - rect.y0, 0, h, phase, period_scale);
}

/// CROP-not-shrink variant (the About slab, 2026-09-02): the wave's pattern is computed over its FULL logical band (`top`..`top+logical_h`, `top` may be negative when scrolled past the pane top) and only rows inside `clip_y0..clip_y1` (∩ the buffer) are painted. Shrinking the rect instead RESCALES the pattern — the "wave scales when scrolling" field bug.
#[allow(clippy::too_many_arguments)]
pub fn chromatic_wave_clipped<P: Pixel>(
    canvas: &mut Canvas<P>,
    x0: usize,
    x1: usize,
    top: isize,
    logical_h: usize,
    clip_y0: usize,
    clip_y1: usize,
    phase: f32,
    period_scale: f32,
) {
    let buf_w = canvas.width;
    let buf_h = canvas.height;
    // WHY: the layout hands this a region that collapses to nothing when the window is narrower than the wave's margins.
    // PROOF: an inverted region is zero-wide — returned on just below — instead of wrapping to a region wider than any buffer.
    let region_w = x1.saturating_sub(x0);
    let region_h = logical_h;
    if region_w == 0 || x1 > buf_w {
        return;
    }
    let clip_y1 = clip_y1.min(buf_h);
    // WHY: `logo_height` is the denominator in `scale = ... / logo_height_f` below; if it were 0 the f32 division produces ±Inf / NaN and the per-pixel `as u8` cast saturates to 0 across the entire region, painting nothing — at which point the loops are wasted work. PROOF: triggered exactly when `region_h ≤ 1` (one or zero pixels of wave); the wave is undefined at that height anyway.
    let logo_height = region_h / 2;
    if logo_height == 0 {
        return;
    }
    {
        let dy0 = top.max(clip_y0 as isize).max(0) as usize; // WHY/PROOF: the band's top can sit above the canvas (negative isize), and the usize cast would wrap it to a huge row
        let dy1 = ((top + region_h as isize).max(0) as usize).min(clip_y1); // WHY/PROOF: as above for the bottom edge
        if dy1 > dy0 {
            canvas.damage.add(PixelRect::new(x0, dy0, x1, dy1));
        }
    }

    // Harmonic mean of region dims for brightness scaling — keeps the bar's overall energy stable across aspect ratios. Matches legacy line 5103. The `2` is part of the harmonic-mean formula, not a tuning knob.
    let region_span = 2. * region_w as f32 * region_h as f32 / (region_w as f32 + region_h as f32);
    // Base waves count = aspect × 2 (legacy line 5106), then multiplied by the scroll-driven period_scale. The `2` is the legacy waves-per-aspect ratio, not a tuning knob.
    let waves_per_region = (region_w as f32 / region_h as f32 * 2.) * period_scale;

    // LMS2006SO is indexed in 1 nm steps starting at 390 nm; the legacy bar samples 350-750 nm. The constants below match the legacy off-by-zero treatment — START_NM = 350 means LAMBDA_START = 0 and LAMBDA_END = 400, indexing the first 400 nm-steps of the table.
    const START_NM: usize = 350;
    const LAMBDA_START: usize = 350 - START_NM;
    const LAMBDA_END: usize = 750 - START_NM;

    // α + darkness: each stored channel is ONE − the visible code (the bg is opaque); α is preserved thru the blend.
    let one = P::ONE;

    let logo_height_f = logo_height as f32;
    let two_logo_height_f = (logo_height * 2) as f32;

    for y in 0..region_h {
        // Logical row → canvas row; rows outside the clip band (or the buffer) are computed-free skips — the PATTERN stays anchored to the logical band while only visible rows paint.
        let py_i = top + y as isize;
        if py_i < clip_y0 as isize || py_i >= clip_y1 as isize {
            continue;
        }
        let py = py_i as usize;
        let row_base = py * buf_w;
        let y_f = y as f32;
        for x in 0..region_w {
            let px = x0 + x;

            // Flip x for wave calculations so the wave's frequency ramp matches the spectrum direction (blue on the left where x_flipped is largest).
            let x_flipped = region_w - 1 - x;
            let x_norm = x_flipped as f32 / region_w as f32;
            // 12 is the legacy amplitude-curve magic number — algorithm constant, not a tuning knob, so left as-is.
            let amplitude = logo_height_f / (1. + 12. * x_norm);

            // Logarithmic frequency ramp: ~1× at red end (x_norm=0), ~4× at blue end (x_norm=1) — exactly the legacy 2^(-x_norm + 2) shape. Both 2s here are powers-of-two-as-bases / exponents, intrinsic to the algorithm.
            let freq_ramp = 2_f32.powf(-x_norm + 2.);
            let wave_phase = x_norm * TAU * waves_per_region * freq_ramp - phase;
            let wave_offset = wave_phase.sin() * amplitude;

            // Float arithmetic for the scale denominator — the legacy `(logo_height * 2 - y)` is unsigned subtraction that would underflow for odd region_h at the last row; doing it in f32 produces a small negative instead of a panic/wrap. `1 << 15` (= 32768) replaces the legacy `32000.` brightness multiplier — a single-digit tuning knob.
            let mut scale = (y_f + wave_offset - logo_height_f) / logo_height_f;
            scale = ((two_logo_height_f - y_f) / logo_height_f)
                * (y_f / logo_height_f)
                * (1 << 15) as f32
                / (scale.abs() + amplitude / region_span * (1. / ((1 << 2) as f32)));

            // Wavelength index runs blue → red as x_flipped decreases; `region_w ≥ 1` follows from the early-return on `x0 >= x1`, so the division needs no guard.
            let wavelength_idx =
                LAMBDA_START + ((region_w - 1 - x) * (LAMBDA_END - LAMBDA_START)) / region_w;
            let lms_idx = wavelength_idx * 3;

            let l = LMS2006SO[lms_idx];
            let m = LMS2006SO[lms_idx + 1];
            let s = LMS2006SO[lms_idx + 2];

            // LMS → REC2020 magic 9 (identical numeric values to compositing.rs:5148-5154 and to colour.rs's LMS2REC2020 matrix; trailing zeros from the legacy 16-digit form stripped — f32 has ~7 digits of precision so the trimmed last digit was already noise).
            let r = 3.16824109881169 * l + -2.15688285649183 * m + 0.0964568792112096 * s;
            let g = -0.266362510245695 * l + 1.40494573257753 * m + -0.175554801656117 * s;
            let b = 0.00389152987374033 * l + -0.0205676800313948 * m + 0.945832607950864 * s;

            let idx = row_base + px;
            let existing = canvas.pixels[idx];
            // Extract the visible codes from α + darkness storage. Alpha stays unchanged. On Android the storage channels are in B, G, R order (RGBA_8888 read as little-endian u32), so the field at the top is dark_B and the one at the bottom is dark_R — swap when extracting so the per-pixel math below stays in canonical R/G/B positions regardless of platform.
            let alpha = existing.alpha();
            let [hi, mid, lo] = existing.channels().map(|d| one - d);
            #[cfg(target_os = "android")]
            let (r_bg, b_bg) = (lo, hi);
            #[cfg(not(target_os = "android"))]
            let (r_bg, b_bg) = (hi, lo);
            let g_bg = mid;

            // The quadrature add is a LIGHT add: on the old γ2 buffer `sqrt(c · scale + c_bg²)` added c · scale / 255² of light to the background's. The buffer is `g2t` now, so decode the background's light, add the same light, stop at diffuse white (where γ2 255 used to clip — past it is a 9.7× glow on HDR), and take VSF's bucket. Negative or NaN light adds nothing.
            // 8 bits: fluor's tables, as always. A wider depth: VSF's own codec at that depth (decode at the bucket bottom, encode by its settled truncation), light capped at diffuse white the same way.
            let add = |c: f32, bg: u32| -> u32 {
                if P::DEPTH == 8 {
                    let light = fluor::theme::G2T_LIGHT_Q16[bg as usize] as f32 + (c * scale).max(0.0) * (65536.0 / 65025.0);
                    fluor::theme::g2t_code(light.min(65536.0) as u32).min(194).max(bg)
                } else {
                    let light = vsf::colour::convert::linearize_gamma2tanh_bits_f32(bg, P::DEPTH) + (c * scale).max(0.0) * (1. / 65025.0);
                    vsf::colour::convert::delinearize_gamma2tanh_bits_f32(light.min(1.), P::DEPTH).max(bg)
                }
            };
            let r_new = add(r, r_bg);
            let g_new = add(g, g_bg);
            let b_new = add(b, b_bg);

            // Pack R/G/B back into platform channel order — mirror image of the extract above.
            #[cfg(target_os = "android")]
            let visible_new = [b_new, g_new, r_new];
            #[cfg(not(target_os = "android"))]
            let visible_new = [r_new, g_new, b_new];
            canvas.pixels[idx] = P::from_channels(alpha, visible_new.map(|v| one - v));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fluor::pixel::{Argb8, G2t16};

    /// The wave over the same opaque background at both depths, read back at 8 bits: the light added agrees to within the 8-bit buckets it lands in (the 16-bit path decodes and encodes thru vsf's own codec, fluor's 8-bit tables round to Q16 first), the background's α untouched, and nothing darker than it was.
    #[test]
    fn the_wave_adds_the_same_light_at_both_depths() {
        let (w, h) = (96usize, 48usize);
        let bg8: Vec<Argb8> = (0..w * h).map(|i| fluor::theme::dark(0x00_10_20_30 + ((i as u32 * 7) & 0x0F))).collect();
        let mut p8 = bg8.clone();
        let mut p16: Vec<G2t16> = bg8.iter().map(|&c| G2t16::from_g2t8(c)).collect();
        let mut dmg = fluor::canvas::Damage::new();
        let rect = PixelRect::new(0, 0, w, h);
        chromatic_wave(&mut Canvas::new(&mut p8, w, h, &mut dmg), rect, 0.5, 1.);
        chromatic_wave(&mut Canvas::new(&mut p16, w, h, &mut dmg), rect, 0.5, 1.);
        let mut lit = 0;
        for i in 0..w * h {
            assert_eq!(p16[i].alpha(), G2t16::ONE);
            for ((a, b), base) in p8[i].channels().into_iter().zip(p16[i].channels()).zip(bg8[i].channels()) {
                assert!(((b >> 8) as i32 - a as i32).abs() <= 1, "pixel {i}: {:08x} against {:016x}", p8[i], p16[i]);
                lit += (a < base) as u32;
            }
        }
        assert!(lit > 0, "the wave lights something");
    }
}
