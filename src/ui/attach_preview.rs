//! Attachment PREPARATION, off the UI thread: sniff the kind, read the dims, and mint the row's two preview tiers (typed attachments, 2026-09-10).
//! ONE linear decode feeds both: legacy images thru the `image` crate exactly as the avatar path does (EXIF orientation honoured, sRGB assumed for an untagged file — the honest `assumed` tier), JPEG XL thru jxl-oxide from its tagged primaries and transfer, camera RAW / DNG thru limbus (native-depth counts, 2×2 Bayer binned — no demosaic — thru the DNG matrix to XYZ, auto-white on the brightest channel, the same picture lumis embeds).
//! The MICRO tier is a ≤24-px gamma-2 VSF RGB thumb that rides on the row; the PREVIEW tier is an AV1-in-VSF image (`vsf::builders::compressed_image`, the avatar's own container) at ≤512 px on the long edge, content-addressed and pushed beside the original. Nothing here re-encodes the original: the bytes that travel are the bytes that were picked, and the only thing photon ever encodes is its own house-format preview.

use crate::types::{AttachKind, AttachMeta, MICRO_PREVIEW_MAX_EDGE};

/// Long edge of the preview blob.
pub const PREVIEW_MAX_EDGE: usize = 512;
/// Long edge of the "open original" render (a 50 MP photo folds to this on the way to the screen; the file itself stays untouched).
pub const FULL_VIEW_MAX_EDGE: usize = 4096;
/// Long edge of the LINEAR buffer the viewer keeps for its exposure control — twelve bytes a pixel, so a phone keeps 2048 (50 MB) and a desktop 4096.
pub const LINEAR_VIEW_MAX_EDGE: usize = if cfg!(target_os = "android") { 2048 } else { 4096 };

/// THE COLOUR-MANAGED ORIGINAL (Nick 2026-09-11, "colour/spectral managed, vsf rgb as much as possible"): the bytes go thru opsin's ingest (limbus for DNG/RAW with both DNG matrices and the illuminant, jxl-oxide, zune for JPEG, the image crate for the rest) into one native-depth spectral image, then `to_linear_in(VsfRgb)` — the profile's matrix, illuminant-normalised, integer pipeline — gives linear VSF RGB with 65535 = the profile's white. EXIF orientation is applied and the buffer folded to [`LINEAR_VIEW_MAX_EDGE`] here, off the UI thread; exposure is a gain at the display encode ([`encode_linear`]), so it is live. RAW stays CFA-binned (no demosaic) — the same picture opsin shows. None = opsin could not read it (the caller falls back to the gamma-2 path).
pub fn full_image_linear(path: &std::path::Path, kind: AttachKind) -> Option<(usize, usize, Vec<i32>)> {
    if !kind.is_image() {
        return None;
    }
    let ext = path.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
    {
        let dec = match opsin::convert::load_any(path) {
            Ok(d) => d,
            Err(e) => {
                crate::logf!("attach: opsin ingest declined {}: {}", ext, e);
                return None;
            }
        };
        let (w, h, lin) = match opsin::convert::to_linear_in(&dec, opsin::convert::Target::VsfRgb) {
            Ok(v) => v,
            Err(e) => {
                crate::logf!("attach: opsin linear render failed: {}", e);
                return None;
            }
        };
        // opsin's to_linear_in already applied the EXIF orientation (convert.rs apply_orientation is its last step) — re-orienting here transposed rotated photos a second time with the wrong stride.
        Some(fold_i32_to_edge(&lin, w, h, LINEAR_VIEW_MAX_EDGE))
    }
}

