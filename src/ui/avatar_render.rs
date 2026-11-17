//! Avatar render — Mitchell resize + textured AA circle into a fluor `Canvas`.
//!
//! Inputs are γ=2.0 BT.2020 u8 RGB (already thru `colour_convert::vsf_rgb_to_bt2020`); the buffer is α + darkness packed (see `fluor::pixel`). The render path samples each output pixel from the pre-scaled texture, converts it to a darkness-packed pixel via `fluor::theme::dark(fmt(visible))`, and composes thru `under()` — matching the convention every other photon rasterizer follows.

use crate::ui::Depth;
use fluor::canvas::Canvas;
use fluor::coord::Coord;
use fluor::paint::Clip;
use fluor::pixel::BlendMode;

/// Mitchell-filtered square resize of a 3-byte-per-pixel image. Input and output are γ=2.0 RGB triples (so this is technically not gamma-correct resampling, but it matches legacy photon behaviour and is visually acceptable; doing the resize in linear is a follow-up).
pub fn update_avatar_scaled(src: &[u8], src_size: usize, dst_diameter: usize) -> Vec<u8> {
    use resize::Pixel::RGB8;
    use resize::Type::Mitchell;
    // A zero-pixel avatar is an empty texture, never a panic: the 2026-10-15 monitor hotplug shrank X's framebuffer to 320×200, the avatar diameter rounded to 0, resize refused it and the expect below aborted the whole app. draw_avatar draws nothing for a zero diameter.
    if dst_diameter == 0 || src_size == 0 || src.len() < src_size * src_size * 3 {
        return Vec::new();
    }

    let mut resizer = resize::new(
        src_size,
        src_size,
        dst_diameter,
        dst_diameter,
        RGB8,
        Mitchell,
    )
    .expect("avatar resize: failed to build resizer");
    let mut dst = vec![0u8; dst_diameter * dst_diameter * 3];
    let src_rgb: &[rgb::RGB8] = unsafe {
        core::slice::from_raw_parts(src.as_ptr() as *const rgb::RGB8, src_size * src_size)
    };
    let dst_rgb: &mut [rgb::RGB8] = unsafe {
        core::slice::from_raw_parts_mut(
            dst.as_mut_ptr() as *mut rgb::RGB8,
            dst_diameter * dst_diameter,
        )
    };
    resizer
        .resize(src_rgb, dst_rgb)
        .expect("avatar resize failed");
    dst
}

