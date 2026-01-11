//! Avatar encoding/decoding using AV1 compression with circular masking
//!
//! Avatars are circular images encoded with anti-aliased edges to avoid
//! compression artifacts. The circular mask blends to black at the edge.

/// Avatar size in pixels (256x256 square)
pub const AVATAR_SIZE: usize = 256;

use ed25519_dalek::{SigningKey, VerifyingKey};
use img_parts::jpeg::Jpeg;
use img_parts::png::Png;
use img_parts::ImageICC;
use rav1e::prelude::*;

#[cfg(not(target_os = "android"))]
use winit::event_loop::EventLoopProxy;

#[cfg(not(target_os = "android"))]
use super::PhotonEvent;

/// Type alias for optional EventLoopProxy - actual proxy on desktop, unit type on Android
#[cfg(not(target_os = "android"))]
pub type OptionalEventProxy = Option<EventLoopProxy<PhotonEvent>>;
#[cfg(target_os = "android")]
pub type OptionalEventProxy = Option<()>;

/// Tone Reproduction Curve from ICC profile
#[derive(Clone)]
enum TrcCurve {
    /// Linear (identity) - no gamma correction
    Linear,
    /// Simple gamma curve
    Gamma(f32),
    /// Lookup table with pre-normalized values (0.0-1.0 range)
    Lut(Vec<f32>),
    /// Parametric curve with formula type and parameters
    Parametric { funtion_type: u16, vals: Vec<f32> },
}

/// Pre-parsed ICC colour converter for fast per-pixel conversion
struct IccColourConverter {
    /// ICC RGB → XYZ transformation matrix (column-major)
    icc_to_xyz: [f32; 9],
    /// XYZ → VSF RGB transformation matrix (column-major)
    xyz_to_vsf: [f32; 9],
    /// Tone Reproduction Curve for R channel
    r_trc: TrcCurve,
    /// Tone Reproduction Curve for G channel
    g_trc: TrcCurve,
    /// Tone Reproduction Curve for B channel
    b_trc: TrcCurve,
}

/// Encodes an image as a circular AV1-compressed avatar in VSF RGB colourspace
///
/// Supports JPEG, PNG, and WebP formats with ICC profile colour management.
///
/// # Arguments
/// * `image_data` - Raw image file bytes
///
/// # Returns
/// Raw AV1 OBU bitstream encoded with VSF RGB colourspace (256x256)
pub fn encode_avatar_from_image(image_data: &[u8]) -> Result<Vec<u8>, String> {
    use resize::Type::Lanczos3;
    use rgb::FromSlice;
    use vsf::colour::convert::delinearize_gamma2;

    let size = AVATAR_SIZE;

    // Detect format and extract ICC profile
    let icc_profile_bytes = extract_icc_profile(image_data)?;

    // Parse ICC profile once (if present)
    let icc_converter = if let Some(ref profile) = icc_profile_bytes {
        Some(parse_icc_converter(profile)?)
    } else {
        None
    };

    // Decode image to RGB
    // Note: image crate has default memory limits (~512MB decoded).
    // This is fine - avatars are 256x256 output, huge sources should be resized first.
    let img = image::load_from_memory(image_data)
        .map_err(|e| format!("Failed to decode image: {}", e))?;

    let orig_width = img.width() as usize;
    let orig_height = img.height() as usize;

    // Center-crop to square before color conversion
    let crop_size = orig_width.min(orig_height);
    let crop_x = (orig_width - crop_size) / 2;
    let crop_y = (orig_height - crop_size) / 2;

    // Convert cropped region to linear VSF RGB (f32)
    // Handle both 8-bit and 16-bit source images
    let mut linear_vsf_cropped = vec![0.0f32; crop_size * crop_size * 3];

    use image::DynamicImage;
    match &img {
        DynamicImage::ImageRgb16(_) | DynamicImage::ImageRgba16(_) => {
            // 16-bit image: convert with /65536 normalization
            let rgb16_img = img.to_rgb16();
            let rgb_pixels = rgb16_img.as_raw();
            for y in 0..crop_size {
                for x in 0..crop_size {
                    let src_idx = ((crop_y + y) * orig_width + (crop_x + x)) * 3;
                    let dst_idx = (y * crop_size + x) * 3;
                    let r = rgb_pixels[src_idx];
                    let g = rgb_pixels[src_idx + 1];
                    let b = rgb_pixels[src_idx + 2];

                    let linear_vsf = if let Some(ref converter) = icc_converter {
                        convert_pixel_linear_u16(r, g, b, converter)
                    } else {
                        // No ICC profile - assume sRGB (legacy but needed for compatibility)
                        #[allow(deprecated)]
                        {
                            use vsf::colour::convert::apply_matrix_3x3;
                            use vsf::colour::legacy::convert::linearize_srgb;
                            use vsf::colour::SRGB2VSF_RGB;
                            let r_lin = linearize_srgb(r as f32 / 65536.);
                            let g_lin = linearize_srgb(g as f32 / 65536.);
                            let b_lin = linearize_srgb(b as f32 / 65536.);
                            apply_matrix_3x3(&SRGB2VSF_RGB, &[r_lin, g_lin, b_lin])
                        }
                    };

                    linear_vsf_cropped[dst_idx] = linear_vsf[0];
                    linear_vsf_cropped[dst_idx + 1] = linear_vsf[1];
                    linear_vsf_cropped[dst_idx + 2] = linear_vsf[2];
                }
            }
        }
        _ => {
            // 8-bit image (or convert to 8-bit)
            let rgb_img = img.to_rgb8();
            let rgb_pixels = rgb_img.as_raw();
            for y in 0..crop_size {
                for x in 0..crop_size {
                    let src_idx = ((crop_y + y) * orig_width + (crop_x + x)) * 3;
                    let dst_idx = (y * crop_size + x) * 3;
                    let r = rgb_pixels[src_idx];
                    let g = rgb_pixels[src_idx + 1];
                    let b = rgb_pixels[src_idx + 2];

                    let linear_vsf = if let Some(ref converter) = icc_converter {
                        convert_pixel_linear(r, g, b, converter)
                    } else {
                        // No ICC profile - assume sRGB (legacy but needed for compatibility)
                        #[allow(deprecated)]
                        {
                            use vsf::colour::convert::apply_matrix_3x3;
                            use vsf::colour::legacy::convert::linearize_srgb_u8;
                            use vsf::colour::SRGB2VSF_RGB;
                            let r_lin = linearize_srgb_u8(r);
                            let g_lin = linearize_srgb_u8(g);
                            let b_lin = linearize_srgb_u8(b);
                            apply_matrix_3x3(&SRGB2VSF_RGB, &[r_lin, g_lin, b_lin])
                        }
                    };

                    linear_vsf_cropped[dst_idx] = linear_vsf[0];
                    linear_vsf_cropped[dst_idx + 1] = linear_vsf[1];
                    linear_vsf_cropped[dst_idx + 2] = linear_vsf[2];
                }
            }
        }
    }

    // Resize in linear space using Lanczos3
    let mut linear_vsf_resized = vec![0.0f32; size * size * 3];
    let mut resizer = resize::new(
        crop_size,
        crop_size,
        size,
        size,
        resize::Pixel::RGBF32,
        Lanczos3,
    )
    .map_err(|e| format!("Failed to create resizer: {:?}", e))?;

    resizer
        .resize(linear_vsf_cropped.as_rgb(), linear_vsf_resized.as_rgb_mut())
        .map_err(|e| format!("Failed to resize: {:?}", e))?;

    // Apply circular mask in linear space and encode to gamma
    let mut vsf_rgb_f32 = vec![0.0f32; size * size * 3];
    let center = (size / 2) as isize;
    let r_outer = (size / 2) as isize + 1;
    let r_outer2 = r_outer * r_outer;
    let r_inner = r_outer - 1;
    let r_inner2 = r_inner * r_inner;
    let edge_range_f32 = (r_outer2 - r_inner2) as f32;

    for y in 0..size {
        for x in 0..size {
            let idx = (y * size + x) * 3;

            // Calculate distance from center
            let dx = x as isize - center;
            let dy = y as isize - center;
            let dist2 = dx * dx + dy * dy;

            // Outside outer radius: skip (leave as zero/black)
            if dist2 > r_outer2 {
                continue;
            }

            // Get linear VSF RGB
            let linear_vsf = [
                linear_vsf_resized[idx],
                linear_vsf_resized[idx + 1],
                linear_vsf_resized[idx + 2],
            ];

            // Apply circular mask alpha in linear space
            let masked_linear = if dist2 <= r_inner2 {
                linear_vsf
            } else {
                let alpha = 1.0 - ((dist2 - r_inner2) as f32 / edge_range_f32);
                [
                    linear_vsf[0] * alpha,
                    linear_vsf[1] * alpha,
                    linear_vsf[2] * alpha,
                ]
            };

            // Apply VSF gamma 2 encoding
            // .max(0.) prevents NaN from sqrt() - Lanczos3 ringing can produce negatives
            vsf_rgb_f32[idx] = delinearize_gamma2(masked_linear[0].max(0.));
            vsf_rgb_f32[idx + 1] = delinearize_gamma2(masked_linear[1].max(0.));
            vsf_rgb_f32[idx + 2] = delinearize_gamma2(masked_linear[2].max(0.));
        }
    }

    encode_av1(&vsf_rgb_f32, size)
}

