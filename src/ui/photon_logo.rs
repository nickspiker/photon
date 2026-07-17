//! "Photon" wordmark for the Launch screen — three fluor `under()` layers (white glow + highlight, black body).
//!
//! Each visual layer is a proper α+darkness layer composited via `Blend::under` — white (darkness 0) with α = coverage for the glow and highlight rim, black (darkness 0xFFFFFF) with α = coverage for the body.
//! Because they're real under() layers, the logo needs nothing opaque beneath it and draws front-to-back like any fluor content; the background (noise + chromatic wave) composes under it afterward.
//!
//! Composite order is TOPMOST-FIRST (fluor's convention — first-drawn wins the pixel):
//! 1. **Sharp body** — "Photon" rasterized as u8 glyph coverage; each value `a` is the α of a full-darkness (black) under() write, driving the destination toward black (bit-identical to the legacy `visible × (255 − a) / 255` darken). Drawn FIRST so the solid black letters sit on top.
//! 2. **Highlight scratch (`scratch_highlight`)** — "Photon" at [`LOGO_HIGHLIGHT_GRAY`] PLUS black-carve passes at `(x+1, y)` and `(x, y+1)` to bevel the right/bottom edges, then a sharp horizontal blur. A white under() layer, so it shows only where it extends past the black body — a rim bleeding from the letter edges.
//! 3. **Glow scratch (`scratch_glow`)** — "Photon" at [`LOGO_GLOW_GRAY`] with horizontal+vertical falloff blur → soft outer halo. A white under() layer, drawn last so it fills the remaining α budget behind everything: an additive-white glow that soft-clamps at 255 (no wrap artifacts).
//!
//! Font: Oxanium 800 (ExtraBold). Sized via harmonic mean of region-width-by-aspect and region-height so it fits inside the layout's `photon_text` rect smoothly across viewport sizes.

use fluor::canvas::{Canvas, PixelRect};
use fluor::text::TextRenderer;

/// Grayscale weight written into the glow scratch — soft halo source.
const LOGO_GLOW_GRAY: u8 = 192;

/// Grayscale weight written into the highlight scratch — tight rim source.
const LOGO_HIGHLIGHT_GRAY: u8 = 128;

/// Aspect ratio (width : height) of the "Photon" wordmark at the rendered weight. Used by the harmonic-mean sizer to fit the text inside `rect`.
const TEXT_ASPECT: f32 = 6.;

/// Oxanium font family name. Must match the family name in the loaded `.ttf` files (Oxanium-* weights all share this family). Weight 800 selects ExtraBold; weights 200/300/400/500/600/700/800 are loaded by `PhotonApp::init`.
const FONT: &str = "Oxanium";
const WEIGHT: u16 = 800;

