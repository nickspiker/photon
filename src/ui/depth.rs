//! Photon's pixel depth ([`super::Px`]): what Photon's own paint needs beyond fluor's [`Pixel`] trait, once per depth.
//!
//! Colour in: decoded images and avatars arrive as the display's γ2 RGB (bytes, or f32 where the decode has it) and land at the depth's own precision — an 8-bit γ2 byte goes to the 16-bit `g2t` code of its exact light (one 256-entry table from vsf's encoder), never thru the 8-bit code first.
//! The chrome: fluor's [`DefaultChrome`] is 8-bit only (its layers and the perimeter it draws are `Argb8`), so at a wider depth Photon keeps its background layer at its own depth and widens the chrome's 8-bit front (perimeter, chord hint, orb, strip, title) into the frame once a frame; at 8 bits every call is the chrome's own, exactly as before.

use fluor::canvas::{Canvas, Damage, PixelRect};
use fluor::host::chrome_widget::DefaultChrome;
use fluor::paint::Clip;
use fluor::pixel::{Argb8, G2t16, Pixel};
use rayon::prelude::*;

/// A pixel depth Photon paints at: fluor's [`Pixel`], plus colour in from the display's γ2 RGB and the bridge to fluor's 8-bit chrome.
pub trait Depth: Pixel {
    /// A colour given as the display's γ2 visible bytes `0x00RRGGBB` (a decoded image, an avatar texel) at straight α `alpha` (`0..=ONE`): its `g2t` darkness at this depth, in the platform's channel order (fluor's `fmt`).
    fn from_gamma2(alpha: u32, visible: u32) -> Self;
    /// A colour given as linear light in the display's primaries (diffuse white = 1, light above white kept up to the depth's top code) at straight α `alpha`.
    fn from_light(alpha: u32, light: [f32; 3]) -> Self;
    /// An 8-bit composite pixel (premultiplied α + darkness, as every fluor layer holds) at this depth: the same light and the same fraction of α.
    fn widen_composite(p: Argb8) -> Self;

    /// The window perimeter, the frame's first writer ([`DefaultChrome::rasterize_perimeter`]). `front` is the 8-bit front buffer a wider depth gathers the chrome in (untouched at 8 bits).
    fn chrome_perimeter(chrome: &mut DefaultChrome, target: &mut [Self], w: usize, h: usize, clip_mask: &mut [u8], front: &mut Vec<Argb8>);
    /// The debug chord hint, painted under the perimeter and over the chrome (into the front at a wider depth, so the three keep their order).
    fn chrome_chord_hint(target: &mut [Self], front: &mut [Argb8], w: usize, h: usize, damage: &mut Damage, text: &mut fluor::text::TextRenderer, hints: &[(&str, &str)], span: fluor::Coord);
    /// The chrome layer (orb, controls strip, title) under what is in the frame ([`DefaultChrome::flatten_chrome_into`]); at a wider depth the gathered front then goes into the frame, widened.
    fn chrome_flatten(chrome: &mut DefaultChrome, target: &mut [Self], w: usize, h: usize, front: &mut [Argb8]);
    /// Paint the background layer when the chrome says it needs it: whole ([`DefaultChrome::rasterize_bg`]) or, on a scroll the hint carries, shifted and its exposed band repainted ([`DefaultChrome::scroll_bg`]). `paint` gets the layer's canvas and the band's clip (`None` for a whole repaint).
    /// At 8 bits the layer is the chrome's own; at a wider depth it is `bg`, kept at this depth, repainted exactly when the chrome's own would be.
    fn chrome_paint_bg(chrome: &mut DefaultChrome, bg: &mut Vec<Self>, scroll: Option<(PixelRect, i32, PixelRect)>, damage: &mut Damage, paint: impl FnOnce(&mut Canvas<Self>, Option<Clip>));
    /// The background layer under everything in the frame, the frame's last paint ([`DefaultChrome::flatten_bg_into`]), within `clip`.
    fn chrome_flatten_bg(chrome: &mut DefaultChrome, bg: &[Self], target: &mut [Self], w: usize, h: usize, clip: Option<Clip>);
    /// Run a painter that only draws 8-bit (opsin's viewer) under what is on `canvas`: at 8 bits straight onto it; at a wider depth into the 8-bit `front`, cleared, then widened under the canvas, which is the same front-to-back composite.
    fn paint_8bit(canvas: &mut Canvas<Self>, front: &mut Vec<Argb8>, paint: impl FnOnce(&mut Canvas<Argb8>));
}