/// Extract ICC profile from image data
fn extract_icc_profile(image_data: &[u8]) -> Result<Option<Vec<u8>>, String> {
    // Try JPEG first
    if let Ok(jpeg) = Jpeg::from_bytes(image_data.to_vec().into()) {
        if let Some(icc) = jpeg.icc_profile() {
            return Ok(Some(icc.to_vec()));
        }
    }

    // Try PNG
    if let Ok(png) = Png::from_bytes(image_data.to_vec().into()) {
        if let Some(icc) = png.icc_profile() {
            return Ok(Some(icc.to_vec()));
        }
    }

    // Try TIFF - ICC profile is in tag 34675 (InterColorProfile)
    if let Some(icc) = extract_tiff_icc(image_data) {
        return Ok(Some(icc));
    }

    // No ICC profile found - will assume sRGB
    Ok(None)
}

/// Extract ICC profile from TIFF image data
/// TIFF stores ICC in tag 34675 (InterColorProfile / 0x8773)
fn extract_tiff_icc(data: &[u8]) -> Option<Vec<u8>> {
    // Check TIFF magic bytes
    if data.len() < 8 {
        return None;
    }

    let (big_endian, magic) = match &data[0..4] {
        [b'I', b'I', 0x2A, 0x00] => (false, true), // Little-endian
        [b'M', b'M', 0x00, 0x2A] => (true, true),  // Big-endian
        _ => (false, false),
    };

    if !magic {
        return None;
    }

    // Helper to read u16/u32 based on endianness
    let read_u16 = |offset: usize| -> u16 {
        if big_endian {
            u16::from_be_bytes([data[offset], data[offset + 1]])
        } else {
            u16::from_le_bytes([data[offset], data[offset + 1]])
        }
    };

    let read_u32 = |offset: usize| -> u32 {
        if big_endian {
            u32::from_be_bytes([
                data[offset],
                data[offset + 1],
                data[offset + 2],
                data[offset + 3],
            ])
        } else {
            u32::from_le_bytes([
                data[offset],
                data[offset + 1],
                data[offset + 2],
                data[offset + 3],
            ])
        }
    };

    // Get IFD offset
    let ifd_offset = read_u32(4) as usize;
    if ifd_offset + 2 > data.len() {
        return None;
    }

    // Read number of directory entries
    let num_entries = read_u16(ifd_offset) as usize;
    let entries_start = ifd_offset + 2;

    // Each entry is 12 bytes
    for i in 0..num_entries {
        let entry_offset = entries_start + i * 12;
        if entry_offset + 12 > data.len() {
            break;
        }

        let tag = read_u16(entry_offset);

        // Tag 34675 (0x8773) = InterColorProfile (ICC)
        if tag == 34675 {
            let _field_type = read_u16(entry_offset + 2);
            let count = read_u32(entry_offset + 4) as usize;
            let value_offset = read_u32(entry_offset + 8) as usize;

            // ICC profile data is at value_offset with length count
            if value_offset + count <= data.len() {
                return Some(data[value_offset..value_offset + count].to_vec());
            }
        }
    }

    None
}

/// Parse ICC profile into fast per-pixel converter
fn parse_icc_converter(icc_profile: &[u8]) -> Result<IccColourConverter, String> {
    use icc_profile::{Data, DecodedICCProfile, ICCNumber};
    use vsf::colour::XYZ2VSF_RGB;

    // Parse ICC profile and extract tags
    let icc_vec = icc_profile.to_vec();
    let profile = DecodedICCProfile::new(&icc_vec)
        .map_err(|e| format!("Failed to parse ICC profile: {:?}", e))?;

    // Extract RGB→XYZ matrix from rXYZ, gXYZ, bXYZ tags
    let r_xyz = match profile.tags.get("rXYZ") {
        Some(Data::XYZNumber(xyz)) => [xyz.x.as_f32(), xyz.y.as_f32(), xyz.z.as_f32()],
        Some(Data::XYZNumberArray(arr)) if !arr.is_empty() => {
            let xyz = &arr[0];
            [xyz.x.as_f32(), xyz.y.as_f32(), xyz.z.as_f32()]
        }
        _ => return Err("ICC profile missing rXYZ tag".to_string()),
    };
    let g_xyz = match profile.tags.get("gXYZ") {
        Some(Data::XYZNumber(xyz)) => [xyz.x.as_f32(), xyz.y.as_f32(), xyz.z.as_f32()],
        Some(Data::XYZNumberArray(arr)) if !arr.is_empty() => {
            let xyz = &arr[0];
            [xyz.x.as_f32(), xyz.y.as_f32(), xyz.z.as_f32()]
        }
        _ => return Err("ICC profile missing gXYZ tag".to_string()),
    };
    let b_xyz = match profile.tags.get("bXYZ") {
        Some(Data::XYZNumber(xyz)) => [xyz.x.as_f32(), xyz.y.as_f32(), xyz.z.as_f32()],
        Some(Data::XYZNumberArray(arr)) if !arr.is_empty() => {
            let xyz = &arr[0];
            [xyz.x.as_f32(), xyz.y.as_f32(), xyz.z.as_f32()]
        }
        _ => return Err("ICC profile missing bXYZ tag".to_string()),
    };

    // Build ICC_RGB→XYZ matrix (column-major format like VSF)
    let icc_to_xyz = [
        r_xyz[0], r_xyz[1], r_xyz[2], g_xyz[0], g_xyz[1], g_xyz[2], b_xyz[0], b_xyz[1], b_xyz[2],
    ];

    // Pre-compute XYZ→VSF_RGB matrix (just copy, no transformation needed)
    let xyz_to_vsf = XYZ2VSF_RGB;

    // Extract and parse TRC curves
    let r_trc = parse_trc_curve(profile.tags.get("rTRC"))?;
    let g_trc = parse_trc_curve(profile.tags.get("gTRC"))?;
    let b_trc = parse_trc_curve(profile.tags.get("bTRC"))?;

    Ok(IccColourConverter {
        icc_to_xyz,
        xyz_to_vsf,
        r_trc,
        g_trc,
        b_trc,
    })
}

/// Parse TRC curve from ICC profile tag data
fn parse_trc_curve(trc: Option<&icc_profile::Data>) -> Result<TrcCurve, String> {
    use icc_profile::{Data, ICCNumber};

    match trc {
        Some(Data::Curve(curve)) => {
            if curve.is_empty() {
                // Empty curve = linear (identity)
                Ok(TrcCurve::Linear)
            } else if curve.len() == 1 {
                // Single entry = simple gamma (stored as gamma * 256)
                let gamma = curve[0] as f32 / 256.;
                Ok(TrcCurve::Gamma(gamma))
            } else {
                // Full LUT - pre-normalize to 0-1 range
                let normalized: Vec<f32> = curve.iter().map(|&v| v as f32 / 65535.).collect();
                Ok(TrcCurve::Lut(normalized))
            }
        }
        Some(Data::ParametricCurve(param)) => {
            // Store parametric curve parameters
            let vals: Vec<f32> = param.vals.iter().map(|v| v.as_f32()).collect();
            Ok(TrcCurve::Parametric {
                funtion_type: param.funtion_type,
                vals,
            })
        }
        None => {
            // No TRC - assume gamma 2.2 as fallback
            Ok(TrcCurve::Gamma(2.2))
        }
        _ => Err("Unsupported TRC type in ICC profile".to_string()),
    }
}

