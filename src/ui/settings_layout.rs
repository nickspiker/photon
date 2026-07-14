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
        // The rail|content divider is ALWAYS at 1/3 of the width (split [1.0, 2.0] → rail = 1/3, content = 2/3), portrait and landscape alike — so the background texture origin can sit exactly on it (docs: the bg mirrors left/right around the divider, each half scrolling with its pane).
        if portrait {
            let header = Region::new(root.x + root.w * 0.12, root.y, root.w * 0.88, header_h);
            let below = Region::new(root.x, root.y + header_h, root.w, root.h - header_h);
            let [rail, content] = below.split_h([1.0, 2.0]);
            Self { header, rail, content, portrait, unit }
        } else {
            let header = Region::new(root.x, root.y, root.w, header_h);
            let below = Region::new(root.x, root.y + header_h, root.w, root.h - header_h);
            let [rail, content] = below.split_h([1.0, 2.0]);
            Self { header, rail, content, portrait, unit }
        }
    }

    /// Natural nav-rail row height — `unit · 1.5`, NO clamp-to-fit. The rail (Back + 9 pages = 10 rows) overflows at high zoom and scrolls instead of compressing (docs: "no clamps"). The render stacks rows from `rail.y − rail_scroll`.
    pub fn nav_row_h(&self) -> Coord {
        self.unit * 1.5
    }

    /// The inset rail band the nav rows tile (x-inset so the touch band doesn't kiss the pane edge).
    pub fn rail_inset(&self) -> Region {
        self.rail.inset_xy(0.06, 0.0)
    }

    /// Natural content line height — `unit · 1.25`, NO clamp. Page bodies stack `N` rows of this from `content_inset().y − content_scroll`; tall pages overflow and scroll.
    pub fn content_line_h(&self) -> Coord {
        self.unit * 1.25
    }

    /// The VISIBLE content pane (inset reading column) — the clip rect for the scrolled body, and the height the scroll extent is measured against.
    pub fn content_inset(&self) -> Region {
        self.content.inset_xy(0.06, 0.03)
    }

    /// The scrolled, natural-height body for a page of `n` rows: anchored at the content inset, shifted up by `scroll`, `n · content_line_h` tall. `split_v([1.0; n])` on it yields natural-height line rows that scroll (no compression). Clip draws to [`content_inset`].
    pub fn content_scrolled(&self, n: usize, scroll: Coord) -> Region {
        let inset = self.content_inset();
        Region::new(inset.x, inset.y - scroll, inset.w, self.content_line_h() * n as Coord)
    }
}