/// Fold linear i32 RGB to `max_edge` on the long side: a box mean over the source block of each output pixel. Integer all the way; the buffer arrives already EXIF-oriented from opsin.
fn fold_i32_to_edge(lin: &[i32], w: usize, h: usize, max_edge: usize) -> (usize, usize, Vec<i32>) {
    use rayon::prelude::*;
    let (tw, th) = fit_dims(w, h, max_edge);
    let mut out = vec![0i32; tw * th * 3];
    out.par_chunks_mut(tw * 3).enumerate().for_each(|(ty, row)| {
        let y0 = ty * h / th;
        let y1 = ((ty + 1) * h / th).max(y0 + 1).min(h);
        for tx in 0..tw {
            let x0 = tx * w / tw;
            let x1 = ((tx + 1) * w / tw).max(x0 + 1).min(w);
            let mut acc = [0i64; 3];
            let mut n = 0i64;
            for sy in y0..y1 {
                for sx in x0..x1 {
                    let i = (sy * w + sx) * 3;
                    acc[0] += lin[i] as i64;
                    acc[1] += lin[i + 1] as i64;
                    acc[2] += lin[i + 2] as i64;
                    n += 1;
                }
            }
            let n = n.max(1);
            row[tx * 3] = (acc[0] / n) as i32;
            row[tx * 3 + 1] = (acc[1] / n) as i32;
            row[tx * 3 + 2] = (acc[2] / n) as i32;
        }
    });
    (tw, th, out)
}

/// Display encode of linear VSF RGB at `ev` stops: gain, VSF RGB → Rec.2020 (the panel space every platform is tagged for), gamma 2, fluor's α + darkness pixel in the platform byte order. `clip` paints a blown channel black and a crushed one white, opsin's raw-inversion convention.
pub fn encode_linear(lin: &[i32], ev: f32, clip: bool) -> Vec<u32> {
    use rayon::prelude::*;
    let m = transpose3(&vsf::colour::VSF_RGB2REC2020);
    let gain = 2f32.powf(ev) / 65535.0;
    lin.par_chunks_exact(3)
        .map(|px| {
            let c = [px[0] as f32 * gain, px[1] as f32 * gain, px[2] as f32 * gain];
            let mut vis = [0u32; 3];
            for o in 0..3 {
                let v = m[o * 3] * c[0] + m[o * 3 + 1] * c[1] + m[o * 3 + 2] * c[2];
                vis[o] = if clip && v >= 1.0 {
                    0
                } else if clip && v < 0.0 {
                    255
                } else {
                    (v.clamp(0.0, 1.0).sqrt() * 255.0 + 0.5) as u32
                };
            }
            fluor::theme::dark(fluor::theme::fmt((vis[0] << 16) | (vis[1] << 8) | vis[2]))
        })
        .collect()
}
/// rav1e base quantizer for preview blobs (the avatar uses 32 at 256 px; a hair coarser keeps a 512-px preview near the 32 KB target).
const PREVIEW_QUANTIZER: usize = 40;

/// What the worker hands back for a picked file: the typed metadata for the row (dims + preview-blob hash filled when a preview exists), the micro preview bytes, and the preview blob itself (an AV1-in-VSF file the sender stores under its own hash and pushes).
pub struct Prepared {
    pub meta: AttachMeta,
    pub preview: Vec<u8>,
    pub blob: Option<Vec<u8>>,
}

/// A decoded, folded image: source dims, folded dims, and gamma-2 VSF RGB f32 triples (row-major, values 0..1).
struct Folded {
    src_w: u32,
    src_h: u32,
    w: usize,
    h: usize,
    px: Vec<f32>,
}