/// Apply TRC curve to linearize a normalized [0,1] value
#[inline]
fn apply_trc_normalized(normalized: f32, trc: &TrcCurve) -> f32 {
    match trc {
        TrcCurve::Linear => normalized,
        TrcCurve::Gamma(gamma) => normalized.powf(*gamma),
        TrcCurve::Lut(lut) => {
            // Interpolate in pre-normalized LUT
            let index = normalized * (lut.len() - 1) as f32;
            let i0 = index.floor() as usize;
            let i1 = (i0 + 1).min(lut.len() - 1);
            let frac = index - i0 as f32;
            lut[i0] + (lut[i1] - lut[i0]) * frac
        }
        TrcCurve::Parametric { funtion_type, vals } => {
            // Apply parametric curve formula
            match funtion_type {
                0x0000 => {
                    // Y = X^gamma
                    normalized.powf(vals[0])
                }
                0x0001 => {
                    // Y = (aX + b)^gamma if X >= -b/a, else 0
                    let gamma = vals[0];
                    let a = vals[1];
                    let b = vals[2];
                    if normalized >= -b / a {
                        (a * normalized + b).powf(gamma)
                    } else {
                        0.0
                    }
                }
                0x0002 => {
                    // Y = (aX + b)^gamma + c if X >= -b/a, else c
                    let gamma = vals[0];
                    let a = vals[1];
                    let b = vals[2];
                    let c = vals[3];
                    if normalized >= -b / a {
                        (a * normalized + b).powf(gamma) + c
                    } else {
                        c
                    }
                }
                0x0003 => {
                    // Y = (aX + b)^gamma if X >= d, else cX
                    let gamma = vals[0];
                    let a = vals[1];
                    let b = vals[2];
                    let c = vals[3];
                    let d = vals[4];
                    if normalized >= d {
                        (a * normalized + b).powf(gamma)
                    } else {
                        c * normalized
                    }
                }
                0x0004 => {
                    // Y = (aX + b)^gamma + e if X >= d, else cX + f
                    let gamma = vals[0];
                    let a = vals[1];
                    let b = vals[2];
                    let c = vals[3];
                    let d = vals[4];
                    let e = vals[5];
                    let f = vals[6];
                    if normalized >= d {
                        (a * normalized + b).powf(gamma) + e
                    } else {
                        c * normalized + f
                    }
                }
                _ => normalized, // Unsupported - fallback to linear
            }
        }
    }
}

/// Fast per-pixel conversion from ICC RGB (u8) to linear VSF RGB
#[inline]
fn convert_pixel_linear(r: u8, g: u8, b: u8, converter: &IccColourConverter) -> [f32; 3] {
    use vsf::colour::convert::apply_matrix_3x3;

    // Linearize using TRC curves (normalize u8 to [0,1])
    let r_lin = apply_trc_normalized(r as f32 / 255., &converter.r_trc);
    let g_lin = apply_trc_normalized(g as f32 / 255., &converter.g_trc);
    let b_lin = apply_trc_normalized(b as f32 / 255., &converter.b_trc);

    // Apply ICC_RGB → XYZ matrix
    let xyz = apply_matrix_3x3(&converter.icc_to_xyz, &[r_lin, g_lin, b_lin]);

    // Apply XYZ → VSF_RGB matrix
    // Clamp negative values: out-of-gamut colors can produce negatives, but
    // negative light intensity is physically impossible. This prevents NaN
    // from sqrt() in delinearize_gamma2.
    let vsf = apply_matrix_3x3(&converter.xyz_to_vsf, &xyz);
    [vsf[0].max(0.), vsf[1].max(0.), vsf[2].max(0.)]
}

/// Fast per-pixel conversion from ICC RGB (u16) to linear VSF RGB
#[inline]
fn convert_pixel_linear_u16(r: u16, g: u16, b: u16, converter: &IccColourConverter) -> [f32; 3] {
    use vsf::colour::convert::apply_matrix_3x3;

    // Linearize using TRC curves (normalize u16 to [0,1])
    let r_lin = apply_trc_normalized(r as f32 / 65536., &converter.r_trc);
    let g_lin = apply_trc_normalized(g as f32 / 65536., &converter.g_trc);
    let b_lin = apply_trc_normalized(b as f32 / 65536., &converter.b_trc);

    // Apply ICC_RGB → XYZ matrix
    let xyz = apply_matrix_3x3(&converter.icc_to_xyz, &[r_lin, g_lin, b_lin]);

    // Apply XYZ → VSF_RGB matrix
    // Clamp negative values: out-of-gamut colors can produce negatives, but
    // negative light intensity is physically impossible. This prevents NaN
    // from sqrt() in delinearize_gamma2.
    let vsf = apply_matrix_3x3(&converter.xyz_to_vsf, &xyz);
    [vsf[0].max(0.), vsf[1].max(0.), vsf[2].max(0.)]
}

/// Encodes VSF RGB f32 data as AV1 using rav1e (optimized for f32 pipeline)
fn encode_av1(rgb_data: &[f32], size: usize) -> Result<Vec<u8>, String> {
    let enc_cfg = EncoderConfig {
        width: size,
        height: size,
        bit_depth: 8,
        chroma_sampling: ChromaSampling::Cs420,
        time_base: Rational::new(1, 1),
        low_latency: true,
        speed_settings: SpeedSettings::from_preset(6),
        quantizer: 32,
        min_quantizer: 0,
        ..Default::default()
    };

    let cfg = Config::new().with_encoder_config(enc_cfg);
    let mut ctx: Context<u8> = cfg
        .new_context()
        .map_err(|e| format!("Failed to create rav1e context: {}", e))?;

    let mut frame = ctx.new_frame();

    // Build Y plane - VSF luma: Y = (R + 2G + B) / 4
    let mut y_plane = vec![0u8; size * size];
    for i in 0..(size * size) {
        let idx = i * 3;
        let r = rgb_data[idx];
        let g = rgb_data[idx + 1];
        let b = rgb_data[idx + 2];
        let y = (r + 2. * g + b) / 4.;
        y_plane[i] = (y * 255.) as u8;
    }
    frame.planes[0].copy_from_raw_u8(&y_plane, size, 1);

    // Build Cb and Cr planes (4:2:0 = half width, half height)
    // VSF: Cb = (B - Y) / 2 + 0.5, Cr = (R - Y) / 2 + 0.5
    let chroma_size = size / 2;
    let mut cb_plane = vec![128u8; chroma_size * chroma_size];
    let mut cr_plane = vec![128u8; chroma_size * chroma_size];

    for cy in 0..chroma_size {
        for cx in 0..chroma_size {
            // Average 2×2 block
            let y0 = cy * 2;
            let x0 = cx * 2;
            let idx00 = (y0 * size + x0) * 3;
            let idx01 = (y0 * size + x0 + 1) * 3;
            let idx10 = ((y0 + 1) * size + x0) * 3;
            let idx11 = ((y0 + 1) * size + x0 + 1) * 3;

            let r = (rgb_data[idx00] + rgb_data[idx01] + rgb_data[idx10] + rgb_data[idx11]) / 4.;
            let g = (rgb_data[idx00 + 1]
                + rgb_data[idx01 + 1]
                + rgb_data[idx10 + 1]
                + rgb_data[idx11 + 1])
                / 4.;
            let b = (rgb_data[idx00 + 2]
                + rgb_data[idx01 + 2]
                + rgb_data[idx10 + 2]
                + rgb_data[idx11 + 2])
                / 4.;

            let y = (r + 2. * g + b) / 4.;
            let cb = (b - y) / 2. + 0.5;
            let cr = (r - y) / 2. + 0.5;

            cb_plane[cy * chroma_size + cx] = (cb * 255.) as u8;
            cr_plane[cy * chroma_size + cx] = (cr * 255.) as u8;
        }
    }

    frame.planes[1].copy_from_raw_u8(&cb_plane, chroma_size, 1);
    frame.planes[2].copy_from_raw_u8(&cr_plane, chroma_size, 1);

    ctx.send_frame(frame)
        .map_err(|e| format!("Failed to send frame: {}", e))?;
    ctx.flush();

    // Receive encoded packets
    let mut output = Vec::new();
    loop {
        match ctx.receive_packet() {
            Ok(packet) => output.extend_from_slice(&packet.data),
            Err(EncoderStatus::LimitReached) => break,
            Err(EncoderStatus::Encoded | EncoderStatus::NeedMoreData) => continue,
            Err(e) => return Err(format!("Encoding error: {:?}", e)),
        }
    }

    if output.is_empty() {
        return Err("AV1 encoder produced no output".to_string());
    }

    Ok(output)
}

