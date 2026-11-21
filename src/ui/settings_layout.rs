//! Settings-panel layout — the nav-rail-vs-content split and the stacked content rows, expressed with fluor's [`Region`] so everything scales with the viewport.
//! ONE scaling unit: [`SettingsLayout::unit`] is ReadyLayout's formula — `HM(span/32 · ru, height-budget)` — so every settings element (header, rail rows, content rows, and thru them every font, pill, and control) scales with BOTH the window shape and the zoom factor, exactly like the launch/contacts screens. No element sizes off a bare region fraction anymore; that's what made zoom hit-or-miss (text grew, rows and controls didn't).
//! Portrait vs landscape is arithmetic on the same path (header band + rail split fractions), not a layout fork.

use fluor::geom::Viewport;
use fluor::region::Region;
use fluor::Coord;

/// Harmonic mean — C¹-smooth blend of two size candidates (no kink where they cross); zero if either is.
fn hm(a: Coord, b: Coord) -> Coord {
    let sum = a + b;
    if sum <= 0.0 {
        0.0
    } else {
        2.0 * a * b / sum
    }
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
        // Under the status bar (ui::safe_top_px): the header and every band below it shift down; the root keeps its width.
        let top = (crate::ui::safe_top_px() as Coord).min(root.h - 1.0);
        let root = Region::new(root.x, root.y + top, root.w, root.h - top);
        let portrait = root.h > root.w;
        let unit = hm((root.span / 32.0) * vp.ru.max(0.2), root.h / 13.0);
        let header_h = (unit * 2.1).min(root.w * 0.13).min(root.h * 0.15);
        // A safe circle (ferros): the settings circle's grammar instead of the rail. The header is the band under the orb where the page title rides its arc; the body takes the whole width below it, down to where the circle's floor meets its sides (clear of the helm); the rail has no width (the page picker borrows the body when it is up).
        if let Some(c) = crate::ui::safe_circle() {
            let orb = crate::ui::safe_layout::OrbLayout::compute(&c, vp.width_px as Coord, vp.effective_span() / 32.);
            let top = orb.orb_bottom();
            let header = Region::new(root.x, top, root.w, unit * 2.);
            let bottom = root.y + root.h - crate::ui::safe_layout::bar_floor(&c, vp.height_px as Coord, root.w * 0.06, unit);
            let content = Region::new(root.x, header.bottom(), root.w, bottom - header.bottom());
            return Self { header, rail: Region::new(root.x, content.y, 0.0, content.h), content, portrait, unit };
        }
        // The rail|content divider is ALWAYS at 1/3 of the width (split [1.0, 2.0] → rail = 1/3, content = 2/3), portrait and landscape alike — so the background texture origin can sit exactly on it (docs: the bg mirrors left/right around the divider, each half scrolling with its pane).
        if portrait {
            let header = Region::new(root.x + root.w * 0.12, root.y, root.w * 0.88, header_h);
            let below = Region::new(root.x, root.y + header_h, root.w, root.h - header_h);
            let [rail, content] = below.split_h([1.0, 2.0]);
            Self {
                header,
                rail,
                content,
                portrait,
                unit,
            }
        } else {
            let header = Region::new(root.x, root.y, root.w, header_h);
            let below = Region::new(root.x, root.y + header_h, root.w, root.h - header_h);
            let [rail, content] = below.split_h([1.0, 2.0]);
            Self {
                header,
                rail,
                content,
                portrait,
                unit,
            }
        }
    }

    /// Natural nav-rail row height — `unit · 1.5`, NO clamp-to-fit. The rail (Back + 9 pages = 10 rows) overflows at high zoom and scrolls instead of compressing (docs: "no clamps"). The render stacks rows from `rail.y − rail_scroll`.
    pub fn nav_row_h(&self) -> Coord {
        self.unit * 1.5
    }

    /// The inset rail band the nav rows tile (x-inset so the touch band doesn't kiss the pane edge).
    pub fn rail_inset(&self) -> Region {
        // The rail's own padding, or the left cutout / side nav bar where that is wider (Nick 2026-09-28: "it can touch the cam, just not go under it" — the max of the two, never added).
        let r = self.rail.inset_xy(0.06, 0.0);
        let left = (self.rail.x + crate::ui::safe_left_px() as Coord).max(r.x);
        Region::new(left, r.y, (r.right() - left).max(0.0), r.h)
    }

    /// Natural content line height — `unit · 1.25`, NO clamp. Page bodies stack `N` rows of this from `content_inset().y − content_scroll`; tall pages overflow and scroll.
    pub fn content_line_h(&self) -> Coord {
        self.unit * 1.25
    }

    /// The VISIBLE content pane (inset reading column) — the clip rect for the scrolled body, and the height the scroll extent is measured against.
    pub fn content_inset(&self) -> Region {
        // As the rail: the right cutout / side nav bar where it is wider than the pane's own padding, never added to it.
        let r = self.content.inset_xy(0.06, 0.03);
        let right = (self.content.right() - crate::ui::safe_right_px() as Coord).min(r.right());
        Region::new(r.x, r.y, (right - r.x).max(0.0), r.h)
    }

    /// The scrolled, natural-height body for a page of `n` rows: anchored at the content inset, shifted up by `scroll`, `n · content_line_h` tall. `split_v([1.0; n])` on it yields natural-height line rows that scroll (no compression). Clip draws to [`content_inset`].
    pub fn content_scrolled(&self, n: usize, scroll: Coord) -> Region {
        let inset = self.content_inset();
        Region::new(
            inset.x,
            inset.y - scroll,
            inset.w,
            self.content_line_h() * n as Coord,
        )
    }

    /// The same panes with the body (and the rail beside it) moved `dx` pixels across, the header staying: a page beside the open one while the title arc turns (ferros), drawn thru the same rows a page at rest is.
    pub fn shifted(&self, dx: Coord) -> Self {
        let r = self.rail;
        let c = self.content;
        Self { header: self.header, rail: Region::new(r.x + dx, r.y, r.w, r.h), content: Region::new(c.x + dx, c.y, c.w, c.h), portrait: self.portrait, unit: self.unit }
    }

    /// How far below the body's top a list `total` pixels tall starts in a body `h` tall: centred while it fits, at the top once it does not (where it scrolls), joined smoothly.
    /// The centring offset s = (h − total)/2 goes thru (s + √(s² + e²))/2, which is s far above 0, 0 far below it and C∞ between, with e a quarter unit so the join is narrower than a row.
    pub fn list_inset(&self, h: Coord, total: Coord) -> Coord {
        let s = (h - total) * 0.5;
        let e = self.unit * 0.25;
        (s + (s * s + e * e).sqrt()) * 0.5
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The list's start is the centring offset while it fits and the top once it overflows, within a quarter unit, with no step between.
    #[test]
    fn a_list_centres_while_it_fits_and_starts_at_the_top_once_it_does_not() {
        let l = SettingsLayout { header: Region::new(0., 0., 100., 10.), rail: Region::new(0., 10., 0., 100.), content: Region::new(0., 10., 100., 100.), portrait: true, unit: 8. };
        assert!((l.list_inset(1000., 200.) - 400.).abs() < 1.);
        assert!(l.list_inset(200., 1000.) < 1.);
        let mut prev = l.list_inset(400., 0.);
        for k in 1..=800 {
            let v = l.list_inset(400., k as f32);
            assert!(v <= prev && prev - v <= 0.5 + 1e-3, "the start falls no faster than the list grows, by half: {prev} -> {v}");
            prev = v;
        }
    }
}
