//! Chromatic wave: a sine-modulated visible-spectrum colour bar that composes additively over whatever's already on the canvas. Direct port of the legacy `draw_spectrum` from `compositing.rs:5092-5168` (Android side, behind cfg-gate), adapted to fluor's α + darkness pixel format and gaining a `period_scale` parameter so the user's scroll wheel can stretch/compress the wave horizontally.
//!
//! The wave colour at each x sweeps the 350-750 nm band of the visible spectrum (sampled via [`super::lms2006so::LMS2006SO`] and converted to RGB via the LMS → REC2020 magic 9 coefficients inlined below — identical numerical values to the legacy implementation). The y modulation is a sine whose **frequency** ramps up toward the blue end (logarithmic) and whose **amplitude** rises toward blue too — same shape as the legacy bar; the period_scale knob multiplies the base `waves_per_region` so the same shape stretches/compresses globally.
//!
//! Composition model matches the legacy "square-root of sum of squares" per-channel additive blend: `c_new_visible = sqrt(c_wave * scale + c_bg_visible²)`. Out-of-gamut values saturate to [0, 255] via `as u8` exactly as in the legacy. Alpha of the destination pixel is preserved unchanged.

use super::lms2006so::LMS2006SO;
use fluor::canvas::{Canvas, PixelRect};
use std::f32::consts::TAU;

/// Paint a chromatic wave covering `rect` of `canvas`. `phase` is the wave's horizontal phase shift in radians (advance over time to animate); `period_scale` multiplies the base waves-per-region (`1.` = legacy density, `>1.` = more waves, `<1.` = fewer waves; values can be ≤0 to invert though that's mostly a visual curiosity).
pub fn chromatic_wave(canvas: &mut Canvas, rect: PixelRect, phase: f32, period_scale: f32) {
    let buf_w = canvas.width;
    let buf_h = canvas.height;
    // Clip the requested rect to the canvas — rasterizers don't get to write past the buffer. Anything entirely off-canvas no-ops.
    let x0 = rect.x0.min(buf_w);
    let y0 = rect.y0.min(buf_h);
    let x1 = rect.x1.min(buf_w);
    let y1 = rect.y1.min(buf_h);
    if x0 >= x1 || y0 >= y1 {
        return;
    }
    let region_w = x1 - x0;
    let region_h = y1 - y0;
    // logo_height = half the region; the wave oscillates around the vertical center. region_h < 2 means we can't form a meaningful wave — bail out rather than divide by zero in the scale calc.
    let logo_height = region_h / 2;
    if logo_height == 0 {
        return;
    }
    canvas.damage.add(PixelRect::new(x0, y0, x1, y1));

    // Harmonic mean of region dims for brightness scaling — keeps the bar's overall energy stable across aspect ratios. Matches legacy line 5103. The `2` is part of the harmonic-mean formula, not a tuning knob.
    let region_span =
        2. * region_w as f32 * region_h as f32 / (region_w as f32 + region_h as f32);
    // Base waves count = aspect × 2 (legacy line 5106), then multiplied by the scroll-driven period_scale. The `2` is the legacy waves-per-aspect ratio, not a tuning knob.
    let waves_per_region = (region_w as f32 / region_h as f32 * 2.) * period_scale;

    // LMS2006SO is indexed in 1 nm steps starting at 390 nm; the legacy bar samples 350-750 nm. The constants below match the legacy off-by-zero treatment — START_NM = 350 means LAMBDA_START = 0 and LAMBDA_END = 400, indexing the first 400 nm-steps of the table.
    const START_NM: usize = 350;
    const LAMBDA_START: usize = 350 - START_NM;
    const LAMBDA_END: usize = 750 - START_NM;

    // α + darkness: stored RGB = visible RGB XOR 0x00FFFFFF; alpha is the top byte and is preserved through the blend.
    const VISIBLE_FLIP: u32 = 0x00FFFFFF;
    const RGB_MASK: u32 = 0x00FFFFFF;
    const ALPHA_MASK: u32 = 0xFF000000;

    let logo_height_f = logo_height as f32;
    let two_logo_height_f = (logo_height * 2) as f32;

    for y in 0..region_h {
        let py = y0 + y;
        // py < y1 ≤ buf_h, x bounds checked similarly below — indexes proven in-bounds by loop invariants (no bounds-check needed at the pixel write).
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

            // Wavelength index runs blue → red as x_flipped decreases; `region_w.max(1)` is a no-op guard since `region_w ≥ 1` follows from the early-return on `x0 >= x1`, but kept verbatim to match legacy.
            let wavelength_idx = LAMBDA_START
                + ((region_w - 1 - x) * (LAMBDA_END - LAMBDA_START)) / region_w;
            let lms_idx = wavelength_idx * 3;

            let l = LMS2006SO[lms_idx];
            let m = LMS2006SO[lms_idx + 1];
            let s = LMS2006SO[lms_idx + 2];

            // LMS → REC2020 magic 9 (identical numeric values to compositing.rs:5148-5154 and to colour.rs's LMS2REC2020 matrix; trailing zeros from the legacy 16-digit form stripped — f32 has ~7 digits of precision so the trimmed last digit was already noise).
            let r =  3.16824109881169 * l + -2.15688285649183 * m +  0.0964568792112096 * s;
            let g = -0.266362510245695 * l +  1.40494573257753 * m + -0.175554801656117  * s;
            let b =  0.00389152987374033 * l + -0.0205676800313948 * m + 0.945832607950864 * s;

            let idx = row_base + px;
            let existing = canvas.pixels[idx];
            // Extract visible RGB from α + darkness storage. Alpha stays unchanged.
            let alpha = existing & ALPHA_MASK;
            let visible = (existing & RGB_MASK) ^ VISIBLE_FLIP;
            let r_bg = ((visible >> 16) & 0xFF) as f32;
            let g_bg = ((visible >> 8) & 0xFF) as f32;
            let b_bg = (visible & 0xFF) as f32;

            // Square-root-of-sum-of-squares additive blend; `as u8` saturates [0, 255] (legacy semantics: out-of-gamut values clip to gamut edge, NaN → 0).
            let r_new = (r * scale + r_bg * r_bg).sqrt() as u8 as u32;
            let g_new = (g * scale + g_bg * g_bg).sqrt() as u8 as u32;
            let b_new = (b * scale + b_bg * b_bg).sqrt() as u8 as u32;

            let visible_new = (r_new << 16) | (g_new << 8) | b_new;
            canvas.pixels[idx] = ((visible_new ^ VISIBLE_FLIP) & RGB_MASK) | alpha;
        }
    }
}
