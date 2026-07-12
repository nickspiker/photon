//! Settings-panel layout — the nav-rail-vs-content split and the stacked content rows, expressed with fluor's [`Region`] so everything scales with the viewport.
//! ONE scaling unit: [`SettingsLayout::unit`] is ReadyLayout's formula — `HM(span/32 · ru, height-budget)` — so every settings element (header, rail rows, content rows, and thru them every font, pill, and control) scales with BOTH the window shape and the zoom factor, exactly like the launch/contacts screens. No element sizes off a bare region fraction anymore; that's what made zoom hit-or-miss (text grew, rows and controls didn't).
//! Portrait vs landscape is arithmetic on the same path (header band + rail split fractions), not a layout fork.

use fluor::geom::Viewport;
use fluor::region::Region;
use fluor::Coord;

/// Harmonic mean — C¹-smooth blend of two size candidates (no kink where they cross); zero if either is.
fn hm(a: Coord, b: Coord) -> Coord {
    let sum = a + b;
    if sum <= 0.0 { 0.0 } else { 2.0 * a * b / sum }
}

/// The two panes of the settings screen plus the shared header strip.
pub struct SettingsLayout {
    /// Top strip carrying the "Settings" title and the back affordance.
    pub header: Region,
    /// Left nav rail listing the nine pages.
    pub rail: Region,
    /// Right pane holding the selected page's body.
    pub content: Region,
    /// Viewport taller than wide — affects only the header/rail split fractions.
    pub portrait: bool,
    /// The one scaling unit everything derives from: `HM(span/32 · ru, h/13)` — span-based (aspect-robust), zoom-aware (ru), height-budgeted (a short window can't overflow). Fonts, row heights, pill heights, and control sizes are all multiples of this.
    pub unit: Coord,
}

impl SettingsLayout {
    /// Slice the viewport into header / rail / content, deriving the shared [`unit`](Self::unit) first.
    /// Portrait additionally left-insets the header past the chrome orb (top-left app icon) so the title never collides with it.
    pub fn compute(vp: &Viewport) -> Self {
        let root = Region::from_viewport(vp);
        let portrait = root.h > root.w;
        let unit = hm((root.span / 32.0) * vp.ru.max(0.2), root.h / 13.0);
        let header_h = (unit * 2.1).min(root.w * 0.13).min(root.h * 0.15);
        if portrait {
            let header = Region::new(root.x + root.w * 0.12, root.y, root.w * 0.88, header_h);
            let below = Region::new(root.x, root.y + header_h, root.w, root.h - header_h);
            let [rail, content] = below.split_h([1.0, 2.4]);
            Self { header, rail, content, portrait, unit }
        } else {
            let header = Region::new(root.x, root.y, root.w, header_h);
            let below = Region::new(root.x, root.y + header_h, root.w, root.h - header_h);
            let [rail, content] = below.split_h([1.0, 2.2]);
            Self { header, rail, content, portrait, unit }
        }
    }

    /// The nine nav-rail rows, top to bottom, in [`crate::ui::state::SettingsPage::ALL`] order — unit-tall touch bands stacked from the top (capped so nine always fit), leftover rail left empty. Each row is inset slightly so the clickable band doesn't touch the pane edges.
    pub fn rail_rows(&self) -> [Region; 9] {
        let inset = self.rail.inset_xy(0.06, 0.0);
        let row_h = (self.unit * 1.5).min(inset.h / 9.0);
        let band = Region::new(inset.x, inset.y, inset.w, row_h * 9.0);
        band.split_v([1.0; 9])
    }

    /// The content pane inset to a comfortable reading column, height clamped to ~9.5 line units (top-aligned) — so every render arm's `body.split_v([1.0; N])` yields unit-scaled line rows on ANY aspect and any zoom. This one clamp is what keeps text spans, pills, and checkboxes in step, because they all derive their size from row height.
    pub fn content_body(&self) -> Region {
        let inset = self.content.inset_xy(0.06, 0.03);
        let body_h = (self.unit * 1.25 * 9.5).min(inset.h);
        Region::new(inset.x, inset.y, inset.w, body_h)
    }

    /// Stack the content body into `N` top-aligned rows of one "line unit" each, returning each row's region. Rows past what fits still tile the body (they compress) — fine for a stub where every control just needs a slot. `n` is passed as a const generic so the caller gets a fixed-size array back.
    pub fn content_rows<const N: usize>(&self) -> [Region; N] {
        self.content_body().split_v([1.0; N])
    }
}
