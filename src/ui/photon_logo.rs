//! "Photon" wordmark for the Launch screen — direct port of the legacy `compositing.rs::draw_logo_text` (compositing.rs:5170-5340), preserving the legacy visible-RGB composite ops (wrap-add glow + highlight; alpha-weighted darken-toward-black body).
//!
//! Compose order matches legacy compositing.rs exactly: glow → body → highlight, painted bottom-up over a bg that's already in place (noise + chromatic wave). Each pass reads the pixel's visible RGB, applies the legacy op, writes back to α + darkness storage with α preserved.
//!
//! Visual layers (bottom to top in compose order):
//! 1. **Glow scratch (`scratch_glow`)** — "Photon" rendered at [`LOGO_GLOW_GRAY`] gets horizontal+vertical exponential-falloff blur passes → soft outer halo. Wrap-added per visible-RGB channel over the bg (wraps to dark where the bg is already bright — the characteristic Photon chromatic interaction).
//! 2. **Sharp body** — "Photon" rasterized as u8 glyph coverage; each coverage value `a` darkens the bg toward pure black via `visible_new = visible_bg × (255 − a) / 255` (legacy text colour is `LOGO_TEXT_COLOUR` = pure visible black).
//! 3. **Highlight scratch (`scratch_highlight`)** — "Photon" rendered at [`LOGO_HIGHLIGHT_GRAY`] PLUS black-carve passes at `(x+1, y)` and `(x, y+1)` to bevel the right + bottom edges of every glyph, then a sharper horizontal-only blur. Wrap-added over the body → bright top-left rim with a beveled-into-the-surface look.
//!
//! Wrap-add (rather than saturating add) is intentional: where the bg is already light, the glow value wraps around darker — the chromatic interaction between the logo and the spectrum bar behind it that's part of Photon's visual identity. Don't "fix" it to saturating.
//!
//! Font: Oxanium 800 (ExtraBold). Sized via harmonic mean of region-width-by-aspect and region-height so it fits inside the layout's `photon_text` rect smoothly across viewport sizes.

use fluor::canvas::{Canvas, PixelRect};
use fluor::text::TextRenderer;

/// Legacy `theme::LOGO_GLOW_GRAY`. Grayscale weight written into the glow scratch — soft halo source.
const LOGO_GLOW_GRAY: u8 = 192;

/// Legacy `theme::LOGO_HIGHLIGHT_GRAY`. Grayscale weight written into the highlight scratch — tight rim source.
const LOGO_HIGHLIGHT_GRAY: u8 = 128;

/// Aspect ratio (width : height) of the "Photon" wordmark at the rendered weight. Used by the harmonic-mean sizer to fit the text inside `rect`. Sourced from legacy `compositing.rs:5176`.
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
    text.draw_text_center(
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

    // Body scratch — "Photon" at full coverage (255). The coverage byte at each pixel is the alpha-weight used by `composite_body_darken_to_black` to drive the bg toward pure black; AA edges get partial coverage so the rasterized body inherits cosmic-text's hinted antialiasing.
    let mut scratch_body = vec![0_u8; scratch_size];
    text.draw_text_center(
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
    text.draw_text_center(
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
    text.draw_text_center(
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
    text.draw_text_center(
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

    // Compose bottom-to-top over the existing bg (noise + wave): glow under body, body under highlight. Each pass operates in visible-RGB space and preserves α — the bg is opaque (α=0xFF) so no transparency math is needed at the merge points.
    composite_grey_wrap_add(canvas.pixels, buf_w, start, &scratch_glow);
    composite_body_darken_to_black(canvas.pixels, buf_w, start, &scratch_body);
    composite_grey_wrap_add(canvas.pixels, buf_w, start, &scratch_highlight);

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
fn blur_horizontal_soft(buf: &mut [u8]) {
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
fn blur_vertical_soft(buf: &mut [u8], buf_w: usize, virtual_height: usize) {
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
fn composite_grey_wrap_add(pixels: &mut [u32], buf_w: usize, start_row: usize, scratch: &[u8]) {
    const VISIBLE_FLIP: u32 = 0x00FFFFFF;
    const ALPHA_MASK: u32 = 0xFF000000;
    for (i, &grey) in scratch.iter().enumerate() {
        if grey == 0 {
            continue;
        }
        let pixel_idx = i + start_row * buf_w;
        let pixel = pixels[pixel_idx];
        let alpha = pixel & ALPHA_MASK;
        let visible = (pixel & 0x00FFFFFF) ^ VISIBLE_FLIP;
        let r = ((visible >> 16) & 0xFF) as u8;
        let g = ((visible >> 8) & 0xFF) as u8;
        let b = (visible & 0xFF) as u8;
        let r_new = r.wrapping_add(grey) as u32;
        let g_new = g.wrapping_add(grey) as u32;
        let b_new = b.wrapping_add(grey) as u32;
        let visible_new = (r_new << 16) | (g_new << 8) | b_new;
        let darkness_new = visible_new ^ VISIBLE_FLIP;
        pixels[pixel_idx] = alpha | darkness_new;
    }
}

/// Alpha-weighted darken-toward-pure-black (legacy compose op for the sharp body — text colour was visible 0x000000). For coverage byte `a`, blend `visible_new = visible_bg × (255 − a) / 255` per channel. Coverage 255 → fully black, 0 → bg unchanged. α stays put.
fn composite_body_darken_to_black(
    pixels: &mut [u32],
    buf_w: usize,
    start_row: usize,
    scratch: &[u8],
) {
    const VISIBLE_FLIP: u32 = 0x00FFFFFF;
    const ALPHA_MASK: u32 = 0xFF000000;
    for (i, &cov) in scratch.iter().enumerate() {
        if cov == 0 {
            continue;
        }
        let pixel_idx = i + start_row * buf_w;
        let pixel = pixels[pixel_idx];
        let alpha = pixel & ALPHA_MASK;
        let visible = (pixel & 0x00FFFFFF) ^ VISIBLE_FLIP;
        let r = (visible >> 16) & 0xFF;
        let g = (visible >> 8) & 0xFF;
        let b = visible & 0xFF;
        let inv_cov = 0xFF - cov as u32;
        let r_new = (r * inv_cov) / 0xFF;
        let g_new = (g * inv_cov) / 0xFF;
        let b_new = (b * inv_cov) / 0xFF;
        let visible_new = (r_new << 16) | (g_new << 8) | b_new;
        let darkness_new = visible_new ^ VISIBLE_FLIP;
        pixels[pixel_idx] = alpha | darkness_new;
    }
}
