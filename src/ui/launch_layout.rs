//! Launch-screen layout: aspect-adaptive port of the pre-fluor proportional slicing from `app.rs::Layout::new` (the `AppState::Launch` arm). The window divides into ~23 vertical units and 8 horizontal units; named slices land at unit boundaries; widgets occupy the rectangles those boundaries cut. Proportions are the Photon design — algorithm constants, not tuning knobs.
//!
//! Two slices interpolate with viewport aspect ratio so the layout stays correct from portrait to ultrawide:
//!   * **`gap0`** (top margin above the spectrum) — `0.75` units in portrait/square, shrinking to `0.25` units in extreme landscape. Tight against the top edge when the window is short.
//!   * **`gap1`** (vertical positioning of the wordmark relative to the spectrum) — `-2` units in portrait (wordmark floats UP into the bottom of the spectrum bar), `+2` units in extreme landscape (wordmark sits BELOW the spectrum with breathing room). Sign flip is intentional: in a short landscape window there's no room to overlap, the spectrum + text need to stack cleanly.
//!
//! Interpolation is `t = (tanh((aspect − 2) · 1.5) + 1) / 2` — a C∞ sigmoid, centred at aspect=2 with slope 1.5. Every derivative is continuous everywhere on ℝ, so the layout has no kinks as you drag the window edge through any aspect ratio. Portrait (aspect≈0.5) gives t≈0.01, square (aspect=1) t≈0.05, 16:9 t≈0.34, 21:9 t≈0.74, ultrawide (aspect≥3) t≈0.99.

use fluor::canvas::PixelRect;

/// 7-row vertical subdivision of [`LaunchLayout::attest_block`] — port of legacy `app::AttestBlockLayout::new`. Slice ratios `[error: 1.5, gap0: 0.5, textbox: 2.0, gap1: 0.25, hint: 1.8, gap2: 0.5, attest: 2.7]` sum to 9.25 units of the block height. Error and textbox are full block width; hint and attest are centred at 3/4 width so the wordmark feels framed by the controls.
pub struct AttestBlockLayout {
    /// Error message slot (rendered when `LaunchState` carries an error). Reserved for slice 8 of the migration.
    pub error: PixelRect,
    /// Handle textbox — full width.
    pub textbox: PixelRect,
    /// "handle" hint label slot (rendered between textbox and attest button). Reserved for slice 8.
    pub hint: PixelRect,
    /// Attest button — centred, 3/4 width.
    pub attest: PixelRect,
}

impl AttestBlockLayout {
    pub fn compute(block: PixelRect) -> Self {
        let block_x = block.x0;
        let block_y = block.y0;
        let block_w = block.x1 - block.x0;
        let block_h = block.y1 - block.y0;

        // Slice ratios in unitless units; cumulative positions converted to pixels via block_h / sum. Matches the legacy `slice_positions` shape — explicit cum so rounding accumulates monotonically (no per-row independent rounding gap).
        let slices = [1.5_f32, 0.5, 2.0, 0.25, 1.8, 0.5, 2.7];
        let sum: f32 = slices.iter().sum();
        let unit_px = block_h as f32 / sum;
        let mut v = [0_usize; 8];
        let mut cum = 0_f32;
        for (i, s) in slices.iter().enumerate() {
            v[i] = (cum * unit_px) as usize;
            cum += s;
        }
        v[7] = (cum * unit_px) as usize;

        // Hint + attest sit at 3/4 width, horizontally centred.
        let narrow_w = block_w * 3 / 4;
        let narrow_x = block_x + (block_w - narrow_w) / 2;

        Self {
            error: PixelRect::new(block_x, block_y + v[0], block_x + block_w, block_y + v[1]),
            textbox: PixelRect::new(block_x, block_y + v[2], block_x + block_w, block_y + v[3]),
            hint: PixelRect::new(narrow_x, block_y + v[4], narrow_x + narrow_w, block_y + v[5]),
            attest: PixelRect::new(narrow_x, block_y + v[6], narrow_x + narrow_w, block_y + v[7]),
        }
    }
}

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

        // Aspect interpolant via tanh — C∞ everywhere, no clamp. Centred at aspect=2 with slope 1.5: portrait (aspect≈0.5) → t≈0.01, ultrawide (aspect≈3) → t≈0.99, square (aspect=1) → t≈0.05. The layout's first derivative w.r.t. aspect stays continuous through every resize step.
        let aspect = buf_w as f32 / buf_h as f32;
        let t = (((aspect - 2.) * 1.5).tanh() + 1.) * 0.5;

        // Aspect-interpolated slices: `gap0` tightens, `gap1` flips sign so the wordmark moves below the spectrum instead of overlapping it.
        let gap0 = 0.75 + (0.25 - 0.75) * t;
        let gap1 = -2. + (2. - -2.) * t;

        // Constant slices — Photon design proportions, not tuning knobs.
        const SPECTRUM: f32 = 6.;
        const PHOTON_TEXT: f32 = 3.5;
        const GAP2: f32 = 1.5;
        const ATTEST_BLOCK: f32 = 5.;
        // Below the attest block: 6 (empty) + 1 (version row, wired when ported) + 1 (bottom gap).
        const RESERVED_BELOW: f32 = 8.;

        let units_total =
            gap0 + SPECTRUM + gap1 + PHOTON_TEXT + GAP2 + ATTEST_BLOCK + RESERVED_BELOW;

        let unit_px = buf_h as f32 / units_total;
        let mut cum = 0_f32;
        cum += gap0;
        let y_spectrum_start = (cum * unit_px) as usize;
        cum += SPECTRUM;
        let y_spectrum_end = (cum * unit_px) as usize;
        cum += gap1;
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