/// Paint a circular avatar at `(cx, cy)` with fractional `radius`, sampling from a `scaled_diameter × scaled_diameter` BT.2020 γ=2.0 RGB texture. AA edge over the outer half-pixel; composes via `under()` so the caller can paint avatars on top of an existing partial composite. `clip` restricts painting to a sub-rect (e.g. a scrolling list's visible region); `None` = whole buffer.
pub fn draw_avatar<P: Depth>(
    canvas: &mut Canvas<P>,
    cx: Coord,
    cy: Coord,
    radius: Coord,
    scaled: &[u8],
    scaled_diameter: usize,
    clip: Option<Clip>,
) {
    let width = canvas.width;
    let height = canvas.height;
    if radius <= 0.0 || scaled_diameter == 0 || scaled.len() < scaled_diameter * scaled_diameter * 3 || width == 0 || height == 0 {
        return;
    }
    let r_in = (radius - 0.5).max(0.0); // the algorithm: the anti-aliased rim's inner radius; a disk under half a pixel has no solid interior
    let r_out = radius + 0.5;
    let r_in2 = r_in * r_in;
    let r_out2 = r_out * r_out;
    let inv_diff = 1.0 / (r_out2 - r_in2);
    let x_min = (cx - r_out) as i32;
    let x_max = (cx + r_out + 1.0) as i32;
    let y_min = (cy - r_out) as i32;
    let y_max = (cy + r_out + 1.0) as i32;
    let Some((x_start, y_start, x_end, y_end)) =
        Clip::intersect_bbox(clip, width, height, x_min, x_max, y_min, y_max)
    else {
        return;
    };
    canvas.damage.add_bounds(x_start, y_start, x_end, y_end);
    let pixels: &mut [P] = canvas.pixels;
    let diam = scaled_diameter as Coord;

    for py in y_start..y_end {
        let dy = (py as Coord + 0.5) - cy;
        let dy2 = dy * dy;
        let tex_y_f = dy + radius;
        if tex_y_f < 0.0 || tex_y_f >= diam {
            continue;
        }
        let tex_y = tex_y_f as usize;
        let tex_row = tex_y * scaled_diameter * 3;
        let row_off = py * width;
        for px in x_start..x_end {
            let dx = (px as Coord + 0.5) - cx;
            let dist2 = dx * dx + dy2;
            if dist2 >= r_out2 {
                continue;
            }
            let tex_x_f = dx + radius;
            if tex_x_f < 0.0 || tex_x_f >= diam {
                continue;
            }
            let tex_x = tex_x_f as usize;
            let tex_idx = tex_row + tex_x * 3;
            // Defensive: never index past the texture even if the caller's `scaled_diameter` disagrees with the actual `scaled` buffer size (a stale per-contact cache built at a different diameter — this used to panic on conversation-open). Skip the pixel rather than crash; the correct fix is the caller rebuilding the cache at the right diameter, but this makes any future mismatch a cosmetic glitch, not a crash.
            if tex_idx + 2 >= scaled.len() {
                continue;
            }
            let r = scaled[tex_idx] as u32;
            let g = scaled[tex_idx + 1] as u32;
            let b = scaled[tex_idx + 2] as u32;
            let visible = (r << 16) | (g << 8) | b;
            // Coverage in the depth's own scale (the AA rim's ramp, `t` in (0, 1) there).
            let alpha = if dist2 <= r_in2 {
                P::ONE
            } else {
                let t = (r_out2 - dist2) * inv_diff;
                P::cov(t)
            };
            if alpha == 0 {
                continue;
            }
            // The texel's γ2 light at the depth's precision ([`Depth::from_gamma2`]: fluor's 8-bit conversion, or the 16-bit code of the byte's exact light).
            let avatar_pixel = P::from_gamma2(alpha, visible);
            let idx = row_off + px;
            pixels[idx] = pixels[idx].under(avatar_pixel, BlendMode::Normal);
        }
    }
}

