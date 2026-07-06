//! Settings-panel layout — the nav-rail-vs-content split and the stacked content rows, expressed with fluor's [`Region`] so everything scales with the viewport.
//! STUB scope: geometry only. The render pass reads these regions to place labels, controls, and hit-stamps; nothing here is behavioural.

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
}

impl SettingsLayout {
    /// Slice the viewport into header / rail / content. The header takes a thin top band; below it the remaining area splits into a narrow rail on the left and the content pane on the right. On a portrait phone the rail stays usably wide by weight; a future pass may collapse it to top tabs on very narrow viewports.
    pub fn compute(vp: &Viewport) -> Self {
        let root = Region::from_viewport(vp);
        let [header, below] = root.split_v([1.6, 12.0]);
        // Rail : content = 1 : 2.2 — enough for the longest label ("Notifications") without crowding the body.
        let [rail, content] = below.split_h([1.0, 2.2]);
        Self {
            header,
            rail,
            content,
        }
    }

    /// The nine equal-height nav-rail rows, top to bottom, in [`crate::ui::state::SettingsPage::ALL`] order. Each row is inset slightly so the clickable band doesn't touch the pane edges.
    pub fn rail_rows(&self) -> [Region; 9] {
        let inset = self.rail.inset_xy(0.06, 0.0);
        inset.split_v([1.0; 9])
    }

    /// The content pane inset to a comfortable reading column with a little top / side margin.
    pub fn content_body(&self) -> Region {
        self.content.inset_xy(0.06, 0.03)
    }

    /// Stack the content body into `N` top-aligned rows of one "line unit" each, returning each row's region. Rows past what fits still tile the body (they compress) — fine for a stub where every control just needs a slot. `n` is passed as a const generic so the caller gets a fixed-size array back.
    pub fn content_rows<const N: usize>(&self) -> [Region; N] {
        self.content_body().split_v([1.0; N])
    }
}
