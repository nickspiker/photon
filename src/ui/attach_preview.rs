//! Attachment PREPARATION, off the UI thread: sniff the kind, read the dims, and mint the row's micro preview (typed attachments, 2026-09-10).
//! Images decode thru the `image` crate exactly as the avatar path does (EXIF orientation honoured), fold to a ≤24-px thumb in LINEAR VSF RGB (sRGB assumed for untagged legacy files — the honest `assumed` tier), and store gamma-2 bytes. Text and code carry their first 240 bytes. Nothing here re-encodes the original: the bytes that travel are the bytes that were picked.

use crate::types::{AttachKind, AttachMeta, MICRO_PREVIEW_MAX_EDGE};

/// What the worker hands back for a picked file: the typed metadata for the row plus its micro preview bytes (empty when the kind has none).
pub struct Prepared {
    pub meta: AttachMeta,
    pub preview: Vec<u8>,
}

/// Sniff + dims + micro preview for `bytes`. Never fails: an undecodable image keeps its kind and simply has no preview.
pub fn prepare(bytes: &[u8], name: &str) -> Prepared {
    let kind = crate::types::sniff(bytes, name);
    let mut meta = AttachMeta { kind, dims: None, preview_hash: None };
    let preview = match kind {
        AttachKind::Image => match decode_thumb(bytes) {
            Some((w, h, tw, th, px)) => {
                meta.dims = Some((w, h));
                crate::types::encode_micro_image(tw, th, &px)
            }
            None => Vec::new(),
        },
        AttachKind::Text | AttachKind::Code => crate::types::micro_text(bytes),
        _ => Vec::new(),
    };
    Prepared { meta, preview }
}

/// Decode a legacy image and fold it to the micro thumb: (source w, source h, thumb w, thumb h, gamma-2 VSF RGB bytes).
fn decode_thumb(bytes: &[u8]) -> Option<(u32, u32, usize, usize, Vec<u8>)> {
    use image::ImageDecoder;
    let mut decoder = image::ImageReader::new(std::io::Cursor::new(bytes))
        .with_guessed_format()
        .ok()?
        .into_decoder()
        .ok()?;
    let orientation = decoder
        .orientation()
        .unwrap_or(image::metadata::Orientation::NoTransforms);
    let mut img = image::DynamicImage::from_decoder(decoder).ok()?;
    img.apply_orientation(orientation);
    let (w, h) = (img.width(), img.height());
    if w == 0 || h == 0 {
        return None;
    }
    let (tw, th) = thumb_dims(w as usize, h as usize, MICRO_PREVIEW_MAX_EDGE);
    // A fast box pass to ~4× the thumb first (a 50 MP decode must not be walked per output pixel), then the linear fold to the final size.
    let pre = img.thumbnail((tw * 4) as u32, (th * 4) as u32).to_rgb8();
    let px = fold_linear_vsf(pre.as_raw(), pre.width() as usize, pre.height() as usize, tw, th);
    Some((w, h, tw, th, px))
}

/// Aspect-preserving thumb dims with the long edge at `max_edge` (never below 1×1).
pub fn thumb_dims(w: usize, h: usize, max_edge: usize) -> (usize, usize) {
    if w >= h {
        (max_edge, ((h * max_edge + w / 2) / w).max(1))
    } else {
        (((w * max_edge + h / 2) / h).max(1), max_edge)
    }
}

#[allow(deprecated)] // the sRGB linearization is exactly what an untagged legacy file needs — the `assumed` tier, deliberately.
/// Box-fold sRGB u8 RGB into `tw × th` gamma-2 VSF RGB bytes, averaging in LINEAR light after the sRGB → VSF RGB matrix.
fn fold_linear_vsf(src: &[u8], sw: usize, sh: usize, tw: usize, th: usize) -> Vec<u8> {
    use vsf::colour::convert::apply_matrix_3x3_f32;
    use vsf::colour::legacy::convert::linearize_srgb_u8;
    use vsf::colour::SRGB2VSF_RGB;
    let mut out = vec![0u8; tw * th * 3];
    for ty in 0..th {
        let y0 = ty * sh / th;
        let y1 = ((ty + 1) * sh / th).max(y0 + 1).min(sh);
        for tx in 0..tw {
            let x0 = tx * sw / tw;
            let x1 = ((tx + 1) * sw / tw).max(x0 + 1).min(sw);
            let mut acc = [0f32; 3];
            let mut n = 0f32;
            for y in y0..y1 {
                for x in x0..x1 {
                    let i = (y * sw + x) * 3;
                    let lin = [
                        linearize_srgb_u8(src[i]),
                        linearize_srgb_u8(src[i + 1]),
                        linearize_srgb_u8(src[i + 2]),
                    ];
                    let v = apply_matrix_3x3_f32(&SRGB2VSF_RGB, &lin);
                    acc[0] += v[0].max(0.0);
                    acc[1] += v[1].max(0.0);
                    acc[2] += v[2].max(0.0);
                    n += 1.0;
                }
            }
            let o = (ty * tw + tx) * 3;
            for c in 0..3 {
                out[o + c] = ((acc[c] / n.max(1.0)).clamp(0.0, 1.0).sqrt() * 255.0 + 0.5) as u8;
            }
        }
    }
    out
}

/// Gamma-2 VSF RGB thumb bytes → fluor's packed α + darkness pixels for `paint::draw_image` (BT.2020 on the way, the same display conversion the avatar takes).
pub fn micro_to_display(vsf_gamma2: &[u8]) -> Vec<u32> {
    let bt = crate::ui::colour_convert::vsf_rgb_to_bt2020(vsf_gamma2);
    bt.chunks_exact(3)
        .map(|p| fluor::paint::pack_argb(p[0], p[1], p[2], 255))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thumb_dims_keep_aspect_and_never_vanish() {
        assert_eq!(thumb_dims(4000, 3000, 24), (24, 18));
        assert_eq!(thumb_dims(3000, 4000, 24), (18, 24));
        assert_eq!(thumb_dims(10_000, 10, 24), (24, 1));
        assert_eq!(thumb_dims(50, 50, 24), (24, 24));
    }

    #[test]
    fn a_png_prepares_as_an_image_with_dims_and_a_bounded_thumb() {
        // A 64×32 solid mid-grey PNG built in memory (the encoder is fine in tests: it never ships).
        let mut png = Vec::new();
        let img = image::RgbImage::from_pixel(64, 32, image::Rgb([128, 128, 128]));
        image::DynamicImage::ImageRgb8(img)
            .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .unwrap();
        let p = prepare(&png, "grey.png");
        assert_eq!(p.meta.kind, AttachKind::Image);
        assert_eq!(p.meta.dims, Some((64, 32)));
        let (w, h, px) = crate::types::parse_micro_image(&p.preview).unwrap();
        assert_eq!((w, h), (24, 12));
        // A flat field stays flat per channel (sRGB grey is NOT equal-channel in VSF RGB's Illuminant-E primaries, so compare each channel to its own first pixel).
        assert!(px.chunks_exact(3).all(|p| (0..3).all(|c| (p[c] as i32 - px[c] as i32).abs() <= 1)));
        assert!(p.preview.len() <= crate::types::MICRO_PREVIEW_MAX_BYTES);
    }
}
