//! Settings-panel layout — the nav-rail-vs-content split and the stacked content rows, expressed with fluor's [`Region`] so everything scales with the viewport.
//! Aspect-aware, one code path: portrait ties the header height and the content-row heights to the viewport WIDTH (a row is a LINE unit, not a fraction of a tall screen), landscape keeps the original proportional splits. The render arms are geometry-blind — they just consume header/rail/body rows — so the portrait/landscape difference lives entirely in this module's arithmetic (no layout fork).

use fluor::geom::Viewport;
use fluor::region::Region;

/// The two panes of the settings screen plus the shared header strip.
pub struct SettingsLayout {
    /// Top strip carrying the "Settings" title and the back affordance.
    pub header: Region,
    /// Left nav rail listing the nine pages.
    pub rail: Region,
    /// Right pane holding the selected page's body.
    pub content: Region,
    /// Viewport taller than wide — row heights ride the width instead of splitting the full height.
    pub portrait: bool,
}

impl SettingsLayout {
    /// Slice the viewport into header / rail / content.
    /// Landscape: the header takes a thin proportional top band; below it a 1 : 2.2 rail : content split.
    /// Portrait: the header height is a fraction of the WIDTH (a text band — the title must stay a title on a tall screen, not a third of it), and the rail gets a slightly smaller share so the body column keeps room for controls.
    pub fn compute(vp: &Viewport) -> Self {
        let root = Region::from_viewport(vp);
        let portrait = root.h > root.w;
        if portrait {
            // Left-inset past the chrome orb (top-left app icon, ~0.10·w) so the title never collides with it; the back affordance keeps the right edge.
            let header_h = root.w * 0.11;
            let header = Region::new(root.x + root.w * 0.12, root.y, root.w * 0.88, header_h);
            let below = Region::new(root.x, root.y + header_h, root.w, root.h - header_h);
            let [rail, content] = below.split_h([1.0, 2.4]);
            Self { header, rail, content, portrait }
        } else {
            let [header, below] = root.split_v([1.6, 12.0]);
            let [rail, content] = below.split_h([1.0, 2.2]);
            Self { header, rail, content, portrait }
        }
    }

    /// The nine equal-height nav-rail rows, top to bottom, in [`crate::ui::state::SettingsPage::ALL`] order. Each row is inset slightly so the clickable band doesn't touch the pane edges.
    /// Portrait: rows are width-tied line units stacked from the top (comfortable touch bands) instead of a ninth of the whole screen each — the leftover rail below simply stays empty.
    pub fn rail_rows(&self) -> [Region; 9] {
        let inset = self.rail.inset_xy(0.06, 0.0);
        if self.portrait {
            let row_h = (inset.w * 0.30).min(inset.h / 9.0);
            let band = Region::new(inset.x, inset.y, inset.w, row_h * 9.0);
            band.split_v([1.0; 9])
        } else {
            inset.split_v([1.0; 9])
        }
    }

    /// The content pane inset to a comfortable reading column with a little top / side margin.
    /// Portrait: the body's HEIGHT is clamped to ~0.85 of its width, top-aligned — so every render arm's `body.split_v([1.0; N])` yields line-unit rows (~w/9 tall) instead of screen-height fractions. This one clamp is what keeps text spans, pills, and checkboxes sane on a phone, because they all derive their size from row height.
    pub fn content_body(&self) -> Region {
        let inset = self.content.inset_xy(0.06, 0.03);
        if self.portrait {
            let body_h = (inset.w * 0.85).min(inset.h);
            Region::new(inset.x, inset.y, inset.w, body_h)
        } else {
            inset
        }
    }

    /// Stack the content body into `N` top-aligned rows of one "line unit" each, returning each row's region. Rows past what fits still tile the body (they compress) — fine for a stub where every control just needs a slot. `n` is passed as a const generic so the caller gets a fixed-size array back.
    pub fn content_rows<const N: usize>(&self) -> [Region; N] {
        self.content_body().split_v([1.0; N])
    }
}