/// Sniff + dims + both preview tiers for `bytes`. `raw_path` = a temp copy of the bytes on disk for a RAW (limbus reads files); the caller minted it after its own sniff and removes it after the drain. Never fails: an undecodable image keeps its kind and simply has no previews.
pub fn prepare(bytes: &[u8], name: &str, raw_path: Option<&std::path::Path>) -> Prepared {
    let kind = crate::types::sniff(bytes, name);
    let mut meta = AttachMeta { kind, dims: None, preview_hash: None };
    let mut preview = Vec::new();
    let mut blob = None;
    match kind {
        AttachKind::Image | AttachKind::RawImage => {
            if let Some(f) = decode_folded(bytes, name, kind, raw_path, PREVIEW_MAX_EDGE) {
                meta.dims = Some((f.src_w, f.src_h));
                // Micro tier from the same decode (a second fold, linear).
                let (tw, th) = thumb_dims(f.w, f.h, MICRO_PREVIEW_MAX_EDGE);
                let micro = fold_gamma2(&f.px, f.w, f.h, tw, th);
                preview = crate::types::encode_micro_image(tw, th, &micro.iter().map(|v| (v.clamp(0.0, 1.0) * 255.0 + 0.5) as u8).collect::<Vec<u8>>());
                // Preview tier: even dims for 4:2:0, AV1, the VSF image container.
                let (ew, eh) = (f.w & !1, f.h & !1);
                if ew >= 2 && eh >= 2 {
                    let cropped = crop_rgb_f32(&f.px, f.w, ew, eh);
                    match crate::ui::avatar::encode_av1_wh(&cropped, ew, eh, PREVIEW_QUANTIZER).and_then(vsf::builders::compressed_image) {
                        Ok(vsf_bytes) => {
                            meta.preview_hash = Some(*blake3::hash(&vsf_bytes).as_bytes());
                            blob = Some(vsf_bytes);
                        }
                        Err(e) => crate::logf!("attach: preview encode failed: {}", e),
                    }
                }
            }
        }
        AttachKind::Text | AttachKind::Code => preview = crate::types::micro_text(bytes),
        _ => {}
    }
    Prepared { meta, preview, blob }
}

/// Decode a held preview blob (AV1-in-VSF) → (w, h, fluor packed α + darkness pixels) for the card and the viewer.
pub fn decode_preview_blob(vsf_bytes: &[u8]) -> Option<(usize, usize, Vec<u32>)> {
    let parsed = vsf::builders::parse_compressed_image(vsf_bytes).ok()?;
    if parsed.encoding != vsf::builders::ENCODING_AV1 {
        return None;
    }
    let (w, h, rgb) = crate::ui::avatar::decode_avatar(&parsed.data).ok()?;
    Some((w, h, micro_to_display(&rgb)))
}

/// The "open original" render: the original bytes decoded and folded to ≤ FULL_VIEW_MAX_EDGE, as display pixels. Off-thread only.
pub fn full_image(bytes: &[u8], name: &str, kind: AttachKind, raw_path: Option<&std::path::Path>) -> Option<(usize, usize, Vec<u32>)> {
    let f = decode_folded(bytes, name, kind, raw_path, FULL_VIEW_MAX_EDGE)?;
    let rgb: Vec<u8> = f.px.iter().map(|v| (v.clamp(0.0, 1.0) * 255.0 + 0.5) as u8).collect();
    Some((f.w, f.h, micro_to_display(&rgb)))
}

fn decode_folded(bytes: &[u8], name: &str, kind: AttachKind, raw_path: Option<&std::path::Path>, max_edge: usize) -> Option<Folded> {
    let ext = name.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase()).unwrap_or_default();
    match kind {
        AttachKind::RawImage => decode_raw(raw_path?, max_edge),
        AttachKind::Image if ext == "jxl" || bytes.starts_with(&[0xFF, 0x0A]) || bytes.starts_with(b"\0\0\0\x0cJXL ") => decode_jxl(bytes, max_edge),
        AttachKind::Image => decode_legacy(bytes, max_edge),
        _ => None,
    }
}