/// Decodes an AV1-compressed avatar back to VSF RGB
///
/// # Arguments
/// * `av1_data` - Raw AV1 OBU bitstream
///
/// # Returns
/// (width, height, RGB pixel data in VSF RGB colourspace)
pub fn decode_avatar(av1_data: &[u8]) -> Result<(usize, usize, Vec<u8>), String> {
    use rav1d::include::dav1d::data::Dav1dData;
    use rav1d::include::dav1d::dav1d::{Dav1dContext, Dav1dSettings};
    use rav1d::include::dav1d::picture::Dav1dPicture;
    use rav1d::src::lib::{
        dav1d_close, dav1d_data_create, dav1d_default_settings, dav1d_get_picture, dav1d_open,
        dav1d_picture_unref, dav1d_send_data,
    };
    use std::ptr::NonNull;

    // Initialize settings with defaults
    let mut settings = std::mem::MaybeUninit::<Dav1dSettings>::uninit();
    unsafe { dav1d_default_settings(NonNull::new(settings.as_mut_ptr()).unwrap()) };
    let settings = unsafe { settings.assume_init() };

    // Open decoder context
    let mut ctx: Option<Dav1dContext> = None;
    let result = unsafe {
        dav1d_open(
            NonNull::new(&mut ctx as *mut _),
            NonNull::new(&settings as *const _ as *mut _),
        )
    };
    if result.0 < 0 {
        return Err(format!("dav1d_open failed: {}", result.0));
    }
    let ctx = ctx.ok_or("dav1d_open returned null context")?;

    // Create data buffer and copy input
    let mut data = Dav1dData::default();
    let data_ptr = unsafe { dav1d_data_create(NonNull::new(&mut data), av1_data.len()) };
    if data_ptr.is_null() {
        unsafe { dav1d_close(NonNull::new(&mut Some(ctx) as *mut _)) };
        return Err("dav1d_data_create failed".to_string());
    }
    // Copy input data into the allocated buffer
    unsafe {
        std::ptr::copy_nonoverlapping(av1_data.as_ptr(), data_ptr, av1_data.len());
    }

    // Send data to decoder - keep sending until consumed
    loop {
        let send_result = unsafe { dav1d_send_data(Some(ctx), NonNull::new(&mut data)) };
        if send_result.0 == 0 {
            break; // Data consumed
        } else if send_result.0 == -11 {
            // EAGAIN - decoder is busy, try to drain pictures first
            continue;
        } else if send_result.0 < 0 {
            unsafe { dav1d_close(NonNull::new(&mut Some(ctx) as *mut _)) };
            return Err(format!("dav1d_send_data failed: {}", send_result.0));
        }
    }

    // Get decoded picture - may need multiple attempts
    let mut pic = Dav1dPicture::default();
    loop {
        let get_result = unsafe { dav1d_get_picture(Some(ctx), NonNull::new(&mut pic)) };
        if get_result.0 == 0 {
            break; // Got a picture
        } else if get_result.0 == -11 {
            // EAGAIN - no picture ready yet, decoder needs to process
            // For single-frame decode, this shouldn't happen after send succeeds
            // but let's be safe
            std::thread::yield_now();
            continue;
        } else {
            unsafe { dav1d_close(NonNull::new(&mut Some(ctx) as *mut _)) };
            return Err(format!("dav1d_get_picture failed: {}", get_result.0));
        }
    }

    // Extract dimensions
    let width = pic.p.w as usize;
    let height = pic.p.h as usize;
    let stride_y = pic.stride[0] as usize;
    let stride_uv = pic.stride[1] as usize;

    // Get plane pointers
    let y_ptr = pic.data[0].ok_or("No Y plane")?.as_ptr() as *const u8;
    let u_ptr = pic.data[1].ok_or("No U plane")?.as_ptr() as *const u8;
    let v_ptr = pic.data[2].ok_or("No V plane")?.as_ptr() as *const u8;

    // Convert YUV 4:2:0 to RGB using VSF colour formulas
    // VSF: Y = (R + 2G + B) / 4, Cb = (B - Y) / 2 + 0.5, Cr = (R - Y) / 2 + 0.5
    // Inverse: B = 2*(Cb - 0.5) + Y, R = 2*(Cr - 0.5) + Y, G = (4*Y - R - B) / 2
    let mut rgb = vec![0u8; width * height * 3];

    for y in 0..height {
        for x in 0..width {
            let y_val = unsafe { *y_ptr.add(y * stride_y + x) } as f32 / 255.0;
            let u_val = unsafe { *u_ptr.add((y / 2) * stride_uv + (x / 2)) } as f32 / 255.0;
            let v_val = unsafe { *v_ptr.add((y / 2) * stride_uv + (x / 2)) } as f32 / 255.0;

            // VSF inverse: Cb/Cr are centered at 0.5
            let cb = u_val - 0.5;
            let cr = v_val - 0.5;

            // R = Y + 2*Cr, B = Y + 2*Cb, G = (4*Y - R - B) / 2
            let r = (y_val + 2.0 * cr).clamp(0.0, 1.0);
            let b = (y_val + 2.0 * cb).clamp(0.0, 1.0);
            let g: f32 = ((4.0 * y_val - r - b) / 2.0).clamp(0.0, 1.0);

            let idx = (y * width + x) * 3;
            rgb[idx] = (r * 255.0) as u8;
            rgb[idx + 1] = (g * 255.0) as u8;
            rgb[idx + 2] = (b * 255.0) as u8;
        }
    }

    // Cleanup
    unsafe {
        dav1d_picture_unref(NonNull::new(&mut pic));
        dav1d_close(NonNull::new(&mut Some(ctx) as *mut _));
    }

    Ok((width, height, rgb))
}

/// Android data directory (set at init time)
#[cfg(target_os = "android")]
static ANDROID_DATA_DIR: std::sync::OnceLock<String> = std::sync::OnceLock::new();

/// Set the Android data directory (called from JNI init)
#[cfg(target_os = "android")]
pub fn set_android_data_dir(path: String) {
    let _ = ANDROID_DATA_DIR.set(path);
}

/// Get the Android data directory (for use by other modules)
#[cfg(target_os = "android")]
pub fn get_android_data_dir() -> Option<std::path::PathBuf> {
    ANDROID_DATA_DIR.get().map(|s| std::path::PathBuf::from(s))
}

/// Get the avatars directory
/// - Android: /data/data/com.photon.messenger/files/avatars/
/// - Desktop: ~/.config/photon/avatars/
pub fn avatars_dir() -> std::io::Result<std::path::PathBuf> {
    #[cfg(target_os = "android")]
    let base_dir = {
        let data_dir = ANDROID_DATA_DIR.get().ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::NotFound, "Android data dir not set")
        })?;
        std::path::PathBuf::from(data_dir)
    };

    #[cfg(not(target_os = "android"))]
    let base_dir = dirs::config_dir()
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "No config dir"))?
        .join("photon");

    let avatars_dir = base_dir.join("avatars");
    std::fs::create_dir_all(&avatars_dir)?;

    Ok(avatars_dir)
}

/// Get path for a cached avatar by its storage key
/// Storage key = base64url(BLAKE3(BLAKE3(handle) || "avatar"))
pub fn avatar_cache_path(storage_key: &str) -> std::io::Result<std::path::PathBuf> {
    Ok(avatars_dir()?.join(format!("{}.vsf", storage_key)))
}

/// Load avatar from local cache by handle (checks avatars/ directory)
/// Returns None if not cached locally
pub fn load_cached_avatar(handle: &str) -> Option<(usize, Vec<u8>)> {
    let storage_key = avatar_storage_key(handle);
    let cache_path = avatar_cache_path(&storage_key).ok()?;

    if !cache_path.exists() {
        return None;
    }

    let vsf_data = std::fs::read(&cache_path).ok()?;
    crate::log(&format!("Avatar: Loading {} from local cache", handle));
    load_avatar_from_bytes(&vsf_data, handle)
}

/// Save avatar VSF bytes to local cache by handle
fn save_avatar_to_cache(handle: &str, vsf_data: &[u8]) -> std::io::Result<()> {
    let storage_key = avatar_storage_key(handle);
    let cache_path = avatar_cache_path(&storage_key)?;
    std::fs::write(&cache_path, vsf_data)?;
    crate::log(&format!(
        "Avatar: Cached {} locally ({}...)",
        handle,
        &storage_key[..8]
    ));
    Ok(())
}

/// Load avatar from disk by handle (returns None if not cached)
pub fn load_avatar(handle: &str) -> Option<(usize, Vec<u8>)> {
    load_cached_avatar(handle)
}

