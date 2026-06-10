//! Avatar render — Mitchell resize + textured AA circle into a fluor `Canvas`.
//!
//! Inputs are γ=2.0 BT.2020 u8 RGB (already through `colour_convert::vsf_rgb_to_bt2020`); the buffer is α + darkness packed (see `fluor::pixel`). The render path samples each output pixel from the pre-scaled texture, converts it to a darkness-packed pixel via `fluor::theme::dark(fmt(visible))`, and composes through `under()` — matching the convention every other photon rasterizer follows.

use fluor::canvas::Canvas;
use fluor::coord::Coord;
use fluor::paint::Clip;
use fluor::pixel::{Blend, BlendMode};

/// Mitchell-filtered square resize of a 3-byte-per-pixel image. Input and output are γ=2.0 RGB triples (so this is technically not gamma-correct resampling, but it matches legacy photon behaviour and is visually acceptable; doing the resize in linear is a follow-up).
pub fn update_avatar_scaled(src: &[u8], src_size: usize, dst_diameter: usize) -> Vec<u8> {
    use resize::Pixel::RGB8;
    use resize::Type::Mitchell;

    let mut resizer = resize::new(src_size, src_size, dst_diameter, dst_diameter, RGB8, Mitchell)
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
    resizer.resize(src_rgb, dst_rgb).expect("avatar resize failed");
    dst
}

/// Paint a circular avatar at `(cx, cy)` with fractional `radius`, sampling from a `scaled_diameter × scaled_diameter` BT.2020 γ=2.0 RGB texture. AA edge over the outer half-pixel; composes via `under()` so the caller can paint avatars on top of an existing partial composite.
///
/// `ring` is a future hook for the connectivity-indicator ring; currently unused — pass `None`.
pub fn draw_avatar(
    canvas: &mut Canvas,
    cx: Coord,
    cy: Coord,
    radius: Coord,
    scaled: &[u8],
    scaled_diameter: usize,
    _ring: Option<u32>,
) {
    let width = canvas.width;
    let height = canvas.height;
    if radius <= 0.0 || scaled_diameter == 0 || width == 0 || height == 0 {
        return;
    }
    let r_in = (radius - 0.5).max(0.0);
    let r_out = radius + 0.5;
    let r_in2 = r_in * r_in;
    let r_out2 = r_out * r_out;
    let inv_diff = 1.0 / (r_out2 - r_in2);
    let x_min = (cx - r_out) as i32;
    let x_max = (cx + r_out + 1.0) as i32;
    let y_min = (cy - r_out) as i32;
    let y_max = (cy + r_out + 1.0) as i32;
    let Some((x_start, y_start, x_end, y_end)) =
        Clip::intersect_bbox(None, width, height, x_min, x_max, y_min, y_max)
    else {
        return;
    };
    canvas.damage.add_bounds(x_start, y_start, x_end, y_end);
    let pixels: &mut [u32] = canvas.pixels;
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
            let r = scaled[tex_idx] as u32;
            let g = scaled[tex_idx + 1] as u32;
            let b = scaled[tex_idx + 2] as u32;
            let visible = (r << 16) | (g << 8) | b;
            let dark = fluor::theme::dark(fluor::theme::fmt(visible)) & 0x00FFFFFF;
            let alpha = if dist2 <= r_in2 {
                0xFFu32
            } else {
                let t = (r_out2 - dist2) * inv_diff;
                (255.0 * t) as u32
            };
            if alpha == 0 {
                continue;
            }
            let avatar_pixel = (alpha << 24) | dark;
            let idx = row_off + px;
            pixels[idx] = pixels[idx].under(avatar_pixel, BlendMode::Normal);
        }
    }
}