impl Depth for Argb8 {
    /// fluor's own conversion, as every 8-bit avatar and image always took it.
    #[inline]
    fn from_gamma2(alpha: u32, visible: u32) -> Self {
        fluor::theme::dark(fluor::theme::fmt(fluor::theme::from_gamma2(visible))).with_alpha(alpha)
    }
    /// VSF's rule at 8 bits ([`fluor::theme::buffer_code`]), light in Q16.
    fn from_light(alpha: u32, light: [f32; 3]) -> Self {
        let code = |l: f32| fluor::theme::buffer_code((l * (1u32 << 16) as f32) as i64);
        let [r, g, b] = ORDER.map(|i| Self::ONE - code(light[i]));
        Self::from_channels(alpha, [r, g, b])
    }
    #[inline(always)]
    fn widen_composite(p: Argb8) -> Self {
        p
    }

    fn chrome_perimeter(chrome: &mut DefaultChrome, target: &mut [Self], w: usize, h: usize, clip_mask: &mut [u8], _front: &mut Vec<Argb8>) {
        chrome.rasterize_perimeter(target, w, h, clip_mask);
    }
    fn chrome_chord_hint(target: &mut [Self], _front: &mut [Argb8], w: usize, h: usize, damage: &mut Damage, text: &mut fluor::text::TextRenderer, hints: &[(&str, &str)], span: fluor::Coord) {
        fluor::paint::draw_chord_hint(&mut Canvas::new(target, w, h, damage), text, hints, span);
    }
    fn chrome_flatten(chrome: &mut DefaultChrome, target: &mut [Self], w: usize, h: usize, _front: &mut [Argb8]) {
        chrome.flatten_chrome_into(target, w, h, None);
    }
    fn chrome_paint_bg(chrome: &mut DefaultChrome, _bg: &mut Vec<Self>, scroll: Option<(PixelRect, i32, PixelRect)>, damage: &mut Damage, paint: impl FnOnce(&mut Canvas<Self>, Option<Clip>)) {
        match scroll {
            Some((rect, dy, band)) => chrome.scroll_bg(rect, dy, band, damage, |canvas| paint(canvas, Some(Clip::new(band.x0, band.y0, band.x1, band.y1)))),
            None => chrome.rasterize_bg(damage, |canvas| paint(canvas, None)),
        }
    }
    fn chrome_flatten_bg(chrome: &mut DefaultChrome, _bg: &[Self], target: &mut [Self], w: usize, h: usize, clip: Option<Clip>) {
        chrome.flatten_bg_into(target, w, h, clip);
    }
    fn paint_8bit(canvas: &mut Canvas<Self>, _front: &mut Vec<Argb8>, paint: impl FnOnce(&mut Canvas<Argb8>)) {
        paint(canvas);
    }
}

impl Depth for G2t16 {
    fn from_gamma2(alpha: u32, visible: u32) -> Self {
        let [r, g, b] = Argb8::channels(fluor::theme::fmt(visible)).map(|c| Self::ONE - GAMMA2_TO_G2T16[c as usize] as u32);
        Self::from_channels(alpha, [r, g, b])
    }
    /// VSF's 16-bit encoder (×2¹⁶ truncation settled against its decoder); NaN and negative light encode to 0, black.
    fn from_light(alpha: u32, light: [f32; 3]) -> Self {
        let [r, g, b] = ORDER.map(|i| Self::ONE - vsf::colour::convert::delinearize_gamma2tanh_u16_f32(light[i]) as u32);
        Self::from_channels(alpha, [r, g, b])
    }
    /// 8-bit code `c` is 16-bit code `c × 2⁸` (the same light, [`Pixel::from_g2t8`]), so an opaque darkness `d` is `256·d + 255`; under α the darkness is premultiplied, `256·d + α`, which keeps darkness ≤ α and takes empty to empty. α is a fraction, `× 257`.
    #[inline]
    fn widen_composite(p: Argb8) -> Self {
        let a = p.alpha();
        Self::from_channels(a * 257, p.channels().map(|d| (d << 8) + a))
    }