/// Load avatar from raw VSF bytes (used for both local and network avatars)
///
/// Avatar data is encrypted with handle-derived key, so handle is required for decryption.
/// Format: v'e'(encrypted v'a'(AV1 data))
pub fn load_avatar_from_bytes(vsf_data: &[u8], handle: &str) -> Option<(usize, Vec<u8>)> {
    // Verify this is an unmodified original before processing
    if let Err(e) = vsf::verification::is_original(vsf_data) {
        crate::log(&format!(
            "Avatar: Provenance hash verification failed: {}",
            e
        ));
        return None;
    }

    // Parse VSF container - expecting v'e' encrypted wrapper
    let parsed = vsf::builders::parse_compressed_image(vsf_data).ok()?;

    // Verify encrypted format
    if parsed.encoding != b'e' {
        crate::log(&format!(
            "Avatar: Expected encrypted (v'e'), got: {}",
            parsed.encoding as char
        ));
        return None;
    }

    // Decrypt to get raw AV1 data
    let av1_data = match decrypt_av1_data(&parsed.data, handle) {
        Ok(data) => data,
        Err(e) => {
            crate::log(&format!("Avatar: Decryption failed: {}", e));
            return None;
        }
    };

    // Decode AV1 to pixels (dimensions come from AV1 bitstream)
    let (width, height, pixels) = match decode_avatar(&av1_data) {
        Ok(result) => result,
        Err(e) => {
            crate::log(&format!("Avatar: decode_avatar failed: {}", e));
            return None;
        }
    };

    // Avatar must be square
    if width != height {
        return None;
    }

    Some((width, pixels))
}

/// Save avatar to disk as VSF by handle
/// Uses "image" section with "pixels" field containing v'e'(encrypted v'a'(AV1))
/// Only people who know the handle plaintext can decrypt the avatar.
/// Stored in avatars/ directory using handle-based storage key
pub fn save_avatar(av1_data: &[u8], handle: &str) -> std::io::Result<()> {
    use vsf::{VsfBuilder, VsfType};

    // Encrypt AV1 data (wraps in v'a' then encrypts)
    let encrypted = encrypt_av1_data(av1_data, handle)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;

    // Build VSF with v'e' wrapped encrypted payload
    let vsf_bytes = VsfBuilder::new()
        .creation_time_nanos(vsf::eagle_time_nanos())
        .provenance_only()
        .add_section(
            "image",
            vec![("pixels".to_string(), VsfType::v(b'e', encrypted))],
        )
        .build()
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;

    // Save to avatars directory using handle's storage key
    save_avatar_to_cache(handle, &vsf_bytes)
}

const FGTW_URL: &str = "https://fgtw.org";

/// Extract AV1 data from avatar VSF (decrypts v'e' wrapper)
fn extract_av1_data(vsf_bytes: &[u8], handle: &str) -> Result<Vec<u8>, String> {
    let parsed = vsf::builders::parse_compressed_image(vsf_bytes)?;

    if parsed.encoding != b'e' {
        return Err(format!(
            "Expected encrypted (v'e'), got: {}",
            parsed.encoding as char
        ));
    }

    decrypt_av1_data(&parsed.data, handle)
}

/// Get avatar's provenance hash by handle (if cached locally)
/// Used to include in ping/pong messages for avatar sync
pub fn get_avatar_provenance_hash(handle: &str) -> Option<[u8; 32]> {
    use vsf::file_format::VsfHeader;
    use vsf::VsfType;

    let storage_key = avatar_storage_key(handle);
    let cache_path = avatar_cache_path(&storage_key).ok()?;
    let vsf_data = std::fs::read(&cache_path).ok()?;

    // Parse header to extract provenance hash
    let (header, _) = VsfHeader::decode(&vsf_data).ok()?;
    match header.provenance_hash {
        VsfType::hp(bytes) if bytes.len() == 32 => {
            let mut arr = [0u8; 32];
            arr.copy_from_slice(&bytes);
            Some(arr)
        }
        _ => None,
    }
}

/// Get avatar's creation timestamp (Eagle Time) from local cache
/// Returns None if not cached or parsing fails
pub fn get_local_avatar_timestamp(handle: &str) -> Option<f64> {
    use vsf::file_format::VsfHeader;
    use vsf::types::EagleTime;
    use vsf::VsfType;

    let storage_key = avatar_storage_key(handle);
    let cache_path = avatar_cache_path(&storage_key).ok()?;

    #[cfg(feature = "development")]
    crate::log(&format!(
        "Avatar: Looking for local cache at {:?}",
        cache_path
    ));

    let vsf_data = match std::fs::read(&cache_path) {
        Ok(data) => data,
        Err(e) => {
            #[cfg(feature = "development")]
            crate::log(&format!("Avatar: No local cache: {}", e));
            return None;
        }
    };

    // Parse header to extract creation timestamp
    let (header, _) = VsfHeader::decode(&vsf_data).ok()?;
    match header.creation_time {
        VsfType::e(et) => {
            let ts = EagleTime::new(et).to_f64();
            #[cfg(feature = "development")]
            crate::log(&format!("Avatar: Local timestamp = {:.0}", ts));
            Some(ts)
        }
        _ => None,
    }
}

/// Derive the avatar Ed25519 keypair from device private key and handle
///
/// This creates a deterministic keypair tied to both the device identity and handle.
/// The private key never leaves the client - only the public key is shared.
///
/// Formula: avatar_priv_seed = BLAKE3(device_private_key || handle_hash || "handle-avatar")
/// Then derive Ed25519 keypair from that 32-byte seed.
///
/// # Arguments
/// * `device_secret` - The device's Ed25519 signing key (32 bytes)
/// * `handle` - The user's handle string
///
/// # Returns
/// (SigningKey, VerifyingKey) - The avatar's Ed25519 keypair
pub fn derive_avatar_keypair(
    device_secret: &SigningKey,
    handle: &str,
) -> (SigningKey, VerifyingKey) {
    // VSF normalize handle for consistent key derivation
    let vsf_bytes = vsf::VsfType::x(handle.to_string()).flatten();
    let handle_hash = blake3::hash(&vsf_bytes);

    // Derive avatar private key seed: BLAKE3(device_priv || handle_hash || "handle-avatar")
    let mut hasher = blake3::Hasher::new();
    hasher.update(device_secret.as_bytes());
    hasher.update(handle_hash.as_bytes());
    hasher.update(b"handle-avatar");
    let seed = hasher.finalize();

    // Create Ed25519 keypair from seed
    let signing_key = SigningKey::from_bytes(seed.as_bytes());
    let verifying_key = signing_key.verifying_key();

    (signing_key, verifying_key)
}

/// Compute the avatar storage key from handle
///
/// This is the public URL-safe identifier for fetching avatars from FGTW.
/// Anyone who knows a handle can compute the storage key and fetch the avatar.
/// Formula: base64url(BLAKE3(BLAKE3(VsfType::x(handle).flatten()) || "avatar"))
///
/// # Arguments
/// * `handle` - The user's handle string
///
/// # Returns
/// Base64url-encoded 32-byte hash (no padding)
pub fn avatar_storage_key(handle: &str) -> String {
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};

    // VSF normalize handle for consistent key derivation
    let vsf_bytes = vsf::VsfType::x(handle.to_string()).flatten();
    let handle_hash = blake3::hash(&vsf_bytes);
    let mut salted = handle_hash.as_bytes().to_vec();
    salted.extend_from_slice(b"avatar");
    let avatar_hash = blake3::hash(&salted);
    URL_SAFE_NO_PAD.encode(avatar_hash.as_bytes())
}

/// Derive the avatar encryption key from handle
///
/// This key is used to encrypt avatar data so only people who know
/// the handle plaintext can decrypt it.
/// Formula: BLAKE3(BLAKE3(VsfType::x(handle).flatten()) || "avatar-encryption")
///
/// # Arguments
/// * `handle` - The user's handle string
///
/// # Returns
/// 32-byte AES-256-GCM key
pub fn derive_avatar_encryption_key(handle: &str) -> [u8; 32] {
    // VSF normalize handle for consistent key derivation
    let vsf_bytes = vsf::VsfType::x(handle.to_string()).flatten();
    let handle_hash = blake3::hash(&vsf_bytes);
    let mut salted = handle_hash.as_bytes().to_vec();
    salted.extend_from_slice(b"avatar-encryption");
    *blake3::hash(&salted).as_bytes()
}

/// Encrypt AV1 avatar data using handle-derived key
///
/// Wraps AV1 data in v'a', then encrypts with AES-256-GCM.
/// Format: [nonce:12][ciphertext][tag:16]
///
/// # Arguments
/// * `av1_data` - Raw AV1 OBU bitstream
/// * `handle` - User's handle (for key derivation)
///
/// # Returns
/// Encrypted blob ready to be wrapped in v'e'
pub fn encrypt_av1_data(av1_data: &[u8], handle: &str) -> Result<Vec<u8>, String> {
    use aes_gcm::{aead::Aead, Aes256Gcm, KeyInit};
    use rand::RngCore;

    // Build v'a' wrapped AV1 data
    let va_wrapped = encode_va_wrapper(av1_data);

    // Derive encryption key from handle
    let key = derive_avatar_encryption_key(handle);
    let cipher =
        Aes256Gcm::new_from_slice(&key).map_err(|e| format!("Failed to create cipher: {}", e))?;

    // Generate random nonce
    let mut nonce_bytes = [0u8; 12];
    rand::thread_rng().fill_bytes(&mut nonce_bytes);

    // Encrypt
    let ciphertext = cipher
        .encrypt(&nonce_bytes.into(), va_wrapped.as_ref())
        .map_err(|e| format!("Encryption failed: {}", e))?;

    // Format: [nonce:12][ciphertext+tag]
    let mut result = Vec::with_capacity(12 + ciphertext.len());
    result.extend_from_slice(&nonce_bytes);
    result.extend_from_slice(&ciphertext);

    Ok(result)
}