pub fn paint_photon_logo(canvas: &mut Canvas, text: &mut TextRenderer, rect: PixelRect) {
    let buf_w = canvas.width;

    // Pick a text size that fits inside `rect` smoothly: harmonic mean of "size that fits the width" and "size that fits the height". Both branches → the right answer at the limit; harmonic mean blends them with a smooth derivative (no piecewise jumps).
    let region_w = (rect.x1 - rect.x0) as f32;
    let region_h = (rect.y1 - rect.y0) as f32;
    let max_by_w = region_w / TEXT_ASPECT;
    let max_by_h = region_h;
    let text_size = 2. * max_by_w * max_by_h / (max_by_w + max_by_h);

    // Text is centred horizontally and vertically within `rect`. Anchor coords are the centre point.
    let text_x = (rect.x0 + rect.x1) as f32 * 0.5;
    let text_y = (rect.y0 + rect.y1) as f32 * 0.5;

    // Scratch [start, stop) is 1.5× the rect's height, centred on `text_y`. Padding tied to rect height (not text size) so the scratch dimensions stay inside the rect's safety margin: half_span = rect_h × 3/4, so start = y_center − rect_h × 3/4 = rect.y0 × 5/4 − rect.y1 × 1/4 and stop = rect.y1 × 5/4 − rect.y0 × 1/4. Since launch_layout produces rects with y0 > 0 (the wordmark always sits below the spectrum bar) and y1 < buf_h (the attest block follows below), both bounds land inside [0, buf_h) by construction — no clamps needed.
    let rect_h_px = rect.y1 - rect.y0;
    let half_span = rect_h_px * 3 / 4;
    let y_center = (rect.y0 + rect.y1) / 2;
    let start = y_center - half_span;
    let stop = y_center + half_span;
    let virtual_height = stop - start;
    let scratch_size = buf_w * virtual_height;

    // Glow scratch — single rasterization at LOGO_GLOW_GRAY, then horizontal (factor 3/4) + vertical (factor 1/2) blurs. Soft outer halo source.
    let mut scratch_glow = vec![0_u8; scratch_size];
    text.draw_text_center_legacy(
        &mut scratch_glow,
        buf_w as u32,
        virtual_height as u32,
        "Photon",
        text_x,
        text_y - start as f32,
        text_size,
        WEIGHT,
        vec![LOGO_GLOW_GRAY],
        0,
        FONT,
    );
    blur_horizontal_soft(&mut scratch_glow);
    blur_vertical_soft(&mut scratch_glow, buf_w, virtual_height);

    // Body scratch — "Photon" at full coverage (255). The coverage byte at each pixel is the α-weight used by `composite_body_black` (an all-dark under() layer) to drive the bg toward pure black; AA edges get partial coverage so the rasterized body inherits cosmic-text's hinted antialiasing.
    let mut scratch_body = vec![0_u8; scratch_size];
    text.draw_text_center_legacy(
        &mut scratch_body,
        buf_w as u32,
        virtual_height as u32,
        "Photon",
        text_x,
        text_y - start as f32,
        text_size,
        WEIGHT,
        vec![0xFF],
        0,
        FONT,
    );

    // Highlight scratch — three rasterizations into one buffer:
    //   1. "Photon" at LOGO_HIGHLIGHT_GRAY (the rim source).
    //   2. "Photon" at black offset 1px right → carves the right edge of each glyph.
    //   3. "Photon" at black offset 1px down → carves the bottom edge.
    // The carves leave the highlight as a thin top-left rim; the subsequent sharp horizontal blur fades it right-to-left.
    let mut scratch_highlight = vec![0_u8; scratch_size];
    text.draw_text_center_legacy(
        &mut scratch_highlight,
        buf_w as u32,
        virtual_height as u32,
        "Photon",
        text_x,
        text_y - start as f32,
        text_size,
        WEIGHT,
        vec![LOGO_HIGHLIGHT_GRAY],
        0,
        FONT,
    );
    text.draw_text_center_legacy(
        &mut scratch_highlight,
        buf_w as u32,
        virtual_height as u32,
        "Photon",
        text_x + 1.,
        text_y - start as f32,
        text_size,
        WEIGHT,
        vec![0],
        0,
        FONT,
    );
    text.draw_text_center_legacy(
        &mut scratch_highlight,
        buf_w as u32,
        virtual_height as u32,
        "Photon",
        text_x,
        text_y - start as f32 + 1.,
        text_size,
        WEIGHT,
        vec![0],
        0,
        FONT,
    );
    blur_horizontal_sharp(&mut scratch_highlight);

    // Composite via under() — fluor is TOPMOST-FIRST (first-drawn = frontmost). Each layer is a proper α+darkness layer (white for glow/highlight, black for body), so the logo needs nothing opaque beneath it — it can be drawn first/topmost and the noise composes under it. (Was painter's-order bottom-to-top RMW that read the background and required the noise painted first; coupling gone.)
    //
    // Stack, front to back: black BODY on top (solid legible letters), then the highlight rim and glow halo behind it (they bleed out from the letter edges). Chosen over highlight-on-the-letters — the embossed look read worse.
    composite_body_black(canvas.pixels, buf_w, start, &scratch_body);
    composite_glow_white(canvas.pixels, buf_w, start, &scratch_highlight);
    composite_glow_white(canvas.pixels, buf_w, start, &scratch_glow);

    // Report the rasterized area to the damage accumulator — full window width since blur passes spread horizontally.
    canvas.damage.add_bounds(0, start, buf_w, stop);
}

/// Sharp 1D blur (factor 15/16): each pixel mixes in 15/16 of the running accumulator (decays slowly across the buffer) and takes the max with its own value so glyph centres stay bright. Two passes (left-to-right then right-to-left) so the rim appears on both sides. Operating linearly on the flat `[u8]` buffer rather than per-row is the legacy behaviour and produces a continuous "ribbon" that tracks the text shape across line wraps; for a single-line wordmark it's effectively per-row.
fn blur_horizontal_sharp(buf: &mut [u8]) {
    let len = buf.len();
    if len == 0 {
        return;
    }
    let mut prev = buf[0];
    for i in 1..len {
        prev = (((buf[i] as u16 + prev as u16 * 15) >> 4) as u8).max(buf[i]);
        buf[i] = prev;
    }
    let mut prev = buf[len - 1];
    for i in (0..len).rev() {
        prev = (((buf[i] as u16 + prev as u16 * 15) >> 4) as u8).max(buf[i]);
        buf[i] = prev;
    }
}