/// [`draw_avatar`] for a picture already at the canvas's depth: `src` is a `d × d` square, premultiplied and opaque (a mark's 256 square resampled to `d`, black past its disc), drawn centred on (`cx`, `cy`) with the circle of `radius` cut at this size, its rim anti-aliased over one pixel, under what is on the canvas.
pub fn draw_avatar_px<P: Depth>(canvas: &mut Canvas<P>, cx: Coord, cy: Coord, radius: Coord, src: &[P], d: usize, clip: Option<Clip>) {
    let (width, height) = (canvas.width, canvas.height);
    if radius <= 0.0 || d == 0 {
        return;
    }
    let r_in = (radius - 0.5).max(0.0); // the algorithm: the anti-aliased rim's inner radius; a disk under half a pixel has no solid interior
    let r_out = radius + 0.5;
    let (r_in2, r_out2) = (r_in * r_in, r_out * r_out);
    let inv_diff = 1.0 / (r_out2 - r_in2);
    let Some((x_start, y_start, x_end, y_end)) = Clip::intersect_bbox(clip, width, height, (cx - r_out) as i32, (cx + r_out + 1.0) as i32, (cy - r_out) as i32, (cy + r_out + 1.0) as i32) else {
        return;
    };
    canvas.damage.add_bounds(x_start, y_start, x_end, y_end);
    // Square texels per canvas pixel: the square spans the circle's diameter.
    let scale = d as Coord / (2.0 * radius);
    let black = P::from_channels(P::ONE, [P::ONE; 3]);
    let one = P::ONE as u64;
    for py in y_start..y_end {
        let dy = (py as Coord + 0.5) - cy;
        let ty = ((dy + radius) * scale).floor() as isize;
        for px in x_start..x_end {
            let dx = (px as Coord + 0.5) - cx;
            let dist2 = dx * dx + dy * dy;
            if dist2 >= r_out2 {
                continue;
            }
            let cov = if dist2 <= r_in2 { P::ONE } else { P::cov((r_out2 - dist2) * inv_diff) };
            if cov == 0 {
                continue;
            }
            let tx = ((dx + radius) * scale).floor() as isize;
            // The rim reaches half a pixel past the radius, which is past the square's edge there: the square is black past its disc, so a texel beyond its edge IS black, by the square's own definition.
            let s = if tx >= 0 && ty >= 0 && (tx as usize) < d && (ty as usize) < d { src[ty as usize * d + tx as usize] } else { black };
            let c = cov as u64;
            let p = P::from_channels((s.alpha() as u64 * c / one) as u32, s.channels().map(|ch| (ch as u64 * c / one) as u32));
            let idx = py * width + px;
            canvas.pixels[idx] = canvas.pixels[idx].under(p, BlendMode::Normal);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The 2026-10-15 hotplug abort: a framebuffer squeezed to 320×200 rounds an avatar to zero pixels — that must scale to nothing and draw nothing, never panic.
    #[test]
    fn a_zero_pixel_avatar_is_empty_not_a_panic() {
        let src = vec![128u8; 4 * 4 * 3];
        assert!(update_avatar_scaled(&src, 4, 0).is_empty());
        assert!(update_avatar_scaled(&src, 0, 8).is_empty());
        assert_eq!(update_avatar_scaled(&src, 4, 2).len(), 2 * 2 * 3);
        let mut px = vec![0u32; 16 * 16];
        let mut dmg = fluor::canvas::Damage::default();
        let mut canvas = Canvas::new(&mut px, 16, 16, &mut dmg);
        draw_avatar(&mut canvas, 8.0, 8.0, 0.4, &[], 0, None);
        draw_avatar(&mut canvas, 8.0, 8.0, 4.0, &[], 8, None);
        let mut px16 = vec![0u64; 16 * 16];
        let mut canvas = Canvas::new(&mut px16, 16, 16, &mut dmg);
        draw_avatar(&mut canvas, 8.0, 8.0, 4.0, &[], 8, None);
    }

    /// A square at the canvas's depth cut to its circle at the drawn size: nothing outside it, the picture whole at its centre, a rim pixel partly covered.
    #[test]
    fn a_square_is_cut_to_its_circle_where_it_is_drawn() {
        use fluor::pixel::{G2t16, Pixel};
        let d = 20;
        let colour = G2t16::from_channels(G2t16::ONE, [0x2000, 0x4000, 0x8000]);
        let src = vec![colour; d * d];
        let mut dmg = fluor::canvas::Damage::default();
        let mut px = vec![G2t16::EMPTY; 32 * 32];
        draw_avatar_px(&mut Canvas::new(&mut px, 32, 32, &mut dmg), 16.0, 16.0, 10.0, &src, d, None);
        // Whole at the centre: exactly what the picture under an empty canvas is (the kernel's own rounding).
        assert_eq!(px[16 * 32 + 16], fluor::pixel::Blend::under(G2t16::EMPTY, colour, BlendMode::Normal));
        assert_eq!(px[0], G2t16::EMPTY);
        assert_eq!(px[16 * 32 + 3], G2t16::EMPTY);
        // The pixel whose centre sits exactly on the radius is half covered.
        let rim = px[16 * 32 + 25];
        assert!(rim.alpha() > 0 && rim.alpha() < G2t16::ONE, "{rim:?}");
    }

    /// The same avatar at both depths, read back at 8 bits: α within a step, darkness at most one step under and three over the 8-bit render — fluor's 8-bit kernels divide by 256 where 255 is meant (the deliberate dark bias its own cross-check pins), which a partly covered rim pixel shows as up to 2 steps.
    #[test]
    fn an_avatar_draws_alike_at_both_depths() {
        use fluor::pixel::Pixel;
        let d = 12;
        let tex: Vec<u8> = (0..d * d * 3).map(|i| (i * 37 % 256) as u8).collect();
        let mut dmg = fluor::canvas::Damage::default();
        let mut p8 = vec![0u32; 16 * 16];
        draw_avatar(&mut Canvas::new(&mut p8, 16, 16, &mut dmg), 8.0, 8.0, 6.0, &tex, d, None);
        let mut p16 = vec![0u64; 16 * 16];
        draw_avatar(&mut Canvas::new(&mut p16, 16, 16, &mut dmg), 8.0, 8.0, 6.0, &tex, d, None);
        for (a, b) in p8.iter().zip(&p16) {
            assert!((a.alpha() as i32 - (b.alpha() >> 8) as i32).abs() <= 1, "α {a:08x} against {b:016x}");
            for (x, y) in a.channels().into_iter().zip(b.channels()) {
                let step = (y >> 8) as i32 - x as i32;
                assert!((-1..=3).contains(&step), "{a:08x} against {b:016x}");
            }
        }
    }
}