/// Decrypt avatar data using handle-derived key
///
/// Decrypts v'e' payload and unwraps the inner v'a' to get raw AV1 data.
/// Format: [nonce:12][ciphertext][tag:16]
///
/// # Arguments
/// * `encrypted` - Encrypted blob (from v'e' wrapper)
/// * `handle` - User's handle (for key derivation)
///
/// # Returns
/// Raw AV1 OBU bitstream
pub fn decrypt_av1_data(encrypted: &[u8], handle: &str) -> Result<Vec<u8>, String> {
    use aes_gcm::{aead::Aead, Aes256Gcm, KeyInit};

    if encrypted.len() < 12 + 16 {
        return Err(format!(
            "Encrypted data too short: {} bytes (need at least 28)",
            encrypted.len()
        ));
    }

    // Extract nonce and ciphertext
    let nonce_bytes: [u8; 12] = encrypted[0..12]
        .try_into()
        .map_err(|_| "Failed to extract nonce")?;
    let ciphertext = &encrypted[12..];

    // Derive encryption key from handle
    let key = derive_avatar_encryption_key(handle);
    let cipher =
        Aes256Gcm::new_from_slice(&key).map_err(|e| format!("Failed to create cipher: {}", e))?;

    // Decrypt
    let va_wrapped = cipher
        .decrypt(&nonce_bytes.into(), ciphertext)
        .map_err(|e| format!("Decryption failed: {}", e))?;

    // Unwrap v'a' to get raw AV1 data
    decode_va_wrapper(&va_wrapped)
}

/// Encode raw bytes as v'a' VSF wrapper
/// Format: 'v' 'a' [length encoding] [data]
fn encode_va_wrapper(data: &[u8]) -> Vec<u8> {
    let mut result = Vec::with_capacity(2 + 5 + data.len());
    result.push(b'v');
    result.push(b'a');
    // Encode length using VSF length encoding (len-1, size class)
    encode_vsf_length(&mut result, data.len());
    result.extend_from_slice(data);
    result
}

/// Decode v'a' VSF wrapper to get raw bytes
fn decode_va_wrapper(wrapped: &[u8]) -> Result<Vec<u8>, String> {
    if wrapped.len() < 2 {
        return Err("v'a' wrapper too short".to_string());
    }
    if wrapped[0] != b'v' || wrapped[1] != b'a' {
        return Err(format!(
            "Expected v'a' wrapper, got {:?}{:?}",
            wrapped[0] as char, wrapped[1] as char
        ));
    }

    // Decode length
    let (len, consumed) = decode_vsf_length(&wrapped[2..])?;
    let data_start = 2 + consumed;

    if wrapped.len() < data_start + len {
        return Err(format!(
            "v'a' wrapper truncated: expected {} bytes, got {}",
            data_start + len,
            wrapped.len()
        ));
    }

    Ok(wrapped[data_start..data_start + len].to_vec())
}

/// Encode VSF length (len-1 with size class marker)
fn encode_vsf_length(buf: &mut Vec<u8>, len: usize) {
    let len_minus_1 = len.saturating_sub(1);
    if len_minus_1 <= 0xFF {
        buf.push(b'3'); // u8 size class
        buf.push(len_minus_1 as u8);
    } else if len_minus_1 <= 0xFFFF {
        buf.push(b'4'); // u16 size class
        buf.extend_from_slice(&(len_minus_1 as u16).to_le_bytes());
    } else if len_minus_1 <= 0xFFFFFFFF {
        buf.push(b'5'); // u32 size class
        buf.extend_from_slice(&(len_minus_1 as u32).to_le_bytes());
    } else {
        buf.push(b'6'); // u64 size class
        buf.extend_from_slice(&(len_minus_1 as u64).to_le_bytes());
    }
}

/// Decode VSF length encoding
fn decode_vsf_length(buf: &[u8]) -> Result<(usize, usize), String> {
    if buf.is_empty() {
        return Err("Empty length encoding".to_string());
    }

    match buf[0] {
        b'3' => {
            if buf.len() < 2 {
                return Err("Truncated u8 length".to_string());
            }
            Ok((buf[1] as usize + 1, 2))
        }
        b'4' => {
            if buf.len() < 3 {
                return Err("Truncated u16 length".to_string());
            }
            let len = u16::from_le_bytes([buf[1], buf[2]]) as usize + 1;
            Ok((len, 3))
        }
        b'5' => {
            if buf.len() < 5 {
                return Err("Truncated u32 length".to_string());
            }
            let len = u32::from_le_bytes([buf[1], buf[2], buf[3], buf[4]]) as usize + 1;
            Ok((len, 5))
        }
        b'6' => {
            if buf.len() < 9 {
                return Err("Truncated u64 length".to_string());
            }
            let len = u64::from_le_bytes([
                buf[1], buf[2], buf[3], buf[4], buf[5], buf[6], buf[7], buf[8],
            ]) as usize
                + 1;
            Ok((len, 9))
        }
        _ => Err(format!("Unknown length size class: {}", buf[0] as char)),
    }
}

/// Build a signed avatar VSF for upload
///
/// Creates a VSF with:
/// - Encrypted AV1 data in "image" section with "pixels" v'e'(v'a'(AV1)) field
/// - Avatar public key (ke) in header
/// - Signature (ge) over provenance hash
/// - Creation timestamp for replay protection
///
/// # Arguments
/// * `av1_data` - Raw AV1 OBU bitstream (from encode_avatar_from_image)
/// * `handle` - User's handle (for encryption key derivation)
/// * `avatar_signing_key` - Avatar's Ed25519 signing key
/// * `avatar_verifying_key` - Avatar's Ed25519 verifying key
///
/// # Returns
/// Complete signed VSF bytes ready for upload
pub fn build_signed_avatar_vsf(
    av1_data: &[u8],
    handle: &str,
    avatar_signing_key: &SigningKey,
    avatar_verifying_key: &VerifyingKey,
) -> Result<Vec<u8>, String> {
    use ed25519_dalek::Signer;
    use vsf::{VsfBuilder, VsfType};

    // Encrypt AV1 data (wraps in v'a' then encrypts)
    let encrypted = encrypt_av1_data(av1_data, handle)?;

    // Build VSF with avatar pubkey and signature placeholder
    // Uses "image" section with "pixels" v'e' field for encrypted data
    let vsf_bytes = VsfBuilder::new()
        .creation_time_nanos(vsf::eagle_time_nanos())
        .signature_ed25519(
            *avatar_verifying_key.as_bytes(),
            [0u8; 64], // Placeholder - will be filled after hp computed
        )
        .add_section(
            "image",
            vec![("pixels".to_string(), VsfType::v(b'e', encrypted))],
        )
        .build()?;

    // Now we need to:
    // 1. Compute provenance hash (hp) - already done by builder
    // 2. Sign the provenance hash
    // 3. Write signature into ge placeholder

    // Extract provenance hash from the built VSF
    let prov_hash = vsf::verification::compute_provenance_hash(&vsf_bytes)?;

    // Sign the provenance hash
    let signature = avatar_signing_key.sign(&prov_hash);

    // Find and write signature into the ge placeholder
    let vsf_bytes = write_signature_to_vsf(vsf_bytes, &signature.to_bytes())?;

    Ok(vsf_bytes)
}

/// Write signature bytes into the ge placeholder in a VSF file
fn write_signature_to_vsf(mut vsf_bytes: Vec<u8>, signature: &[u8; 64]) -> Result<Vec<u8>, String> {
    // Scan for "ge" marker followed by length encoding and 64 zero bytes
    // VSF encodes length as (len-1) with size marker:
    // For 64 bytes: len-1 = 63, which fits in u8, so: '3' (0x33) + 63 (0x3F)
    // Full encoding: 'g' 'e' '3' 63 <64 bytes>
    let mut pos = 0;
    while pos < vsf_bytes.len().saturating_sub(68) {
        if vsf_bytes[pos] == b'g' && vsf_bytes[pos + 1] == b'e' {
            // Check length encoding: '3' followed by 63 (for 64-byte signature)
            if vsf_bytes[pos + 2] == b'3' && vsf_bytes[pos + 3] == 63 {
                let sig_start = pos + 4;
                // Verify it's all zeros (placeholder)
                if vsf_bytes[sig_start..sig_start + 64].iter().all(|&b| b == 0) {
                    // Write signature
                    vsf_bytes[sig_start..sig_start + 64].copy_from_slice(signature);
                    return Ok(vsf_bytes);
                }
            }
        }
        pos += 1;
    }
    Err("Could not find signature placeholder in VSF".to_string())
}