/// Soft 1D blur (factor 3/4) — same shape as `blur_horizontal_sharp` but with a wider spread per step.
pub(crate) fn blur_horizontal_soft(buf: &mut [u8]) {
    let len = buf.len();
    if len == 0 {
        return;
    }
    let mut prev = buf[0];
    for i in 1..len {
        prev = (((buf[i] as u16 + prev as u16 * 3) >> 2) as u8).max(buf[i]);
        buf[i] = prev;
    }
    let mut prev = buf[len - 1];
    for i in (0..len).rev() {
        prev = (((buf[i] as u16 + prev as u16 * 3) >> 2) as u8).max(buf[i]);
        buf[i] = prev;
    }
}

/// Vertical blur (factor 1/2) per column. Two passes (top-to-bottom + bottom-to-top) so the halo spreads symmetrically. Stride is `buf_w` because the scratch is row-major.
pub(crate) fn blur_vertical_soft(buf: &mut [u8], buf_w: usize, virtual_height: usize) {
    if virtual_height < 2 {
        return;
    }
    for x in 0..buf_w {
        let mut prev = buf[x];
        for y in 1..virtual_height {
            let idx = y * buf_w + x;
            prev = (((buf[idx] as u16 + prev as u16) >> 1) as u8).max(buf[idx]);
            buf[idx] = prev;
        }
    }
    for x in 0..buf_w {
        let last_idx = (virtual_height - 1) * buf_w + x;
        let mut prev = buf[last_idx];
        for y in (0..virtual_height - 1).rev() {
            let idx = y * buf_w + x;
            prev = (((buf[idx] as u16 + prev as u16) >> 1) as u8).max(buf[idx]);
            buf[idx] = prev;
        }
    }
}

/// Wrap-add each scratch byte into the canvas pixel's visible RGB (legacy compose op for glow + highlight). Read pixel → XOR to visible → wrap-add grey per channel → XOR back to darkness → preserve α. Wrap-around (not saturating) is intentional: produces Photon's characteristic chromatic-interaction look where bright bg pixels wrap dark.
/// Glow: a WHITE light layer (darkness = 0) composited UNDER whatever's there, α = the blurred coverage byte. `under()` of a darkness-0 pixel brightens the destination toward white by α — an additive-white halo that soft-clamps at 255 (no wrap artifacts). A proper fluor layer, so it needs nothing opaque beneath it and the logo can draw first/topmost.
pub(crate) fn composite_glow_white(pixels: &mut [u32], buf_w: usize, start_row: usize, scratch: &[u8]) {
    use fluor::pixel::{Blend, BlendMode};
    for (i, &grey) in scratch.iter().enumerate() {
        if grey == 0 {
            continue;
        }
        let pixel_idx = i + start_row * buf_w;
        // darkness = 0x000000 (white), α = coverage.
        let src = (grey as u32) << 24;
        pixels[pixel_idx] = pixels[pixel_idx].under(src, BlendMode::Normal);
    }
}

/// Body: a fully-DARK layer (darkness = 0xFFFFFF, i.e. visible black) composited UNDER what's there, α = the glyph coverage byte. `under()` of a full-darkness pixel drives the destination toward black by α — bit-identical to the legacy `visible_bg × (255 − cov) / 255` darken, with AA edges feathering via partial α. A proper fluor layer (needs no opaque base).
fn composite_body_black(pixels: &mut [u32], buf_w: usize, start_row: usize, scratch: &[u8]) {
    use fluor::pixel::{Blend, BlendMode};
    for (i, &cov) in scratch.iter().enumerate() {
        if cov == 0 {
            continue;
        }
        let pixel_idx = i + start_row * buf_w;
        // darkness = 0x00FFFFFF (black), α = coverage.
        let src = ((cov as u32) << 24) | 0x00FF_FFFF;
        pixels[pixel_idx] = pixels[pixel_idx].under(src, BlendMode::Normal);
    }
}