/// Legacy (JPEG/PNG/WebP/TIFF/GIF/BMP): sRGB assumed, linearized, folded in linear light.
fn decode_legacy(bytes: &[u8], max_edge: usize) -> Option<Folded> {
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
    let (sw, sh) = (img.width() as usize, img.height() as usize);
    if sw == 0 || sh == 0 {
        return None;
    }
    let (tw, th) = fit_dims(sw, sh, max_edge);
    // A fast box pass to ≤2× the target first (a 50 MP decode must not be walked per output pixel), then the linear fold.
    let pre = if sw > tw * 2 || sh > th * 2 { img.thumbnail((tw * 2) as u32, (th * 2) as u32).to_rgb8() } else { img.to_rgb8() };
    let (pw, ph) = (pre.width() as usize, pre.height() as usize);
    let lin = srgb8_to_linear_vsf(pre.as_raw(), pw, ph);
    let px = fold_linear(&lin, pw, ph, tw, th);
    Some(Folded { src_w: sw as u32, src_h: sh as u32, w: tw, h: th, px: gamma2(&px) })
}

/// JPEG XL: decoded in its TAGGED space (primaries + transfer from the header; an ICC-profiled file falls back to the sRGB assumption), linearized, matrixed to VSF RGB, folded.
fn decode_jxl(bytes: &[u8], max_edge: usize) -> Option<Folded> {
    use jxl_oxide::color::{ColourEncoding, Primaries, TransferFunction};
    let image = jxl_oxide::JxlImage::builder().read(std::io::Cursor::new(bytes)).ok()?;
    let (to_vsf, tf): ([f32; 9], TransferFunction) = match &image.image_header().metadata.colour_encoding {
        ColourEncoding::Enum(enc) => {
            let m = match enc.primaries {
                Primaries::Srgb => transpose3(&vsf::colour::SRGB2VSF_RGB),
                Primaries::Bt2100 => inv3(&transpose3(&vsf::colour::VSF_RGB2REC2020))?,
                _ => transpose3(&vsf::colour::SRGB2VSF_RGB),
            };
            (m, enc.tf)
        }
        _ => (transpose3(&vsf::colour::SRGB2VSF_RGB), TransferFunction::Srgb),
    };
    let render = image.render_frame(0).ok()?;
    let fb = render.image_all_channels();
    let (sw, sh, ch) = (fb.width(), fb.height(), fb.channels());
    if ch < 3 || sw == 0 || sh == 0 {
        return None;
    }
    let buf = fb.buf();
    #[allow(deprecated)]
    let lin_of = |e: f32| -> f32 {
        let e = e.clamp(0.0, 1.0);
        match tf {
            TransferFunction::Linear => e,
            TransferFunction::Srgb => vsf::colour::srgb_eotf(e),
            TransferFunction::Gamma { g, inverted } if g > 0 => e.powf(if inverted { 1e7 / g as f32 } else { g as f32 / 1e7 }),
            _ => vsf::colour::srgb_eotf(e),
        }
    };
    let mut lin = vec![0f32; sw * sh * 3];
    for i in 0..sw * sh {
        let v = [lin_of(buf[i * ch]), lin_of(buf[i * ch + 1]), lin_of(buf[i * ch + 2])];
        let o = mat_vec(&to_vsf, &v);
        lin[i * 3] = o[0].max(0.0);
        lin[i * 3 + 1] = o[1].max(0.0);
        lin[i * 3 + 2] = o[2].max(0.0);
    }
    let (tw, th) = fit_dims(sw, sh, max_edge);
    let px = fold_linear(&lin, sw, sh, tw, th);
    Some(Folded { src_w: sw as u32, src_h: sh as u32, w: tw, h: th, px: gamma2(&px) })
}

