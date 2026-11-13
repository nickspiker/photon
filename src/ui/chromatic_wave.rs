//! Chromatic wave: a sine-modulated visible-spectrum colour bar, a light layer of its own composed front to back thru fluor's `under`. Port of `compositing.rs::draw_spectrum` (compositing.rs:5092-5168), whose quadrature add put the wave's light ONTO the finished background.
//!
//! Front to back (owner, 2026-11-07): the wave goes in after what sits in front of it (the logo) and before what sits behind it (the noise), and reads nothing behind it. Each pixel is the wave's light at an α of its own (premultiplied, light = α − darkness), so over black it sheds exactly the light the old add put there, and the noise behind shows thru its faint tails and is covered by its bright core.
//!
//! Colour at each x sweeps the 350-750 nm band of the visible spectrum (sampled via [`super::lms2006so::LMS2006SO`] and converted to RGB via the LMS → REC2020 magic 9 coefficients inlined below). The y modulation is a sine whose **frequency** ramps up toward the blue end (logarithmic) and whose **amplitude** rises toward blue too; `period_scale` multiplies the base `waves_per_region` so the same shape stretches/compresses globally.

use super::lms2006so::LMS2006SO;
use fluor::canvas::PixelRect;
use fluor::canvas::Canvas;
use fluor::pixel::{BlendMode, Pixel};
use std::f32::consts::TAU;

/// Paint a chromatic wave covering `rect` of `canvas`. `phase` is the wave's horizontal phase shift in radians (advance over time to animate); `period_scale` multiplies the base waves-per-region (`1.` = legacy density, `>1.` = more waves, `<1.` = fewer waves).
pub fn chromatic_wave<P: Pixel>(canvas: &mut Canvas<P>, rect: PixelRect, phase: f32, period_scale: f32) {
    let h = canvas.height;
    chromatic_wave_clipped(canvas, rect.x0, rect.x1, rect.y0 as isize, rect.y1 - rect.y0, 0, h, phase, period_scale);
}


/// The wave's terms that depend on the column alone, everything but the row's `scale`: computed once a column, not once a pixel (ferros, 2026-11-13: the powf and the sin ran for every pixel of a 1344 x 789 band, the same values down every column).
struct Column {
    /// The sine's vertical offset at this column: its phase, frequency ramp and amplitude folded in.
    offset: f32,
    /// The scale divisor's floor at this column, `amplitude / region_span / 4`.
    floor: f32,
    /// The spectrum's colour at this column, LMS to REC2020.
    rgb: [f32; 3],
}

/// Steps of [`tanh_table`] over [0, 1].
const TANH_STEPS: usize = 1 << 12;

/// tanh at `TANH_STEPS` even steps over [0, 1], and one step past 1 so the interpolation's upper neighbour exists at 1 itself.
/// A depth wider than 8 bits encodes the wave's light as gamma2tanh, tanh(√light): read here and interpolated instead of VSF's settled encoder, which called sqrt, tanh and at least two atanh for each of a pixel's three channels (ferros, 2026-11-13: most of a 580 ms frame on the attest screen).
/// The interpolation's error is under a millionth of a code at 16 bits (tanh'' is under 0.77, the step 2⁻¹²); the estimate is truncated, as VSF's is before it settles, so a code can land one either side of VSF's exact one (`the_tanh_table_codes_are_vsfs_within_a_code`): well under what any panel shows.
fn tanh_table() -> &'static [f32; TANH_STEPS + 2] {
    static TABLE: std::sync::OnceLock<[f32; TANH_STEPS + 2]> = std::sync::OnceLock::new();
    TABLE.get_or_init(|| core::array::from_fn(|i| (i as f32 / TANH_STEPS as f32).tanh()))
}

/// The wave's light `c × scale` in one channel as the depth's code over black: the same light the old add put on a black background, capped at diffuse white the same way (where γ2 255 used to clip; past it is a 9.7× glow on HDR). Negative or NaN light (the matrix's negative lobes) is no light.
/// 8 bits: fluor's tables, as always. A wider depth: tanh(√light) from [`tanh_table`].
#[inline]
fn wave_code<P: Pixel>(c: f32, scale: f32, tanh: &[f32; TANH_STEPS + 2]) -> u32 {
    if P::DEPTH == 8 {
        fluor::theme::g2t_code(((c * scale).max(0.0) * (65536.0 / 65025.0)).min(65536.0) as u32)
    } else {
        let u = ((c * scale).max(0.0) * (1. / 65025.0)).min(1.).sqrt() * TANH_STEPS as f32;
        // u is in [0, TANH_STEPS] (the light is capped at 1 above), so i ≤ TANH_STEPS and i + 1 is the table's last entry at most.
        let i = u as usize;
        let t = tanh[i] + (tanh[i + 1] - tanh[i]) * (u - i as f32);
        (t * (1u32 << P::DEPTH) as f32) as u32
    }
}

