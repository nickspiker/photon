//! Launch-screen layout: faithful port of the pre-fluor proportional slicing from `app.rs::Layout::new` (the `AppState::Launch` arm). The window divides into 22.75 vertical units and 8 horizontal units; named slices land at fixed unit boundaries; widgets occupy the rectangles those boundaries cut. Proportions are the Photon design — algorithm constants, not tuning knobs, so they stay decimal rather than getting `1 << N`-ified.
//!
//! The wave (`spectrum`) and the logo text (`photon_text`) **overlap by 2 units** on the vertical axis — that's the `gap1: -2` slice in the legacy. Lets the logo float into the bottom of the spectrum bar visually.

use fluor::canvas::PixelRect;

/// Pixel rects for every widget on the Launch screen. `spectrum` is full-width (no horizontal margin); everything else sits inside the 6/8 content column with 1/8 margin on each side.
pub struct LaunchLayout {
    pub spectrum: PixelRect,
    pub photon_text: PixelRect,
    /// Unified rect containing the handle textbox + hint label + attest button. Subdivided by a future `AttestBlockLayout` when we wire those widgets — for now the whole block is one rectangle so caller can stub the slot.
    pub attest_block: PixelRect,
}

impl LaunchLayout {
    pub fn compute(buf_w: usize, buf_h: usize) -> Self {
        // Horizontal: 1/8 margin | 6/8 content | 1/8 margin. Spectrum ignores this and uses full width.
        let content_x = buf_w >> 3;
        let content_w = buf_w - 2 * content_x;

        // Vertical slicing in f32 units. Negative `gap1` is intentional — it pulls `photon_text` 2 units UP into the bottom of `spectrum` so the wordmark floats against the spectrum bar. Total: 0.75 + 6 + (-2) + 3.5 + 1.5 + 5 + 6 + 1 + 1 = 22.75 units.
        const UNITS_TOTAL: f32 = 22.75;
        const GAP0: f32 = 0.75;
        const SPECTRUM: f32 = 6.;
        const GAP1_OVERLAP: f32 = -2.;
        const PHOTON_TEXT: f32 = 3.5;
        const GAP2: f32 = 1.5;
        const ATTEST_BLOCK: f32 = 5.;
        // Remaining (6 + 1 + 1 = 8) is empty + version + bottom gap — version row gets wired when we port that slice.

        let unit_px = buf_h as f32 / UNITS_TOTAL;
        let mut cum = 0_f32;
        cum += GAP0;
        let y_spectrum_start = (cum * unit_px) as usize;
        cum += SPECTRUM;
        let y_spectrum_end = (cum * unit_px) as usize;
        cum += GAP1_OVERLAP;
        let y_text_start = (cum * unit_px) as usize;
        cum += PHOTON_TEXT;
        let y_text_end = (cum * unit_px) as usize;
        cum += GAP2;
        let y_block_start = (cum * unit_px) as usize;
        cum += ATTEST_BLOCK;
        let y_block_end = (cum * unit_px) as usize;

        LaunchLayout {
            spectrum: PixelRect::new(0, y_spectrum_start, buf_w, y_spectrum_end),
            photon_text: PixelRect::new(content_x, y_text_start, content_x + content_w, y_text_end),
            attest_block: PixelRect::new(
                content_x,
                y_block_start,
                content_x + content_w,
                y_block_end,
            ),
        }
    }
}