/// Camera RAW / DNG thru limbus: 2×2 Bayer bin to half-res camera RGB (no demosaic), black subtracted and white normalized, camera → XYZ thru the inverse of the DNG ColorMatrix (D65's when present), XYZ → VSF RGB, auto-white on the brightest channel, folded.
fn decode_raw(path: &std::path::Path, max_edge: usize) -> Option<Folded> {
    let (info, pixels) = limbus::read_dng(path)?;
    let (w, h) = (info.width, info.height);
    if w == 0 || h == 0 {
        return None;
    }
    let black = info.black;
    let scale = 1.0 / (info.white - black).max(1.0);
    let sub = |v: u16| (v as f32 - black) * scale;
    // Half-res camera-space RGB.
    let (hw, hh, cam): (usize, usize, Vec<f32>) = if info.rgb {
        if pixels.len() < w * h * 3 {
            return None;
        }
        (w, h, pixels.iter().map(|&v| sub(v)).collect())
    } else {
        if pixels.len() < w * h || info.cfaw != 2 || info.cfah != 2 || info.cfa.len() < 4 {
            return None;
        }
        let (hw, hh) = (w / 2, h / 2);
        let mut out = vec![0f32; hw * hh * 3];
        for cy in 0..hh {
            for cx in 0..hw {
                let (ox, oy) = (cx * 2, cy * 2);
                let cell = [
                    (0usize, sub(pixels[oy * w + ox])),
                    (1, sub(pixels[oy * w + ox + 1])),
                    (2, sub(pixels[(oy + 1) * w + ox])),
                    (3, sub(pixels[(oy + 1) * w + ox + 1])),
                ];
                let mut rgb = [0f32; 3];
                let mut gn = 0f32;
                for (i, v) in cell {
                    match info.cfa[i] {
                        0 => rgb[0] += v,
                        1 => {
                            rgb[1] += v;
                            gn += 1.0;
                        }
                        _ => rgb[2] += v,
                    }
                }
                if gn > 1.0 {
                    rgb[1] /= gn;
                }
                let o = (cy * hw + cx) * 3;
                out[o..o + 3].copy_from_slice(&rgb);
            }
        }
        (hw, hh, out)
    };
    // Fold FIRST (cheap matrix work on the small image), then colour.
    let (tw, th) = fit_dims(hw, hh, max_edge);
    let small = fold_linear(&cam, hw, hh, tw, th);
    // Camera → XYZ: the inverse of the DNG's XYZ → camera matrix (row-major as DNG stores it). No matrix = the counts as they are (uncharacterized, never a fake guess).
    let cam_to_xyz = info.colourmatrix2.or(info.colourmatrix1).and_then(|m| inv3(&m));
    let xyz_to_vsf = transpose3(&vsf::colour::XYZ2VSF_RGB);
    let mut lin = vec![0f32; tw * th * 3];
    let mut peak = 1e-6f32;
    for i in 0..tw * th {
        let c = [small[i * 3], small[i * 3 + 1], small[i * 3 + 2]];
        let v = match cam_to_xyz {
            Some(m) => mat_vec(&xyz_to_vsf, &mat_vec(&m, &c)),
            None => c,
        };
        for k in 0..3 {
            let x = v[k].max(0.0);
            lin[i * 3 + k] = x;
            peak = peak.max(x);
        }
    }
    let inv = 1.0 / peak;
    for v in &mut lin {
        *v *= inv;
    }
    Some(Folded { src_w: w as u32, src_h: h as u32, w: tw, h: th, px: gamma2(&lin) })
}

/// Aspect-preserving dims with the long edge at `max_edge` (never below 1×1, never upscaled).
pub fn fit_dims(w: usize, h: usize, max_edge: usize) -> (usize, usize) {
    if w <= max_edge && h <= max_edge {
        return (w.max(1), h.max(1));
    }
    thumb_dims(w, h, max_edge)
}

/// Aspect-preserving thumb dims with the long edge at exactly `max_edge` (never below 1×1).
pub fn thumb_dims(w: usize, h: usize, max_edge: usize) -> (usize, usize) {
    if w >= h {
        (max_edge, ((h * max_edge + w / 2) / w).max(1))
    } else {
        (((w * max_edge + h / 2) / h).max(1), max_edge)
    }
}