/// Upload avatar to FGTW with signature authentication
///
/// # Arguments
/// * `device_secret` - Device's Ed25519 signing key
/// * `handle` - User's handle
/// * `handle_proof` - 32-byte handle proof (proves registered peer)
///
/// # Returns
/// The avatar storage key on success (for sharing with peers)
pub fn upload_avatar(
    device_secret: &SigningKey,
    handle: &str,
    handle_proof: &[u8; 32],
) -> Result<String, String> {
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};

    // Read from cache by handle
    let storage_key = avatar_storage_key(handle);
    let cache_path = avatar_cache_path(&storage_key).map_err(|e| e.to_string())?;
    let local_vsf = std::fs::read(&cache_path)
        .map_err(|e| format!("Failed to read avatar for {}: {}", handle, e))?;

    // Verify local file is unmodified original
    vsf::verification::is_original(&local_vsf)?;

    // Extract AV1 data from local avatar VSF (decrypts if encrypted)
    let av1_data = extract_av1_data(&local_vsf, handle)?;

    // Derive avatar keypair
    let (avatar_signing, avatar_verifying) = derive_avatar_keypair(device_secret, handle);

    // Build signed avatar VSF (contains compressed_image section)
    let avatar_vsf =
        build_signed_avatar_vsf(&av1_data, handle, &avatar_signing, &avatar_verifying)?;

    // Build conduit request: avatar_put section with key, handle_proof, and avatar VSF
    let request_vsf = vsf::vsf_builder::VsfBuilder::new()
        .creation_time_nanos(vsf::eagle_time_nanos())
        .signed_only(vsf::VsfType::ke(avatar_verifying.as_bytes().to_vec()))
        .add_section("avatar_put", vec![
            ("key".to_string(), vsf::VsfType::d(storage_key.clone())),
            ("handle_proof".to_string(), vsf::VsfType::hP(handle_proof.to_vec())),
            ("avatar_vsf".to_string(), vsf::VsfType::v(b'r', avatar_vsf)),
        ])
        .build()
        .map_err(|e| format!("Build avatar_put request: {}", e))?;

    let url = format!("{}/conduit", FGTW_URL);

    #[cfg(feature = "development")]
    crate::log(&crate::network::inspect::vsf_inspect(
        &request_vsf,
        "FGTW",
        "TX",
        &format!("/conduit avatar_put {}", &storage_key[..8]),
    ));

    let client = reqwest::blocking::Client::new();
    let response = client
        .post(&url)
        .header("Content-Type", "application/octet-stream")
        .body(request_vsf)
        .send()
        .map_err(|e| format!("Failed to upload avatar: {}", e))?;

    let status = response.status();
    let response_bytes = response.bytes().unwrap_or_default();

    #[cfg(feature = "development")]
    if !response_bytes.is_empty() {
        crate::log(&crate::network::inspect::vsf_inspect(
            &response_bytes,
            "FGTW",
            "RX",
            &format!("conduit/avatar_put {}", &storage_key[..8]),
        ));
    }

    if status.is_success() {
        crate::log(&format!(
            "Avatar: Uploaded to FGTW (key: {}...)",
            &storage_key[..8]
        ));
        Ok(storage_key)
    } else {
        let body = String::from_utf8_lossy(&response_bytes);
        Err(format!("Avatar upload failed: {} - {}", status, body))
    }
}

/// Download avatar from FGTW by handle
///
/// Checks local cache first, only fetches from network if not cached.
/// Computes storage key from handle (anyone can fetch anyone's avatar).
/// FGTW strips ke/ge from stored avatars, so we verify provenance hash only.
///
/// # Arguments
/// * `handle` - The peer's handle string
///
/// # Returns
/// (size, pixels) if successful, None otherwise
pub fn download_avatar(handle: &str) -> Option<(usize, Vec<u8>)> {
    // Check local cache first (no network request needed)
    if let Some(cached) = load_cached_avatar(handle) {
        return Some(cached);
    }

    // Not cached locally, fetch from FGTW
    let storage_key = avatar_storage_key(handle);
    crate::log(&format!(
        "Avatar: Fetching {} from FGTW ({}...)",
        handle,
        &storage_key[..8]
    ));

    // Build conduit request: avatar_get section with key
    let request_vsf = match vsf::vsf_builder::VsfBuilder::new()
        .creation_time_nanos(vsf::eagle_time_nanos())
        .provenance_only()
        .add_section("avatar_get", vec![
            ("key".to_string(), vsf::VsfType::d(storage_key.clone())),
        ])
        .build()
    {
        Ok(vsf) => vsf,
        Err(e) => {
            crate::log(&format!("Avatar: Failed to build avatar_get request: {}", e));
            return None;
        }
    };

    let url = format!("{}/conduit", FGTW_URL);
    let client = reqwest::blocking::Client::new();
    let response = client
        .post(&url)
        .header("Content-Type", "application/octet-stream")
        .body(request_vsf)
        .send().ok()?;

    if !response.status().is_success() {
        crate::log(&format!("Avatar: FGTW returned {}", response.status()));
        return None;
    }

    let vsf_data = response.bytes().ok()?;

    #[cfg(feature = "development")]
    crate::log(&crate::network::inspect::vsf_inspect(
        &vsf_data,
        "FGTW",
        "RX",
        &format!("/conduit avatar_get {}", &storage_key[..8]),
    ));

    // Save to local cache before decoding
    let _ = save_avatar_to_cache(handle, &vsf_data);

    // Verify, decrypt, and decode (FGTW stripped ke/ge, so only provenance hash is verified)
    load_avatar_from_bytes(&vsf_data, handle)
}

/// Sync avatar bidirectionally with FGTW (newest wins)
///
/// For the user's own avatar only - compares local and server timestamps,
/// uploads if local is newer, downloads if server is newer.
///
/// # Arguments
/// * `device_secret` - Device's Ed25519 signing key (for uploading)
/// * `handle` - User's handle
///
/// # Returns
/// Ok(SyncResult) describing what action was taken, Err on failure
#[derive(Debug)]
pub enum AvatarSyncResult {
    NoLocalAvatar, // No local avatar to sync
    LocalNewer,    // Uploaded local (was newer)
    ServerNewer,   // Downloaded from server (was newer)
    InSync,        // Timestamps equal, no action needed
    ServerEmpty,   // Server has no avatar
    Error(String), // Something went wrong
}

