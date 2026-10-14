//! The beam's resampler (beam_ladder.rs): a camera frame → the coded size at the ladder's softness, so the encoder's coded size changes only at a band crossing. Integer throughout.
//!
//! Three steps per plane: CROP the source to 16:9 about its centre (a 4:3 camera loses its top and bottom bands; a sensor-native portrait-mounted frame is still landscape, so this is its long axis), SHRINK to the effective size by box averaging (every source pixel counted once — no aliasing), GROW back to the coded size by linear interpolation in Q8. With no softness the middle step is skipped and the crop is resampled straight to the coded size.
//!
//! Planes are addressed by (row stride, pixel stride), so the same code reads a camera Image's planes and writes a MediaCodec input Image's planes in place (Android's chroma planes often step two bytes per pixel).

/// One plane as raw memory: `ptr` at (0,0), `row` bytes per row, `px` bytes per pixel.
#[derive(Clone, Copy)]
pub struct Plane {
    pub ptr: *mut u8,
    pub len: usize,
    pub row: usize,
    pub px: usize,
    pub w: usize,
    pub h: usize,
}

impl Plane {
    #[inline]
    fn at(&self, x: usize, y: usize) -> u8 {
        let i = y * self.row + x * self.px;
        debug_assert!(i < self.len);
        // SAFETY: callers build planes from buffers of `len` bytes whose geometry covers w×h at these strides (checked in `fits`).
        unsafe { *self.ptr.add(i) }
    }
    #[inline]
    fn put(&self, x: usize, y: usize, v: u8) {
        let i = y * self.row + x * self.px;
        debug_assert!(i < self.len);
        unsafe { *self.ptr.add(i) = v }
    }
    /// Does the buffer hold every pixel the geometry names?
    pub fn fits(&self) -> bool {
        self.w > 0 && self.h > 0 && self.px > 0 && (self.h - 1) * self.row + (self.w - 1) * self.px < self.len
    }
    /// Copy the top-left w×h of the plane into a packed buffer (row = w, one byte per pixel).
    pub fn copy_to(&self, out: &mut [u8], w: usize, h: usize) {
        let (w, h) = (w.min(self.w), h.min(self.h));
        for y in 0..h {
            for x in 0..w {
                out[y * w + x] = self.at(x, y);
            }
        }
    }
    pub fn from_slice(buf: &mut [u8], row: usize, px: usize, w: usize, h: usize) -> Plane {
        Plane { ptr: buf.as_mut_ptr(), len: buf.len(), row, px, w, h }
    }
}

/// The centred 16:9 window of a w×h frame: (x0, y0, cw, ch), even.
pub fn crop_16x9(w: usize, h: usize) -> (usize, usize, usize, usize) {
    if w * 9 >= h * 16 {
        let cw = (h * 16 / 9) & !1;
        ((w - cw) / 2 & !1, 0, cw, h & !1)
    } else {
        let ch = (w * 9 / 16) & !1;
        (0, (h - ch) / 2 & !1, w & !1, ch)
    }
}

/// Box-average the (x0,y0,cw,ch) window of `src` into an ew×eh scratch plane (ew ≤ cw, eh ≤ ch).
fn shrink(src: &Plane, x0: usize, y0: usize, cw: usize, ch: usize, ew: usize, eh: usize, out: &mut [u8]) {
    for oy in 0..eh {
        let sy0 = y0 + oy * ch / eh;
        let sy1 = (y0 + (oy + 1) * ch / eh).max(sy0 + 1);
        for ox in 0..ew {
            let sx0 = x0 + ox * cw / ew;
            let sx1 = (x0 + (ox + 1) * cw / ew).max(sx0 + 1);
            let mut sum = 0u32;
            for sy in sy0..sy1 {
                for sx in sx0..sx1 {
                    sum += src.at(sx, sy) as u32;
                }
            }
            let n = ((sy1 - sy0) * (sx1 - sx0)) as u32;
            out[oy * ew + ox] = (sum / n) as u8;
        }
    }
}

/// Linear-interpolate a sw×sh window (read thru `get`) onto every pixel of `dst`, Q8, truncating.
fn grow(get: impl Fn(usize, usize) -> u8, sw: usize, sh: usize, dst: &Plane) {
    let (dw, dh) = (dst.w, dst.h);
    for dy in 0..dh {
        // Source coordinate of the destination pixel centre, Q8: ((dy + ½)·sh/dh − ½)·256.
        let fy = (((2 * dy + 1) * sh * 128) / dh).saturating_sub(128);
        let y0 = (fy >> 8).min(sh - 1);
        let y1 = (y0 + 1).min(sh - 1);
        let wy = (fy & 255) as u32;
        for dx in 0..dw {
            let fx = (((2 * dx + 1) * sw * 128) / dw).saturating_sub(128);
            let x0 = (fx >> 8).min(sw - 1);
            let x1 = (x0 + 1).min(sw - 1);
            let wx = (fx & 255) as u32;
            let top = get(x0, y0) as u32 * (256 - wx) + get(x1, y0) as u32 * wx;
            let bot = get(x0, y1) as u32 * (256 - wx) + get(x1, y1) as u32 * wx;
            dst.put(dx, dy, ((top * (256 - wy) + bot * wy) >> 16) as u8);
        }
    }
}