#[allow(deprecated)] // the sRGB linearization is exactly what an untagged legacy file needs — the `assumed` tier, deliberately.
fn srgb8_to_linear_vsf(src: &[u8], w: usize, h: usize) -> Vec<f32> {
    use vsf::colour::legacy::convert::linearize_srgb_u8;
    let m = transpose3(&vsf::colour::SRGB2VSF_RGB);
    let mut out = vec![0f32; w * h * 3];
    for i in 0..w * h {
        let lin = [linearize_srgb_u8(src[i * 3]), linearize_srgb_u8(src[i * 3 + 1]), linearize_srgb_u8(src[i * 3 + 2])];
        let v = mat_vec(&m, &lin);
        out[i * 3] = v[0].max(0.0);
        out[i * 3 + 1] = v[1].max(0.0);
        out[i * 3 + 2] = v[2].max(0.0);
    }
    out
}

/// Box-fold linear f32 RGB triples from `sw × sh` to `tw × th` (averaging in linear light).
fn fold_linear(src: &[f32], sw: usize, sh: usize, tw: usize, th: usize) -> Vec<f32> {
    if (sw, sh) == (tw, th) {
        return src.to_vec();
    }
    let mut out = vec![0f32; tw * th * 3];
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
                    acc[0] += src[i];
                    acc[1] += src[i + 1];
                    acc[2] += src[i + 2];
                    n += 1.0;
                }
            }
            let o = (ty * tw + tx) * 3;
            for c in 0..3 {
                out[o + c] = acc[c] / n.max(1.0);
            }
        }
    }
    out
}

/// Fold GAMMA-2 triples: square to linear, box-fold, back to gamma 2.
fn fold_gamma2(src: &[f32], sw: usize, sh: usize, tw: usize, th: usize) -> Vec<f32> {
    let lin: Vec<f32> = src.iter().map(|v| v * v).collect();
    gamma2(&fold_linear(&lin, sw, sh, tw, th))
}

fn gamma2(lin: &[f32]) -> Vec<f32> {
    lin.iter().map(|v| v.clamp(0.0, 1.0).sqrt()).collect()
}

fn crop_rgb_f32(src: &[f32], sw: usize, w: usize, h: usize) -> Vec<f32> {
    let mut out = Vec::with_capacity(w * h * 3);
    for y in 0..h {
        out.extend_from_slice(&src[(y * sw) * 3..(y * sw + w) * 3]);
    }
    out
}

/// Row-major 3×3 × vec.
fn mat_vec(m: &[f32; 9], v: &[f32; 3]) -> [f32; 3] {
    [
        m[0] * v[0] + m[1] * v[1] + m[2] * v[2],
        m[3] * v[0] + m[4] * v[1] + m[5] * v[2],
        m[6] * v[0] + m[7] * v[1] + m[8] * v[2],
    ]
}

/// vsf's colour constants are column-major; this bridges them to the row-major convention above (opsin does the same).
fn transpose3(m: &[f32; 9]) -> [f32; 9] {
    [m[0], m[3], m[6], m[1], m[4], m[7], m[2], m[5], m[8]]
}

fn inv3(m: &[f32; 9]) -> Option<[f32; 9]> {
    let det = m[0] * (m[4] * m[8] - m[5] * m[7]) - m[1] * (m[3] * m[8] - m[5] * m[6]) + m[2] * (m[3] * m[7] - m[4] * m[6]);
    if det.abs() < 1e-12 {
        return None;
    }
    let d = 1.0 / det;
    Some([
        (m[4] * m[8] - m[5] * m[7]) * d,
        (m[2] * m[7] - m[1] * m[8]) * d,
        (m[1] * m[5] - m[2] * m[4]) * d,
        (m[5] * m[6] - m[3] * m[8]) * d,
        (m[0] * m[8] - m[2] * m[6]) * d,
        (m[2] * m[3] - m[0] * m[5]) * d,
        (m[3] * m[7] - m[4] * m[6]) * d,
        (m[1] * m[6] - m[0] * m[7]) * d,
        (m[0] * m[4] - m[1] * m[3]) * d,
    ])
}