pub fn sync_avatar_bidirectional(
    device_secret: &SigningKey,
    handle: &str,
    handle_proof: Option<&[u8; 32]>,
) -> AvatarSyncResult {
    let storage_key = avatar_storage_key(handle);

    // Get local timestamp (if we have a local avatar)
    let local_ts = get_local_avatar_timestamp(handle);

    // Build conduit request: avatar_get section with key
    let request_vsf = match vsf::vsf_builder::VsfBuilder::new()
        .creation_time_nanos(vsf::eagle_time_nanos())
        .provenance_only()
        .add_section("avatar_get", vec![
            ("key".to_string(), vsf::VsfType::d(storage_key.clone())),
        ])
        .build()
    {
        Ok(vsf) => vsf,
        Err(e) => return AvatarSyncResult::Error(format!("Build avatar_get request: {}", e)),
    };

    // Query server for avatar with timestamp header
    #[cfg(feature = "development")]
    crate::log(&crate::network::inspect::vsf_inspect(
        &request_vsf,
        "FGTW",
        "TX",
        &format!("conduit/avatar_get {}", &storage_key[..8]),
    ));

    let url = format!("{}/conduit", FGTW_URL);
    let client = reqwest::blocking::Client::new();
    let response = match client
        .post(&url)
        .header("Content-Type", "application/octet-stream")
        .body(request_vsf)
        .send()
    {
        Ok(r) => r,
        Err(e) => return AvatarSyncResult::Error(format!("Network error: {}", e)),
    };

    if response.status() == reqwest::StatusCode::NOT_FOUND {
        // Server has no avatar
        if local_ts.is_some() {
            // We have local, upload it (only if we have handle_proof)
            if let Some(hp) = handle_proof {
                crate::log("Avatar sync: Server empty, uploading local");
                match upload_avatar(device_secret, handle, hp) {
                    Ok(_) => return AvatarSyncResult::LocalNewer,
                    Err(e) => return AvatarSyncResult::Error(format!("Upload failed: {}", e)),
                }
            } else {
                crate::log("Avatar sync: Server empty, but no handle_proof for upload");
                return AvatarSyncResult::Error("No handle_proof for upload".to_string());
            }
        } else {
            return AvatarSyncResult::ServerEmpty;
        }
    }

    if !response.status().is_success() {
        return AvatarSyncResult::Error(format!("Server returned {}", response.status()));
    }

    // Extract server timestamp from header
    let server_ts: Option<f64> = response
        .headers()
        .get("X-Avatar-Timestamp")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse().ok());

    match (local_ts, server_ts) {
        (None, Some(_)) => {
            // No local, server has one - download
            crate::log("Avatar sync: No local avatar, downloading from server");
            let vsf_data = match response.bytes() {
                Ok(b) => b,
                Err(e) => return AvatarSyncResult::Error(format!("Read body: {}", e)),
            };
            #[cfg(feature = "development")]
            crate::log(&crate::network::inspect::vsf_inspect(
                &vsf_data,
                "FGTW",
                "RX",
                &format!("/conduit avatar_get {}", &storage_key[..8]),
            ));
            let _ = save_avatar_to_cache(handle, &vsf_data);
            AvatarSyncResult::ServerNewer
        }
        (Some(local), Some(server)) => {
            if local > server {
                // Local is newer - upload (only if we have handle_proof)
                if let Some(hp) = handle_proof {
                    crate::log(&format!(
                        "Avatar sync: Local newer ({:.0} > {:.0}), uploading",
                        local, server
                    ));
                    match upload_avatar(device_secret, handle, hp) {
                        Ok(_) => AvatarSyncResult::LocalNewer,
                        Err(e) => AvatarSyncResult::Error(format!("Upload failed: {}", e)),
                    }
                } else {
                    crate::log("Avatar sync: Local newer, but no handle_proof for upload");
                    AvatarSyncResult::Error("No handle_proof for upload".to_string())
                }
            } else if server > local {
                // Server is newer - download
                crate::log(&format!(
                    "Avatar sync: Server newer ({:.0} > {:.0}), downloading",
                    server, local
                ));
                let vsf_data = match response.bytes() {
                    Ok(b) => b,
                    Err(e) => return AvatarSyncResult::Error(format!("Read body: {}", e)),
                };
                #[cfg(feature = "development")]
                crate::log(&crate::network::inspect::vsf_inspect(
                    &vsf_data,
                    "FGTW",
                    "RX",
                    &format!("/avatar/{}", &storage_key[..8]),
                ));
                let _ = save_avatar_to_cache(handle, &vsf_data);
                AvatarSyncResult::ServerNewer
            } else {
                AvatarSyncResult::InSync
            }
        }
        (Some(_), None) => {
            // Have local but server didn't send timestamp (shouldn't happen)
            // Upload to be safe (only if we have handle_proof)
            if let Some(hp) = handle_proof {
                crate::log("Avatar sync: Server missing timestamp, uploading local");
                match upload_avatar(device_secret, handle, hp) {
                    Ok(_) => AvatarSyncResult::LocalNewer,
                    Err(e) => AvatarSyncResult::Error(format!("Upload failed: {}", e)),
                }
            } else {
                crate::log("Avatar sync: Server missing timestamp, but no handle_proof for upload");
                AvatarSyncResult::Error("No handle_proof for upload".to_string())
            }
        }
        (None, None) => {
            // Server returned 200 but no timestamp header - still download the avatar
            crate::log("Avatar sync: Server has avatar (no timestamp), downloading");
            let vsf_data = match response.bytes() {
                Ok(b) => b,
                Err(e) => return AvatarSyncResult::Error(format!("Read body: {}", e)),
            };
            #[cfg(feature = "development")]
            crate::log(&crate::network::inspect::vsf_inspect(
                &vsf_data,
                "FGTW",
                "RX",
                &format!("/conduit avatar_get {}", &storage_key[..8]),
            ));
            let _ = save_avatar_to_cache(handle, &vsf_data);
            AvatarSyncResult::ServerNewer
        }
    }
}

/// Result of a background avatar download
pub struct AvatarDownloadResult {
    pub handle: String,
    pub pixels: Option<Vec<u8>>, // 256x256 VSF RGB pixels (None if download/decode failed)
}

/// Spawn background thread to download avatar from FGTW by handle
/// Results are sent to the provided channel
///
/// # Arguments
/// * `handle` - Peer's handle (storage key is derived from this)
/// * `tx` - Channel to send result
/// * `event_proxy` - Optional EventLoopProxy to wake the event loop when done
pub fn download_avatar_background(
    handle: String,
    tx: std::sync::mpsc::Sender<AvatarDownloadResult>,
    #[allow(unused_variables)] event_proxy: OptionalEventProxy,
) {
    std::thread::spawn(move || {
        let result = download_avatar(&handle);
        let pixels = result.map(|(_, p)| p);
        let _ = tx.send(AvatarDownloadResult { handle, pixels });

        // Wake the event loop on desktop
        #[cfg(not(target_os = "android"))]
        if let Some(proxy) = event_proxy {
            let _ = proxy.send_event(PhotonEvent::NetworkUpdate);
        }
    });
}

/// Spawn background thread to sync avatar bidirectionally with FGTW
/// For user's own avatar - compares timestamps and syncs newest version
///
/// # Arguments
/// * `device_secret` - Device's Ed25519 signing key bytes (cloned for thread)
/// * `handle` - User's handle
/// * `handle_proof` - Optional handle proof (for uploads)
/// * `tx` - Channel to send result (pixels if server was newer)
/// * `event_proxy` - Optional EventLoopProxy to wake the event loop when done
pub fn sync_avatar_background(
    device_secret_bytes: [u8; 32],
    handle: String,
    handle_proof: Option<[u8; 32]>,
    tx: std::sync::mpsc::Sender<AvatarDownloadResult>,
    #[allow(unused_variables)] event_proxy: OptionalEventProxy,
) {
    std::thread::spawn(move || {
        let device_secret = SigningKey::from_bytes(&device_secret_bytes);
        let result = sync_avatar_bidirectional(&device_secret, &handle, handle_proof.as_ref());

        // Only send pixels if we downloaded a newer version from server
        let pixels = match result {
            AvatarSyncResult::ServerNewer => {
                // Load the newly downloaded avatar from cache
                load_cached_avatar(&handle).map(|(_, p)| p)
            }
            AvatarSyncResult::LocalNewer => {
                crate::log("Avatar sync: Uploaded local avatar to FGTW");
                None // No need to update UI, local was already displayed
            }
            AvatarSyncResult::InSync => {
                crate::log("Avatar sync: Already in sync with FGTW");
                None
            }
            AvatarSyncResult::NoLocalAvatar | AvatarSyncResult::ServerEmpty => {
                crate::log("Avatar sync: No avatar to sync");
                None
            }
            AvatarSyncResult::Error(e) => {
                crate::log(&format!("Avatar sync error: {}", e));
                None
            }
        };

        let _ = tx.send(AvatarDownloadResult { handle, pixels });

        // Wake the event loop on desktop
        #[cfg(not(target_os = "android"))]
        if let Some(proxy) = event_proxy {
            let _ = proxy.send_event(PhotonEvent::NetworkUpdate);
        }
    });
}

/// Scale avatar pixels from AVATAR_SIZE to target diameter using Mitchell filter
/// Returns None if src is wrong size or scaling fails
pub fn scale_avatar(src: &[u8], diameter: usize) -> Option<Vec<u8>> {
    if src.len() != AVATAR_SIZE * AVATAR_SIZE * 3 {
        return None;
    }

    use resize::Pixel::RGB8;
    use resize::Type::Mitchell;

    let mut resizer =
        resize::new(AVATAR_SIZE, AVATAR_SIZE, diameter, diameter, RGB8, Mitchell).ok()?;
    let mut dst = vec![0u8; diameter * diameter * 3];

    // Convert slices to rgb::RGB8 slices
    let src_rgb: &[rgb::RGB8] = unsafe {
        std::slice::from_raw_parts(src.as_ptr() as *const rgb::RGB8, AVATAR_SIZE * AVATAR_SIZE)
    };
    let dst_rgb: &mut [rgb::RGB8] = unsafe {
        std::slice::from_raw_parts_mut(dst.as_mut_ptr() as *mut rgb::RGB8, diameter * diameter)
    };

    resizer.resize(src_rgb, dst_rgb).ok()?;
    Some(dst)
}