    fn chrome_perimeter(chrome: &mut DefaultChrome, _target: &mut [Self], w: usize, h: usize, clip_mask: &mut [u8], front: &mut Vec<Argb8>) {
        front.clear();
        front.resize(w * h, Argb8::EMPTY);
        chrome.rasterize_perimeter(front, w, h, clip_mask);
    }
    fn chrome_chord_hint(_target: &mut [Self], front: &mut [Argb8], w: usize, h: usize, damage: &mut Damage, text: &mut fluor::text::TextRenderer, hints: &[(&str, &str)], span: fluor::Coord) {
        fluor::paint::draw_chord_hint(&mut Canvas::new(front, w, h, damage), text, hints, span);
    }
    fn chrome_flatten(chrome: &mut DefaultChrome, target: &mut [Self], w: usize, h: usize, front: &mut [Argb8]) {
        chrome.flatten_chrome_into(front, w, h, None);
        compose_widened(target, w, front);
    }
    fn chrome_paint_bg(chrome: &mut DefaultChrome, bg: &mut Vec<Self>, scroll: Option<(PixelRect, i32, PixelRect)>, damage: &mut Damage, paint: impl FnOnce(&mut Canvas<Self>, Option<Clip>)) {
        let (w, h) = chrome.dims();
        // The chrome's own 8-bit layer is still cleared and shifted (its closure runs exactly when it repaints, which is what says this layer must), but stays empty: this layer is the one painted and flattened.
        match scroll {
            Some((rect, dy, band)) => chrome.scroll_bg(rect, dy, band, damage, |c8| {
                fluor::paint::scroll_copy_rect(bg, w, h, rect.x0, rect.y0, rect.x1, rect.y1, dy);
                // WHY/PROOF: band is the app's exposed-band rect, which can overhang the layer (a pane past a shrunken window), as the chrome's own clear guards — the fill would index past the layer.
                let (bx0, bx1) = (band.x0.min(w), band.x1.min(w));
                for y in band.y0.min(h)..band.y1.min(h) {
                    bg[y * w + bx0..y * w + bx1].fill(Self::EMPTY);
                }
                paint(&mut Canvas::new(bg, w, h, c8.damage), Some(Clip::new(band.x0, band.y0, band.x1, band.y1)));
            }),
            None => chrome.rasterize_bg(damage, |c8| {
                bg.clear();
                bg.resize(w * h, Self::EMPTY);
                paint(&mut Canvas::new(bg, w, h, c8.damage), None);
            }),
        }
    }
    fn chrome_flatten_bg(chrome: &mut DefaultChrome, bg: &[Self], target: &mut [Self], w: usize, h: usize, clip: Option<Clip>) {
        let (bw, bh) = chrome.dims();
        // WHY/PROOF: the layer is the chrome's size, which lags the frame's on a resize frame (fluor's own layer blit clips the same way) — rows and columns past either would index outside one of the buffers; and a clip cut to a shrunken frame can end before it starts, which the row slice would panic on.
        let c = Clip::resolve(clip, bw.min(w), bh.min(h));
        if c.x_end > c.x_start {
            target.par_chunks_mut(w).enumerate().filter(|(y, _)| (c.y_start..c.y_end).contains(y)).for_each(|(y, row)| {
                fluor::paint::flatten_premult(&mut row[c.x_start..c.x_end], &bg[y * bw + c.x_start..y * bw + c.x_end]);
            });
        }
        // Settle the chrome's (empty) 8-bit layer, which only a flatten marks clean: an empty target takes nothing.
        chrome.flatten_bg_into(&mut [], 0, 0, None);
    }
    fn paint_8bit(canvas: &mut Canvas<Self>, front: &mut Vec<Argb8>, paint: impl FnOnce(&mut Canvas<Argb8>)) {
        let (w, h) = (canvas.width, canvas.height);
        front.clear();
        front.resize(w * h, Argb8::EMPTY);
        paint(&mut Canvas::new(front, w, h, canvas.damage));
        compose_widened(canvas.pixels, w, front);
    }
}