/// One plane: crop `src` to 16:9, soften by `soft` half-stops, land on `dst` (the coded size).
pub fn resample_plane(src: &Plane, dst: &Plane, soft: u8) {
    if !src.fits() || !dst.fits() {
        return;
    }
    let (x0, y0, cw, ch) = crop_16x9(src.w, src.h);
    let (cw, ch) = (cw.max(1), ch.max(1));
    let scale = 2f64.powf(-(soft.min(4) as f64) / 2.0);
    let ew = ((dst.w as f64 * scale).round() as usize).clamp(1, dst.w).min(cw);
    let eh = ((dst.h as f64 * scale).round() as usize).clamp(1, dst.h).min(ch);
    if soft == 0 && dst.w <= cw && dst.h <= ch {
        // Full detail and a downscale: box-average the crop straight onto the coded grid.
        let mut tmp = vec![0u8; dst.w * dst.h];
        shrink(src, x0, y0, cw, ch, dst.w, dst.h, &mut tmp);
        for y in 0..dst.h {
            for x in 0..dst.w {
                dst.put(x, y, tmp[y * dst.w + x]);
            }
        }
        return;
    }
    // Soften: shrink to the effective size, then grow back to the coded size.
    let mut tmp = vec![0u8; ew * eh];
    shrink(src, x0, y0, cw, ch, ew, eh, &mut tmp);
    grow(|x, y| tmp[y * ew + x], ew, eh, dst);
}

/// A whole I420-shaped frame (Y full size, U and V half in each axis): the three planes, each with its own strides.
pub fn resample_frame(src: [Plane; 3], dst: [Plane; 3], soft: u8) {
    for i in 0..3 {
        resample_plane(&src[i], &dst[i], soft);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_16x9_crop_is_centred_and_even() {
        assert_eq!(crop_16x9(640, 480), (0, 60, 640, 360));
        assert_eq!(crop_16x9(1280, 720), (0, 0, 1280, 720));
        assert_eq!(crop_16x9(1000, 400), (144, 0, 710, 400));
    }

    #[test]
    fn a_flat_plane_stays_flat_at_every_softness() {
        let mut s = vec![77u8; 640 * 480];
        let src = Plane::from_slice(&mut s, 640, 1, 640, 480);
        for soft in 0..=4u8 {
            let mut d = vec![0u8; 320 * 180];
            let dst = Plane::from_slice(&mut d, 320, 1, 320, 180);
            resample_plane(&src, &dst, soft);
            assert!(d.iter().all(|&v| v == 77 || v == 76), "soft {soft}: a flat field must survive (truncation may shave one code)");
        }
    }

    #[test]
    fn softness_removes_fine_detail_and_keeps_coarse() {
        // A one-pixel checkerboard (finest detail) and a half-width split (coarsest) on a 16:9 source.
        let (w, h) = (640, 360);
        let mut fine: Vec<u8> = (0..w * h).map(|i| if (i % w + i / w) % 2 == 0 { 0 } else { 200 }).collect();
        let mut coarse: Vec<u8> = (0..w * h).map(|i| if i % w < w / 2 { 0 } else { 200 }).collect();
        let fsrc = Plane::from_slice(&mut fine, w, 1, w, h);
        let csrc = Plane::from_slice(&mut coarse, w, 1, w, h);
        let mut d = vec![0u8; w * h];
        let dst = Plane::from_slice(&mut d, w, 1, w, h);
        resample_plane(&fsrc, &dst, 2);
        let spread = d.iter().map(|&v| v as i32).max().unwrap() - d.iter().map(|&v| v as i32).min().unwrap();
        assert!(spread < 20, "a checkerboard softened one stop is near-uniform grey, spread {spread}");
        resample_plane(&csrc, &dst, 2);
        assert!(d[h / 2 * w + 10] < 20 && d[h / 2 * w + w - 10] > 180, "the half split survives");
    }

    #[test]
    fn strided_planes_are_read_and_written_in_place() {
        // A 4×2 chroma plane stored two bytes per pixel (Android's semi-planar layout), into a packed 2×1 destination.
        let mut s = vec![0u8; 8 * 2];
        for y in 0..2 {
            for x in 0..4 {
                s[y * 8 + x * 2] = 100;
            }
        }
        let src = Plane::from_slice(&mut s, 8, 2, 4, 2);
        let mut d = vec![0u8; 2];
        let dst = Plane::from_slice(&mut d, 2, 1, 2, 1);
        resample_plane(&src, &dst, 0);
        assert_eq!(d, vec![100, 100]);
    }
}
