//! Photon round the safe circle (ferros: the frame fills the glass, the system's controls float in front of its corners, `ui::SafeCircle`).
//! The orb hangs at the circle's top, centred under the camera; in a conversation Wave and Beam ride the shoulders of an arc through the orb's centre, concentric with the circle.
//! Every size is the span unit (`span · ru / 32`, the chrome's button), softly capped so the orb and its beads stay on the glass from 25% to 400% zoom with no kink where the cap takes over.

use super::SafeCircle;

/// The exponent of the soft cap: 8 (`1 << 3`), sharp enough that sizes well under the cap are themselves, C∞ everywhere.
const SOFT_POWER: i32 = 1 << 3;

/// `z` softly capped at `limit`: `z / (1 + (z / limit)⁸)^(1/8)`, the deck's soft minimum (ferros WINDOWS.md "Deck's turn"). C∞ for positive `z` and `limit`, equal to `z` well below the cap and to `limit` well above it, never a clamp's kink.
pub fn soft_cap(z: f32, limit: f32) -> f32 {
    z / (1. + (z / limit).powi(SOFT_POWER)).powf(1. / SOFT_POWER as f32)
}

/// The orb, its beads and the gaps between them, in units of the span unit: the orb's radius, a bead's radius, the gap from the circle's top to the orb, and the gap from the orb to a bead.
const ORB_R: f32 = 2.;
const BEAD_R: f32 = 3. / 4.;
const TOP_GAP: f32 = 1. / 2.;
const BEAD_GAP: f32 = 3. / 8.;

/// Where the orb and the two shoulder beads sit, in the frame's pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OrbLayout {
    /// The orb: centre and radius.
    pub orb: (f32, f32, f32),
    /// The left and right beads (Wave left, Beam right in a conversation): centre and radius each.
    pub beads: [(f32, f32, f32); 2],
    /// The span unit after its soft cap: the size every other thing round the orb (a label under it, the page titles on its arc) takes.
    pub unit: f32,
}

impl OrbLayout {
    /// The orb at the top of `c`, centred on it, and its beads on the arc through its centre concentric with `c`, for a frame `w` wide and a span unit `unit` (`span · ru / 32`).
    /// The cap: the orb and both beads with their gaps span `2 · (ORB_R + BEAD_GAP + 2 · BEAD_R)` units across; that is held under seven eighths of the width, softly, so the beads never leave the glass at 400%.
    pub fn compute(c: &SafeCircle, w: f32, unit: f32) -> Self {
        let half_units = ORB_R + BEAD_GAP + 2. * BEAD_R;
        let unit = soft_cap(unit, w * (7. / 16.) / half_units);
        let r = unit * ORB_R;
        let cy = c.top() + unit * TOP_GAP + r;
        let bead_r = unit * BEAD_R;
        // The arc through the orb's centre, concentric with the safe circle: its radius is the orb centre's depth below the circle's centre.
        let arc = c.cy - cy;
        // Each bead's centre stands one chord from the orb's: the orb's radius, the gap and its own radius, so the chord's half-angle is asin(chord / 2·arc).
        let chord = r + unit * BEAD_GAP + bead_r;
        let half = (chord / (2. * arc)).asin();
        let (s, k) = (2. * half).sin_cos();
        let bead = |side: f32| (c.cx + side * arc * s, c.cy - arc * k, bead_r);
        OrbLayout { orb: (c.cx, cy, r), beads: [bead(-1.), bead(1.)], unit }
    }

    /// The orb's lowest row, where whatever hangs under it starts.
    pub fn orb_bottom(&self) -> f32 {
        self.orb.1 + self.orb.2
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The Pixel 8 Pro's panel and its span unit at a zoom.
    fn pro(ru: f32) -> (SafeCircle, f32, f32) {
        let (w, h) = (1344., 2992.);
        let span = 2. * w * h / (w + h);
        (SafeCircle::of_panel(1344, 2992), w, span * ru / 32.)
    }

    fn inside(c: &SafeCircle, (x, y, r): (f32, f32, f32)) -> bool {
        ((x - c.cx).powi(2) + (y - c.cy).powi(2)).sqrt() + r <= c.r + 0.01
    }

    /// From 25% to 400% the orb and its beads stay inside the safe circle (so clear of the console's circles) and on the glass, and no bead touches the orb.
    #[test]
    fn orb_and_beads_fit_from_a_quarter_to_four_times() {
        for k in 0..=16 {
            let ru = 0.25 * 2f32.powf(k as f32 / 4.);
            let (c, w, unit) = pro(ru);
            let l = OrbLayout::compute(&c, w, unit);
            assert!(inside(&c, l.orb), "ru {ru}: the orb leaves the circle: {:?}", l.orb);
            assert!((l.orb.0 - c.cx).abs() < 0.01, "ru {ru}: the orb is off centre");
            for b in l.beads {
                assert!(inside(&c, b), "ru {ru}: a bead leaves the circle: {b:?}");
                assert!(b.0 - b.2 > 0. && b.0 + b.2 < w, "ru {ru}: a bead leaves the glass: {b:?}");
                let gap = ((b.0 - l.orb.0).powi(2) + (b.1 - l.orb.1).powi(2)).sqrt() - b.2 - l.orb.2;
                assert!(gap > 0., "ru {ru}: a bead touches the orb");
            }
        }
    }

    /// The camera hole's bottom is the circle's top: the orb starts under it.
    #[test]
    fn the_orb_starts_under_the_camera() {
        let (c, w, unit) = pro(1.);
        let l = OrbLayout::compute(&c, w, unit);
        assert!(l.orb.1 - l.orb.2 > 118.56);
    }

    /// The cap is smooth: sizes grow with the zoom at every step, with no flat run where a clamp would hold them.
    #[test]
    fn the_cap_is_smooth() {
        let mut last = 0.;
        for k in 1..=64 {
            let z = k as f32 * 8.;
            let v = soft_cap(z, 200.);
            assert!(v > last && v < 200.);
            last = v;
        }
    }
}
