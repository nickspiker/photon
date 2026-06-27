//! Ready-screen layout: the content block divides into vertical units stacking gap / avatar / gap / handle / gap / hint / gap / textbox / gap / separator / gap, with a `rows` region below for the (not-yet-built) scrollable contact list.
//!
//! Sizing uses a harmonic-mean unit height: `unit_height = HM(span/32 · ru, block.h / total_units)`. The span term keeps the avatar reasonable on a tall narrow window; the height term keeps it from blowing out on a wide short window. Harmonic mean is C¹ at the crossover, no kink as the aspect ratio changes.
//!
//! Avatar, hint (the avatar-update prompt), and search textbox render into their named slots; the handle label is settings-gated (off by default) and the contact rows region awaits the list.

use fluor::canvas::PixelRect;

/// Vertical slices of the Ready user section, in units (summed to derive the unit height).
const V_SLICES: [f32; 11] = [
    1.5,  // gap — top margin above the avatar
    5.,   // avatar
    0.5,  // gap1
    2.,   // handle
    0.,   // gap2
    1.5,  // hint
    0.,   // gap3
    1.5,  // textbox (search)
    0.25, // gap4
    0.5,  // separator
    0.25, // gap5
];

const IDX_AVATAR: usize = 1;
const IDX_HANDLE: usize = 3;
const IDX_HINT: usize = 5;
const IDX_TEXTBOX: usize = 7;
const IDX_SEPARATOR: usize = 9;

pub struct ReadyLayout {
    /// Square region the avatar circle is inscribed in. Width = block width; height = avatar slice height. Circle diameter = the smaller dim (= height in normal aspect ratios).
    pub avatar: PixelRect,
    /// Handle text slot (optional, settings-gated — off by default for security).
    pub handle: PixelRect,
    /// Hint text slot — the avatar update prompt (drag/drop on desktop, tap-to-pick on Android).
    pub hint: PixelRect,
    /// Search/add textbox slot.
    pub textbox: PixelRect,
    /// Thin horizontal separator between user section and contact rows. Half block width, centred.
    pub separator: PixelRect,
    /// Remaining vertical space below the user section — where the scrollable contact rows render.
    pub rows: PixelRect,
    /// Height of one contact row in pixels (avatar + handle text). 1.5× the layout unit for readability.
    pub row_height: usize,
    /// Diameter of a contact-row avatar circle (half the row height).
    pub contact_avatar_diameter: usize,
    /// The base layout unit: `HM(span/32 · ru, block_h / total_units)` — span-based, aspect-ratio-robust, zoom-aware, no hardcoded pixels. Other screens (e.g. Conversation) scale off this so they match the contacts screen's feel.
    pub unit_height: f32,
}

impl ReadyLayout {
    /// Compute the layout from viewport dimensions + ru zoom factor. `span` (harmonic mean of width and height) is computed internally — Photon's universal scaling unit. Block is the full viewport; chrome composites on top and the gap at the slice stack's head keeps the avatar visually clear of the title bar.
    pub fn compute(buf_w: usize, buf_h: usize, ru: f32) -> Self {
        // Horizontal: 1/8 margin | 6/8 content | 1/8 margin (matches launch layout for visual continuity).
        let content_x = buf_w >> 3;
        let content_w = buf_w - 2 * content_x;

        let block_y = 0;
        let block_h = buf_h;

        let perimeter = (buf_w + buf_h) as f32;
        let span = if perimeter > 0. {
            2. * buf_w as f32 * buf_h as f32 / perimeter
        } else {
            0.
        };

        let total_units: f32 = V_SLICES.iter().sum();
        // Two constraints on unit_height: a span-driven term so the avatar stays reasonable on tall/narrow windows, and a height-driven term so it doesn't overflow on short/wide ones. Harmonic mean blends smoothly at the crossover.
        let unit_from_span = (span / 32.) * ru;
        let unit_from_height = block_h as f32 / total_units;
        let unit_height = if unit_from_span + unit_from_height > 0. {
            2. * unit_from_span * unit_from_height / (unit_from_span + unit_from_height)
        } else {
            0.
        };

        // Cumulative slice y-positions in pixels: accumulate the float total, truncate once per boundary so slices stay tight (no per-slice rounding drift).
        let mut v = [0_usize; 12];
        let mut cum = 0.;
        for (i, s) in V_SLICES.iter().enumerate() {
            v[i] = (cum * unit_height) as usize;
            cum += s;
        }
        v[11] = (cum * unit_height) as usize;

        let user_section_h = v[11];
        let rows_y = block_y + user_section_h;

        let block_x1 = content_x + content_w;
        let slot = |i: usize| -> PixelRect {
            PixelRect::new(content_x, block_y + v[i], block_x1, block_y + v[i + 1])
        };

        // Separator is half-width, centred.
        let sep_w = content_w / 2;
        let sep_x = content_x + (content_w - sep_w) / 2;
        let separator = PixelRect::new(
            sep_x,
            block_y + v[IDX_SEPARATOR],
            sep_x + sep_w,
            block_y + v[IDX_SEPARATOR + 1],
        );

        // Contact rows: 1.5× the unit for readability; the row avatar is half the row height.
        let row_height = (unit_height * 1.5) as usize;
        let contact_avatar_diameter = row_height / 2;

        ReadyLayout {
            avatar: slot(IDX_AVATAR),
            handle: slot(IDX_HANDLE),
            hint: slot(IDX_HINT),
            textbox: slot(IDX_TEXTBOX),
            separator,
            rows: PixelRect::new(content_x, rows_y, block_x1, buf_h),
            row_height,
            contact_avatar_diameter,
            unit_height,
        }
    }

    /// Avatar circle center + radius, derived from the avatar slot. Circle is inscribed in the smaller dimension of the slot (= height for normal aspect ratios).
    pub fn avatar_center_radius(&self) -> (f32, f32, f32) {
        let w = (self.avatar.x1 - self.avatar.x0) as f32;
        let h = (self.avatar.y1 - self.avatar.y0) as f32;
        let radius = w.min(h) * 0.5;
        let cx = (self.avatar.x0 + self.avatar.x1) as f32 * 0.5;
        let cy = (self.avatar.y0 + self.avatar.y1) as f32 * 0.5;
        (cx, cy, radius)
    }
}
