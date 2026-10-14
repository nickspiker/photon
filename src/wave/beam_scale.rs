//! The beam's resampler (beam_ladder.rs): a camera frame → the coded size at the ladder's softness, so the encoder's coded size changes only at a band crossing. Integer throughout.
//!
//! Three steps per plane: CROP the source to the coded size's aspect about its centre — the ladder's bands share the capture's own aspect (beam_ladder::bands_for), so this is at most a pixel of rounding and the picture goes out uncropped — SHRINK to the effective size by box averaging (every source pixel counted once — no aliasing), GROW back to the coded size by linear interpolation in Q8. With no softness the middle step is skipped and the crop is resampled straight to the coded size.
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

/// The centred window of a w×h frame at the aspect aw:ah: (x0, y0, cw, ch), even.
pub fn crop_to(w: usize, h: usize, aw: usize, ah: usize) -> (usize, usize, usize, usize) {
    let (aw, ah) = (aw.max(1), ah.max(1));
    if w * ah >= h * aw {
        let cw = ((h * aw + ah / 2) / ah).min(w) & !1;
        ((w - cw) / 2 & !1, 0, cw, h & !1)
    } else {
        let ch = ((w * ah + aw / 2) / aw).min(h) & !1;
        (0, (h - ch) / 2 & !1, w & !1, ch)
    }
}

/// Box-average the (x0,y0,cw,ch) window of `src` into an ew×eh scratch plane (ew ≤ cw, eh ≤ ch).
fn shrink(src: &Plane, x0: usize, y0: usize, cw: usize, ch: usize, ew: usize, eh: usize, out: &mut [u8]) {
    // The column spans once per plane (a 5 MP capture is ~7 M samples a frame; no division per sample), then whole source rows summed into a row of accumulators.
    let spans: Vec<(usize, usize)> = (0..ew)
        .map(|ox| {
            let a = x0 + ox * cw / ew;
            (a, (x0 + (ox + 1) * cw / ew).max(a + 1))
        })
        .collect();
    let mut acc = vec![0u32; ew];
    for oy in 0..eh {
        let sy0 = y0 + oy * ch / eh;
        let sy1 = (y0 + (oy + 1) * ch / eh).max(sy0 + 1);
        acc.iter_mut().for_each(|a| *a = 0);
        for sy in sy0..sy1 {
            let base = sy * src.row;
            debug_assert!(base + (x0 + cw - 1) * src.px < src.len);
            // SAFETY: `fits` checked the plane covers w×h at its strides, and the crop window lies inside w×h.
            let row = unsafe { std::slice::from_raw_parts(src.ptr.add(base), src.len - base) };
            if src.px == 1 {
                for (a, &(sx0, sx1)) in acc.iter_mut().zip(&spans) {
                    *a += row[sx0..sx1].iter().map(|&v| v as u32).sum::<u32>();
                }
            } else {
                for (a, &(sx0, sx1)) in acc.iter_mut().zip(&spans) {
                    *a += (sx0..sx1).map(|sx| row[sx * src.px] as u32).sum::<u32>();
                }
            }
        }
        let rows = (sy1 - sy0) as u32;
        for (ox, (&a, &(sx0, sx1))) in acc.iter().zip(&spans).enumerate() {
            out[oy * ew + ox] = (a / (rows * (sx1 - sx0) as u32)) as u8;
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
    let (x0, y0, cw, ch) = crop_to(src.w, src.h, dst.w, dst.h);
    let (cw, ch) = (cw.max(1), ch.max(1));
    let scale = 2f64.powf(-(soft.min(4) as f64) / 2.0);
    let ew = ((dst.w as f64 * scale).round() as usize).clamp(1, dst.w).min(cw);
    let eh = ((dst.h as f64 * scale).round() as usize).clamp(1, dst.h).min(ch);
    if soft == 0 && dst.w <= cw && dst.h <= ch {
        // Full detail and a downscale (or the same size): box-average the crop straight onto the coded grid.
        let mut tmp = vec![0u8; dst.w * dst.h];
        shrink(src, x0, y0, cw, ch, dst.w, dst.h, &mut tmp);
        for y in 0..dst.h {
            let line = &tmp[y * dst.w..(y + 1) * dst.w];
            if dst.px == 1 {
                // SAFETY: `fits` checked the destination covers w×h at its strides.
                unsafe { std::ptr::copy_nonoverlapping(line.as_ptr(), dst.ptr.add(y * dst.row), dst.w) };
            } else {
                for (x, &v) in line.iter().enumerate() {
                    dst.put(x, y, v);
                }
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
    fn the_crop_is_centred_even_and_follows_the_coded_aspect() {
        assert_eq!(crop_to(640, 480, 16, 9), (0, 60, 640, 360));
        assert_eq!(crop_to(1280, 720, 640, 360), (0, 0, 1280, 720));
        assert_eq!(crop_to(1000, 400, 16, 9), (144, 0, 710, 400));
        assert_eq!(crop_to(2560, 1920, 640, 480), (0, 0, 2560, 1920), "a band of the capture's own aspect: uncropped");
    }

    #[test]
    fn an_exact_half_is_a_two_by_two_box() {
        let (w, h) = (8, 4);
        let s: Vec<u8> = (0..w * h).map(|i| (i * 7 % 251) as u8).collect();
        let mut buf = s.clone();
        let src = Plane::from_slice(&mut buf, w, 1, w, h);
        let mut d = vec![0u8; 4 * 2];
        let dst = Plane::from_slice(&mut d, 4, 1, 4, 2);
        resample_plane(&src, &dst, 0);
        for oy in 0..2 {
            for ox in 0..4 {
                let sum: u32 = [(0, 0), (1, 0), (0, 1), (1, 1)].iter().map(|&(dx, dy)| s[(2 * oy + dy) * w + 2 * ox + dx] as u32).sum();
                assert_eq!(d[oy * 4 + ox] as u32, sum / 4);
            }
        }
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