/// Gamma-2 VSF RGB bytes → fluor's packed α + darkness pixels for `paint::draw_image` (BT.2020 on the way, the same display conversion the avatar takes).
pub fn micro_to_display(vsf_gamma2: &[u8]) -> Vec<u32> {
    let bt = crate::ui::colour_convert::vsf_rgb_to_bt2020(vsf_gamma2);
    // Platform byte order thru fluor's `fmt` (R↔B on Android's RGBA_8888 buffer) exactly as the avatar path packs — `pack_argb` alone is desktop order, which is why previews came up blue-for-red on Android (2026-09-10).
    bt.chunks_exact(3)
        .map(|p| {
            let visible = ((p[0] as u32) << 16) | ((p[1] as u32) << 8) | p[2] as u32;
            fluor::theme::dark(fluor::theme::fmt(visible))
        })
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
        assert_eq!(fit_dims(300, 200, 512), (300, 200));
        assert_eq!(fit_dims(3000, 2000, 512), (512, 341));
    }

    #[test]
    fn inverse_undoes_the_matrix() {
        let m = transpose3(&vsf::colour::SRGB2VSF_RGB);
        let inv = inv3(&m).unwrap();
        let v = [0.2f32, 0.5, 0.8];
        let back = mat_vec(&inv, &mat_vec(&m, &v));
        for k in 0..3 {
            assert!((back[k] - v[k]).abs() < 1e-4);
        }
    }

    #[test]
    fn a_png_prepares_as_an_image_with_both_tiers_and_the_preview_round_trips() {
        // A 64×32 solid mid-grey PNG built in memory (the encoder is fine in tests: it never ships).
        let mut png = Vec::new();
        let img = image::RgbImage::from_pixel(64, 32, image::Rgb([128, 128, 128]));
        image::DynamicImage::ImageRgb8(img)
            .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .unwrap();
        let p = prepare(&png, "grey.png", None);
        assert_eq!(p.meta.kind, AttachKind::Image);
        assert_eq!(p.meta.dims, Some((64, 32)));
        let (w, h, px) = crate::types::parse_micro_image(&p.preview).unwrap();
        assert_eq!((w, h), (24, 12));
        // A flat field stays flat per channel (sRGB grey is NOT equal-channel in VSF RGB's Illuminant-E primaries, so compare each channel to its own first pixel).
        assert!(px.chunks_exact(3).all(|p| (0..3).all(|c| (p[c] as i32 - px[c] as i32).abs() <= 1)));
        assert!(p.preview.len() <= crate::types::MICRO_PREVIEW_MAX_BYTES);
        let blob = p.blob.expect("preview blob");
        assert_eq!(p.meta.preview_hash, Some(*blake3::hash(&blob).as_bytes()));
        let (pw, ph, pixels) = decode_preview_blob(&blob).expect("preview decodes");
        assert_eq!((pw, ph), (64, 32));
        assert_eq!(pixels.len(), 64 * 32);
    }
}

#[cfg(test)]
mod colour_order_tests {
    use super::*;

    /// Red in, red out: the micro thumb and the display packing keep channel order (a swap would put the peak in the blue byte).
    #[test]
    fn red_stays_red_thru_the_micro_thumb_and_the_display_pack() {
        let mut png = Vec::new();
        let img = image::RgbImage::from_pixel(32, 32, image::Rgb([220, 20, 20]));
        image::DynamicImage::ImageRgb8(img).write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).unwrap();
        let p = prepare(&png, "red.png", None);
        let (_, _, px) = crate::types::parse_micro_image(&p.preview).unwrap();
        assert!(px[0] > px[1] && px[0] > px[2], "VSF gamma2 thumb: R {} G {} B {}", px[0], px[1], px[2]);
        let disp = micro_to_display(&px[..3]);
        let packed = disp[0];
        let (dr, dg, db) = (((packed >> 16) & 0xFF) as i32, ((packed >> 8) & 0xFF) as i32, (packed & 0xFF) as i32);
        // Darkness convention: the RED byte is the LEAST dark.
        assert!(dr < dg && dr < db, "display darkness R {dr} G {dg} B {db}");
    }
}