/// CROP-not-shrink variant (the About slab, 2026-09-02): the wave's pattern is computed over its FULL logical band (`top`..`top+logical_h`, `top` may be negative when scrolled past the pane top) and only rows inside `clip_y0..clip_y1` (∩ the buffer) are painted. Shrinking the rect instead RESCALES the pattern — the "wave scales when scrolling" field bug.
/// The rows go to the worker threads (rayon, as the wave field's paint does): each pixel depends on its row and column alone.
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
    // The canvas rows painted: the band's rows inside the clip and the buffer.
    let dy0 = top.max(clip_y0 as isize).max(0) as usize; // WHY/PROOF: the band's top can sit above the canvas (negative isize), and the usize cast would wrap it to a huge row
    let dy1 = ((top + region_h as isize).max(0) as usize).min(clip_y1); // WHY/PROOF: as above for the bottom edge
    if dy1 <= dy0 {
        return;
    }
    canvas.damage.add(PixelRect::new(x0, dy0, x1, dy1));

    // Harmonic mean of region dims for brightness scaling — keeps the bar's overall energy stable across aspect ratios. Matches legacy line 5103. The `2` is part of the harmonic-mean formula, not a tuning knob.
    let region_span = 2. * region_w as f32 * region_h as f32 / (region_w as f32 + region_h as f32);
    // Base waves count = aspect × 2 (legacy line 5106), then multiplied by the scroll-driven period_scale. The `2` is the legacy waves-per-aspect ratio, not a tuning knob.
    let waves_per_region = (region_w as f32 / region_h as f32 * 2.) * period_scale;

    // LMS2006SO is indexed in 1 nm steps starting at 390 nm; the legacy bar samples 350-750 nm. The constants below match the legacy off-by-zero treatment — START_NM = 350 means LAMBDA_START = 0 and LAMBDA_END = 400, indexing the first 400 nm-steps of the table.
    const START_NM: usize = 350;
    const LAMBDA_START: usize = 350 - START_NM;
    const LAMBDA_END: usize = 750 - START_NM;

    let logo_height_f = logo_height as f32;
    let two_logo_height_f = (logo_height * 2) as f32;

    // Each column's terms, once.
    let columns: Vec<Column> = (0..region_w)
        .map(|x| {
            // Flip x for wave calculations so the wave's frequency ramp matches the spectrum direction (blue on the left where x_flipped is largest).
            let x_flipped = region_w - 1 - x;
            let x_norm = x_flipped as f32 / region_w as f32;
            // 12 is the legacy amplitude-curve magic number — algorithm constant, not a tuning knob, so left as-is.
            let amplitude = logo_height_f / (1. + 12. * x_norm);
            // Logarithmic frequency ramp: ~1× at red end (x_norm=0), ~4× at blue end (x_norm=1) — exactly the legacy 2^(-x_norm + 2) shape. Both 2s here are powers-of-two-as-bases / exponents, intrinsic to the algorithm.
            let freq_ramp = 2_f32.powf(-x_norm + 2.);
            let wave_phase = x_norm * TAU * waves_per_region * freq_ramp - phase;
            // Wavelength index runs blue → red as x_flipped decreases; `region_w ≥ 1` follows from the early-return on `x0 >= x1`, so the division needs no guard.
            let wavelength_idx = LAMBDA_START + (x_flipped * (LAMBDA_END - LAMBDA_START)) / region_w;
            let lms_idx = wavelength_idx * 3;
            let l = LMS2006SO[lms_idx];
            let m = LMS2006SO[lms_idx + 1];
            let s = LMS2006SO[lms_idx + 2];
            // LMS → REC2020 magic 9 (identical numeric values to compositing.rs:5148-5154 and to colour.rs's LMS2REC2020 matrix; trailing zeros from the legacy 16-digit form stripped — f32 has ~7 digits of precision so the trimmed last digit was already noise).
            let r = 3.16824109881169 * l + -2.15688285649183 * m + 0.0964568792112096 * s;
            let g = -0.266362510245695 * l + 1.40494573257753 * m + -0.175554801656117 * s;
            let b = 0.00389152987374033 * l + -0.0205676800313948 * m + 0.945832607950864 * s;
            // `1 << 2`: the divisor's floor, a tuning knob of the legacy shape.
            Column { offset: wave_phase.sin() * amplitude, floor: amplitude / region_span * (1. / ((1 << 2) as f32)), rgb: [r, g, b] }
        })
        .collect();
    let tanh = tanh_table();

    // α + darkness: a channel's darkness is ONE − its visible code.
    let one = P::ONE;
    let one64 = one as u64;
    use rayon::prelude::*;
    canvas.pixels[dy0 * buf_w..dy1 * buf_w].par_chunks_mut(buf_w).enumerate().for_each(|(k, row)| {
        // Canvas row → logical row of the band: dy0 ≥ top, so py − top ≥ 0.
        let py = dy0 + k;
        let y_f = (py as isize - top) as f32;
        // The row's share of the brightness, the same down the row.
        let row_gain = ((two_logo_height_f - y_f) / logo_height_f) * (y_f / logo_height_f) * (1 << 15) as f32;
        for (col, px) in columns.iter().zip(&mut row[x0..x1]) {
            // `1 << 15` (= 32768) replaces the legacy `32000.` brightness multiplier — a single-digit tuning knob.
            let scale = row_gain / (((y_f + col.offset - logo_height_f) / logo_height_f).abs() + col.floor);
            let lit = col.rgb.map(|c| wave_code::<P>(c, scale, tanh));

            // The wave is a light layer of its own, composed UNDER whatever already sits in front of it (the logo; the noise goes in after, behind it).
            // Its α is the union of the three channels' light as opacities, ONE − ∏(ONE − code): C∞ in every code, and at least each channel's code, so every channel's light fits under it (premultiplied, light = α − darkness).
            // Each product is floored, which only shrinks the clear part, so α ≥ every code holds exactly in the integers too; u64 because two 16-bit complements multiply to just under 2³².
            let [kr, kg, kb] = lit.map(|v| (one - v) as u64);
            let alpha = one - (kr * kg / one64 * kb / one64) as u32;
            // WHY: α is the divisor that turns the premultiplied light into the straight darkness `under` takes as its bottom.
            // PROOF: α = 0 exactly when every channel's code is 0, where the layer is empty, and an empty bottom under anything leaves it unchanged, so skipping is the same pixel without the division by zero.
            if alpha == 0 {
                continue;
            }
            // Straight darkness: the visible code over α, complemented. code ≤ α (above), so code × ONE / α ≤ ONE and the darkness lands in 0..=ONE.
            let dark = lit.map(|v| one - (v as u64 * one64 / alpha as u64) as u32);
            // fluor's channels are in storage order: on Android RGBA_8888 read as little-endian u32 puts dark_B at the top and dark_R at the bottom, so swap into it.
            #[cfg(target_os = "android")]
            let dark = [dark[2], dark[1], dark[0]];
            *px = px.under(P::from_channels(alpha, dark), BlendMode::Normal);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use fluor::pixel::{Argb8, Blend, G2t16};

    /// The wave at both depths into an empty buffer, then opaque black under it, read back at 8 bits: the light agrees within 3 codes (the 8-bit `under` floors by up to a code at each of its two steps, black under an empty pixel alone reading one code light, and the dim buckets near black differ between vsf's 16-bit codec and fluor's Q16 tables), every pixel ends opaque, and the wave lights something.
    #[test]
    fn the_wave_sheds_the_same_light_at_both_depths() {
        let (w, h) = (96usize, 48usize);
        let mut p8 = vec![Argb8::EMPTY; w * h];
        let mut p16 = vec![G2t16::EMPTY; w * h];
        let mut dmg = fluor::canvas::Damage::new();
        let rect = PixelRect::new(0, 0, w, h);
        chromatic_wave(&mut Canvas::new(&mut p8, w, h, &mut dmg), rect, 0.5, 1.);
        chromatic_wave(&mut Canvas::new(&mut p16, w, h, &mut dmg), rect, 0.5, 1.);
        let black8 = Argb8::from_channels(Argb8::ONE, [Argb8::ONE; 3]);
        let black16 = G2t16::from_channels(G2t16::ONE, [G2t16::ONE; 3]);
        let mut lit = 0;
        for i in 0..w * h {
            let (a, b) = (p8[i].under(black8, BlendMode::Normal), p16[i].under(black16, BlendMode::Normal));
            assert_eq!(a.alpha(), Argb8::ONE);
            assert_eq!(b.alpha(), G2t16::ONE);
            for (da, db) in a.channels().into_iter().zip(b.channels()) {
                assert!(((db >> 8) as i32 - da as i32).abs() <= 3, "pixel {i}: {a:08x} against {b:016x}");
                lit += (da < Argb8::ONE) as u32;
            }
        }
        assert!(lit > 0, "the wave lights something");
    }

    /// The 16-bit codes from the tanh table against VSF's settled encoder over the whole light range the wave uses (0 to diffuse white): at most one code apart.
    #[test]
    fn the_tanh_table_codes_are_vsfs_within_a_code() {
        let tanh = tanh_table();
        let steps = 1u32 << 20;
        for k in 0..=steps {
            let light = k as f32 / steps as f32;
            // wave_code takes the light as c × scale over 65025 (the old add's 255²): c = 65025, scale = light.
            let ours = wave_code::<G2t16>(65025., light, tanh) as i64;
            let vsfs = vsf::colour::convert::delinearize_gamma2tanh_bits_f32(light, 16) as i64;
            assert!((vsfs - ours).abs() <= 1, "light {light}: ours {ours}, VSF's {vsfs}");
        }
    }
}