/// `src`, an 8-bit composite the frame's size, under `target`, widened a row at a time; rows `src` left empty are skipped.
fn compose_widened<P: Depth>(target: &mut [P], w: usize, src: &[Argb8]) {
    target.par_chunks_mut(w).zip(src.par_chunks(w)).for_each_init(Vec::new, |row, (t, s)| {
        if s.iter().all(|&p| p == Argb8::EMPTY) {
            return;
        }
        row.clear();
        row.extend(s.iter().map(|&p| P::widen_composite(p)));
        fluor::paint::flatten_premult(t, row);
    });
}

/// Which input channel lands in each of the pixel's red, green and blue fields: fluor's `fmt` (identity, or R↔B on Android's RGBA surface) read off one probe value.
pub(crate) const ORDER: [usize; 3] = {
    let p = fluor::theme::fmt(0x00_00_01_02);
    [((p >> 16) & 0xFF) as usize, ((p >> 8) & 0xFF) as usize, (p & 0xFF) as usize]
};

/// A γ2 byte (÷255, white at 255) → the 16-bit `g2t` code of its exact light, by vsf's encoder: the 16-bit twin of fluor's `GAMMA2_TO_G2T`, without the 8-bit code in between.
static GAMMA2_TO_G2T16: std::sync::LazyLock<[u16; 256]> = std::sync::LazyLock::new(|| {
    core::array::from_fn(|c| {
        let v = c as f32 / 255.;
        vsf::colour::convert::delinearize_gamma2tanh_u16_f32(v * v)
    })
});

#[cfg(test)]
mod tests {
    use super::*;

    /// The 16-bit table lands each γ2 byte in the 8-bit code fluor gives it, or on the edge of a neighbour: the same light, 2⁸ times finer. fluor rounds the light to Q16 before it takes the bucket, so a light just under a bucket's bottom can land in it at 8 bits (γ2 28: 16-bit 7167, one step under 28·2⁸).
    #[test]
    fn gamma2_table_refines_fluors() {
        let mut inside = 0;
        for c in 0..256 {
            let (wide, narrow) = (GAMMA2_TO_G2T16[c] as i32, fluor::theme::GAMMA2_TO_G2T[c] as i32);
            assert!(((wide >> 8) - narrow).abs() <= 1, "γ2 {c}: 16-bit code {wide} against 8-bit {narrow}");
            inside += ((wide >> 8) == narrow) as u32;
        }
        assert!(inside >= 250, "{inside} of 256 inside their 8-bit bucket");
    }

    /// At 8 bits `from_gamma2` is fluor's conversion bit for bit; at 16 it agrees with it to within the 8-bit step, and opaque white and black are the depth's own.
    #[test]
    fn from_gamma2_matches_across_depths() {
        for v in [0x00_00_00_00u32, 0x00_FF_FF_FF, 0x00_80_40_C0, 0x00_12_34_56, 0x00_C2_01_FE] {
            let a8 = <Argb8 as Depth>::from_gamma2(255, v);
            assert_eq!(a8, fluor::theme::dark(fluor::theme::fmt(fluor::theme::from_gamma2(v))));
            let a16 = <G2t16 as Depth>::from_gamma2(G2t16::ONE, v);
            assert_eq!(a16.alpha(), G2t16::ONE);
            for (d8, d16) in a8.channels().into_iter().zip(a16.channels()) {
                assert!(((d16 >> 8) as i32 - d8 as i32).abs() <= 1, "{v:06x}: {d16} against {d8}");
            }
        }
    }

    /// Widening keeps empty empty, opaque colours where `from_g2t8` puts them, and darkness ≤ α at every α.
    #[test]
    fn widen_composite_keeps_the_convention() {
        assert_eq!(G2t16::widen_composite(0), G2t16::EMPTY);
        for c in [0xFF_00_00_00u32, 0xFF_FF_FF_FF, 0xFF_3D_3D_3D, 0xFF_12_80_EE] {
            assert_eq!(G2t16::widen_composite(c), G2t16::from_g2t8(c));
        }
        for a in 0..256u32 {
            let p = G2t16::widen_composite((a << 24) | (a << 16) | (a / 2));
            assert!(p.channels().into_iter().all(|d| d <= p.alpha()));
        }
    }
}
