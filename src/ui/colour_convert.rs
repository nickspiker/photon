//! VSF RGB → the display surface's RGB: BT.2020 on Android, Linux and Windows; nothing on macOS, whose surface is ICC-tagged VSF RGB (fluor's renderer_wgpu), so the bytes are already the surface's.
//!
//! Avatar storage is VSF γ=2.0 u8 RGB. The Android ANativeWindow buffer is tagged BT.2020 + γ=2.2 + full-range — we ship γ=2.0 pixels into it because there is no `TRANSFER_GAMMA2_0` constant, and γ=2.2 is the closest named transfer Android offers (see fluor::host::android::surface for the full trade-off note). The result is a slight darkening on Android that we accept; ferros honours γ=2.0 end-to-end.
//!
//! The conversion is a single spectral 3×3 matrix multiply in linear: VSF RGB primaries (703/523/462 nm, Illuminant E) → Rec.2020 primaries (630/532/467 nm, D65). Colorimetric since vsf 02076e6: no chromatic adaptation, so VSF white (Illuminant E) shows as E light on a D65 display, a little warm, and a D65 white shows as D65. D50 never enters this path because we are not round-tripping thru XYZ.

use vsf::colour::convert::apply_matrix_3x3_f32;
use vsf::colour::VSF_RGB2REC2020;

/// Convert γ=2.0 VSF RGB u8 bytes (`[R, G, B, R, G, B, …]`) to the display surface's γ=2.0 RGB u8 bytes of the same shape: BT.2020, or unchanged on macOS (converting there too put avatars and previews thru BT.2020 primaries on a surface that is VSF RGB, a second conversion the compositor then undid wrongly).
pub fn vsf_rgb_to_bt2020(src: &[u8]) -> Vec<u8> {
    #[cfg(target_os = "macos")]
    {
        return src.to_vec();
    }
    #[cfg(not(target_os = "macos"))]
    bt2020(src)
}

#[cfg(not(target_os = "macos"))]
fn bt2020(src: &[u8]) -> Vec<u8> {
    let n = src.len() / 3;
    let mut out = vec![0u8; n * 3];
    for i in 0..n {
        let i3 = i * 3;
        let r = (src[i3] as f32 / 255.0).powi(2);
        let g = (src[i3 + 1] as f32 / 255.0).powi(2);
        let b = (src[i3 + 2] as f32 / 255.0).powi(2);
        let bt = apply_matrix_3x3_f32(&VSF_RGB2REC2020, &[r, g, b]);
        // The u8 cast saturates: an out-of-gamut value above 1 lands on 255, and a negative's √ is NaN, which casts to 0 — the same bytes the old clamp produced, without it.
        out[i3] = (bt[0].sqrt() * 255.0) as u8;
        out[i3 + 1] = (bt[1].sqrt() * 255.0) as u8;
        out[i3 + 2] = (bt[2].sqrt() * 255.0) as u8;
    }
    out
}
