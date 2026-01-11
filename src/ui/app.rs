use super::renderer::Renderer;
use super::text_rasterizing::TextRenderer;
use super::theme;
#[cfg(not(target_os = "android"))]
use super::PhotonEvent;
use crate::crypto::clutch::ClutchAllKeypairs;

/// Asymptotic clamping: approaches max but never exceeds it.
/// soft_limit(x, max) = max * x / (x + max)
/// At x=0: returns 0. At x=max: returns max/2. As x→∞: approaches max.
pub fn soft_limit(x: f32, max: f32) -> f32 {
    max * x / (x + max)
}
use crate::network::status::AckRequest;
use crate::network::StatusChecker;
use crate::network::{HandleQuery, QueryResult};
use crate::types::{ChatMessage, Contact, ContactId, FriendshipChains, FriendshipId, HandleText};

/// Result from background CLUTCH keypair generation
pub struct ClutchKeygenResult {
    pub contact_id: ContactId,
    pub keypairs: ClutchAllKeypairs,
    // NOTE: ceremony_id is now computed on-demand from handle_hashes + offer_provenances
    // after we receive enough offers (2 for 2-party DM). No longer computed in background.
}

/// Result from background CLUTCH KEM encapsulation
pub struct ClutchKemEncapResult {
    pub contact_id: ContactId,
    pub kem_response: crate::crypto::clutch::ClutchKemResponsePayload,
    pub local_secrets: crate::crypto::clutch::ClutchKemSharedSecrets,
    pub ceremony_id: [u8; 32],
    pub conversation_token: [u8; 32],
    pub peer_addr: std::net::SocketAddr,
}

/// Result from background CLUTCH ceremony completion (avalanche_expand)
pub struct ClutchCeremonyResult {
    pub contact_id: ContactId,
    pub friendship_chains: FriendshipChains,
    pub eggs_proof: [u8; 32],
    pub their_handle_hash: [u8; 32],
    pub ceremony_id: [u8; 32],
    pub conversation_token: [u8; 32],
    pub peer_addr: std::net::SocketAddr,
    pub their_hqc_prefix: [u8; 8],
}

#[cfg(not(target_os = "android"))]
use winit::{
    dpi::PhysicalSize, event_loop::EventLoopProxy, keyboard::ModifiersState, window::Window,
};

/// Cross-platform keyboard modifier state
#[cfg(target_os = "android")]
#[derive(Clone, Copy, Default)]
pub struct ModifiersState {
    pub shift: bool,
    pub ctrl: bool,
    pub alt: bool,
}

#[cfg(target_os = "android")]
impl ModifiersState {
    pub fn empty() -> Self {
        Self::default()
    }
    pub fn shift_key(&self) -> bool {
        self.shift
    }
    pub fn control_key(&self) -> bool {
        self.ctrl
    }
    pub fn alt_key(&self) -> bool {
        self.alt
    }
}

impl TextState {
    pub fn new() -> Self {
        Self {
            chars: Vec::new(),
            widths: Vec::new(),
            width: 0,
            blinkey_index: 0,
            scroll_offset: 0.0,
            selection_anchor: None,
            textbox_focused: false,
            is_empty: true,
        }
    }

    /// Insert a character with its width at the given index
    pub fn insert(&mut self, index: usize, ch: char, width: usize) {
        self.chars.insert(index, ch);
        self.widths.insert(index, width);
        self.width += width;
    }

    /// Insert a string at the given index (widths must be provided)
    pub fn insert_str(&mut self, index: usize, s: &str, widths: &[usize]) {
        assert_eq!(
            s.chars().count(),
            widths.len(),
            "Widths must match character count"
        );

        let new_chars: Vec<char> = s.chars().collect();
        for (i, (ch, &width)) in new_chars.iter().zip(widths.iter()).enumerate() {
            self.chars.insert(index + i, *ch);
            self.widths.insert(index + i, width);
            self.width += width;
        }
    }

    /// Remove character at index and return it
    pub fn remove(&mut self, index: usize) -> char {
        let removed_width = self.widths.remove(index);
        self.width -= removed_width;
        self.chars.remove(index)
    }

    /// Delete a range of characters
    pub fn delete_range(&mut self, range: std::ops::Range<usize>) {
        let removed_width: usize = self.widths[range.clone()].iter().sum();
        self.chars.drain(range.clone());
        self.widths.drain(range);
        self.width -= removed_width;
    }
}

/// Text layout geometry - THE SINGLE SOURCE OF TRUTH for all textbox rendering.
/// Used by both compositing (draw_textbox) and text input (blinkey position).
/// Designed to be extensible for future multi-line text entry.
#[derive(Clone, Copy)]
pub struct TextLayout {
    // === Drawing coordinates (for compositing) ===
    /// Center X of textbox (for draw_textbox)
    pub center_x: usize,
    /// Center Y of textbox (for draw_textbox)
    pub center_y: usize,
    /// Full width of textbox (for draw_textbox)
    pub box_width: usize,
    /// Full height of textbox (for draw_textbox)
    pub box_height: usize,

    // === Text area (excludes button) ===
    /// Left edge of usable text area (pixels from window left)
    pub usable_left: usize,
    /// Right edge of usable text area (pixels from window left)
    pub usable_right: usize,
    /// Center of usable text area (pixels from window left)
    pub usable_center: usize,
    /// Width of usable text area
    pub usable_width: usize,
    /// Margin from usable edges (for blinkey limits)
    pub margin: usize,

    // === Text rendering ===
    /// Font size for text rendering
    pub font_size: f32,
    /// Line height (for future multi-line support)
    pub line_height: usize,
    /// Button area width (0 when button is below textbox)
    pub button_area: usize,
}

impl TextLayout {
    /// Create layout geometry from app dimensions and state
    /// ru = dimensionless zoom multiplier (1.0 = default)
    pub fn new(width: usize, height: usize, span: usize, ru: f32, app_state: &AppState) -> Self {
        // Get region from Layout (single source of truth)
        let layout = Layout::new(width, height, span, ru, app_state);

        // Textbox sizing depends on state:
        // - Launch: from AttestBlockLayout slice heights
        // - Ready/Searching: from ContactsHeaderLayout slice heights (already scaled with ru)
        // - Conversation: from layout.textbox with span*ru sizing
        let (
            textbox_left,
            textbox_right,
            textbox_width,
            textbox_y,
            box_height,
            font_size,
            button_area,
        ) = if matches!(app_state, AppState::Launch(_)) {
            // Launch: sizes from AttestBlockLayout slice heights
            let block = layout.attest_block.as_ref().unwrap();
            let sub = AttestBlockLayout::new(block);
            let tb_left = sub.textbox.x;
            let tb_right = sub.textbox.x + sub.textbox.w;
            let tb_width = sub.textbox.w;
            let tb_y = sub.textbox.y + sub.textbox.h / 2;
            let box_h = sub.textbox.h;
            let font_sz = box_h as f32 / 2.0;
            (tb_left, tb_right, tb_width, tb_y, box_h, font_sz, 0)
        } else if let Some(contacts) = layout.contacts.as_ref() {
            // Ready/Searching: from ContactsUnifiedLayout (all scaled with span-based row_height)
            let sub = ContactsUnifiedLayout::new(contacts, span, ru, 0);
            let tb_left = sub.textbox.x;
            let tb_right = sub.textbox.x + sub.textbox.w;
            let tb_width = sub.textbox.w;
            let tb_y = sub.textbox.y + sub.textbox.h / 2; // center of textbox slice
            let box_h = sub.textbox.h;
            // Font size from textbox height (like attest), not from row_height
            let font_sz = box_h as f32 / 2.0;
            // Button area same as conversation - keeps text from overlapping search button
            let button_size = box_h * 7 / 8;
            let inset = box_h / 16;
            let button_area = button_size + inset * 2;
            (
                tb_left,
                tb_right,
                tb_width,
                tb_y,
                box_h,
                font_sz,
                button_area,
            )
        } else {
            // Conversation: sizes from span*ru
            let tb = layout.textbox.as_ref().unwrap();
            let tb_left = tb.x;
            let tb_right = tb.x + tb.w;
            let tb_width = tb.w;
            let tb_y = tb.y + tb.h / 2;
            let box_h = (span as f32 / 8.0 * ru) as usize;
            let font_sz = span as f32 / 16.0 * ru;
            let button_size = box_h * 7 / 8;
            let inset = box_h / 16;
            let button_area = button_size + inset * 2;
            (
                tb_left,
                tb_right,
                tb_width,
                tb_y,
                box_h,
                font_sz,
                button_area,
            )
        };
        let line_height = box_height;

        // Drawing coordinates
        let center_x = textbox_left + textbox_width / 2;
        let center_y = textbox_y; // textbox_y is already center

        // Usable text area (excludes button)
        let usable_width = textbox_width - button_area;
        let usable_left = textbox_left;
        let usable_right = textbox_right - button_area;
        let usable_center = usable_left + usable_width / 2;
        let margin = usable_width / 40;

        Self {
            // Drawing coordinates
            center_x,
            center_y,
            box_width: textbox_width,
            box_height,
            // Text area
            usable_left,
            usable_right,
            usable_center,
            usable_width,
            margin,
            // Text rendering
            font_size,
            line_height,
            button_area,
        }
    }

    /// Calculate blinkey x position from text state
    pub fn blinkey_x(&self, text: &TextState) -> usize {
        // When text is empty, center in full textbox (no button shown)
        if text.chars.is_empty() {
            return self.usable_left + (self.usable_width + self.button_area) / 2;
        }
        if text.blinkey_index == 0 {
            return (self.usable_center as f32 - (text.width / 2) as f32 + text.scroll_offset)
                as usize;
        }
        let blinkey_offset: usize = text.widths[..text.blinkey_index].iter().sum();
        let text_half = text.width / 2;
        (self.usable_center as f32 - text_half as f32 + text.scroll_offset + blinkey_offset as f32)
            as usize
    }

    /// Calculate blinkey y position (top of cursor)
    pub fn blinkey_y(&self) -> usize {
        (self.center_y as f32 - self.box_height as f32 * 0.25) as usize
    }

    /// Calculate text start x from text state
    pub fn text_start_x(&self, text: &TextState) -> f32 {
        let text_half = (text.width / 2) as f32;
        self.usable_center as f32 - text_half + text.scroll_offset
    }
}

/// A rectangular region in pixel coordinates.
/// Used for layout bounds - regions don't scale with ru, content within does.
#[derive(Clone, Copy, Debug)]
pub struct PixelRegion {
    pub x: usize,
    pub y: usize,
    pub w: usize,
    pub h: usize,
}

impl PixelRegion {
    pub fn new(x: usize, y: usize, w: usize, h: usize) -> Self {
        Self { x, y, w, h }
    }

    /// Create from signed values - allows negative positions for off-screen content
    pub fn from_signed(x: isize, y: isize, w: isize, h: isize) -> Self {
        Self {
            x: x as usize,
            y: y as usize,
            w: w as usize,
            h: h as usize,
        }
    }

    #[inline]
    pub fn contains(&self, px: usize, py: usize) -> bool {
        px >= self.x && px < self.x + self.w && py >= self.y && py < self.y + self.h
    }

    #[inline]
    pub fn right(&self) -> usize {
        self.x + self.w
    }

    #[inline]
    pub fn bottom(&self) -> usize {
        self.y + self.h
    }

    pub fn center(&self) -> (usize, usize) {
        (self.x + self.w / 2, self.y + self.h / 2)
    }
}

/// UI layout regions - fixed containers that don't scale with ru.
/// Content within each region scales with ru, but the region bounds are fixed.
/// Note: Window controls are NOT included - they're window chrome, not content regions.
pub struct Layout {
    /// Logo and spectrum animation area - launch screens only
    pub logo_spectrum: Option<PixelRegion>,
    /// "Photon" text area - launch screens only
    pub photon_text: Option<PixelRegion>,
    /// Text input box - present in non-launch states (conversation only)
    pub textbox: Option<PixelRegion>,
    /// Unified block containing hint + textbox + attest button - launch screens only
    /// Subdivided via AttestBlockLayout
    pub attest_block: Option<PixelRegion>,
    /// Unified contacts block (Ready/Searching states) - contains user section + contact rows.
    /// Subdivided via ContactsUnifiedLayout. All elements scale with span-based row_height.
    pub contacts: Option<PixelRegion>,
    /// Header with back arrow and contact info - conversation state
    pub header: Option<PixelRegion>,
    /// Message display area - conversation state
    pub message_area: Option<PixelRegion>,
}

/// Subdivision of attest_block into textbox, hint label, and attest button regions.
/// Uses proportional slicing so you can fiddle with gaps and proportions.
#[derive(Clone, Copy)]
pub struct AttestBlockLayout {
    /// Error message region (top)
    pub error: PixelRegion,
    /// Handle textbox region
    pub textbox: PixelRegion,
    /// "handle" hint label region (middle)
    pub hint: PixelRegion,
    /// Attest button region (bottom)
    pub attest: PixelRegion,
}

impl AttestBlockLayout {
    /// Subdivide an attest_block region into error, textbox, hint, and attest sub-regions.
    ///
    /// The block is already scaled with ru by Layout::new().
    /// This just slices it proportionally - no additional scaling needed.
    ///
    /// Textbox and error are full block width; hint and attest are centered.
    pub fn new(block: &PixelRegion) -> Self {
        // Slice the block proportionally
        const V_SLICES: [Slice; 7] = [
            Slice::new("error", 1.5),
            Slice::new("gap0", 0.5),
            Slice::new("textbox", 2.0),
            Slice::new("gap1", 0.25),
            Slice::new("hint", 1.8),
            Slice::new("gap2", 0.5),
            Slice::new("attest", 2.7),
        ];
        let v = slice_positions(block.h, &V_SLICES);

        // Hint and attest are centered horizontally (narrower than full width)
        // Use 75% of block width for these elements
        let narrow_w = block.w * 3 / 4;
        let narrow_x = block.x + (block.w - narrow_w) / 2;

        Self {
            // Error: full block width
            error: PixelRegion::new(block.x, block.y + v[0], block.w, v[1] - v[0]),
            // Textbox: full block width
            textbox: PixelRegion::new(block.x, block.y + v[2], block.w, v[3] - v[2]),
            // Hint and attest: centered, narrower
            hint: PixelRegion::new(narrow_x, block.y + v[4], narrow_w, v[5] - v[4]),
            attest: PixelRegion::new(narrow_x, block.y + v[6], narrow_w, v[7] - v[6]),
        }
    }
}

/// Subdivision of contacts_header block into avatar, handle, textbox, separator.
/// This is the top scaled block on the contacts screen (Ready/Searching states).
/// The whole block scales with ru as a unit.
#[derive(Clone, Copy)]
pub struct ContactsHeaderLayout {
    /// Avatar circle region (square, centered)
    pub avatar: PixelRegion,
    /// Handle text region (below avatar)
    pub handle: PixelRegion,
    /// Avatar hint text region (below handle, for drag-drop hint)
    pub hint: PixelRegion,
    /// Search textbox region
    pub textbox: PixelRegion,
    /// Separator line region (thin hairline)
    pub separator: PixelRegion,
}

impl ContactsHeaderLayout {
    /// Subdivide contacts_header block into avatar, handle, hint, textbox, separator.
    /// Block is already scaled with ru by Layout::new().
    pub fn new(block: &PixelRegion) -> Self {
        const V_SLICES: [Slice; 11] = [
            Slice::new("gap0", 1.),
            Slice::new("avatar", 4.),
            Slice::new("gap1", 0.25),
            Slice::new("handle", 1.0),
            Slice::new("gap2", 0.25),
            Slice::new("hint", 1.),
            Slice::new("gap3", 0.25),
            Slice::new("textbox", 1.5),
            Slice::new("gap4", 0.25),
            Slice::new("separator", 0.5),
            Slice::new("gap5", 0.25),
        ];
        let v = slice_positions(block.h, &V_SLICES);

        // Avatar is square, centered horizontally
        let avatar_h = v[2] - v[1];
        let avatar_w = avatar_h.min(block.w);
        let avatar_x = block.x + (block.w - avatar_w) / 2;

        // Separator is centered horizontally, narrower than full width
        let sep_w = block.w / 2;
        let sep_x = block.x + (block.w - sep_w) / 2;

        Self {
            avatar: PixelRegion::new(avatar_x, block.y + v[1], avatar_w, avatar_h),
            handle: PixelRegion::new(block.x, block.y + v[3], block.w, v[4] - v[3]),
            hint: PixelRegion::new(block.x, block.y + v[5], block.w, v[6] - v[5]),
            textbox: PixelRegion::new(block.x, block.y + v[7], block.w, v[8] - v[7]),
            separator: PixelRegion::new(sep_x, block.y + v[9], sep_w, v[10] - v[9]),
        }
    }

    /// Get avatar center and radius from the avatar region.
    pub fn avatar_center_radius(&self) -> (usize, usize, usize) {
        // Harmonic mean avoids discontinuity when w ≈ h
        let radius = self.avatar.w * self.avatar.h / (self.avatar.w + self.avatar.h);
        let cx = self.avatar.x + self.avatar.w / 2;
        let cy = self.avatar.y + self.avatar.h / 2;
        (cx, cy, radius)
    }
}

/// Subdivision of contacts_rows block into scrollable contact rows.
/// Used on the contacts screen (Ready/Searching states).
/// Note: Header elements (avatar, handle, textbox, separator) are in ContactsHeaderLayout.
#[derive(Clone, Copy)]
pub struct ContactsRowsLayout {
    /// Scrollable contact rows region (the whole block)
    pub rows: PixelRegion,
    /// Row height for contact entries
    pub row_height: usize,
    /// Avatar diameter for contact entries (derived from row_height)
    pub avatar_diameter: usize,
    /// Text left offset (after avatar + spacing)
    pub text_left_offset: usize,
    /// Text size derived from row height
    pub text_size: f32,
    /// X offset for centering based on longest contact (0 = left-aligned)
    pub center_offset: usize,
}

impl ContactsRowsLayout {
    /// Subdivide contacts_rows block into contact rows.
    /// span and ru are used to determine row height, then all other sizes derive from row_height.
    /// center_offset allows centering the contact list based on longest name width.
    pub fn new(block: &PixelRegion, span: usize, ru: f32, center_offset: usize) -> Self {
        // Row height is the primary unit - everything derives from it
        let row_height = ((span * 1 / 16) as f32 * ru) as usize;

        let avatar_diameter = row_height / 2;
        let avatar_spacing = avatar_diameter / 2;

        let text_size = row_height as f32 / 2.;

        Self {
            rows: *block,
            row_height,
            avatar_diameter,
            text_left_offset: avatar_diameter + avatar_spacing,
            text_size,
            center_offset,
        }
    }

    /// Get the region for a specific contact row by index.
    /// Returns None if the row would be outside the rows region.
    pub fn row_region(&self, index: usize) -> Option<PixelRegion> {
        let row_y = self.rows.y + index * self.row_height;
        if row_y + self.row_height > self.rows.bottom() {
            None
        } else {
            Some(PixelRegion::new(
                self.rows.x,
                row_y,
                self.rows.w,
                self.row_height,
            ))
        }
    }

    /// Get avatar center position for a row.
    pub fn row_avatar_center(&self, index: usize) -> Option<(usize, usize)> {
        let row_y = self.rows.y + index * self.row_height;
        if row_y + self.row_height > self.rows.bottom() {
            None
        } else {
            let cx = self.rows.x + self.center_offset + self.avatar_diameter / 2;
            let cy = row_y + self.row_height / 2;
            Some((cx, cy))
        }
    }

    /// Get text position for a row.
    pub fn row_text_position(&self, index: usize) -> Option<(f32, f32)> {
        let row_y = self.rows.y + index * self.row_height;
        if row_y + self.row_height > self.rows.bottom() {
            None
        } else {
            let x = (self.rows.x + self.center_offset + self.text_left_offset) as f32;
            let y = (row_y + self.row_height / 2) as f32;
            Some((x, y))
        }
    }

    /// Get the number of visible rows that fit in the rows region.
    pub fn visible_row_count(&self) -> usize {
        if self.row_height == 0 {
            0
        } else {
            self.rows.h / self.row_height
        }
    }
}

/// Unified layout for contacts screen (Ready/Searching states).
/// Combines user section (avatar, handle, hint, textbox, separator) and contact rows
/// into a single layout where everything scales with the same unit_height base.
#[derive(Clone, Copy)]
pub struct ContactsUnifiedLayout {
    // User section (at top)
    /// User avatar region (5 units, centered horizontally)
    pub user_avatar: PixelRegion,
    /// Handle text region (2 units)
    pub handle: PixelRegion,
    /// Hint text region (1.5 units)
    pub hint: PixelRegion,
    /// Search textbox region (1.5 units)
    pub textbox: PixelRegion,
    /// Separator line region (0.5 units, centered)
    pub separator: PixelRegion,

    // Contact rows section (below user section)
    /// Scrollable contact rows region
    pub rows: PixelRegion,

    // Sizing
    /// Base unit height from span - user section elements are multiples of this
    pub unit_height: usize,
    /// Contact row height (1.5x unit_height for better readability)
    pub row_height: usize,
    /// Avatar diameter for contact rows
    pub avatar_diameter: usize,
    /// Text left offset after avatar + spacing
    pub text_left_offset: usize,
    /// Text size for contact names
    pub text_size: f32,
    /// X offset for centering based on longest contact
    pub center_offset: usize,

    // User avatar sizing
    /// User avatar diameter (much larger than contact avatars)
    pub user_avatar_diameter: usize,
}

impl ContactsUnifiedLayout {
    /// Create unified layout from a block region.
    /// span and ru determine unit_height (base scaling unit), then all sizes derive from it.
    /// Slices define how many units each element occupies (e.g., user_avatar = 5 units).
    pub fn new(block: &PixelRegion, span: usize, ru: f32, center_offset: usize) -> Self {
        // User section uses proportional slices
        const V_SLICES: [Slice; 11] = [
            Slice::new("gap0", 1.),
            Slice::new("avatar", 5.),
            Slice::new("gap1", 0.5),
            Slice::new("handle", 2.),
            Slice::new("gap2", 0.0),
            Slice::new("hint", 1.5),
            Slice::new("gap3", 0.0),
            Slice::new("textbox", 1.5),
            Slice::new("gap4", 0.25),
            Slice::new("separator", 0.5),
            Slice::new("gap5", 0.25),
        ];
        let total_units: f64 = V_SLICES.iter().map(|s| s.units).sum();

        // Two constraints for unit_height:
        // 1. span-based: span/32 * ru (halved to match other UI elements)
        // 2. height-based: block.h / total_units (fits user section in window)
        // Harmonic mean 2ab/(a+b) smoothly blends both constraints
        let unit_from_span = (span / 32) as f32 * ru;
        let unit_from_height = block.h as f32 / total_units as f32;
        let unit_height = (2.0 * unit_from_span * unit_from_height
            / (unit_from_span + unit_from_height)) as usize;

        // Contact row sizing: 1.5x unit_height for better readability
        let row_height = unit_height * 3 / 2;
        let avatar_diameter = row_height / 2;
        let avatar_spacing = avatar_diameter / 2;
        let text_size = row_height as f32 / 2.;

        // User section height = sum of slice units × unit_height
        let user_section_h = (total_units * unit_height as f64) as usize;

        // Slice within user section
        let v = slice_positions(user_section_h, &V_SLICES);

        // Avatar region full width; circle uses height for diameter
        let avatar_h = v[2] - v[1];
        let user_avatar_region = PixelRegion::new(block.x, block.y + v[1], block.w, avatar_h);
        let user_avatar_diameter = avatar_h;

        let handle_region = PixelRegion::new(block.x, block.y + v[3], block.w, v[4] - v[3]);
        let hint_region = PixelRegion::new(block.x, block.y + v[5], block.w, v[6] - v[5]);
        let textbox_region = PixelRegion::new(block.x, block.y + v[7], block.w, v[8] - v[7]);

        let sep_w = block.w / 2;
        let separator_region = PixelRegion::new(
            block.x + (block.w - sep_w) / 2, // Centered
            block.y + v[9],
            sep_w,
            v[10] - v[9],
        );

        // Remaining space is for contact rows
        let rows_y = block.y + user_section_h;
        let rows_h = if block.bottom() > rows_y {
            block.bottom() - rows_y
        } else {
            0
        };
        let rows_region = PixelRegion::new(block.x, rows_y, block.w, rows_h);

        Self {
            user_avatar: user_avatar_region,
            handle: handle_region,
            hint: hint_region,
            textbox: textbox_region,
            separator: separator_region,
            rows: rows_region,
            unit_height,
            row_height,
            avatar_diameter,
            text_left_offset: avatar_diameter + avatar_spacing,
            text_size,
            center_offset,
            user_avatar_diameter,
        }
    }

    /// Get user avatar center and radius (isize for scroll support).
    pub fn user_avatar_center_radius(&self) -> (isize, isize, isize) {
        // Circle uses height for diameter, centered in region
        let radius = (self.user_avatar.h / 2) as isize;
        let cx = self.user_avatar.x as isize + (self.user_avatar.w / 2) as isize;
        let cy = self.user_avatar.y as isize + (self.user_avatar.h / 2) as isize;
        (cx, cy, radius)
    }

    /// Get avatar center position for a contact row (isize for scroll support).
    /// Returns positions unconditionally - visibility determined at render time with scroll offset.
    pub fn row_avatar_center(&self, index: usize) -> (isize, isize) {
        let row_y = self.rows.y as isize + index as isize * self.row_height as isize;
        let cx = self.rows.x as isize + self.center_offset as isize + (self.avatar_diameter / 2) as isize;
        let cy = row_y + (self.row_height / 2) as isize;
        (cx, cy)
    }

    /// Get text position for a contact row (isize for scroll support).
    /// Returns positions unconditionally - visibility determined at render time with scroll offset.
    pub fn row_text_position(&self, index: usize) -> (isize, isize) {
        let row_y = self.rows.y as isize + index as isize * self.row_height as isize;
        let x = self.rows.x as isize + self.center_offset as isize + self.text_left_offset as isize;
        let y = row_y + (self.row_height / 2) as isize;
        (x, y)
    }

    /// Get handle text center position (isize for scroll support).
    pub fn handle_center(&self) -> (isize, isize) {
        let cx = self.handle.x as isize + (self.handle.w / 2) as isize;
        let cy = self.handle.y as isize + (self.handle.h / 2) as isize;
        (cx, cy)
    }

    /// Get hint text center position (isize for scroll support).
    pub fn hint_center(&self) -> (isize, isize) {
        let cx = self.hint.x as isize + (self.hint.w / 2) as isize;
        let cy = self.hint.y as isize + (self.hint.h / 2) as isize;
        (cx, cy)
    }

    /// Get textbox center position (isize for scroll support).
    pub fn textbox_center(&self) -> (isize, isize) {
        let cx = self.textbox.x as isize + (self.textbox.w / 2) as isize;
        let cy = self.textbox.y as isize + (self.textbox.h / 2) as isize;
        (cx, cy)
    }

    /// Get separator Y position (isize for scroll support).
    pub fn separator_y(&self) -> isize {
        self.separator.y as isize + (self.separator.h / 2) as isize
    }

    /// Get the number of visible contact rows.
    pub fn visible_row_count(&self) -> usize {
        if self.row_height == 0 {
            0
        } else {
            self.rows.h / self.row_height
        }
    }
}

/// A named slice in a proportional layout grid.
/// Each slice has a name (for documentation) and a unit size.
struct Slice {
    #[allow(dead_code)]
    name: &'static str,
    units: f64,
}

impl Slice {
    const fn new(name: &'static str, units: f64) -> Self {
        Self { name, units }
    }
}

/// Compute pixel positions from named slices.
/// Returns a Vec where positions[i] is the start pixel of slice i,
/// and positions[len] is the end pixel of the last slice.
fn slice_positions(total_pixels: usize, slices: &[Slice]) -> Vec<usize> {
    let total_units: f64 = slices.iter().map(|s| s.units).sum();
    let mut positions = Vec::with_capacity(slices.len() + 1);
    let mut cumulative = 0.0;
    positions.push(0);
    for slice in slices {
        cumulative += slice.units;
        positions.push((total_pixels as f64 * cumulative / total_units) as usize);
    }
    positions
}

impl Layout {
    /// Create layout from window dimensions and app state.
    /// Uses proportional slicing: window divided into fixed unit bands vertically and horizontally.
    /// Each element gets a rectangle from the intersection of bands.
    /// attest_block scales with ru; other elements are window-proportional.
    pub fn new(width: usize, height: usize, _span: usize, ru: f32, app_state: &AppState) -> Self {
        // Common horizontal slicing: margin | content | margin
        const H_SLICES: [Slice; 3] = [
            Slice::new("margin_left", 1.),
            Slice::new("content", 6.),
            Slice::new("margin_right", 1.),
        ];
        let h = slice_positions(width, &H_SLICES);
        let content_x = h[1];
        let content_w = h[2] - h[1];

        match app_state {
            AppState::Launch(_) => {
                // Vertical layout for launch screen
                const V_SLICES: [Slice; 9] = [
                    Slice::new("gap0", 0.75),
                    Slice::new("spectrum", 6.),
                    Slice::new("gap1", -2.),
                    Slice::new("photon_text", 3.5),
                    Slice::new("gap2", 1.5),
                    Slice::new("attest_block", 5.),
                    Slice::new("gap4", 6.),
                    Slice::new("version", 1.),
                    Slice::new("gap5", 1.),
                ];
                let v = slice_positions(height, &V_SLICES);

                // Named indices for clarity
                const SPECTRUM: usize = 1;
                const PHOTON_TEXT: usize = 3;
                const ATTEST_BLOCK: usize = 5;

                // attest_block: height scales with ru, width stays at content_w
                // Use signed math to handle ru > 1.0 (content extends beyond base region)
                let base_block_y = v[ATTEST_BLOCK] as isize;
                let base_block_h = (v[ATTEST_BLOCK + 1] - v[ATTEST_BLOCK]) as isize;
                let scaled_block_h = (base_block_h as f32 * ru) as isize;
                let block_center_y = base_block_y + base_block_h / 2;
                let scaled_block_y = block_center_y - scaled_block_h / 2;

                Self {
                    // Spectrum uses full window width (no margins)
                    logo_spectrum: Some(PixelRegion::new(
                        0,
                        v[SPECTRUM],
                        width,
                        v[SPECTRUM + 1] - v[SPECTRUM],
                    )),
                    photon_text: Some(PixelRegion::new(
                        content_x,
                        v[PHOTON_TEXT],
                        content_w,
                        v[PHOTON_TEXT + 1] - v[PHOTON_TEXT],
                    )),
                    textbox: None, // Launch uses attest_block instead
                    attest_block: Some(PixelRegion::from_signed(
                        content_x as isize,
                        scaled_block_y,
                        content_w as isize,
                        scaled_block_h,
                    )),
                    contacts: None,
                    header: None,
                    message_area: None,
                }
            }
            AppState::Ready | AppState::Searching => {
                // Single unified block from top of screen
                // ContactsUnifiedLayout subdivides into user section + contact rows
                // Everything scales with span-based row_height
                Self {
                    logo_spectrum: None,
                    photon_text: None,
                    textbox: None,
                    attest_block: None,
                    contacts: Some(PixelRegion::new(content_x, 0, content_w, height)),
                    header: None,
                    message_area: None,
                }
            }
            AppState::Conversation | AppState::Connected { .. } => {
                const V_SLICES: [Slice; 4] = [
                    Slice::new("header", 2.0),
                    Slice::new("messages", 12.0),
                    Slice::new("textbox", 1.5),
                    Slice::new("bottom_gap", 0.5),
                ];
                let v = slice_positions(height, &V_SLICES);

                const HEADER: usize = 0;
                const MESSAGES: usize = 1;
                const TEXTBOX: usize = 2;

                Self {
                    logo_spectrum: None,
                    photon_text: None,
                    textbox: Some(PixelRegion::new(
                        content_x,
                        v[TEXTBOX],
                        content_w,
                        v[TEXTBOX + 1] - v[TEXTBOX],
                    )),
                    attest_block: None,
                    contacts: None,
                    header: Some(PixelRegion::new(
                        0,
                        v[HEADER],
                        width,
                        v[HEADER + 1] - v[HEADER],
                    )),
                    message_area: Some(PixelRegion::new(
                        content_x,
                        v[MESSAGES],
                        content_w,
                        v[MESSAGES + 1] - v[MESSAGES],
                    )),
                }
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AppState {
    /// Launch screen states (before main messenger UI)
    Launch(LaunchState),

    /// Main messenger - ready to search peers and chat
    Ready,

    /// Searching for a peer handle (computing handle_proof in background)
    Searching,

    /// Viewing conversation with a contact (contact index stored separately)
    Conversation,

    /// Active P2P conversation (legacy - may remove)
    Connected { peer_handle: String },
}

/// Sub-states for the launch screen
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LaunchState {
    /// Ready to attest - show handle input + "Attest" button
    Fresh,

    /// Computing handle_proof + announcing to FGTW
    /// Show loading spinner, no button
    Attesting,

    /// Attestation failed - show error message, no button
    /// User can edit textbox to return to Fresh
    Error(String),
}

impl LaunchState {
    /// Check if we're in a state where the user can type in the handle textbox
    pub fn can_edit_handle(&self) -> bool {
        !matches!(self, LaunchState::Attesting)
    }

    /// Check if we're waiting for a network response
    pub fn is_loading(&self) -> bool {
        matches!(self, LaunchState::Attesting)
    }
}

/// Result of searching for a handle
#[derive(Debug, Clone)]
pub struct FoundPeer {
    pub handle: HandleText,
    pub handle_proof: [u8; 32], // Cached handle_proof (expensive - ~1 second to compute)
    pub device_pubkey: crate::types::DevicePubkey,
    pub ip: std::net::SocketAddr,
}

#[derive(Debug, Clone)]
pub enum SearchResult {
    Found(FoundPeer),
    NotFound,
    Error(String),
}

impl Default for AppState {
    fn default() -> Self {
        AppState::Launch(LaunchState::Fresh)
    }
}

/// ENTIRE text state, selection, all that (excluding blinkey)
#[derive(Clone)]
pub struct TextState {
    pub chars: Vec<char>,
    pub widths: Vec<usize>,
    pub width: usize,
    pub scroll_offset: f32,
    pub blinkey_index: usize,
    pub selection_anchor: Option<usize>,
    pub textbox_focused: bool,
    pub is_empty: bool, // True if chars is empty (cached for previous frame comparison)
}

pub struct PhotonApp {
    pub renderer: Renderer,
    pub text_renderer: TextRenderer,
    pub width: u32,
    pub height: u32,
    pub window_dirty: bool,
    pub selection_dirty: bool,
    pub text_dirty: bool,
    pub controls_dirty: bool,

    // Universal scaling units (cached for performance)
    /// Span: harmonic mean of width and height = 2wh/(w+h)
    /// Smooth at w==h (no discontinuity), biased toward smaller dimension,
    /// finite slope at axes, slope exactly 1 along diagonal. Base unit for all UI scaling.
    pub span: usize,
    pub perimeter: usize,   // width + height
    pub diagonal_sq: usize, // width² + height²

    /// RU: user's zoom multiplier for content scaling. Starts at 1.0.
    /// Content sizes multiply by effective_ru(): font_size = span/16 * ru, box_height = span/8 * ru
    pub ru: f32,

    /// Keyboard scale compensation (Android only). When keyboard shrinks window,
    /// this compensates to keep UI elements the same visual size.
    /// effective_ru() = ru * keyboard_scale
    pub keyboard_scale: f32,

    /// Initial height at app creation (Android only). Used to compute keyboard_scale.
    #[cfg(target_os = "android")]
    pub initial_height: u32,

    /// Text layout geometry - single source of truth for text/blinkey positioning
    pub text_layout: TextLayout,

    /// UI region layout - fixed containers that don't scale with ru
    pub layout: Layout,

    // Launch screen state
    pub blinkey_blink_rate_ms: u64, // System blinkey blink rate in milliseconds (max for random range)
    pub blinkey_wave_top_bright: bool, // True=top is bright, False=top is dark
    pub blinkey_visible: bool,      // Whether blinkey is currently visible (for blinking)
    pub is_mouse_selecting: bool,   // True when actively dragging mouse to select text
    pub blinkey_pixel_x: usize,     // Cursor x position in pixels
    pub blinkey_pixel_y: usize,     // Cursor y position in pixels
    pub next_blinkey_blink_time: std::time::Instant, // When next blinkey blink should happen
    pub app_state: AppState,        // Application lifecycle state
    pub query_start_time: Option<std::time::Instant>, // When handle query started (for 1s simulation)
    pub handle_query: Option<HandleQuery>,            // Network query system for handle attestation
    pub fgtw_online: bool,                            // True if FGTW server is reachable
    pub prev_fgtw_online: bool,                       // Previous state for differential rendering
    pub hint_was_shown: bool, // Track if network hint was shown (for cleanup)
    pub search_result: Option<SearchResult>, // Result of handle search
    pub search_receiver: Option<std::sync::mpsc::Receiver<SearchResult>>, // Async search result
    pub searching_handle: Option<String>, // Handle being searched (for display)
    pub glow_colour: u32,     // Current textbox glow colour (0x00RRGGBB)
    pub spectrum_phase: f32,  // Rainbow sine wave phase (radians), animates during query
    pub speckle_counter: f32, // Background speckle animation counter, animates during query
    pub hourglass_angle: f32, // Hourglass rotation (degrees), stochastic wobble during search
    pub last_frame_time: std::time::Instant, // Last frame timestamp for delta time calculation
    pub fps: f32,             // Current frames per second
    pub frame_times: Vec<f32>, // Recent frame delta times for FPS averaging
    pub target_frame_duration_ms: u64, // Target frame duration based on monitor refresh rate
    pub next_animation_frame: std::time::Instant, // When next animation frame should be drawn

    // Zoom hint overlay (differential rendering)
    pub zoom_hint_visible: bool, // True when zoom hint is currently drawn
    pub zoom_hint_hide_time: Option<std::time::Instant>, // When to hide the zoom hint
    pub zoom_hint_ru: f32,       // The ru value currently displayed in the hint

    // Text state for differential rendering
    pub current_text_state: TextState,
    pub previous_text_state: TextState,

    pub textbox_mask: Vec<u8>, // Single-channel alpha mask for textbox (0=outside, 255=inside, faded at edges)
    pub show_textbox_mask: bool, // Debug: show textbox mask visualization (Ctrl+T)
    pub frame_counter: usize,  // Every render() call (from RedrawRequested)
    pub update_counter: usize, // Any actual drawing (partial or full)
    pub redraw_counter: usize, // Complete scene redraws only

    // Input state
    pub mouse_x: f32,
    pub mouse_y: f32,
    pub mouse_button_pressed: bool, // True when left mouse button is held down
    pub is_dragging_resize: bool,
    pub is_dragging_move: bool,
    pub resize_edge: ResizeEdge,
    pub drag_start_blinkey_screen_pos: (f64, f64), // Global screen position when drag starts
    pub drag_start_size: (u32, u32),
    pub drag_start_window_pos: (i32, i32),
    pub modifiers: ModifiersState,
    pub is_fullscreen: bool, // True when window is fullscreen

    // Window control buttons
    pub hovered_button: HoveredButton,
    pub prev_hovered_button: HoveredButton, // Previous hover state to detect changes

    // Mouse selection state
    pub selection_last_update_time: Option<std::time::Instant>, // Last time selection scroll was updated

    // Hit test bitmap (one byte per pixel, element ID)
    pub hit_test_map: Vec<u8>,
    pub debug_hit_test: bool,
    pub debug_hit_colours: Vec<(u8, u8, u8)>, // Random colours for each hit area ID

    // Runtime debug flag (toggleable with Ctrl+D)
    pub debug: bool,

    // Contacts list (handles we've searched and found)
    pub contacts: Vec<Contact>,
    // Scroll offset for contacts list (pixels, negative = scrolled up)
    pub contacts_scroll_offset: isize,
    // Shared pubkey list for StatusChecker (synced with contacts)
    pub contact_pubkeys: crate::network::status::ContactPubkeys,
    // Shared sync records for pong responses (last_received_ef6 per conversation)
    pub sync_records_provider: crate::network::status::SyncRecordsProvider,
    // Currently hovered contact index (None if not hovering any)
    pub hovered_contact: Option<usize>,
    pub prev_hovered_contact: Option<usize>,
    // Contact row text size (derived from row height, cached for differential updates)
    pub contact_text_size: f32,
    // Selected contact for conversation view (None = main view)
    pub selected_contact: Option<usize>,

    // P2P status checker for contact online status
    pub status_checker: Option<StatusChecker>,
    pub next_status_ping: std::time::Instant, // When to ping contacts next
    pub our_public_ip: Option<std::net::IpAddr>, // Our public IP from FGTW (for same-NAT detection)

    // Periodic FGTW refresh
    pub next_fgtw_refresh: std::time::Instant, // When to re-announce to FGTW
    pub attesting_handle: Option<String>,      // Handle being attested (for storing handle_proof)

    // User avatar and identity
    pub avatar_pixels: Option<Vec<u8>>, // Decoded VSF RGB pixels (AVATAR_SIZE x AVATAR_SIZE x 3)
    pub avatar_scaled: Option<Vec<u8>>, // Mitchell-resampled avatar at current display size
    pub avatar_scaled_diameter: usize,  // Diameter the scaled avatar was rendered at
    pub user_handle: Option<String>,    // User's attested handle
    pub user_handle_proof: Option<[u8; 32]>, // Our handle_proof (for CLUTCH initiator check)
    pub user_identity_seed: Option<[u8; 32]>, // BLAKE3(handle) for storage encryption key derivation
    pub show_avatar_hint: bool,               // Show "drag and drop" hint after clicking avatar
    pub file_hovering_avatar: bool,           // Track if file is being dragged over avatar

    // Contact avatar fetching (background thread)
    pub contact_avatar_rx: std::sync::mpsc::Receiver<crate::avatar::AvatarDownloadResult>,
    pub contact_avatar_tx: std::sync::mpsc::Sender<crate::avatar::AvatarDownloadResult>,

    // Background CLUTCH keypair generation (McEliece is slow, do it off main thread)
    pub clutch_keygen_rx: std::sync::mpsc::Receiver<ClutchKeygenResult>,
    pub clutch_keygen_tx: std::sync::mpsc::Sender<ClutchKeygenResult>,

    // Background CLUTCH KEM encapsulation (slow PQ ops, do off main thread)
    pub clutch_kem_encap_rx: std::sync::mpsc::Receiver<ClutchKemEncapResult>,
    pub clutch_kem_encap_tx: std::sync::mpsc::Sender<ClutchKemEncapResult>,

    // Background CLUTCH ceremony completion (avalanche_expand is slow)
    pub clutch_ceremony_rx: std::sync::mpsc::Receiver<ClutchCeremonyResult>,
    pub clutch_ceremony_tx: std::sync::mpsc::Sender<ClutchCeremonyResult>,

    // Device keypair for signing (needed by StatusChecker)
    pub device_keypair: crate::network::fgtw::Keypair,

    // Friendship chains (runtime-only, loaded from disk)
    pub friendship_chains: Vec<(FriendshipId, FriendshipChains)>,

    // Event loop proxy for waking event loop on network updates (desktop only)
    #[cfg(not(target_os = "android"))]
    pub event_proxy: EventLoopProxy<PhotonEvent>,

    // WebSocket client for real-time peer IP updates (desktop only)
    #[cfg(not(target_os = "android"))]
    pub peer_update_client: Option<crate::network::PeerUpdateClient>,
}

// Hit test element IDs
pub const HIT_NONE: u8 = 0;
pub const HIT_MINIMIZE_BUTTON: u8 = 1;
pub const HIT_MAXIMIZE_BUTTON: u8 = 2;
pub const HIT_CLOSE_BUTTON: u8 = 3;
pub const HIT_HANDLE_TEXTBOX: u8 = 4;
pub const HIT_PRIMARY_BUTTON: u8 = 5; // "Attest" button
pub const HIT_BACK_HEADER: u8 = 6; // Conversation header back button
pub const HIT_AVATAR: u8 = 7; // User's avatar circle (Ready screen)
pub const HIT_CONTACT_BASE: u8 = 64; // Contact 0 = 64, Contact 1 = 65, etc. (up to 192 contacts)

// Button hover colour deltas are now in theme module

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum HoveredButton {
    None,
    Close,
    Maximize,
    Minimize,
    Textbox,
    QueryButton,
    BackHeader,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ResizeEdge {
    None,
    Top,
    Bottom,
    Left,
    Right,
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

impl PhotonApp {
    /// Effective zoom multiplier for sizing calculations.
    /// Combines user's zoom (ru) with keyboard scale compensation.
    #[inline]
    pub fn effective_ru(&self) -> f32 {
        self.ru * self.keyboard_scale
    }

    /// Desktop constructor (Linux + Windows)
    #[cfg(not(target_os = "android"))]
    pub fn new(
        window: &Window,
        blinkey_blink_rate_ms: u64,
        target_frame_duration_ms: u64,
        event_proxy: EventLoopProxy<PhotonEvent>,
    ) -> Self {
        let size = window.inner_size();
        let renderer = Renderer::new(window, size.width, size.height);
        let text_renderer = TextRenderer::new();

        // Check initial fullscreen/maximized state
        let is_fullscreen = window.fullscreen().is_some() || window.is_maximized();

        let w = size.width as usize;
        let h = size.height as usize;

        // Avatar is loaded after attestation when we have a handle
        // (the storage key is derived from handle)
        let avatar_pixels: Option<Vec<u8>> = None;

        // Create channel for background avatar downloads
        let (contact_avatar_tx, contact_avatar_rx) = std::sync::mpsc::channel();

        // Create channel for background CLUTCH keypair generation
        let (clutch_keygen_tx, clutch_keygen_rx) = std::sync::mpsc::channel();

        // Create channels for background CLUTCH KEM encapsulation and ceremony completion
        let (clutch_kem_encap_tx, clutch_kem_encap_rx) = std::sync::mpsc::channel();
        let (clutch_ceremony_tx, clutch_ceremony_rx) = std::sync::mpsc::channel();

        let mut app = Self {
            renderer,
            text_renderer,
            width: size.width,
            height: size.height,
            window_dirty: true,
            selection_dirty: false,
            text_dirty: false,
            controls_dirty: false,
            span: 2 * w * h / (w + h), // Harmonic mean - smooth at w==h, biased toward smaller
            perimeter: w + h,
            diagonal_sq: w * w + h * h,
            ru: 1.0,             // User's zoom multiplier, starts at 1.0
            keyboard_scale: 1.0, // Keyboard compensation (Android only), starts at 1.0
            text_layout: TextLayout::new(
                w,
                h,
                2 * w * h / (w + h),
                1.0,
                &AppState::Launch(LaunchState::Fresh),
            ),
            layout: Layout::new(
                w,
                h,
                2 * w * h / (w + h),
                1.0,
                &AppState::Launch(LaunchState::Fresh),
            ),
            blinkey_blink_rate_ms,
            blinkey_visible: false,
            is_mouse_selecting: false,
            blinkey_wave_top_bright: false,
            blinkey_pixel_x: 0,
            blinkey_pixel_y: 0,
            next_blinkey_blink_time: std::time::Instant::now(),
            app_state: AppState::Launch(LaunchState::Fresh),
            query_start_time: None,
            handle_query: None, // Initialized below after device_keypair
            device_keypair: {
                // Derive deterministically from machine-id - NEVER stored to disk
                use crate::network::fgtw::{derive_device_keypair, get_machine_fingerprint};
                let fingerprint = get_machine_fingerprint()
                    .expect("Failed to get machine fingerprint for key derivation");
                let keypair = derive_device_keypair(&fingerprint);
                crate::log(&format!(
                    "Device pubkey: {}",
                    hex::encode(keypair.public.as_bytes())
                ));
                keypair
            },
            fgtw_online: false, // Updated by connectivity check
            prev_fgtw_online: false,
            hint_was_shown: false,
            search_result: None,
            search_receiver: None,
            searching_handle: None,
            glow_colour: theme::GLOW_DEFAULT, // White glow by default
            spectrum_phase: 0.0,
            speckle_counter: 0.0,
            hourglass_angle: 0.0,
            last_frame_time: std::time::Instant::now(),
            fps: 0.0,
            frame_times: Vec::with_capacity(60),
            target_frame_duration_ms,
            next_animation_frame: std::time::Instant::now(),
            zoom_hint_visible: false,
            zoom_hint_hide_time: None,
            zoom_hint_ru: 1.0,
            current_text_state: TextState::new(),
            previous_text_state: TextState::new(),
            textbox_mask: vec![0; (size.width * size.height) as usize],
            show_textbox_mask: false,
            frame_counter: 0,
            update_counter: 0,
            redraw_counter: 0,
            mouse_x: 0.,
            mouse_y: 0.,
            mouse_button_pressed: false,
            is_dragging_resize: false,
            is_dragging_move: false,
            resize_edge: ResizeEdge::None,
            drag_start_blinkey_screen_pos: (0., 0.),
            drag_start_size: (0, 0),
            drag_start_window_pos: (0, 0),
            modifiers: ModifiersState::empty(),
            hovered_button: HoveredButton::None,
            prev_hovered_button: HoveredButton::None,
            selection_last_update_time: None,
            hit_test_map: vec![0; (size.width * size.height) as usize],
            debug_hit_test: false,
            debug_hit_colours: Vec::new(),
            debug: false,
            is_fullscreen,
            contacts: Vec::new(),
            contacts_scroll_offset: 0,
            contact_pubkeys: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
            sync_records_provider: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
            hovered_contact: None,
            prev_hovered_contact: None,
            contact_text_size: 16.0, // Will be set during first render
            selected_contact: None,
            status_checker: None, // Initialized AFTER attestation succeeds
            next_status_ping: std::time::Instant::now(),
            our_public_ip: None,
            next_fgtw_refresh: std::time::Instant::now() + std::time::Duration::from_secs(60),
            attesting_handle: None,
            avatar_pixels,
            avatar_scaled: None,
            avatar_scaled_diameter: 0,
            user_handle: None,
            user_handle_proof: None,
            user_identity_seed: None,
            show_avatar_hint: false,
            file_hovering_avatar: false,
            contact_avatar_rx,
            contact_avatar_tx,
            clutch_keygen_rx,
            clutch_keygen_tx,
            clutch_kem_encap_rx,
            clutch_kem_encap_tx,
            clutch_ceremony_rx,
            clutch_ceremony_tx,
            friendship_chains: Vec::new(),
            event_proxy: event_proxy.clone(),
            peer_update_client: None, // Started after attestation
        };

        // Initialize handle_query with the derived keypair
        {
            use crate::network::fgtw::PeerStore;
            use crate::network::HandleQuery;

            let handle_query = HandleQuery::new(app.device_keypair.clone(), event_proxy.clone());
            let peer_store = std::sync::Arc::new(std::sync::Mutex::new(PeerStore::new()));
            handle_query.set_transport(peer_store);

            // Start StatusChecker early so PT receiver is ready before attestation
            // This allows us to receive ClutchOffers from peers who come online before us
            use crate::network::status::StatusChecker;
            app.status_checker = StatusChecker::new(
                handle_query.socket().clone(),
                app.device_keypair.clone(),
                app.contact_pubkeys.clone(),
                app.sync_records_provider.clone(),
                event_proxy.clone(),
            )
            .ok();
            if app.status_checker.is_some() {
                crate::log("UI: Status checker started early (PT receiver ready)");
            }

            app.handle_query = Some(handle_query);
        }

        app
    }

    /// Android constructor - takes device keypair derived from JNI fingerprint
    #[cfg(target_os = "android")]
    pub fn new(width: u32, height: u32, device_keypair: crate::network::fgtw::Keypair) -> Self {
        let renderer = Renderer::new(width, height);
        let text_renderer = TextRenderer::new();

        let w = width as usize;
        let h = height as usize;

        // Create channel for background avatar downloads
        let (contact_avatar_tx, contact_avatar_rx) = std::sync::mpsc::channel();

        // Create channel for background CLUTCH keypair generation
        let (clutch_keygen_tx, clutch_keygen_rx) = std::sync::mpsc::channel();

        // Create channels for background CLUTCH KEM encapsulation and ceremony completion
        let (clutch_kem_encap_tx, clutch_kem_encap_rx) = std::sync::mpsc::channel();
        let (clutch_ceremony_tx, clutch_ceremony_rx) = std::sync::mpsc::channel();

        Self {
            renderer,
            text_renderer,
            width,
            height,
            window_dirty: true,
            selection_dirty: false,
            text_dirty: false,
            controls_dirty: false,
            span: 2 * w * h / (w + h), // Harmonic mean - smooth at w==h, biased toward smaller
            perimeter: w + h,
            diagonal_sq: w * w + h * h,
            ru: 1.0,                // User's zoom multiplier, starts at 1.0
            keyboard_scale: 1.0,    // Keyboard compensation, starts at 1.0
            initial_height: height, // Store initial height for keyboard_scale calculation
            text_layout: TextLayout::new(
                w,
                h,
                2 * w * h / (w + h),
                1.0,
                &AppState::Launch(LaunchState::Fresh),
            ),
            layout: Layout::new(
                w,
                h,
                2 * w * h / (w + h),
                1.0,
                &AppState::Launch(LaunchState::Fresh),
            ),
            blinkey_blink_rate_ms: 500,
            blinkey_visible: false,
            is_mouse_selecting: false,
            blinkey_wave_top_bright: false,
            blinkey_pixel_x: 0,
            blinkey_pixel_y: 0,
            next_blinkey_blink_time: std::time::Instant::now(),
            app_state: AppState::Launch(LaunchState::Fresh),
            query_start_time: None,
            handle_query: None, // Initialized via set_handle_query() after this
            device_keypair,
            fgtw_online: false,
            prev_fgtw_online: false,
            hint_was_shown: false,
            search_result: None,
            search_receiver: None,
            searching_handle: None,
            glow_colour: theme::GLOW_DEFAULT,
            spectrum_phase: 0.0,
            speckle_counter: 0.0,
            hourglass_angle: 0.0,
            last_frame_time: std::time::Instant::now(),
            fps: 0.0,
            frame_times: Vec::with_capacity(60),
            target_frame_duration_ms: 16, // ~60fps
            next_animation_frame: std::time::Instant::now(),
            current_text_state: TextState::new(),
            previous_text_state: TextState::new(),
            textbox_mask: vec![0; (width * height) as usize],
            show_textbox_mask: false,
            frame_counter: 0,
            update_counter: 0,
            redraw_counter: 0,
            mouse_x: 0.,
            mouse_y: 0.,
            mouse_button_pressed: false,
            is_dragging_resize: false,
            is_dragging_move: false,
            resize_edge: ResizeEdge::None,
            drag_start_blinkey_screen_pos: (0., 0.),
            drag_start_size: (0, 0),
            drag_start_window_pos: (0, 0),
            modifiers: ModifiersState::empty(),
            hovered_button: HoveredButton::None,
            prev_hovered_button: HoveredButton::None,
            selection_last_update_time: None,
            hit_test_map: vec![0; (width * height) as usize],
            debug_hit_test: false,
            debug_hit_colours: Vec::new(),
            debug: false,
            is_fullscreen: true, // Android is always fullscreen
            contacts: Vec::new(),
            contacts_scroll_offset: 0,
            contact_pubkeys: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
            sync_records_provider: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
            hovered_contact: None,
            prev_hovered_contact: None,
            contact_text_size: 16.0, // Will be set during first render
            selected_contact: None,
            status_checker: None,
            next_status_ping: std::time::Instant::now(),
            our_public_ip: None,
            next_fgtw_refresh: std::time::Instant::now() + std::time::Duration::from_secs(60),
            attesting_handle: None,
            avatar_pixels: None,
            avatar_scaled: None,
            avatar_scaled_diameter: 0,
            user_handle: None,
            user_handle_proof: None,
            user_identity_seed: None,
            show_avatar_hint: false,
            file_hovering_avatar: false,
            contact_avatar_rx,
            contact_avatar_tx,
            clutch_keygen_rx,
            clutch_keygen_tx,
            clutch_kem_encap_rx,
            clutch_kem_encap_tx,
            clutch_ceremony_rx,
            clutch_ceremony_tx,
            friendship_chains: Vec::new(),
            zoom_hint_visible: false,
            zoom_hint_hide_time: None,
            zoom_hint_ru: 1.0,
        }
    }

    /// Update the fullscreen/maximized state
    /// When true, window edges are not drawn
    pub fn set_fullscreen(&mut self, is_fullscreen: bool) {
        if self.is_fullscreen != is_fullscreen {
            self.is_fullscreen = is_fullscreen;
        }
    }

    /// Update sync_records_provider from friendship_chains.
    /// Called when chains change to keep pong responses up-to-date.
    pub fn update_sync_records(&mut self) {
        use crate::network::fgtw::protocol::SyncRecord;

        let mut records = Vec::new();
        for (_fid, chains) in &self.friendship_chains {
            // Get the max last_received_time across all participants
            // This is when we last received ANY message in this conversation
            let max_time = chains
                .last_received_times()
                .iter()
                .filter_map(|t| *t)
                .fold(None, |acc: Option<f64>, t| {
                    Some(acc.map_or(t, |a| if t > a { t } else { a }))
                });

            if let Some(last_received_ef6) = max_time {
                records.push(SyncRecord {
                    conversation_token: chains.conversation_token,
                    last_received_ef6,
                });
            }
        }

        // Update the shared provider
        let mut provider = self.sync_records_provider.lock().unwrap();
        *provider = records;
    }

    /// Reset textbox state when changing screens
    /// Clears text content, hides blinkey, unfocuses textbox
    pub fn reset_textbox(&mut self) {
        self.current_text_state.chars.clear();
        self.current_text_state.widths.clear();
        self.current_text_state.width = 0;
        self.current_text_state.blinkey_index = 0;
        self.current_text_state.selection_anchor = None;
        self.current_text_state.scroll_offset = 0.0;
        self.current_text_state.is_empty = true;
        self.current_text_state.textbox_focused = false;
        self.blinkey_visible = false;
        self.text_dirty = true;
        self.selection_dirty = true;
    }

    /// Handle touch events on Android
    /// action: 0=DOWN, 1=UP, 2=MOVE, 3=CANCEL
    /// Returns: 1=show keyboard, -1=hide keyboard, 0=no change
    #[cfg(target_os = "android")]
    pub fn handle_touch(&mut self, action: i32, x: f32, y: f32) -> i32 {
        self.mouse_x = x;
        self.mouse_y = y;

        let keyboard_action = match action {
            0 => {
                // DOWN
                self.mouse_button_pressed = true;
                self.handle_touch_down();
                0
            }
            1 | 3 => {
                // UP or CANCEL
                self.mouse_button_pressed = false;
                self.handle_touch_up()
            }
            2 => {
                // MOVE
                if self.mouse_button_pressed {
                    self.handle_touch_move();
                }
                0
            }
            _ => 0,
        };
        self.window_dirty = true;
        keyboard_action
    }

    #[cfg(target_os = "android")]
    fn handle_touch_down(&mut self) {
        // Use hit_test_map to determine what was touched
        let hit_idx = (self.mouse_y as usize) * (self.width as usize) + (self.mouse_x as usize);
        if hit_idx >= self.hit_test_map.len() {
            return;
        }

        let element = self.hit_test_map[hit_idx];

        // Set hover state based on touched element (brightens button)
        self.prev_hovered_button = self.hovered_button;
        self.hovered_button = match element {
            HIT_PRIMARY_BUTTON => HoveredButton::QueryButton,
            HIT_HANDLE_TEXTBOX => {
                if !self.current_text_state.textbox_focused {
                    HoveredButton::Textbox
                } else {
                    HoveredButton::None
                }
            }
            HIT_BACK_HEADER => HoveredButton::BackHeader,
            _ => HoveredButton::None,
        };

        // Track hovered contact
        if element >= HIT_CONTACT_BASE {
            self.prev_hovered_contact = self.hovered_contact;
            self.hovered_contact = Some((element - HIT_CONTACT_BASE) as usize);
        } else {
            self.prev_hovered_contact = self.hovered_contact;
            self.hovered_contact = None;
        }

        if self.hovered_button != self.prev_hovered_button
            || self.hovered_contact != self.prev_hovered_contact
        {
            self.controls_dirty = true;
        }
    }

    #[cfg(target_os = "android")]
    fn handle_touch_move(&mut self) {
        // Already cancelled - stay cancelled until touch up
        if self.hovered_button == HoveredButton::None && self.hovered_contact.is_none() {
            return;
        }

        let hit_idx = (self.mouse_y as usize) * (self.width as usize) + (self.mouse_x as usize);
        if hit_idx >= self.hit_test_map.len() {
            // Off screen - cancel
            self.prev_hovered_button = self.hovered_button;
            self.hovered_button = HoveredButton::None;
            self.prev_hovered_contact = self.hovered_contact;
            self.hovered_contact = None;
            self.controls_dirty = true;
            return;
        }

        let element = self.hit_test_map[hit_idx];

        // Check if still on the SAME element we started on
        let still_on_button = match self.hovered_button {
            HoveredButton::QueryButton => element == HIT_PRIMARY_BUTTON,
            HoveredButton::Textbox => element == HIT_HANDLE_TEXTBOX,
            HoveredButton::BackHeader => element == HIT_BACK_HEADER,
            _ => true, // None stays None
        };

        let still_on_contact = match self.hovered_contact {
            Some(idx) => element == HIT_CONTACT_BASE + idx as u8,
            None => true, // None stays None
        };

        // Dragged off - cancel permanently
        if !still_on_button || !still_on_contact {
            self.prev_hovered_button = self.hovered_button;
            self.hovered_button = HoveredButton::None;
            self.prev_hovered_contact = self.hovered_contact;
            self.hovered_contact = None;
            self.controls_dirty = true;
        }
    }

    /// Returns: 1=show keyboard, -1=hide keyboard, 0=no change
    #[cfg(target_os = "android")]
    fn handle_touch_up(&mut self) -> i32 {
        let mut keyboard_action = 0;

        // Check what element we're over
        let hit_idx = (self.mouse_y as usize) * (self.width as usize) + (self.mouse_x as usize);
        let element = if hit_idx < self.hit_test_map.len() {
            self.hit_test_map[hit_idx]
        } else {
            HIT_NONE
        };

        // Special case: tap on textbox when already focused = position cursor
        // But not during attestation - handle is locked in
        if element == HIT_HANDLE_TEXTBOX
            && self.current_text_state.textbox_focused
            && !matches!(self.app_state, AppState::Launch(LaunchState::Attesting))
        {
            self.current_text_state.blinkey_index = self.blinkey_index_from_x(self.mouse_x);
            self.current_text_state.selection_anchor = None;
            self.text_dirty = true;

            // Clear hover state
            self.prev_hovered_button = self.hovered_button;
            self.hovered_button = HoveredButton::None;
            self.controls_dirty = true;

            // On Android, always request keyboard - user may have dismissed it
            #[cfg(target_os = "android")]
            return 1;
            #[cfg(not(target_os = "android"))]
            return 0;
        }

        // Only execute action if we're still in hover state (didn't drag off)
        match self.hovered_button {
            HoveredButton::Textbox => {
                // Don't allow textbox focus during attestation - handle is locked in
                if matches!(self.app_state, AppState::Launch(LaunchState::Attesting)) {
                    // Ignore textbox taps while attesting
                } else {
                    // Focus textbox and show keyboard
                    // Always return 1 even if already focused - keyboard may have been dismissed
                    self.current_text_state.textbox_focused = true;
                    self.blinkey_visible = true;
                    self.text_dirty = true;
                    #[cfg(target_os = "android")]
                    {
                        keyboard_action = 1; // Always request keyboard on Android
                    }
                }
            }
            HoveredButton::QueryButton => {
                // Execute primary button action
                match &self.app_state {
                    AppState::Launch(LaunchState::Fresh) => {
                        self.start_attestation();
                        // Hide keyboard - user is done entering handle
                        self.current_text_state.textbox_focused = false;
                        self.blinkey_visible = false;
                        keyboard_action = -1;
                    }
                    AppState::Ready => {
                        let handle: String = self.current_text_state.chars.iter().collect();
                        if !handle.is_empty() {
                            self.start_handle_search(&handle);
                        }
                    }
                    _ => {}
                }
            }
            HoveredButton::BackHeader => {
                // Go back to contacts list
                self.app_state = AppState::Ready;
                let eff_ru = self.effective_ru();
                self.text_layout = TextLayout::new(
                    self.width as usize,
                    self.height as usize,
                    self.span,
                    eff_ru,
                    &self.app_state,
                );
                self.layout = Layout::new(
                    self.width as usize,
                    self.height as usize,
                    self.span,
                    eff_ru,
                    &self.app_state,
                );
                self.selected_contact = None;
                self.reset_textbox();
            }
            HoveredButton::None => {
                // Check if we're on avatar (doesn't use hover state)
                if element == HIT_AVATAR {
                    if matches!(self.app_state, AppState::Ready | AppState::Searching) {
                        #[cfg(target_os = "android")]
                        {
                            // Return 2 to signal "open image picker" to Android
                            keyboard_action = 2;
                        }
                        #[cfg(not(target_os = "android"))]
                        {
                            self.show_avatar_hint = true;
                        }
                    }
                } else if self.hovered_contact.is_none() {
                    // Tapped outside interactive elements - unfocus textbox and hide keyboard
                    if self.current_text_state.textbox_focused {
                        self.current_text_state.textbox_focused = false;
                        self.blinkey_visible = false;
                        self.text_dirty = true;
                        keyboard_action = -1;
                    }
                    // Hide avatar hint
                    if self.show_avatar_hint {
                        self.show_avatar_hint = false;
                    }
                }
            }
            // Window controls - not used on Android
            HoveredButton::Close | HoveredButton::Maximize | HoveredButton::Minimize => {}
        }

        // Handle contact tap if we're still hovering on one
        if let Some(contact_idx) = self.hovered_contact {
            if contact_idx < self.contacts.len() {
                self.selected_contact = Some(contact_idx);
                self.app_state = AppState::Conversation;
                let eff_ru = self.effective_ru();
                self.text_layout = TextLayout::new(
                    self.width as usize,
                    self.height as usize,
                    self.span,
                    eff_ru,
                    &self.app_state,
                );
                self.layout = Layout::new(
                    self.width as usize,
                    self.height as usize,
                    self.span,
                    eff_ru,
                    &self.app_state,
                );
                self.reset_textbox();
            }
        }

        // Clear hover state on touch up
        self.prev_hovered_button = self.hovered_button;
        self.hovered_button = HoveredButton::None;
        self.prev_hovered_contact = self.hovered_contact;
        self.hovered_contact = None;
        self.controls_dirty = true;

        keyboard_action
    }

    /// Handle text input from Android soft keyboard
    #[cfg(target_os = "android")]
    pub fn handle_text_input(&mut self, text: &str) {
        if !self.current_text_state.textbox_focused {
            return;
        }

        // Delete selection first if it exists
        if self.current_text_state.selection_anchor.is_some() {
            self.delete_selection();
        }

        let font_size = self.font_size();
        for ch in text.chars() {
            // Measure character width
            let width = self.text_renderer.measure_text_width(
                &ch.to_string(),
                font_size,
                theme::FONT_WEIGHT_USER_CONTENT,
                theme::FONT_USER_CONTENT,
            ) as usize;

            // Insert character with its width
            let blinkey_idx = self.current_text_state.blinkey_index;
            self.current_text_state.insert(blinkey_idx, ch, width);
            self.current_text_state.blinkey_index += 1;
        }

        // Update state
        if matches!(self.app_state, AppState::Launch(_)) {
            self.set_launch_state(LaunchState::Fresh);
        }
        self.text_dirty = true;
        self.glow_colour = theme::GLOW_DEFAULT;
        self.search_result = None;
        self.controls_dirty = true;
    }

    /// Handle backspace key from Android
    #[cfg(target_os = "android")]
    pub fn handle_backspace(&mut self) -> bool {
        if !self.current_text_state.textbox_focused {
            return false;
        }

        if self.current_text_state.selection_anchor.is_some() {
            self.delete_selection();
        } else if self.current_text_state.blinkey_index > 0 {
            let idx = self.current_text_state.blinkey_index - 1;
            self.current_text_state.remove(idx);
            self.current_text_state.blinkey_index -= 1;
        } else {
            return false;
        }

        if matches!(self.app_state, AppState::Launch(_)) {
            self.set_launch_state(LaunchState::Fresh);
        }
        self.text_dirty = true;
        self.glow_colour = theme::GLOW_DEFAULT;
        self.search_result = None;
        self.selection_dirty = true;
        self.controls_dirty = true;
        true
    }

    /// Handle enter key from Android
    #[cfg(target_os = "android")]
    pub fn handle_enter(&mut self) -> bool {
        if !self.current_text_state.textbox_focused || self.current_text_state.chars.is_empty() {
            return false;
        }

        match &self.app_state {
            AppState::Launch(LaunchState::Fresh) => {
                self.start_attestation();
            }
            AppState::Ready => {
                let handle: String = self.current_text_state.chars.iter().collect();
                if !handle.is_empty() {
                    self.start_handle_search(&handle);
                }
            }
            _ => return false,
        }
        true
    }

    /// Handle left arrow key from Android
    #[cfg(target_os = "android")]
    pub fn handle_arrow_left(&mut self) -> bool {
        if !self.current_text_state.textbox_focused {
            return false;
        }

        // Clear selection and move cursor left
        if self.current_text_state.selection_anchor.is_some() {
            let anchor = self.current_text_state.selection_anchor.unwrap();
            let left = anchor.min(self.current_text_state.blinkey_index);
            self.current_text_state.blinkey_index = left;
            self.current_text_state.selection_anchor = None;
            self.selection_dirty = true;
            self.controls_dirty = true;
            return true;
        }

        if self.current_text_state.blinkey_index > 0 {
            self.current_text_state.blinkey_index -= 1;
            self.selection_dirty = true;
            self.controls_dirty = true;
            return true;
        }
        false
    }

    /// Handle right arrow key from Android
    #[cfg(target_os = "android")]
    pub fn handle_arrow_right(&mut self) -> bool {
        if !self.current_text_state.textbox_focused {
            return false;
        }

        // Clear selection and move cursor right
        if self.current_text_state.selection_anchor.is_some() {
            let anchor = self.current_text_state.selection_anchor.unwrap();
            let right = anchor.max(self.current_text_state.blinkey_index);
            self.current_text_state.blinkey_index = right;
            self.current_text_state.selection_anchor = None;
            self.selection_dirty = true;
            self.controls_dirty = true;
            return true;
        }

        if self.current_text_state.blinkey_index < self.current_text_state.chars.len() {
            self.current_text_state.blinkey_index += 1;
            self.selection_dirty = true;
            self.controls_dirty = true;
            return true;
        }
        false
    }

    /// Handle Android back button
    /// Returns true if handled (stay in app), false to allow default back behavior (exit)
    #[cfg(target_os = "android")]
    pub fn handle_back(&mut self) -> bool {
        // If in a chat, go back to contacts list (same as tapping back header button)
        if self.selected_contact.is_some() {
            self.app_state = AppState::Ready;
            let eff_ru = self.effective_ru();
            self.text_layout = TextLayout::new(
                self.width as usize,
                self.height as usize,
                self.span,
                eff_ru,
                &self.app_state,
            );
            self.layout = Layout::new(
                self.width as usize,
                self.height as usize,
                self.span,
                eff_ru,
                &self.app_state,
            );
            self.selected_contact = None;
            self.reset_textbox();
            self.window_dirty = true;
            return true; // Handled - don't exit
        }

        // On contacts screen - allow default back (exit app)
        false
    }

    // ============ Network Methods ============

    /// Set the handle query system (called from JNI after keypair is available on Android,
    /// or can be used to reinitialize on any platform)
    pub fn set_handle_query(&mut self, handle_query: HandleQuery) {
        // Start StatusChecker early so PT receiver is ready before attestation
        // This allows us to receive ClutchOffers from peers who come online before us
        #[cfg(target_os = "android")]
        if self.status_checker.is_none() {
            use crate::network::status::StatusChecker;
            self.status_checker = StatusChecker::new(
                handle_query.socket().clone(),
                self.device_keypair.clone(),
                self.contact_pubkeys.clone(),
                self.sync_records_provider.clone(),
            )
            .ok();
            if self.status_checker.is_some() {
                crate::log("UI: Status checker started early (PT receiver ready)");
            }
        }

        self.handle_query = Some(handle_query);
    }

    /// Set avatar from raw image file bytes (Android image picker)
    ///
    /// This receives the raw file bytes (JPEG/PNG/WebP) from Android's ContentResolver
    /// and passes them to encode_avatar_from_image() which properly handles ICC profiles
    /// for accurate color conversion to VSF RGB.
    #[cfg(target_os = "android")]
    pub fn set_avatar_from_file(&mut self, image_bytes: Vec<u8>) {
        use log::info;

        // Need handle to save avatar (storage key derived from handle)
        let handle = match &self.user_handle {
            Some(h) => h.clone(),
            None => {
                info!("Cannot save avatar: no handle (need to attest first)");
                return;
            }
        };

        info!("Processing avatar from picker: {} bytes", image_bytes.len());

        // Encode avatar using full ICC profile color management
        let av1_data = match crate::avatar::encode_avatar_from_image(&image_bytes) {
            Ok(data) => data,
            Err(e) => {
                info!("Avatar encoding failed: {}", e);
                return;
            }
        };

        info!("AV1 data size: {} bytes", av1_data.len());

        // Save avatar to local cache by handle's storage key
        if let Err(e) = crate::avatar::save_avatar(&av1_data, &handle) {
            info!("Failed to save avatar: {}", e);
            return;
        }

        // Read back from disk to verify end-to-end, convert to display colorspace
        let (_, pixels) = match crate::avatar::load_avatar(&handle) {
            Some(result) => result,
            None => {
                info!("Failed to load saved avatar");
                return;
            }
        };

        self.avatar_pixels =
            Some(crate::display_profile::DisplayConverter::new().convert_avatar(&pixels));
        self.avatar_scaled = None; // Invalidate cache to force re-scale
        self.window_dirty = true;

        info!("Avatar saved successfully");

        // Upload to FGTW (only if we have handle_proof)
        if let Some(ref handle_proof) = self.user_handle_proof {
            if let Err(e) =
                crate::avatar::upload_avatar(&self.device_keypair.secret, &handle, handle_proof)
            {
                info!("Failed to upload avatar to FGTW: {}", e);
            } else {
                info!("Avatar uploaded to FGTW");
            }
        } else {
            info!("Skipping avatar upload - no handle_proof yet");
        }
    }

    /// Handle file hover during drag operation
    pub fn handle_file_hover(&mut self, _path: &std::path::Path) {
        // Only on Ready screen
        if !matches!(self.app_state, AppState::Ready | AppState::Searching) {
            eprintln!("File hover ignored - not on Ready screen");
            return;
        }

        // Check if mouse is over avatar circle
        let mx = self.mouse_x as usize;
        let my = self.mouse_y as usize;

        eprintln!("File hover at ({}, {})", mx, my);

        if mx < self.width as usize && my < self.height as usize {
            let idx = my * self.width as usize + mx;
            let hit = self.hit_test_map[idx];
            eprintln!("Hit test value: {}", hit);
            if hit == HIT_AVATAR {
                // Mouse is over avatar - set hover state
                if !self.file_hovering_avatar {
                    eprintln!("Setting file_hovering_avatar = true");
                    self.file_hovering_avatar = true;
                    self.window_dirty = true;
                }
                return;
            }
        }

        // Mouse not over avatar - clear hover state
        if self.file_hovering_avatar {
            eprintln!("Clearing file_hovering_avatar");
            self.file_hovering_avatar = false;
            self.window_dirty = true;
        }
    }

    /// Handle file hover cancelled
    pub fn handle_file_hover_cancelled(&mut self) {
        if self.file_hovering_avatar {
            self.file_hovering_avatar = false;
            self.window_dirty = true;
        }
    }

    /// Handle dropped file for avatar upload
    pub fn handle_dropped_file(&mut self, path: &std::path::Path) -> Result<(), String> {
        // Only accept file drops on Ready screen (avatar visible)
        if !matches!(self.app_state, AppState::Ready | AppState::Searching) {
            return Ok(()); // Silently ignore on other screens
        }

        eprintln!("Processing dropped file: {:?}", path);

        // Clear hover state
        self.file_hovering_avatar = false;

        // Read file
        let image_data = std::fs::read(path).map_err(|e| format!("Failed to read file: {}", e))?;

        // Need handle to save avatar (storage key derived from handle)
        let handle = self
            .user_handle
            .as_ref()
            .ok_or("Cannot save avatar: no handle (need to attest first)")?;

        // Encode avatar at fixed size, save to .vsf
        let av1_data = crate::avatar::encode_avatar_from_image(&image_data)?;
        eprintln!("AV1 data size: {} bytes", av1_data.len());

        // Save avatar to local cache by handle's storage key
        crate::avatar::save_avatar(&av1_data, handle)
            .map_err(|e| format!("Failed to save avatar: {}", e))?;

        // Read back from disk to verify end-to-end, convert to display colorspace
        let (_, pixels) =
            crate::avatar::load_avatar(handle).ok_or("Failed to load saved avatar")?;

        self.avatar_pixels =
            Some(crate::display_profile::DisplayConverter::new().convert_avatar(&pixels));
        self.avatar_scaled = None; // Invalidate cache to force re-scale
        self.show_avatar_hint = false; // Hide hint after successful upload
        self.window_dirty = true;

        // Upload to FGTW (only if we have handle_proof)
        if let Some(ref handle_proof) = self.user_handle_proof {
            if let Err(e) =
                crate::avatar::upload_avatar(&self.device_keypair.secret, handle, handle_proof)
            {
                eprintln!("Avatar: Failed to upload to FGTW: {}", e);
            }
        } else {
            eprintln!("Avatar: Skipping FGTW upload - no handle_proof yet");
        }

        eprintln!("Avatar saved successfully");

        Ok(())
    }

    /// Adjust zoom level by steps (positive = zoom in, negative = zoom out)
    /// Uses logarithmic scaling: each step multiplies by 33/32 (in) or 32/33 (out).
    /// Clamps to range [1/32, 32] for full design exploration.
    pub fn adjust_zoom(&mut self, steps: f32) {
        let factor = if steps.is_sign_negative() {
            (33f32 / 32.).powf(steps)
        } else {
            (31f32 / 32.).powf(-steps)
        };
        self.ru = self.ru * factor;

        // Clamp ru to sane bounds (release only - dev builds are unbounded for testing)
        #[cfg(not(feature = "development"))]
        {
            const RU_MIN: f32 = 0.125; // 1/8
            const RU_MAX: f32 = 2.0;
            self.ru = self.ru.max(RU_MIN).min(RU_MAX);
        }
        self.window_dirty = true;

        let eff_ru = self.effective_ru();

        // Update text layout with new ru
        self.text_layout = TextLayout::new(
            self.width as usize,
            self.height as usize,
            self.span,
            eff_ru,
            &self.app_state,
        );

        // Update region layout with new ru
        self.layout = Layout::new(
            self.width as usize,
            self.height as usize,
            self.span,
            eff_ru,
            &self.app_state,
        );

        self.recalculate_char_widths();

        // Show zoom hint (will be hidden after 1 second via differential rendering)
        // Shows user's ru, not effective_ru (keyboard compensation is transparent)
        self.zoom_hint_visible = true;
        self.zoom_hint_hide_time =
            Some(std::time::Instant::now() + std::time::Duration::from_secs(1));
        self.zoom_hint_ru = self.ru;
    }

    /// Handle pinch-to-zoom scale gesture from Android
    /// scale_factor: >1.0 = zoom in, <1.0 = zoom out
    #[cfg(target_os = "android")]
    pub fn handle_scale(&mut self, scale_factor: f32) {
        // Convert scale factor to zoom steps using logarithm
        // log2(scale_factor) gives: pinch out (1.5x) -> +0.58, pinch in (0.7x) -> -0.51
        // Sensitivity multiplier controls how responsive the zoom feels
        const SENSITIVITY: f32 = 10.0;
        let steps = scale_factor.log2() * SENSITIVITY;
        self.adjust_zoom(steps);
    }

    /// Resize the application to new dimensions (shared by all platforms)
    pub fn resize_to(&mut self, width: u32, height: u32) {
        let old_height = self.height as isize;
        let w = width as usize;
        let h = height as usize;

        self.width = width;
        self.height = height;

        // Adjust scroll to keep content centered during resize
        // Content at old center (old_height/2 - scroll) should stay at new center
        // new_scroll = scroll + (new_height - old_height) / 2
        let height_delta = (height as isize - old_height) / 2;
        self.contacts_scroll_offset += height_delta;

        // On Android, compensate for keyboard resize by adjusting keyboard_scale
        // This keeps UI elements the same visual size when keyboard appears
        #[cfg(target_os = "android")]
        {
            self.keyboard_scale = self.initial_height as f32 / height as f32;
        }

        // Update cached scaling units
        // Span: harmonic mean of width and height - smooth at w==h, biased toward smaller
        // 2wh/(w+h) has finite slope at axes, slope exactly 1, tastes delicious
        self.span = 2 * w * h / (w + h);
        self.perimeter = w + h;
        self.diagonal_sq = w * w + h * h;

        let eff_ru = self.effective_ru();

        // Update text layout geometry
        self.text_layout = TextLayout::new(w, h, self.span, eff_ru, &self.app_state);

        // Update region layout
        self.layout = Layout::new(w, h, self.span, eff_ru, &self.app_state);

        // Clamp scroll to new content bounds (unless debug mode)
        if !self.debug && matches!(self.app_state, AppState::Ready | AppState::Searching) {
            let contacts_block = PixelRegion { x: 0, y: 0, w, h };
            let layout = ContactsUnifiedLayout::new(&contacts_block, self.span, eff_ru, 0);
            let user_section_bottom = layout.separator.y + layout.separator.h;

            // Count visible contacts (respecting search filter)
            let filter_text: String = self.current_text_state.chars.iter().collect();
            let filter_lower = filter_text.to_lowercase();
            let num_contacts = if filter_lower.is_empty() {
                self.contacts.len()
            } else {
                self.contacts
                    .iter()
                    .filter(|c| c.handle.as_str().to_lowercase().contains(&filter_lower))
                    .count()
            };

            let contacts_height = num_contacts * layout.row_height;
            // +2 rows for padding and version number at bottom
            let total_content_height = user_section_bottom + contacts_height + (2 * layout.row_height);

            let max_scroll_up: isize = 0;
            let max_scroll_down: isize = -((total_content_height as isize - h as isize).max(0));
            self.contacts_scroll_offset = self.contacts_scroll_offset.clamp(max_scroll_down, max_scroll_up);
        }

        self.renderer.resize(width, height);
        self.hit_test_map.resize((width * height) as usize, 0);
        self.textbox_mask.resize((width * height) as usize, 0);

        // Clear hover state on resize since button positions/sizes change
        self.hovered_button = HoveredButton::None;

        // Recalculate character widths for new font size
        self.recalculate_char_widths();

        // Recalculate scroll to keep blinkey in view with new dimensions
        if !self.current_text_state.chars.is_empty() {
            self.update_text_scroll();
        } else {
            // No text - center it
            self.current_text_state.scroll_offset = 0.0;
        }

        // Clear textbox focus on resize - user must click to refocus
        // On Android, preserve focus during keyboard resize (keyboard_scale != 1.0 means keyboard is up)
        #[cfg(not(target_os = "android"))]
        {
            self.current_text_state.textbox_focused = false;
            self.blinkey_visible = false;
        }

        // Trigger full redraw - differential rendering will be skipped automatically
        self.window_dirty = true;
    }

    #[cfg(not(target_os = "android"))]
    pub fn resize(&mut self, size: PhysicalSize<u32>) {
        self.resize_to(size.width, size.height);
    }

    pub fn update_modifiers(&mut self, modifiers: ModifiersState) {
        self.modifiers = modifiers;
    }

    pub fn get_resize_edge(&self, x: f32, y: f32) -> ResizeEdge {
        let resize_border = (self.span as f32 / 32.0).ceil();

        let at_left = x < resize_border;
        let at_right = x > (self.width as f32 - resize_border);
        let at_top = y < resize_border;
        let at_bottom = y > (self.height as f32 - resize_border);

        // Corners have priority
        if at_top && at_left {
            ResizeEdge::TopLeft
        } else if at_top && at_right {
            ResizeEdge::TopRight
        } else if at_bottom && at_left {
            ResizeEdge::BottomLeft
        } else if at_bottom && at_right {
            ResizeEdge::BottomRight
        } else if at_top {
            ResizeEdge::Top
        } else if at_bottom {
            ResizeEdge::Bottom
        } else if at_left {
            ResizeEdge::Left
        } else if at_right {
            ResizeEdge::Right
        } else {
            ResizeEdge::None
        }
    }

    /// Calculate blinkey_index from click/tap X coordinate
    /// Returns the cursor position (0 = before first char, len = after last char)
    pub fn blinkey_index_from_x(&self, click_x: f32) -> usize {
        if self.current_text_state.chars.is_empty() {
            return 0;
        }

        let center_x = self.width as usize / 2;
        let total_text_width: usize = self.current_text_state.width;
        let text_half = total_text_width / 2;
        let text_start_x =
            center_x as f32 - text_half as f32 + self.current_text_state.scroll_offset;

        let mut x_offset = text_start_x;

        for (i, &char_width) in self.current_text_state.widths.iter().enumerate() {
            let char_center = x_offset + char_width as f32 / 2.0;
            if click_x < char_center {
                return i;
            }
            x_offset += char_width as f32;
        }

        self.current_text_state.chars.len()
    }

    /// Start attestation - compute handle_proof and announce to FGTW
    pub fn start_attestation(&mut self) {
        let handle: String = self.current_text_state.chars.iter().collect();

        // Store handle for computing handle_proof after success
        self.attesting_handle = Some(handle.clone());

        // Set status to Attesting and trigger attestation
        self.app_state = AppState::Launch(LaunchState::Attesting);
        self.glow_colour = theme::GLOW_ATTESTING; // Yellow for attesting

        // Disable textbox and hide blinkey during attestation
        self.current_text_state.textbox_focused = false;
        self.blinkey_visible = false;

        if let Some(hq) = &self.handle_query {
            hq.query(handle);
        }
        let now = std::time::Instant::now();
        self.query_start_time = Some(now);
        self.last_frame_time = now; // Reset to prevent animation jerk on first frame
                                    // Initialize animation frame timing
        self.next_animation_frame =
            now + std::time::Duration::from_millis(self.target_frame_duration_ms);
    }

    /// Check if FGTW connectivity status is available and update fgtw_online
    pub fn check_fgtw_online(&mut self) {
        let online_opt = self
            .handle_query
            .as_ref()
            .and_then(|hq| hq.try_recv_online());
        if let Some(online) = online_opt {
            if online != self.fgtw_online {
                self.fgtw_online = online;
                self.controls_dirty = true; // Trigger indicator redraw
            }
        }
    }

    /// Search for a handle in our peer list (async - spawns background thread)
    pub fn start_handle_search(&mut self, handle: &str) {
        use crate::types::Handle;
        use std::sync::mpsc;

        // Set state to Searching and start animation
        self.app_state = AppState::Searching;
        self.searching_handle = Some(handle.to_string());
        self.glow_colour = theme::GLOW_ATTESTING; // Yellow for searching

        // Disable textbox and hide blinkey during search
        self.current_text_state.textbox_focused = false;
        self.blinkey_visible = false;

        let now = std::time::Instant::now();
        self.query_start_time = Some(now);
        self.last_frame_time = now;
        self.next_animation_frame =
            now + std::time::Duration::from_millis(self.target_frame_duration_ms);

        // Trigger FGTW refresh to get fresh peer list before searching
        // Runs in parallel with handle_proof computation (~1s each)
        if let Some(hq) = &self.handle_query {
            hq.refresh();
        }

        // Create channel for result
        let (tx, rx) = mpsc::channel();
        self.search_receiver = Some(rx);

        // Get transport for peer lookup (need to clone Arc)
        let transport = self.handle_query.as_ref().and_then(|hq| hq.get_transport());
        let handle_owned = handle.to_string();

        // Spawn background thread to compute handle_proof and lookup
        std::thread::spawn(move || {
            // Compute handle_proof (~1 second)
            let handle_proof = Handle::username_to_handle_proof(&handle_owned);

            // Brief pause to let refresh complete (both take ~1s, but give FGTW time)
            std::thread::sleep(std::time::Duration::from_millis(200));

            // Check peer store (now with fresh data from FGTW)
            let result = if let Some(peer_store) = transport {
                let store = peer_store.lock().unwrap();

                let peers = store.get_devices_for_handle(&handle_proof);
                if let Some(peer) = peers.first() {
                    SearchResult::Found(FoundPeer {
                        handle: HandleText::new(&handle_owned),
                        handle_proof,
                        device_pubkey: peer.device_pubkey.clone(),
                        ip: peer.ip,
                    })
                } else {
                    SearchResult::NotFound
                }
            } else {
                SearchResult::NotFound
            };

            let _ = tx.send(result);
        });

        self.window_dirty = true;
    }

    /// Check if search result is ready (non-blocking)
    pub fn check_search_result(&mut self) -> bool {
        if let Some(ref receiver) = self.search_receiver {
            if let Ok(result) = receiver.try_recv() {
                match &result {
                    SearchResult::Found(found_peer) => {
                        // Green glow
                        self.glow_colour = theme::GLOW_SUCCESS;

                        // Add to contacts if not already present
                        let already_exists =
                            self.contacts.iter().any(|c| c.handle == found_peer.handle);
                        if !already_exists {
                            let mut contact = Contact::new(
                                found_peer.handle.clone(),
                                found_peer.handle_proof,
                                found_peer.device_pubkey.clone(),
                            )
                            .with_ip(found_peer.ip);
                            let contact_id = contact.id.clone();
                            let their_handle_hash = contact.handle_hash;
                            // Set keygen flag BEFORE spawning to prevent race condition
                            contact.clutch_keygen_in_progress = true;
                            self.contacts.push(contact);

                            // Start background CLUTCH keypair generation + ceremony_id
                            // Both McEliece keygen and handle_proof are slow (~1-2s total)
                            let our_handle_hash = self
                                .user_handle
                                .as_ref()
                                .map(|h| crate::storage::contacts::derive_identity_seed(h))
                                .unwrap_or([0u8; 32]);
                            self.spawn_clutch_keygen(
                                contact_id,
                                our_handle_hash,
                                their_handle_hash,
                            );

                            // Update shared pubkey list for StatusChecker
                            {
                                let mut pubkeys = self.contact_pubkeys.lock().unwrap();
                                pubkeys.push(found_peer.device_pubkey.clone());
                                crate::log(&format!(
                                    "Contact added: {} ({})",
                                    found_peer.handle,
                                    hex::encode(&found_peer.device_pubkey.as_bytes()[..8])
                                ));
                            }

                            // Fetch avatar immediately for new contact
                            let handle = found_peer.handle.as_str().to_string();
                            #[cfg(not(target_os = "android"))]
                            crate::avatar::download_avatar_background(
                                handle.clone(),
                                self.contact_avatar_tx.clone(),
                                Some(self.event_proxy.clone()),
                            );
                            #[cfg(target_os = "android")]
                            crate::avatar::download_avatar_background(
                                handle,
                                self.contact_avatar_tx.clone(),
                                None,
                            );

                            // Save contact (updates both state file and contact list)
                            if let Some(ref identity_seed) = self.user_identity_seed {
                                let device_secret = self.device_keypair.secret.as_bytes();
                                if let Some(contact) = self.contacts.last() {
                                    if let Err(e) = crate::storage::contacts::save_contact(
                                        contact,
                                        identity_seed,
                                        device_secret,
                                    ) {
                                        crate::log(&format!("Failed to save contact: {}", e));
                                    }
                                }

                                // Sync contacts to cloud
                                if let Some(ref handle_proof) = self.user_handle_proof {
                                    if let Err(e) = crate::storage::cloud::sync_contacts_to_cloud(
                                        &self.contacts,
                                        identity_seed,
                                        &self.device_keypair,
                                        handle_proof,
                                    ) {
                                        crate::log(&format!(
                                            "Failed to sync contacts to cloud: {}",
                                            e
                                        ));
                                    }
                                }
                            }

                            // Try to load avatar from local cache immediately
                            if let Some((_, pixels)) =
                                crate::avatar::load_avatar(found_peer.handle.as_str())
                            {
                                if let Some(contact) = self.contacts.last_mut() {
                                    contact.avatar_pixels = Some(
                                        crate::display_profile::DisplayConverter::new()
                                            .convert_avatar(&pixels),
                                    );
                                    crate::log(&format!(
                                        "Avatar: Loaded {} from local cache on add",
                                        found_peer.handle
                                    ));
                                }
                            } else {
                                // Not in cache - fetch from FGTW
                                crate::log(&format!(
                                    "Avatar: {} not in cache, fetching from FGTW",
                                    found_peer.handle
                                ));
                                #[cfg(not(target_os = "android"))]
                                crate::avatar::download_avatar_background(
                                    found_peer.handle.as_str().to_string(),
                                    self.contact_avatar_tx.clone(),
                                    Some(self.event_proxy.clone()),
                                );
                                #[cfg(target_os = "android")]
                                crate::avatar::download_avatar_background(
                                    found_peer.handle.as_str().to_string(),
                                    self.contact_avatar_tx.clone(),
                                    None,
                                );
                            }
                        }

                        // Clear textbox
                        self.current_text_state.chars.clear();
                        self.current_text_state.widths.clear();
                        self.current_text_state.width = 0;
                        self.current_text_state.blinkey_index = 0;
                        self.current_text_state.selection_anchor = None;
                        self.current_text_state.scroll_offset = 0.0;
                        self.current_text_state.is_empty = true;
                        self.text_dirty = true;
                        self.controls_dirty = true;
                        self.selection_dirty = true;

                        // Immediately ping to check if online
                        // CLUTCH starts when PONG confirms they're online
                        if let Some(checker) = &self.status_checker {
                            if let Some(contact) = self.contacts.last() {
                                if let Some(ip) = contact.ip {
                                    crate::log(&format!(
                                        "Status: Immediately pinging {} (on add)",
                                        contact.handle
                                    ));
                                    checker.ping(ip, contact.public_identity.clone());
                                }
                            }
                        }
                    }
                    SearchResult::NotFound | SearchResult::Error(_) => {
                        // Red glow, keep text in box
                        self.glow_colour = theme::GLOW_ERROR;
                    }
                }

                self.search_result = Some(result);
                self.search_receiver = None;
                self.searching_handle = None;
                self.query_start_time = None;
                self.app_state = AppState::Ready;

                // Update layout after contact list change - textbox position depends on contact count
                let w = self.width as usize;
                let h = self.height as usize;
                let eff_ru = self.effective_ru();
                self.layout = Layout::new(w, h, self.span, eff_ru, &self.app_state);
                self.text_layout = TextLayout::new(w, h, self.span, eff_ru, &self.app_state);

                self.window_dirty = true;
                return true;
            }
        }
        false
    }

    /// Check if attestation response is ready and update app_state
    pub fn check_attestation_response(&mut self) -> bool {
        use crate::network::handle_query::AttestationData;

        let result = self.handle_query.as_ref().and_then(|hq| hq.try_recv());
        let Some(result) = result else { return false };

        let (new_state, attestation_data) = match result {
            QueryResult::Success(data) => {
                crate::log("UI: Attestation SUCCESS - transitioning to Ready state");
                (AppState::Ready, Some(data))
            }
            QueryResult::AlreadyAttested(_peers) => {
                crate::log("UI: Handle already attested - showing error");
                (
                    AppState::Launch(LaunchState::Error("Handle already attested".to_string())),
                    None,
                )
            }
            QueryResult::Error(msg) => {
                crate::log(&format!("UI: Attestation error - {}", msg));
                (AppState::Launch(LaunchState::Error(msg)), None)
            }
        };

        debug_println!(
            "Attestation completed: {:?} -> {:?}",
            self.app_state,
            new_state
        );

        // Clear textbox when transitioning to Ready
        if matches!(new_state, AppState::Ready) {
            self.current_text_state.chars.clear();
            self.current_text_state.widths.clear();
            self.current_text_state.width = 0;
            self.current_text_state.blinkey_index = 0;
            self.current_text_state.selection_anchor = None;
            self.current_text_state.scroll_offset = 0.0;
            self.current_text_state.is_empty = true;
            self.text_dirty = true;
            self.controls_dirty = true;
            self.selection_dirty = true;

            // Store peers for initial ping (will be set if attestation_data is Some)
            let mut initial_peers = Vec::new();

            // All data was pre-loaded in background thread - just assign it
            if let Some(data) = attestation_data {
                let handle = data.handle.clone();
                let handle_proof = data.handle_proof;
                let identity_seed = data.identity_seed;
                if let Some(hq) = &self.handle_query {
                    hq.set_handle_proof(handle_proof, &handle);
                }
                self.user_handle = Some(handle.clone());
                self.user_handle_proof = Some(handle_proof);
                self.user_identity_seed = Some(identity_seed);
                crate::log("UI: All data pre-loaded in background - no UI freeze");

                // Assign pre-loaded friendships
                if !data.friendships.is_empty() {
                    crate::log(&format!(
                        "UI: Assigning {} friendship chains",
                        data.friendships.len()
                    ));
                    self.friendship_chains.extend(data.friendships);
                }

                // Assign pre-loaded contacts
                crate::log(&format!("UI: Assigning {} contacts", data.contacts.len()));
                // Store peers for initial ping below
                initial_peers = data.peers.clone();

                for mut contact in data.contacts {
                    // Update shared pubkey list for StatusChecker
                    {
                        let mut pubkeys = self.contact_pubkeys.lock().unwrap();
                        pubkeys.push(contact.public_identity.clone());
                    }

                    // Start CLUTCH keygen if not complete AND no persisted keypairs
                    let needs_keygen = contact.clutch_state != crate::types::ClutchState::Complete
                        && contact.clutch_our_keypairs.is_none();
                    let contact_id = contact.id.clone();
                    let their_handle_hash = contact.handle_hash;
                    if needs_keygen {
                        contact.clutch_keygen_in_progress = true;
                    }
                    self.contacts.push(contact);
                    if needs_keygen {
                        self.spawn_clutch_keygen(contact_id, identity_seed, their_handle_hash);
                    }
                }

                // Proactive avatar loading: fetch all contact avatars in background
                crate::log(&format!(
                    "Avatar: Proactively fetching avatars for {} contact(s)",
                    self.contacts.len()
                ));
                for contact in &self.contacts {
                    if contact.avatar_pixels.is_none() {
                        let handle_str = contact.handle.as_str().to_string();
                        #[cfg(not(target_os = "android"))]
                        crate::avatar::download_avatar_background(
                            handle_str,
                            self.contact_avatar_tx.clone(),
                            Some(self.event_proxy.clone()),
                        );
                        #[cfg(target_os = "android")]
                        crate::avatar::download_avatar_background(
                            handle_str,
                            self.contact_avatar_tx.clone(),
                            None,
                        );
                    }
                }

                // Use pre-loaded avatar if available
                if let Some(pixels) = data.avatar_pixels {
                    self.avatar_pixels = Some(
                        crate::display_profile::DisplayConverter::new().convert_avatar(&pixels),
                    );
                    self.avatar_scaled = None;
                    crate::log("UI: Using pre-loaded avatar");
                }

                // Start bidirectional avatar sync in background
                #[cfg(not(target_os = "android"))]
                crate::avatar::sync_avatar_background(
                    *self.device_keypair.secret.as_bytes(),
                    handle.clone(),
                    self.user_handle_proof,
                    self.contact_avatar_tx.clone(),
                    Some(self.event_proxy.clone()),
                );
                #[cfg(target_os = "android")]
                crate::avatar::sync_avatar_background(
                    *self.device_keypair.secret.as_bytes(),
                    handle.clone(),
                    self.user_handle_proof,
                    self.contact_avatar_tx.clone(),
                    None,
                );
            }
            self.attesting_handle = None;

            // Initialize status checker for P2P contact pinging (if not already started early)
            if self.user_handle.is_some() {
                // Populate sync_records_provider (StatusChecker may already be running)
                self.update_sync_records();

                if self.status_checker.is_none() {
                    if let Some(hq) = &self.handle_query {
                        #[cfg(not(target_os = "android"))]
                        {
                            self.status_checker = StatusChecker::new(
                                hq.socket().clone(),
                                self.device_keypair.clone(),
                                self.contact_pubkeys.clone(),
                                self.sync_records_provider.clone(),
                                self.event_proxy.clone(),
                            )
                            .ok();
                        }
                        #[cfg(target_os = "android")]
                        {
                            self.status_checker = StatusChecker::new(
                                hq.socket().clone(),
                                self.device_keypair.clone(),
                                self.contact_pubkeys.clone(),
                                self.sync_records_provider.clone(),
                            )
                            .ok();
                        }
                        crate::log("UI: Status checker initialized after attestation");
                    }
                } else {
                    crate::log("UI: Status checker already running (started early)");
                }

                // Start WebSocket client for real-time peer IP updates (desktop only)
                #[cfg(not(target_os = "android"))]
                {
                    use crate::network::PeerUpdateClient;
                    self.peer_update_client =
                        Some(PeerUpdateClient::new(self.event_proxy.clone()));
                    crate::log("UI: PeerUpdateClient started for real-time IP updates");
                }

                // Broadcast StatusPing to all peers so they learn our IP (NAT hole punching)
                // PRIVACY: Only ping peers who are in our contacts list
                // Filter out our own pubkey - FGTW returns all peers including ourselves
                let our_pubkey_bytes = self.device_keypair.public.to_bytes();
                let contact_peers: Vec<_> = initial_peers
                    .iter()
                    .filter(|p| {
                        // Skip our own pubkey
                        if p.device_pubkey.as_bytes() == &our_pubkey_bytes {
                            return false;
                        }
                        // Only ping if this peer is in our contacts
                        self.contacts.iter().any(|c| c.public_identity == p.device_pubkey)
                    })
                    .collect();

                if !contact_peers.is_empty() {
                    if let Some(ref checker) = self.status_checker {
                        for peer in &contact_peers {
                            checker.ping(peer.ip, peer.device_pubkey.clone());
                        }
                        crate::log(&format!(
                            "Network: Initial broadcast ping to {} contact(s)",
                            contact_peers.len()
                        ));
                    }
                }
            }

            // Schedule first FGTW refresh in 60-120 seconds
            {
                use rand::Rng;
                let delay = rand::thread_rng().gen_range(60..=120);
                self.next_fgtw_refresh =
                    std::time::Instant::now() + std::time::Duration::from_secs(delay);
            }
        }

        // Set glow colour based on new state
        if matches!(new_state, AppState::Launch(LaunchState::Error(_))) {
            self.glow_colour = theme::GLOW_ERROR;
        } else {
            self.glow_colour = theme::GLOW_DEFAULT;
        }
        self.app_state = new_state;
        // Recalculate layouts since regions change between Launch and Ready states
        let eff_ru = self.effective_ru();
        self.text_layout = TextLayout::new(
            self.width as usize,
            self.height as usize,
            self.span,
            eff_ru,
            &self.app_state,
        );
        self.layout = Layout::new(
            self.width as usize,
            self.height as usize,
            self.span,
            eff_ru,
            &self.app_state,
        );
        self.query_start_time = None;
        self.window_dirty = true;
        crate::log("UI: Attestation complete, window marked dirty for redraw");
        true
    }

    /// Check if we should continuously animate (request redraws every frame)
    pub fn should_animate(&self) -> bool {
        matches!(
            self.app_state,
            AppState::Launch(LaunchState::Attesting) | AppState::Searching
        )
    }

    /// Get the current launch state (if in Launch mode)
    pub fn launch_state(&self) -> Option<&LaunchState> {
        match &self.app_state {
            AppState::Launch(state) => Some(state),
            _ => None,
        }
    }

    /// Set the launch state (only if currently in Launch mode)
    pub fn set_launch_state(&mut self, state: LaunchState) {
        self.app_state = AppState::Launch(state);
    }

    /// Check for status updates from P2P checker (non-blocking)
    /// Returns true if any contact status changed
    pub fn check_status_updates(&mut self) -> bool {
        use crate::crypto::clutch;
        use crate::network::status::StatusUpdate;
        // NOTE: ClutchRequest and ClutchRequestType imports removed - legacy v1 CLUTCH no longer used
        use crate::types::ClutchState;

        let checker = match &self.status_checker {
            Some(c) => c,
            None => return false,
        };

        // Get our handle_hash for CLUTCH (PRIVATE identity seed, used in VSF messages)
        // Formula: BLAKE3(VsfType::x(handle).flatten()) - VSF normalized for Unicode safety
        // SECURITY: This IS sent in CLUTCH offers for contact matching, but only parties
        // who already know our handle can compute it to match us
        let our_handle_hash = match self.user_identity_seed {
            Some(h) => h,
            None => return false, // Can't do CLUTCH without our handle_hash
        };

        // Also need our_identity_seed alias for keygen spawning (same value)
        let our_identity_seed = our_handle_hash;

        let mut changed = false;
        let mut ceremony_completions: Vec<usize> = Vec::new(); // Contact indices to complete after loop
                                                               // Collect pending message retransmit requests (friendship_id, ip, handle, device_pubkey, last_received_ef6) to process after loop
                                                               // last_received_ef6 from pong tells us what they already have - only retransmit newer
        let mut retransmit_requests: Vec<(
            crate::types::FriendshipId,
            std::net::SocketAddr,
            String,
            [u8; 32], // Recipient device pubkey for relay fallback
            Option<f64>,
        )> = Vec::new();
        // Flag to update sync records after the loop (when borrows are released)
        let mut need_sync_update = false;

        while let Some(update) = checker.try_recv() {
            match update {
                StatusUpdate::Online {
                    peer_pubkey,
                    is_online,
                    peer_addr,
                    sync_records,
                } => {
                    // Find matching contact and update status
                    for contact in &mut self.contacts {
                        if contact.public_identity == peer_pubkey {
                            // Note: ceremony_id is now computed from offer_provenances, not ping provenances.
                            // Offer provenances are collected when ClutchOfferReceived messages arrive.

                            // Update IP from the ping/pong source address
                            if let Some(addr) = peer_addr {
                                if contact.ip != Some(addr) {
                                    crate::log(&format!(
                                        "Status: Updated {} IP from ping/pong: {:?} -> {}",
                                        contact.handle, contact.ip, addr
                                    ));
                                    contact.ip = Some(addr);
                                }
                            }

                            if contact.is_online != is_online {
                                contact.is_online = is_online;
                                changed = true;
                                crate::log(&format!(
                                    "Status: {} is now {}",
                                    contact.handle,
                                    if is_online { "ONLINE" } else { "offline" }
                                ));
                            }

                            // Send full offer when contact comes online and keys are ready
                            // Keys are pre-generated in background when contact is added
                            // Send offer when online, in Pending state, and have keypairs
                            // PT handles all retry logic - will keep trying until peer ACKs
                            // When we receive their offer, we'll clear outbound transfers
                            if is_online
                                && contact.clutch_state == ClutchState::Pending
                                && contact.clutch_offer_transfer_id.is_none()
                            {
                                if let Some(ref keypairs) = contact.clutch_our_keypairs {
                                    use crate::network::fgtw::protocol::build_clutch_offer_vsf;
                                    use crate::network::status::ClutchOfferRequest;

                                    let payload =
                                        clutch::ClutchOfferPayload::from_keypairs(keypairs);

                                    if let Some(ip) = contact.ip {
                                        // Build VSF and capture our offer_provenance
                                        let conversation_token =
                                            clutch::derive_conversation_token(&[
                                                our_handle_hash,
                                                contact.handle_hash,
                                            ]);
                                        match build_clutch_offer_vsf(
                                            &conversation_token,
                                            &payload,
                                            self.device_keypair.public.as_bytes(),
                                            self.device_keypair.secret.as_bytes(),
                                        ) {
                                            Ok((vsf_bytes, our_offer_provenance)) => {
                                                crate::log(&format!(
                                                    "CLUTCH: Sending full offer to {} (prov={}...) - PT will retry until ACKed",
                                                    contact.handle,
                                                    hex::encode(&our_offer_provenance[..4])
                                                ));

                                                // Store our offer provenance (for ceremony_id derivation)
                                                if !contact
                                                    .offer_provenances
                                                    .contains(&our_offer_provenance)
                                                {
                                                    contact
                                                        .offer_provenances
                                                        .push(our_offer_provenance);
                                                }

                                                // Persist provenance immediately
                                                let device_secret =
                                                    *self.device_keypair.secret.as_bytes();
                                                if let Err(e) =
                                                    crate::storage::contacts::save_clutch_slots(
                                                        &contact.clutch_slots,
                                                        &contact.offer_provenances,
                                                        contact.ceremony_id,
                                                        contact.handle.as_str(),
                                                        &our_handle_hash,
                                                        &device_secret,
                                                    )
                                                {
                                                    crate::log(&format!(
                                                        "Failed to persist CLUTCH provenance: {}",
                                                        e
                                                    ));
                                                }

                                                checker.send_offer(ClutchOfferRequest {
                                                    peer_addr: ip,
                                                    vsf_bytes,
                                                });
                                                // Mark as sent (PT tracks the actual transfer state)
                                                contact.clutch_offer_transfer_id = Some(0); // Placeholder - PT handles retries
                                                changed = true;
                                            }
                                            Err(e) => {
                                                crate::log(&format!(
                                                    "CLUTCH: Failed to build offer VSF: {}",
                                                    e
                                                ));
                                            }
                                        }
                                    }
                                }
                            }

                            // Queue retransmit of pending messages when contact comes online
                            if is_online {
                                if let (Some(fid), Some(ip)) = (contact.friendship_id, contact.ip) {
                                    // Look up sync record for this friendship's conversation_token
                                    let last_received = if let Some((_, chains)) =
                                        self.friendship_chains.iter().find(|(id, _)| *id == fid)
                                    {
                                        sync_records
                                            .iter()
                                            .find(|r| {
                                                r.conversation_token == chains.conversation_token
                                            })
                                            .map(|r| r.last_received_ef6)
                                    } else {
                                        None
                                    };
                                    retransmit_requests.push((
                                        fid,
                                        ip,
                                        contact.handle.as_str().to_string(),
                                        *contact.public_identity.as_bytes(),
                                        last_received,
                                    ));
                                }
                            }

                            break;
                        }
                    }
                }
                // NOTE: ClutchOffer, ClutchInit, ClutchResponse, ClutchComplete handlers REMOVED
                // Full 8-primitive CLUTCH uses ClutchOfferReceived and ClutchKemResponseReceived
                // which are handled above (via TCP/PT transport).
                StatusUpdate::ChatMessage {
                    conversation_token,
                    prev_msg_hp,
                    ciphertext,
                    timestamp,
                    sender_addr,
                } => {
                    // Get our handle_hash for chain lookups
                    let our_handle_hash = match self.user_identity_seed {
                        Some(h) => h,
                        None => {
                            crate::log("CHAT: No user_identity_seed - cannot decrypt");
                            continue;
                        }
                    };

                    // Find friendship by conversation_token
                    let chains_result = self
                        .friendship_chains
                        .iter_mut()
                        .find(|(_, c)| c.conversation_token == conversation_token);

                    let mut need_sync_records_update = false;
                    if let Some((fid, chains)) = chains_result {
                        // For 2-party chats, infer sender as the "other" participant
                        let from_handle_hash = match chains.other_participant(&our_handle_hash) {
                            Some(h) => *h,
                            None => {
                                crate::log("CHAT: Could not determine sender (not a 2-party chat or we're not a participant)");
                                continue;
                            }
                        };

                        // Find contact by their handle_hash
                        let contact_info = self.contacts.iter().enumerate().find_map(|(idx, c)| {
                            if c.handle_hash == from_handle_hash {
                                Some((idx, c.handle.to_string()))
                            } else {
                                None
                            }
                        });

                        let (contact_idx, handle) = match contact_info {
                            Some((idx, h)) => (idx, h),
                            None => {
                                crate::log(&format!(
                                    "CHAT: Contact not found for handle_hash {}...",
                                    hex::encode(&from_handle_hash[..8])
                                ));
                                continue;
                            }
                        };

                        // Deduplication: skip if we've already processed this exact message
                        // (UDP duplicates have identical eagle_time)
                        // Note: Sender learns our state via last_received_hp in ping/pong - no ACK needed for dupes
                        if chains.is_duplicate(&from_handle_hash, timestamp) {
                            crate::log(&format!(
                                "CHAT: Skipping duplicate message from {} (eagle_time {})",
                                handle, timestamp
                            ));
                            continue;
                        }

                        // Hash chain verification: check prev_msg_hp matches expected
                        // If mismatch: either out-of-order or missing messages
                        if let Err(expected) =
                            chains.verify_chain_link(&from_handle_hash, &prev_msg_hp)
                        {
                            crate::log(&format!(
                                "CHAT: Hash chain mismatch from {} - expected {}..., got {}... (may need resync)",
                                handle,
                                hex::encode(&expected[..8]),
                                hex::encode(&prev_msg_hp[..8])
                            ));
                            // For now, continue with decryption anyway (soft verification)
                            // TODO: Request resync if gap detected
                        }

                        crate::log(&format!(
                            "CHAT: Received message from {} (eagle_time {}), {} bytes ciphertext",
                            handle,
                            timestamp,
                            ciphertext.len()
                        ));

                        use crate::crypto::chain::{
                            decrypt_layers, derive_salt, generate_scratch, CURRENT_KEY_INDEX,
                        };

                        // Get sender's chain for decryption
                        let sender_chain = match chains.chain(&from_handle_hash) {
                            Some(c) => c.clone(), // Clone to avoid borrow issues
                            None => {
                                crate::log(&format!("CHAT: Sender chain not found for {}", handle));
                                continue;
                            }
                        };

                        // Get sender's last plaintext for salt derivation
                        let their_last_plaintext =
                            chains.last_plaintext(&from_handle_hash).to_vec();

                        // Derive salt from their previous plaintext
                        let salt = derive_salt(&their_last_plaintext, &sender_chain);

                        // Generate scratch pad
                        let scratch = generate_scratch(&sender_chain, &salt);

                        // Convert eagle time for decryption
                        let eagle_time = vsf::EagleTime::new(vsf::types::EtType::f6(timestamp));

                        // DEBUG: Log decryption parameters
                        crate::log(&format!(
                            "CHAIN DECRYPT: sender_handle_hash={}..., key={}..., salt={}..., eagle_time={}, ciphertext_len={}",
                            hex::encode(&from_handle_hash[..4]),
                            hex::encode(&sender_chain.current_key()[..4]),
                            hex::encode(&salt[..4]),
                            timestamp,
                            ciphertext.len()
                        ));

                        // Decrypt using sender's chain
                        let plaintext = decrypt_layers(
                            &ciphertext,
                            &sender_chain,
                            CURRENT_KEY_INDEX,
                            &scratch,
                            &eagle_time,
                        );

                        // DEBUG: Log raw decrypted bytes
                        crate::log(&format!(
                            "CHAIN DECRYPT: raw plaintext bytes = {:?}",
                            &plaintext
                        ));

                        // Parse VSF field: (d{message}:x{text},hp{inc_hp},hR{pad})
                        // Uses type-marker parsing (not positional) per AGENT.md
                        let mut ptr = 0usize;
                        let mut message_text = String::new();
                        let mut incorporated_hp = [0u8; 32];

                        // Expect '(' to start field
                        if plaintext.get(ptr) != Some(&b'(') {
                            crate::log("CHAT: Expected '(' to start message field");
                            continue;
                        }
                        ptr += 1;

                        // Parse field name (d{message})
                        match vsf::parse(&plaintext, &mut ptr) {
                            Ok(vsf::VsfType::d(name)) if name == "message" => {}
                            Ok(vsf::VsfType::d(name)) => {
                                crate::log(&format!(
                                    "CHAT: Expected field name 'message', got '{}'",
                                    name
                                ));
                                continue;
                            }
                            Ok(other) => {
                                crate::log(&format!("CHAT: Expected d type, got {:?}", other));
                                continue;
                            }
                            Err(e) => {
                                crate::log(&format!("CHAT: VSF parse error: {:?}", e));
                                continue;
                            }
                        }

                        // Expect ':' separator
                        if plaintext.get(ptr) != Some(&b':') {
                            crate::log("CHAT: Expected ':' after field name");
                            continue;
                        }
                        ptr += 1;

                        // Parse comma-separated values by type marker (not position)
                        loop {
                            match vsf::parse(&plaintext, &mut ptr) {
                                Ok(vsf::VsfType::x(s)) => message_text = s,
                                Ok(vsf::VsfType::hp(hash)) if hash.len() == 32 => {
                                    incorporated_hp.copy_from_slice(&hash);
                                }
                                Ok(vsf::VsfType::hR(_)) => {} // Random padding - ignore
                                Ok(other) => {
                                    crate::log(&format!(
                                        "CHAT: Unexpected type in message: {:?}",
                                        other
                                    ));
                                }
                                Err(_) => break,
                            }

                            // Check for ',' (more values) or ')' (end of field)
                            match plaintext.get(ptr) {
                                Some(b',') => ptr += 1, // Continue to next value
                                Some(b')') => break,    // End of field
                                _ => break,
                            }
                        }

                        if message_text.is_empty() {
                            crate::log("CHAT: No message text found in payload");
                            continue;
                        }

                        crate::log(&format!(
                            "CHAT: Decrypted message from {}: \"{}\" (incorporated_hp={}...)",
                            handle,
                            message_text,
                            hex::encode(&incorporated_hp[..8])
                        ));

                        // Compute plaintext hash for ACK
                        let plaintext_hash = *blake3::hash(&plaintext).as_bytes();

                        // Derive this message's hash pointer (for bidirectional tracking)
                        use crate::types::friendship::derive_msg_hp;
                        let msg_hp = derive_msg_hp(&prev_msg_hp, &plaintext_hash, timestamp);

                        // Update their last_plaintext for next message's salt
                        chains.set_last_plaintext(&from_handle_hash, plaintext.clone());

                        // Update bidirectional entropy state (derive weave hash from full message context)
                        chains.update_received_for_mixing(timestamp, msg_hp, &plaintext);

                        // Look up OUR plaintext that they incorporated (for bidirectional weave)
                        // If incorporated_hp is all zeros, they didn't incorporate any of our messages
                        // Clone to avoid borrow issues with advance()
                        let our_incorporated_plaintext: Option<Vec<u8>> =
                            if incorporated_hp != [0u8; 32] {
                                chains
                                    .get_pending_plaintext_by_hp(&incorporated_hp)
                                    .map(|p| p.to_vec())
                            } else {
                                None
                            };

                        // Advance their chain with bidirectional weave
                        let eagle_time_for_advance =
                            vsf::EagleTime::new(vsf::types::EtType::f6(timestamp));
                        chains.advance(
                            &from_handle_hash,
                            &eagle_time_for_advance,
                            &plaintext,
                            our_incorporated_plaintext.as_deref(),
                        );

                        // Mark as received for deduplication (protects against UDP duplicates)
                        chains.mark_received(&from_handle_hash, timestamp);

                        // Update hash chain state for next message verification
                        chains.update_received_hash(&from_handle_hash, msg_hp);
                        crate::log(&format!(
                            "CHAT: Updated hash chain for {} - msg_hp={}...",
                            handle,
                            hex::encode(&msg_hp[..8])
                        ));

                        // CRASH SAFETY: Persist to disk BEFORE sending ACK
                        // If we crash after ACK but before disk, sender thinks we have it but we don't.
                        // Disk write is the commit point - ACK is just notification.
                        if let Some(ref identity_seed) = self.user_identity_seed {
                            let device_secret = self.device_keypair.secret.as_bytes();
                            if let Err(e) = crate::storage::friendship::save_friendship_chains(
                                chains,
                                identity_seed,
                                device_secret,
                            ) {
                                crate::log(&format!(
                                    "STORAGE: Failed to save chains after recv: {}",
                                    e
                                ));
                            }
                            // Flag to update sync records after borrow ends
                            need_sync_records_update = true;
                        }

                        // Add message to contact's message list and persist
                        if let Some(contact) = self.contacts.get_mut(contact_idx) {
                            // Use actual eagle_time and sorted insert for correct chronological order
                            contact.insert_message_sorted(ChatMessage::new_with_timestamp(
                                message_text,
                                false,     // is_outgoing = false (received)
                                timestamp, // Use message's actual eagle_time, not current time
                            ));
                            contact.message_scroll_offset = 0.0; // Scroll to show new message
                            changed = true;

                            // Persist messages for UI
                            if let Some(ref identity_seed) = self.user_identity_seed {
                                let device_secret = self.device_keypair.secret.as_bytes();
                                if let Err(e) = crate::storage::contacts::save_messages(
                                    contact,
                                    identity_seed,
                                    device_secret,
                                ) {
                                    crate::log(&format!("STORAGE: Failed to save messages: {}", e));
                                }
                            }
                        }

                        // *** THEN send ACK - if we crash here, sender will resend, we can dedup ***
                        // Get recipient pubkey for relay fallback
                        let recipient_pubkey = self.contacts.get(contact_idx)
                            .map(|c| *c.public_identity.as_bytes())
                            .unwrap_or([0u8; 32]);
                        if let Some(ref checker) = self.status_checker {
                            checker.send_ack(AckRequest {
                                peer_addr: sender_addr,
                                recipient_pubkey,
                                conversation_token,
                                acked_eagle_time: timestamp,
                                plaintext_hash,
                            });
                            crate::log(&format!(
                                "CHAT: Sent ACK to {} (eagle_time {}, hash {}...)",
                                handle,
                                timestamp,
                                hex::encode(&plaintext_hash[..8])
                            ));
                        }
                        let _ = fid; // We looked up by token, fid is available if needed
                    } else {
                        crate::log(&format!(
                            "CHAT: No friendship found for conversation_token {}...",
                            hex::encode(&conversation_token[..8])
                        ));
                    }

                    // Flag to update sync records after outer loop (checker borrow must end first)
                    if need_sync_records_update {
                        need_sync_update = true;
                    }
                }
                StatusUpdate::MessageAck {
                    conversation_token,
                    acked_eagle_time,
                    plaintext_hash,
                } => {
                    // Get our handle_hash
                    let our_handle_hash = match self.user_identity_seed {
                        Some(h) => h,
                        None => {
                            crate::log("CHAT: No user_identity_seed - cannot process ACK");
                            continue;
                        }
                    };

                    // Find friendship by conversation_token
                    let chains_result = self
                        .friendship_chains
                        .iter_mut()
                        .find(|(_, c)| c.conversation_token == conversation_token);

                    if let Some((_, chains)) = chains_result {
                        // For 2-party chats, the ACK sender is the "other" participant
                        let from_handle_hash = match chains.other_participant(&our_handle_hash) {
                            Some(h) => *h,
                            None => {
                                crate::log("CHAT: Could not determine ACK sender");
                                continue;
                            }
                        };

                        // Find contact by their handle_hash
                        let contact_info = self.contacts.iter().enumerate().find_map(|(idx, c)| {
                            if c.handle_hash == from_handle_hash {
                                Some((idx, c.handle.to_string()))
                            } else {
                                None
                            }
                        });

                        let (contact_idx, handle) = match contact_info {
                            Some((idx, h)) => (idx, h),
                            None => {
                                crate::log(&format!(
                                    "CHAT: Contact not found for ACK from handle_hash {}...",
                                    hex::encode(&from_handle_hash[..8])
                                ));
                                continue;
                            }
                        };

                        crate::log(&format!(
                            "CHAT: ACK received from {} for eagle_time {} (hash: {}...)",
                            handle,
                            acked_eagle_time,
                            hex::encode(&plaintext_hash[..8])
                        ));

                        // Process ACK: advance our chain and remove pending message
                        if chains.process_ack(&our_handle_hash, acked_eagle_time, &plaintext_hash) {
                            crate::log(&format!(
                                "CHAT: Chain advanced for {} (ACK verified)",
                                handle
                            ));

                            // First ACK confirms both sides have working chains - safe to zeroize CLUTCH keypairs
                            if let Some(contact) = self.contacts.get_mut(contact_idx) {
                                if contact.clutch_our_keypairs.is_some() {
                                    let handle_str = contact.handle.as_str().to_string();
                                    crate::log(&format!(
                                        "CLUTCH: First ACK from {} - zeroizing ephemeral keypairs",
                                        contact.handle
                                    ));
                                    if let Some(ref mut keys) = contact.clutch_our_keypairs {
                                        keys.zeroize();
                                    }
                                    contact.clutch_our_keypairs = None;
                                    for slot in &mut contact.clutch_slots {
                                        slot.offer = None;
                                        if let Some(ref mut s) = slot.kem_secrets_from_them {
                                            s.zeroize();
                                        }
                                        if let Some(ref mut s) = slot.kem_secrets_to_them {
                                            s.zeroize();
                                        }
                                        slot.kem_secrets_from_them = None;
                                        slot.kem_secrets_to_them = None;
                                    }

                                    // Delete persisted keypairs file (no longer needed)
                                    if let Err(e) = crate::storage::contacts::delete_clutch_keypairs(
                                        &handle_str,
                                    ) {
                                        crate::log(&format!(
                                            "CLUTCH: Failed to delete keypairs file for {}: {}",
                                            handle_str, e
                                        ));
                                    }
                                }
                            }

                            // Persist chains (AGENT.md: every change hits disk)
                            if let Some(ref identity_seed) = self.user_identity_seed {
                                let device_secret = self.device_keypair.secret.as_bytes();
                                if let Err(e) = crate::storage::friendship::save_friendship_chains(
                                    chains,
                                    identity_seed,
                                    device_secret,
                                ) {
                                    crate::log(&format!(
                                        "STORAGE: Failed to save chains after ACK: {}",
                                        e
                                    ));
                                }
                            }
                        } else {
                            crate::log(&format!(
                                "CHAT: ACK verification failed for {} (no matching pending message)",
                                handle
                            ));
                        }

                        // Mark message as delivered in UI
                        if let Some(contact) = self.contacts.get_mut(contact_idx) {
                            // Find message by matching eagle_time (within tolerance)
                            let mut found_msg = false;
                            for msg in contact.messages.iter_mut().rev() {
                                if msg.is_outgoing && !msg.delivered {
                                    // Match by eagle_time (same tolerance as process_ack)
                                    if (msg.timestamp - acked_eagle_time).abs() < 0.001 {
                                        msg.delivered = true;
                                        found_msg = true;
                                        changed = true;
                                        break;
                                    }
                                }
                            }

                            // Persist delivered status (AGENT.md: every change hits disk)
                            if found_msg {
                                if let Some(ref identity_seed) = self.user_identity_seed {
                                    let device_secret = self.device_keypair.secret.as_bytes();
                                    if let Err(e) = crate::storage::contacts::save_messages(
                                        contact,
                                        identity_seed,
                                        device_secret,
                                    ) {
                                        crate::log(&format!(
                                            "STORAGE: Failed to save delivered status: {}",
                                            e
                                        ));
                                    }
                                }
                            }
                        }
                    } else {
                        crate::log(&format!(
                            "CHAT: No friendship found for ACK conversation_token {}...",
                            hex::encode(&conversation_token[..8])
                        ));
                    }
                }

                // PT large transfer received (fallback - normally parsed in status.rs)
                // This only fires if the PT data wasn't recognized as CLUTCH message
                StatusUpdate::PTReceived { peer_addr, data } => {
                    crate::log(&format!(
                        "PT: Received unknown {} bytes from {} (not CLUTCH)",
                        data.len(),
                        peer_addr
                    ));
                }

                // PT outbound transfer completed
                StatusUpdate::PTSendComplete { peer_addr } => {
                    crate::log(&format!("PT: Outbound transfer to {} completed", peer_addr));
                    // TODO: Track completion for full CLUTCH flow
                }

                // Full CLUTCH offer received (~548KB with all 8 pubkeys)
                // Payload is already parsed and signature verified by status.rs
                StatusUpdate::ClutchOfferReceived {
                    conversation_token,
                    offer_provenance, // Unique per offer (VSF hp field)
                    sender_pubkey,
                    payload,
                    sender_addr: raw_sender_addr,
                } => {
                    use crate::crypto::clutch::{
                        derive_conversation_token, ClutchKemResponsePayload,
                        ClutchKemSharedSecrets, ClutchOfferPayload,
                    };
                    use crate::network::status::ClutchOfferRequest;
                    use crate::types::ClutchState;

                    crate::log(&format!(
                        "CLUTCH: Processing ClutchOfferReceived from {} (contacts={})",
                        raw_sender_addr,
                        self.contacts.len()
                    ));

                    // Normalize to port 4383 (TCP source port is ephemeral)
                    let sender_addr =
                        std::net::SocketAddr::new(raw_sender_addr.ip(), crate::PHOTON_PORT);

                    // Get our handle_hash
                    let our_handle_hash = match self.user_identity_seed {
                        Some(h) => h,
                        None => {
                            #[cfg(feature = "development")]
                            #[cfg(feature = "development")]
                            crate::log("CLUTCH: No user_identity_seed available");
                            continue;
                        }
                    };

                    // Find contact by conversation_token (compute token for each contact and match)
                    let their_handle_hash = match self
                        .contacts
                        .iter()
                        .find(|c| {
                            derive_conversation_token(&[our_handle_hash, c.handle_hash])
                                == conversation_token
                        })
                        .map(|c| c.handle_hash)
                    {
                        Some(h) => h,
                        None => {
                            crate::log(&format!(
                                "CLUTCH: Received offer with unknown conversation_token {}",
                                hex::encode(&conversation_token[..8])
                            ));
                            continue;
                        }
                    };

                    crate::log(&format!(
                        "CLUTCH: Received full offer (VSF verified) from {} tok={}...",
                        sender_addr,
                        hex::encode(&conversation_token[..8])
                    ));

                    // Verify sender's device pubkey matches the contact's known identity
                    let contact_pubkey = self
                        .contacts
                        .iter()
                        .find(|c| c.handle_hash == their_handle_hash)
                        .map(|c| c.public_identity.key);

                    match contact_pubkey {
                        None => {
                            #[cfg(feature = "development")]
                            #[cfg(feature = "development")]
                            crate::log("CLUTCH: Received offer from unknown contact");
                            continue;
                        }
                        Some(expected) if expected != sender_pubkey => {
                            crate::log(&format!(
                                "CLUTCH: Device pubkey mismatch! Expected {}, got {}",
                                hex::encode(&expected[..8]),
                                hex::encode(&sender_pubkey[..8])
                            ));
                            continue;
                        }
                        Some(_) => {} // Match - proceed
                    }

                    // The payload is already parsed
                    let their_offer = payload;

                    // Find contact by handle_hash
                    let mut rekey_request: Option<(ContactId, [u8; 32])> = None;
                    let mut chains_to_remove: Vec<FriendshipId> = Vec::new();
                    // Deferred KEM encapsulation spawn (to avoid borrow conflict)
                    let mut kem_encap_spawn: Option<(
                        ContactId,
                        ClutchOfferPayload,
                        [u8; 32],
                        [u8; 32],
                        std::net::SocketAddr,
                    )> = None;

                    for (idx, contact) in self.contacts.iter_mut().enumerate() {
                        if contact.handle_hash == their_handle_hash {
                            contact.ip = Some(sender_addr);

                            // Simple re-key logic: if stored keys don't match received keys, re-key.
                            // Same keys = duplicate/stale (ignore). Different/no keys = accept.
                            let stored_hqc_pub = contact
                                .get_slot(&their_handle_hash)
                                .and_then(|slot| slot.offer.as_ref())
                                .map(|o| o.hqc256_public.clone());

                            if let Some(stored_keys) = stored_hqc_pub {
                                if stored_keys == their_offer.hqc256_public {
                                    // Same keys - check if we already sent KEM response
                                    // If so, peer didn't receive it - re-send!
                                    let already_sent_kem = contact
                                        .get_slot(&our_handle_hash)
                                        .map(|s| s.kem_secrets_to_them.is_some())
                                        .unwrap_or(false);

                                    if already_sent_kem {
                                        // We already sent KEM response but peer resent offer
                                        // They didn't receive it - trigger re-send
                                        crate::log(&format!(
                                            "CLUTCH: Re-sending KEM response to {} (peer resent same offer)",
                                            contact.handle
                                        ));
                                        // Don't continue - fall through to re-send KEM below
                                    } else {
                                        // Same keys but no KEM sent yet - truly duplicate, ignore
                                        crate::log(&format!(
                                            "CLUTCH: Ignoring duplicate offer from {} (same keys, no KEM sent yet)",
                                            contact.handle
                                        ));
                                        continue;
                                    }
                                } else {
                                    // Different keys from them - but DON'T immediately nuke!
                                    // This prevents infinite re-key loops where both sides keep regenerating.
                                    //
                                    // Strategy: If we have keypairs, just update their offer and continue.
                                    // We'll send our existing offer, they'll either:
                                    // - Accept it (converge) if they're mid-ceremony
                                    // - Send KEM response (complete) if they're ahead
                                    //
                                    // Only nuke if we're COMPLETE and they're sending fresh keys
                                    // (meaning they lost their chains and need full re-key)
                                    if contact.clutch_state == ClutchState::Complete {
                                        crate::log(&format!(
                                            "CLUTCH: Re-key from {} - we're Complete, they have new keys, nuking for fresh ceremony",
                                            contact.handle
                                        ));
                                        // Full re-key: nuke everything
                                        contact.clutch_our_keypairs = None;
                                        contact.clutch_slots.clear();
                                        contact.ceremony_id = None;
                                        contact.offer_provenances.clear();
                                        contact.clutch_pending_kem = None;
                                        contact.clutch_offer_transfer_id = None;
                                        contact.clutch_state = ClutchState::Pending;
                                        contact.completed_their_hqc_prefix = None;
                                        if let Some(old_friendship_id) =
                                            contact.friendship_id.take()
                                        {
                                            crate::log(&format!(
                                                "CLUTCH: Invalidating old chains for {}",
                                                contact.handle
                                            ));
                                            chains_to_remove.push(old_friendship_id);
                                        }
                                        rekey_request =
                                            Some((contact.id.clone(), contact.handle_hash));
                                    } else {
                                        // Not Complete - just update their offer, don't regenerate our keys
                                        crate::log(&format!(
                                            "CLUTCH: {} sent new keys but we're mid-ceremony (state={:?}) - updating their offer, keeping our keys",
                                            contact.handle, contact.clutch_state
                                        ));
                                        // Clear their old offer data so we use the new one
                                        if let Some(slot) = contact.get_slot_mut(&their_handle_hash)
                                        {
                                            slot.offer = None;
                                            slot.kem_secrets_from_them = None;
                                        }
                                        // Clear our old KEM encap - it was for their OLD keys!
                                        // We need fresh encapsulation against their new pubkeys.
                                        if let Some(slot) = contact.get_slot_mut(&our_handle_hash) {
                                            slot.kem_secrets_to_them = None;
                                            slot.kem_response_for_resend = None;
                                        }
                                        contact.clutch_kem_encap_in_progress = false;
                                        // Clear ceremony_id so it gets recomputed with new provenance
                                        contact.ceremony_id = None;
                                        contact.offer_provenances.retain(|p| {
                                            // Keep our provenance, remove their old one
                                            // Our provenance is computed from our handle_hash
                                            // This is a bit hacky but works for 2-party
                                            p != &offer_provenance
                                        });
                                        // Don't trigger rekey_request - we keep our keys
                                    }
                                }
                            }
                            // No stored keys = fresh start, accept offer below

                            // Initialize slots if not already done
                            if contact.clutch_slots.is_empty() {
                                contact.init_clutch_slots(our_handle_hash);
                            }

                            // Store their offer in their slot
                            if let Some(slot) = contact.get_slot_mut(&their_handle_hash) {
                                slot.offer = Some(their_offer.clone());
                                crate::log(&format!(
                                    "CLUTCH: Stored offer from {} in slot",
                                    contact.handle
                                ));
                            }

                            // Store their offer_provenance for ceremony_id derivation
                            if !contact.offer_provenances.contains(&offer_provenance) {
                                contact.offer_provenances.push(offer_provenance);
                                crate::log(&format!(
                                    "CLUTCH: Stored offer_provenance from {} (now have {})",
                                    contact.handle,
                                    contact.offer_provenances.len()
                                ));
                            }

                            // Compute ceremony_id if we have all provenances (2 for DM)
                            let required_provenances = 2;
                            if contact.ceremony_id.is_none()
                                && contact.offer_provenances.len() >= required_provenances
                            {
                                use crate::types::CeremonyId;
                                let ceremony_id = *CeremonyId::derive(
                                    &[our_handle_hash, contact.handle_hash],
                                    &contact.offer_provenances,
                                )
                                .as_bytes();
                                contact.ceremony_id = Some(ceremony_id);
                                crate::log(&format!(
                                    "CLUTCH: Derived ceremony_id={}... from {} offer_provenances",
                                    hex::encode(&ceremony_id[..4]),
                                    contact.offer_provenances.len()
                                ));

                                // Process any pending KEM response that arrived before ceremony_id
                                if let Some(pending_kem) = contact.clutch_pending_kem.take() {
                                    crate::log(&format!(
                                        "CLUTCH: Processing queued KEM response from {} (ceremony_id now available)",
                                        contact.handle
                                    ));
                                    // Decapsulate remote KEM (remote encapsulated to local pubkeys)
                                    if let Some(ref local_keys) = contact.clutch_our_keypairs {
                                        let remote_secrets =
                                            ClutchKemSharedSecrets::decapsulate_from_peer(
                                                &pending_kem,
                                                local_keys,
                                            );
                                        // Store remote secrets in remote slot
                                        if let Some(remote_slot) =
                                            contact.get_slot_mut(&their_handle_hash)
                                        {
                                            remote_slot.kem_secrets_from_them =
                                                Some(remote_secrets);
                                            crate::log(&format!(
                                                "CLUTCH: Decapsulated queued KEM from {} - stored in slot",
                                                contact.handle
                                            ));
                                        }
                                    }
                                }
                            }

                            // Persist slot state (offer, provenances, ceremony_id)
                            let device_secret = *self.device_keypair.secret.as_bytes();
                            if let Err(e) = crate::storage::contacts::save_clutch_slots(
                                &contact.clutch_slots,
                                &contact.offer_provenances,
                                contact.ceremony_id,
                                contact.handle.as_str(),
                                &our_handle_hash,
                                &device_secret,
                            ) {
                                crate::log(&format!(
                                    "CLUTCH: Failed to save slots for {}: {}",
                                    contact.handle, e
                                ));
                            }

                            // If we have keypairs, send our offer (if not sent) and KEM response
                            if let Some(ref keypairs) = contact.clutch_our_keypairs {
                                // Compute conversation_token once for this contact
                                let conv_token = derive_conversation_token(&[
                                    our_handle_hash,
                                    contact.handle_hash,
                                ]);

                                // Send our offer if not already sent (PT will retry)
                                if contact.clutch_offer_transfer_id.is_none() {
                                    use crate::network::fgtw::protocol::build_clutch_offer_vsf;

                                    let our_offer = ClutchOfferPayload::from_keypairs(keypairs);

                                    // Build VSF and capture our offer_provenance
                                    match build_clutch_offer_vsf(
                                        &conv_token,
                                        &our_offer,
                                        self.device_keypair.public.as_bytes(),
                                        self.device_keypair.secret.as_bytes(),
                                    ) {
                                        Ok((vsf_bytes, our_offer_provenance)) => {
                                            // Store our offer provenance
                                            if !contact
                                                .offer_provenances
                                                .contains(&our_offer_provenance)
                                            {
                                                contact
                                                    .offer_provenances
                                                    .push(our_offer_provenance);
                                            }

                                            checker.send_offer(ClutchOfferRequest {
                                                peer_addr: sender_addr,
                                                vsf_bytes,
                                            });
                                            contact.clutch_offer_transfer_id = Some(0);
                                            // Store local offer in local slot too
                                            if let Some(local_slot) =
                                                contact.get_slot_mut(&our_handle_hash)
                                            {
                                                local_slot.offer = Some(our_offer);
                                            }
                                            crate::log(&format!(
                                                "CLUTCH: Sent full offer to {} (prov={}...)",
                                                contact.handle,
                                                hex::encode(&our_offer_provenance[..4])
                                            ));

                                            // Compute ceremony_id now that we have both provenances
                                            if contact.ceremony_id.is_none()
                                                && contact.offer_provenances.len()
                                                    >= required_provenances
                                            {
                                                use crate::types::CeremonyId;
                                                let ceremony_id = *CeremonyId::derive(
                                                    &[our_handle_hash, contact.handle_hash],
                                                    &contact.offer_provenances,
                                                )
                                                .as_bytes();
                                                contact.ceremony_id = Some(ceremony_id);
                                                crate::log(&format!(
                                                    "CLUTCH: Derived ceremony_id={}... after sending offer",
                                                    hex::encode(&ceremony_id[..4])
                                                ));
                                            }

                                            // Persist provenance/ceremony_id immediately
                                            let device_secret =
                                                *self.device_keypair.secret.as_bytes();
                                            if let Err(e) =
                                                crate::storage::contacts::save_clutch_slots(
                                                    &contact.clutch_slots,
                                                    &contact.offer_provenances,
                                                    contact.ceremony_id,
                                                    contact.handle.as_str(),
                                                    &our_handle_hash,
                                                    &device_secret,
                                                )
                                            {
                                                crate::log(&format!(
                                                    "Failed to persist CLUTCH provenance: {}",
                                                    e
                                                ));
                                            }
                                        }
                                        Err(e) => {
                                            crate::log(&format!(
                                                "CLUTCH: Failed to build offer VSF: {}",
                                                e
                                            ));
                                        }
                                    }
                                }

                                // Send KEM response (encapsulate to remote pubkeys)
                                // Check if we haven't already sent (kem_secrets_to_them in local slot)
                                // KEM response requires ceremony_id (for wire format verification)
                                let already_sent_kem = contact
                                    .get_slot(&our_handle_hash)
                                    .map(|s| s.kem_secrets_to_them.is_some())
                                    .unwrap_or(false);

                                // Check for re-send case: we have stored payload from previous send
                                let resend_payload = contact
                                    .get_slot(&our_handle_hash)
                                    .and_then(|s| s.kem_response_for_resend.clone());

                                if let Some(kem_response) = resend_payload {
                                    // Re-send using stored payload
                                    if let Some(ceremony_id) = contact.ceremony_id {
                                        use crate::network::status::ClutchKemResponseRequest;

                                        checker.send_kem_response(ClutchKemResponseRequest {
                                            peer_addr: sender_addr,
                                            conversation_token: conv_token,
                                            ceremony_id,
                                            payload: kem_response,
                                            device_pubkey: *self.device_keypair.public.as_bytes(),
                                            device_secret: *self.device_keypair.secret.as_bytes(),
                                        });
                                        crate::log(&format!(
                                            "CLUTCH: Re-sent KEM response to {}",
                                            contact.handle
                                        ));
                                    }
                                } else if !already_sent_kem && !contact.clutch_kem_encap_in_progress
                                {
                                    if let Some(ceremony_id) = contact.ceremony_id {
                                        // Defer spawn for KEM encapsulation (to avoid borrow conflict)
                                        // (PQ crypto is slow ~800ms, would block UI/network)
                                        contact.clutch_kem_encap_in_progress = true;
                                        kem_encap_spawn = Some((
                                            contact.id.clone(),
                                            their_offer.clone(),
                                            ceremony_id,
                                            conv_token,
                                            sender_addr,
                                        ));
                                        crate::log(&format!(
                                            "CLUTCH: Will spawn KEM encapsulation for {}",
                                            contact.handle
                                        ));
                                        changed = true;
                                    } else {
                                        crate::log(&format!(
                                            "CLUTCH: Deferring KEM response to {} - waiting for ceremony_id",
                                            contact.handle
                                        ));
                                    }
                                }

                                // Check if ceremony is complete (defer to after outer loop)
                                if contact.all_slots_complete() {
                                    ceremony_completions.push(idx);
                                    changed = true;
                                }
                            } else if contact.clutch_our_keypairs.is_none() {
                                if contact.clutch_keygen_in_progress {
                                    // Keygen already running - don't spawn another
                                    crate::log(&format!(
                                        "CLUTCH: Received offer from {} but keygen already in progress - waiting",
                                        contact.handle
                                    ));
                                } else {
                                    // No keypairs - need to respond (whether Complete or not)
                                    // If Complete: peer lost their chains, accept re-key
                                    // If not Complete: restart mid-ceremony or fresh re-key
                                    if contact.clutch_state == ClutchState::Complete {
                                        // Peer is sending an offer while we think we're Complete.
                                        // This means either:
                                        // 1. Same HQC prefix: peer missed our KEM response (can't re-send without keypairs)
                                        // 2. Different HQC prefix: peer lost chains, wants re-key
                                        //
                                        // Since we have NO keypairs here (we're in the is_none branch),
                                        // we can't re-respond even to the same offer. Accept as re-key.
                                        //
                                        // Note: If peer keeps re-sending same offer, both sides will eventually
                                        // converge on a fresh ceremony (peer will regenerate keys after timeout).
                                        crate::log(&format!(
                                            "CLUTCH: Received offer from {} while Complete - peer lost chains, accepting re-key",
                                            contact.handle
                                        ));
                                        // Delete our old chains - they're useless now
                                        if let Some(fid) = contact.friendship_id {
                                            chains_to_remove.push(fid);
                                        }
                                        // Reset ALL CLUTCH state for new ceremony
                                        contact.clutch_state = ClutchState::Pending;
                                        contact.friendship_id = None;
                                        contact.completed_their_hqc_prefix = None;
                                        contact.clutch_our_keypairs = None;
                                        contact.clutch_slots.clear();
                                        contact.ceremony_id = None;
                                        contact.offer_provenances.clear(); // Clear for fresh ceremony nonce
                                        contact.clutch_pending_kem = None;
                                        contact.clutch_offer_transfer_id = None;
                                        contact.clutch_our_eggs_proof = None;
                                        contact.clutch_their_eggs_proof = None;
                                        // Re-initialize slots and store their offer (was stored earlier but we just cleared)
                                        contact.init_clutch_slots(our_handle_hash);
                                        if let Some(slot) = contact.get_slot_mut(&their_handle_hash)
                                        {
                                            slot.offer = Some(their_offer.clone());
                                        }
                                        // Store their offer_provenance (was cleared, need to re-add)
                                        if !contact.offer_provenances.contains(&offer_provenance) {
                                            contact.offer_provenances.push(offer_provenance);
                                        }

                                        // Persist re-key state immediately
                                        let device_secret = *self.device_keypair.secret.as_bytes();
                                        if let Err(e) = crate::storage::contacts::save_clutch_slots(
                                            &contact.clutch_slots,
                                            &contact.offer_provenances,
                                            contact.ceremony_id,
                                            contact.handle.as_str(),
                                            &our_handle_hash,
                                            &device_secret,
                                        ) {
                                            crate::log(&format!(
                                                "Failed to persist re-key CLUTCH state: {}",
                                                e
                                            ));
                                        }

                                        // Trigger keygen for fresh re-key ceremony
                                        contact.clutch_keygen_in_progress = true;
                                        rekey_request =
                                            Some((contact.id.clone(), contact.handle_hash));
                                    } else if contact.clutch_state == ClutchState::AwaitingProof {
                                        // We're waiting for their proof, but they sent an offer.
                                        // Check if same keys (retransmit) or different (peer reset)
                                        let their_slot = contact.get_slot(&their_handle_hash);
                                        let stored_hqc = their_slot
                                            .and_then(|s| s.offer.as_ref())
                                            .map(|o| &o.hqc256_public);
                                        let is_same_keys = stored_hqc
                                            .map(|h| h == &their_offer.hqc256_public)
                                            .unwrap_or(false);

                                        if is_same_keys {
                                            crate::log(&format!(
                                                "CLUTCH: Ignoring retransmit from {} (already AwaitingProof)",
                                                contact.handle
                                            ));
                                            break;
                                        }

                                        // Different keys = peer reset. Clear their slot and reset to Pending.
                                        crate::log(&format!(
                                            "CLUTCH: Peer {} reset while we were AwaitingProof - resetting",
                                            contact.handle
                                        ));
                                        if let Some(slot) = contact.get_slot_mut(&their_handle_hash) {
                                            slot.offer = None;
                                            slot.kem_secrets_from_them = None;
                                        }
                                        contact.clutch_state = ClutchState::Pending;
                                        contact.clutch_offer_transfer_id = None;
                                        contact.ceremony_id = None;
                                        contact.clutch_our_eggs_proof = None;
                                        contact.clutch_their_eggs_proof = None;
                                        // Remove their old provenance (keep ours)
                                        contact.offer_provenances.retain(|p| p != &offer_provenance);
                                        // Fall through - normal flow will store new offer and trigger keygen
                                    } else {
                                        crate::log(&format!(
                                            "CLUTCH: Received offer from {} but no keypairs (state={:?}) - triggering keygen",
                                            contact.handle, contact.clutch_state
                                        ));
                                        contact.clutch_keygen_in_progress = true;
                                        rekey_request =
                                            Some((contact.id.clone(), contact.handle_hash));
                                    }
                                }
                            }
                            break;
                        }
                    }

                    // Remove invalidated chains from memory and disk
                    for old_id in chains_to_remove {
                        self.friendship_chains.retain(|(id, _)| *id != old_id);
                        // Delete from disk
                        if let Err(e) =
                            crate::storage::friendship::delete_friendship_chains(&old_id)
                        {
                            crate::log(&format!("CLUTCH: Failed to delete old chains: {}", e));
                        }
                    }

                    // Spawn re-key keygen after releasing mutable borrow
                    if let Some((contact_id, their_handle_hash)) = rekey_request {
                        self.spawn_clutch_keygen(contact_id, our_identity_seed, their_handle_hash);
                    }

                    // Spawn deferred KEM encapsulation after releasing mutable borrow
                    if let Some((contact_id, offer, ceremony_id, conv_token, peer_addr)) =
                        kem_encap_spawn
                    {
                        self.spawn_clutch_kem_encap(
                            contact_id,
                            offer,
                            ceremony_id,
                            conv_token,
                            peer_addr,
                        );
                    }
                }

                // CLUTCH KEM response received (~31KB with 4 ciphertexts)
                // Payload is already parsed and signature verified by status.rs
                StatusUpdate::ClutchKemResponseReceived {
                    conversation_token,
                    ceremony_id: received_ceremony_id,
                    sender_pubkey,
                    payload,
                    sender_addr: raw_sender_addr,
                } => {
                    use crate::crypto::clutch::{
                        derive_conversation_token, ClutchKemSharedSecrets,
                    };

                    // Normalize to port 4383 (TCP source port is ephemeral)
                    let sender_addr =
                        std::net::SocketAddr::new(raw_sender_addr.ip(), crate::PHOTON_PORT);

                    // Get our handle_hash
                    let our_handle_hash = match self.user_identity_seed {
                        Some(h) => h,
                        None => {
                            #[cfg(feature = "development")]
                            #[cfg(feature = "development")]
                            crate::log("CLUTCH: No user_identity_seed available");
                            continue;
                        }
                    };

                    // Find contact by conversation_token
                    let their_handle_hash = match self
                        .contacts
                        .iter()
                        .find(|c| {
                            derive_conversation_token(&[our_handle_hash, c.handle_hash])
                                == conversation_token
                        })
                        .map(|c| c.handle_hash)
                    {
                        Some(h) => h,
                        None => {
                            crate::log(&format!(
                                "CLUTCH: Received KEM response with unknown conversation_token {}",
                                hex::encode(&conversation_token[..8])
                            ));
                            continue;
                        }
                    };

                    crate::log(&format!(
                        "CLUTCH: Received KEM response (VSF verified) from {} tok={}...",
                        sender_addr,
                        hex::encode(&conversation_token[..8])
                    ));

                    // Verify sender's device pubkey matches the contact's known identity
                    let contact_pubkey = self
                        .contacts
                        .iter()
                        .find(|c| c.handle_hash == their_handle_hash)
                        .map(|c| c.public_identity.key);

                    match contact_pubkey {
                        None => {
                            #[cfg(feature = "development")]
                            #[cfg(feature = "development")]
                            crate::log("CLUTCH: Received KEM response from unknown contact");
                            continue;
                        }
                        Some(expected) if expected != sender_pubkey => {
                            crate::log(&format!(
                                "CLUTCH: KEM device pubkey mismatch! Expected {}, got {}",
                                hex::encode(&expected[..8]),
                                hex::encode(&sender_pubkey[..8])
                            ));
                            continue;
                        }
                        Some(_) => {} // Match - proceed
                    }

                    // The payload is already parsed
                    let their_kem = payload;

                    // Find contact by handle_hash
                    for (idx, contact) in self.contacts.iter_mut().enumerate() {
                        if contact.handle_hash == their_handle_hash {
                            contact.ip = Some(sender_addr);

                            // Verify ceremony_id matches (if we have one)
                            if let Some(our_ceremony_id) = contact.ceremony_id {
                                if received_ceremony_id != our_ceremony_id {
                                    crate::log(&format!(
                                        "CLUTCH: ceremony_id mismatch! Received {:02x}{:02x}..., expected {:02x}{:02x}...",
                                        received_ceremony_id[0], received_ceremony_id[1],
                                        our_ceremony_id[0], our_ceremony_id[1]
                                    ));
                                    continue;
                                }
                            } else {
                                // No ceremony_id yet - check if we have keypairs and if KEM targets them
                                // This happens when keypairs are loaded from disk but offers not yet exchanged
                                if let Some(ref our_keys) = contact.clutch_our_keypairs {
                                    let our_hqc_prefix: [u8; 8] =
                                        our_keys.hqc256_public[..8].try_into().unwrap();
                                    let all_zeros = their_kem.target_hqc_pub_prefix == [0u8; 8];
                                    if !all_zeros
                                        && their_kem.target_hqc_pub_prefix != our_hqc_prefix
                                    {
                                        // KEM targets different keys - truly stale, discard
                                        crate::log(&format!(
                                            "CLUTCH: KEM response from {} targets old keys (HQC {}) - discarding",
                                            contact.handle,
                                            hex::encode(&their_kem.target_hqc_pub_prefix)
                                        ));
                                        break;
                                    }
                                    // KEM targets our current keypairs - queue it for processing when ceremony_id arrives
                                    crate::log(&format!(
                                        "CLUTCH: KEM response from {} arrived before ceremony_id - queuing for later",
                                        contact.handle
                                    ));
                                    contact.clutch_pending_kem = Some(their_kem.clone());
                                    break;
                                } else {
                                    // No keypairs at all - stale KEM encrypted to unknown keys
                                    crate::log(&format!(
                                        "CLUTCH: KEM response from {} arrived before keygen - discarding (encrypted to old keys)",
                                        contact.handle
                                    ));
                                    break;
                                }
                            }

                            // Initialize slots if needed
                            if contact.clutch_slots.is_empty() {
                                contact.init_clutch_slots(our_handle_hash);
                            }

                            // Verify KEM response targets our CURRENT HQC public key
                            // This prevents panics from stale KEM responses encrypted to old keys
                            if let Some(ref our_keys) = contact.clutch_our_keypairs {
                                let our_hqc_prefix: [u8; 8] =
                                    our_keys.hqc256_public[..8].try_into().unwrap();
                                let all_zeros = their_kem.target_hqc_pub_prefix == [0u8; 8];
                                if !all_zeros && their_kem.target_hqc_pub_prefix != our_hqc_prefix {
                                    crate::log(&format!(
                                        "CLUTCH: Stale KEM response from {} - target HQC {} != our HQC {} (discarding)",
                                        contact.handle,
                                        hex::encode(&their_kem.target_hqc_pub_prefix),
                                        hex::encode(&our_hqc_prefix)
                                    ));
                                    break;
                                }
                            }

                            // Decapsulate remote KEM response using local secret keys
                            if let Some(ref local_keys) = contact.clutch_our_keypairs {
                                let remote_secrets = ClutchKemSharedSecrets::decapsulate_from_peer(
                                    &their_kem, local_keys,
                                );

                                // Store in remote slot (secrets from remote to local)
                                if let Some(slot) = contact.get_slot_mut(&their_handle_hash) {
                                    slot.kem_secrets_from_them = Some(remote_secrets);
                                    crate::log(&format!(
                                        "CLUTCH: Decapsulated KEM from {} - stored in slot",
                                        contact.handle
                                    ));
                                }

                                // Persist slot state after receiving KEM
                                let device_secret = *self.device_keypair.secret.as_bytes();
                                if let Err(e) = crate::storage::contacts::save_clutch_slots(
                                    &contact.clutch_slots,
                                    &contact.offer_provenances,
                                    contact.ceremony_id,
                                    contact.handle.as_str(),
                                    &our_handle_hash,
                                    &device_secret,
                                ) {
                                    crate::log(&format!(
                                        "CLUTCH: Failed to save slots for {}: {}",
                                        contact.handle, e
                                    ));
                                }
                                changed = true;

                                // Check if ceremony is complete (defer to after outer loop)
                                if contact.all_slots_complete() {
                                    ceremony_completions.push(idx);
                                    changed = true;
                                } else {
                                    // Debug: why isn't ceremony complete after KEM response?
                                    crate::log(&format!(
                                        "CLUTCH: Slots not complete after KEM response for {} - checking state:",
                                        contact.handle
                                    ));
                                    for (i, slot) in contact.clutch_slots.iter().enumerate() {
                                        crate::log(&format!(
                                            "  Slot {}: offer={} from_them={} to_them={}",
                                            i,
                                            slot.offer.is_some(),
                                            slot.kem_secrets_from_them.is_some(),
                                            slot.kem_secrets_to_them.is_some()
                                        ));
                                    }
                                }
                            } else {
                                crate::log(&format!(
                                    "CLUTCH: Received KEM response but no keypairs for {}",
                                    contact.handle
                                ));
                            }
                            break;
                        }
                    }
                }

                // CLUTCH complete proof received (~200 bytes with eggs_proof)
                // Both parties exchange this to verify they derived identical eggs
                StatusUpdate::ClutchCompleteReceived {
                    conversation_token,
                    ceremony_id: _received_ceremony_id,
                    sender_pubkey,
                    payload,
                    sender_addr: raw_sender_addr,
                } => {
                    use crate::crypto::clutch::derive_conversation_token;
                    use crate::types::ClutchState;

                    // Normalize to port 4383 (TCP source port is ephemeral)
                    let sender_addr =
                        std::net::SocketAddr::new(raw_sender_addr.ip(), crate::PHOTON_PORT);

                    crate::log(&format!(
                        "CLUTCH: Received complete proof (VSF verified) from {} proof={}...",
                        sender_addr,
                        hex::encode(&payload.eggs_proof[..8])
                    ));

                    // Find contact by conversation_token
                    let their_handle_hash = match self
                        .contacts
                        .iter()
                        .find(|c| {
                            derive_conversation_token(&[our_handle_hash, c.handle_hash])
                                == conversation_token
                        })
                        .map(|c| c.handle_hash)
                    {
                        Some(h) => h,
                        None => {
                            crate::log(&format!(
                                "CLUTCH: Received complete proof with unknown conversation_token {}",
                                hex::encode(&conversation_token[..8])
                            ));
                            continue;
                        }
                    };

                    // Verify sender's device pubkey matches the contact's known identity
                    let contact_pubkey = self
                        .contacts
                        .iter()
                        .find(|c| c.handle_hash == their_handle_hash)
                        .map(|c| c.public_identity.key);

                    match contact_pubkey {
                        None => {
                            #[cfg(feature = "development")]
                            #[cfg(feature = "development")]
                            crate::log("CLUTCH: Received proof from unknown contact");
                            continue;
                        }
                        Some(expected) if expected != sender_pubkey => {
                            crate::log(&format!(
                                "CLUTCH: Proof device pubkey mismatch! Expected {}, got {}",
                                hex::encode(&expected[..8]),
                                hex::encode(&sender_pubkey[..8])
                            ));
                            continue;
                        }
                        Some(_) => {} // Match - proceed
                    }

                    // Find contact and process proof
                    for contact in &mut self.contacts {
                        if contact.handle_hash == their_handle_hash {
                            contact.ip = Some(sender_addr);

                            match contact.clutch_state {
                                ClutchState::AwaitingProof => {
                                    // We have our proof - verify theirs matches
                                    if let Some(our_proof) = contact.clutch_our_eggs_proof {
                                        if payload.eggs_proof == our_proof {
                                            // SUCCESS! Both parties computed same eggs
                                            crate::log(&format!(
                                                "CLUTCH: Proof verified with {}! ✓ proof={}...",
                                                contact.handle,
                                                hex::encode(&our_proof[..8])
                                            ));
                                            contact.clutch_state = ClutchState::Complete;
                                            self.window_dirty = true; // Force UI redraw to show chat textbox
                                            // Store their HQC pub prefix to detect stale offers after restart
                                            if let Some(their_slot) =
                                                contact.get_slot(&contact.handle_hash)
                                            {
                                                if let Some(ref their_offer) = their_slot.offer {
                                                    let prefix: [u8; 8] = their_offer.hqc256_public
                                                        [..8]
                                                        .try_into()
                                                        .unwrap_or_default();
                                                    contact.completed_their_hqc_prefix =
                                                        Some(prefix);
                                                }
                                            }
                                            contact.clutch_our_eggs_proof = None; // Clean up
                                            contact.clutch_their_eggs_proof = None;
                                            changed = true;

                                            // NOTE: Don't clear PT sends here - our ClutchComplete
                                            // proof might still be in flight to them. Let it finish.

                                            // Save Complete state to disk immediately
                                            if let Some(identity_seed) =
                                                self.user_identity_seed.as_ref()
                                            {
                                                let device_secret =
                                                    self.device_keypair.secret.as_bytes();
                                                if let Err(e) =
                                                    crate::storage::contacts::save_contact(
                                                        contact,
                                                        identity_seed,
                                                        device_secret,
                                                    )
                                                {
                                                    crate::log(&format!(
                                                        "Failed to save Complete state: {}",
                                                        e
                                                    ));
                                                } else {
                                                    crate::log(&format!(
                                                        "CLUTCH: Saved {} Complete state to disk",
                                                        contact.handle
                                                    ));
                                                }
                                            }
                                        } else {
                                            // CRYPTOGRAPHIC FAILURE - proofs don't match!
                                            // This should NEVER happen unless:
                                            // 1. MITM attack
                                            // 2. Bug in ceremony
                                            // 3. Corruption
                                            let our_hex = hex::encode(&our_proof);
                                            let their_hex = hex::encode(&payload.eggs_proof);
                                            crate::log(&format!(
                                                "CLUTCH PROOF MISMATCH! This is a critical error.\n\
                                                Our proof:   {}\n\
                                                Their proof: {}",
                                                our_hex, their_hex
                                            ));
                                            panic!(
                                                "CLUTCH PROOF MISMATCH with {}! \
                                                This indicates MITM, bug, or corruption. \
                                                Our: {}... Their: {}...",
                                                contact.handle,
                                                &our_hex[..16],
                                                &their_hex[..16]
                                            );
                                        }
                                    } else {
                                        // Race condition: proof arrived before check_clutch_ceremonies
                                        // processed our ceremony result. Store theirs for when we're ready.
                                        crate::log(&format!(
                                            "CLUTCH: Storing early proof from {} (AwaitingProof but our result not processed yet)",
                                            contact.handle
                                        ));
                                        contact.clutch_their_eggs_proof = Some(payload.eggs_proof);
                                        changed = true;
                                    }
                                }
                                ClutchState::Pending => {
                                    // We haven't computed our proof yet - store theirs for later
                                    crate::log(&format!(
                                        "CLUTCH: Storing early proof from {} (we're still in Pending)",
                                        contact.handle
                                    ));
                                    contact.clutch_their_eggs_proof = Some(payload.eggs_proof);
                                    changed = true;
                                }
                                ClutchState::Complete => {
                                    // Already complete - ignore duplicate
                                    crate::log(&format!(
                                        "CLUTCH: Ignoring duplicate proof from {} (already Complete)",
                                        contact.handle
                                    ));
                                }
                            }
                            break;
                        }
                    }
                }

                // LAN peer discovered via broadcast (NAT hairpinning workaround)
                StatusUpdate::LanPeerDiscovered {
                    handle_proof,
                    local_ip,
                    port,
                } => {
                    // Find contact by handle_proof and store their LAN IP + port
                    for contact in &mut self.contacts {
                        if contact.handle_proof == handle_proof {
                            let old_local = contact.local_ip;
                            let old_port = contact.local_port;
                            contact.local_ip = Some(local_ip);
                            contact.local_port = Some(port);
                            if old_local != Some(local_ip) || old_port != Some(port) {
                                crate::log(&format!(
                                    "LAN: Discovered {} at local {}:{}",
                                    contact.handle, local_ip, port
                                ));
                                changed = true;
                            }
                            break;
                        }
                    }
                }
            }
        }

        // Process deferred ceremony completions (after releasing checker borrow)
        for idx in ceremony_completions {
            self.complete_clutch_ceremony_by_idx(idx, our_handle_hash);
            changed = true;
        }

        // Retransmit pending messages to contacts that just came online
        // Use last_received_ef6 from pong to only retransmit messages they don't have
        for (fid, peer_addr, handle, recipient_pubkey, last_received_ef6) in retransmit_requests {
            if let Some((_, chains)) = self.friendship_chains.iter().find(|(id, _)| *id == fid) {
                let pending = chains.pending_messages();
                if !pending.is_empty() {
                    // Filter to only messages newer than what peer has received
                    let to_retransmit: Vec<_> = pending
                        .iter()
                        .filter(|msg| {
                            if let Some(their_last) = last_received_ef6 {
                                msg.eagle_time > their_last
                            } else {
                                // No sync info from peer - retransmit all
                                true
                            }
                        })
                        .collect();

                    if !to_retransmit.is_empty() {
                        crate::log(&format!(
                            "CHAT: Retransmitting {} of {} pending message(s) to {} (came online, last_received={:?})",
                            to_retransmit.len(),
                            pending.len(),
                            handle,
                            last_received_ef6
                        ));
                        let conversation_token = chains.conversation_token;
                        for msg in to_retransmit {
                            if let Some(ref checker) = self.status_checker {
                                checker.send_message(crate::network::status::MessageRequest {
                                    peer_addr,
                                    recipient_pubkey,
                                    conversation_token,
                                    prev_msg_hp: msg.prev_msg_hp,
                                    ciphertext: msg.ciphertext.clone(),
                                    eagle_time: msg.eagle_time,
                                });
                                crate::log(&format!(
                                    "CHAT: Retransmitted msg with eagle_time {} to {}",
                                    msg.eagle_time, handle
                                ));
                            }
                        }
                    } else if !pending.is_empty() {
                        crate::log(&format!(
                            "CHAT: {} pending messages but peer already has them (last_received={:?})",
                            pending.len(), last_received_ef6
                        ));
                    }
                }
            }
        }

        // NOTE: Proactive CLUTCH initiation is now handled via background keygen:
        // 1. spawn_clutch_keygen() is called when contact is added (background thread)
        // 2. check_clutch_keygens() processes results, stores keypairs + ceremony_id
        // 3. Offers are sent from check_clutch_keygens or the KeysGenerated handler above
        // This avoids UI freeze from synchronous McEliece keygen (~100ms) and handle_proof (~1s)

        // Update sync records if any messages were received (for pong responses)
        if need_sync_update {
            self.update_sync_records();
        }

        changed
    }

    /// Send a message to the currently selected contact
    /// Returns true if message was sent successfully
    pub fn send_message_to_selected_contact(&mut self, message_text: &str) -> bool {
        use crate::network::status::MessageRequest;
        use crate::types::ChatMessage;

        // Get selected contact
        let contact_idx = match self.selected_contact {
            Some(idx) => idx,
            None => return false,
        };

        // Get contact info we need
        let (friendship_id, _our_handle_hash, ip, recipient_pubkey) = {
            let contact = match self.contacts.get(contact_idx) {
                Some(c) => c,
                None => return false,
            };

            // Must have completed CLUTCH
            if contact.clutch_state != crate::types::ClutchState::Complete {
                crate::log(&format!(
                    "Cannot send to {}: CLUTCH not complete",
                    contact.handle
                ));
                return false;
            }

            // Must have friendship chains initialized
            let friendship_id = match &contact.friendship_id {
                Some(id) => *id,
                None => {
                    crate::log(&format!(
                        "Cannot send to {}: chains not initialized (CLUTCH incomplete)",
                        contact.handle
                    ));
                    return false;
                }
            };

            // Prefer local_ip for same-LAN peers (NAT hairpinning workaround)
            let ip = if let (Some(local_ip), Some(local_port)) =
                (contact.local_ip, contact.local_port)
            {
                // Use local IP + port from LAN discovery
                std::net::SocketAddr::new(std::net::IpAddr::V4(local_ip), local_port)
            } else {
                match contact.ip {
                    Some(ip) => ip,
                    None => {
                        crate::log(&format!("Cannot send to {}: no IP", contact.handle));
                        return false;
                    }
                }
            };

            (friendship_id, contact.handle_hash, ip, *contact.public_identity.as_bytes())
        };

        // Get our identity seed (handle_hash) for chain lookup
        let our_identity_seed = match &self.user_identity_seed {
            Some(seed) => *seed, // Already hashed
            None => return false,
        };

        // Get the friendship chains (linear search, ~50 friendships max)
        let chains = match self
            .friendship_chains
            .iter()
            .find(|(id, _)| *id == friendship_id)
        {
            Some((_, c)) => c,
            None => {
                crate::log("Friendship chains not found in memory");
                return false;
            }
        };

        // Get our chain and encrypt (sender uses their own chain)
        let chain = match chains.chain(&our_identity_seed) {
            Some(c) => c,
            None => {
                crate::log("Our chain not found in friendship");
                return false;
            }
        };

        // Encrypt the message using new CHAIN protocol
        use crate::crypto::chain::{derive_salt, encrypt_layers, generate_scratch};

        // Get the hp of their last message we received (for bidirectional weave)
        let incorporated_hp: [u8; 32] = chains
            .other_participant(&our_identity_seed)
            .and_then(|their_hash| chains.last_received_hash(their_hash).copied())
            .unwrap_or([0u8; 32]);

        // Build payload as VSF field: (d{message}:x{text},hp{inc_hp},hR{pad})
        // This is VSF-spec compliant with type-marker parsing (not positional)
        use vsf::schema::section::FieldValue;

        let mut values = vec![
            vsf::VsfType::x(message_text.to_string()),
            vsf::VsfType::hp(incorporated_hp.to_vec()),
        ];

        // Add random padding for traffic analysis resistance
        // Length = min of 3 random u8s → biased toward short (median ~53 bytes)
        let pad_len = rand::random::<u8>()
            .min(rand::random::<u8>())
            .min(rand::random::<u8>()) as usize;
        if pad_len > 0 {
            let random_bytes: Vec<u8> = (0..pad_len).map(|_| rand::random()).collect();
            values.push(vsf::VsfType::hR(random_bytes));
        }

        // Shuffle field order - enforces type-marker parsing (VSF-spec compliant)
        use rand::seq::SliceRandom;
        values.shuffle(&mut rand::thread_rng());

        // Build the field and flatten to bytes
        let field = FieldValue::new("message", values);
        let payload = field.flatten();

        let eagle_time = vsf::datetime_to_eagle_time(chrono::Utc::now());

        // Derive salt from previous plaintext (use tracked plaintext from chains)
        let prev_plaintext = chains.current_send_plaintext(&our_identity_seed);
        let salt = derive_salt(prev_plaintext, chain);

        // Generate memory-hard scratch pad
        let scratch = generate_scratch(chain, &salt);

        // DEBUG: Log encryption parameters
        crate::log(&format!(
            "CHAIN ENCRYPT: our_handle_hash={}..., key={}..., salt={}..., eagle_time={}, payload_len={}",
            hex::encode(&our_identity_seed[..4]),
            hex::encode(&chain.current_key()[..4]),
            hex::encode(&salt[..4]),
            eagle_time.to_f64(),
            payload.len()
        ));

        // Encrypt using 3-layer encryption
        let ciphertext = encrypt_layers(&payload, chain, &scratch, &eagle_time);

        // Compute hash chain pointers using proper derivation
        let plaintext_hash = *blake3::hash(&payload).as_bytes();
        let prev_msg_hp = chains
            .get_prev_msg_hp(&our_identity_seed)
            .unwrap_or_else(|| {
                // Fallback: derive anchor manually (shouldn't happen)
                let mut hasher = blake3::Hasher::new();
                hasher.update(friendship_id.as_bytes());
                hasher.update(b"first");
                *hasher.finalize().as_bytes()
            });

        // Derive this message's hash pointer (links to prev + content + time)
        use crate::types::friendship::derive_msg_hp;
        let msg_hp = derive_msg_hp(&prev_msg_hp, &plaintext_hash, eagle_time.to_f64());

        // Capture conversation_token before mutable borrow
        let conversation_token = chains.conversation_token;

        // CRASH SAFETY: Persist to disk BEFORE sending to network
        // If we crash after network but before disk, we desync permanently.
        // Disk write is the commit point - network is just notification.

        // Track pending message for ACK matching and resync capability
        if let Some((_, chains_mut)) = self
            .friendship_chains
            .iter_mut()
            .find(|(id, _)| *id == friendship_id)
        {
            chains_mut.add_pending(
                eagle_time.to_f64(),
                payload.to_vec(),
                plaintext_hash,
                prev_msg_hp,
                msg_hp,
                ciphertext.clone(),
            );

            // Track sent weave for bidirectional entropy (receiver uses this to advance our chain)
            chains_mut.update_sent_for_mixing(eagle_time.to_f64(), msg_hp, &payload);

            // Get other party's last plaintext for bidirectional weave
            // Clone to avoid borrow issues with advance()
            let their_plaintext: Option<Vec<u8>> = chains_mut
                .other_participant(&our_identity_seed)
                .map(|their_hash| chains_mut.last_plaintext(their_hash).to_vec());

            // Advance our chain immediately after sending, weaving in their last message
            chains_mut.advance(
                &our_identity_seed,
                &eagle_time,
                &payload,
                their_plaintext.as_deref(),
            );

            // *** PERSIST to disk FIRST - this is the commit point ***
            if let Some(ref identity_seed) = self.user_identity_seed {
                let device_secret = self.device_keypair.secret.as_bytes();
                if let Err(e) = crate::storage::friendship::save_friendship_chains(
                    chains_mut,
                    identity_seed,
                    device_secret,
                ) {
                    crate::log(&format!("STORAGE: Failed to save chains after send: {}", e));
                }
            }
        }

        // *** THEN send via network - if we crash here, we can retransmit on restart ***
        if let Some(ref checker) = self.status_checker {
            checker.send_message(MessageRequest {
                peer_addr: ip,
                recipient_pubkey,
                conversation_token,
                prev_msg_hp,
                ciphertext: ciphertext.clone(),
                eagle_time: eagle_time.to_f64(),
            });

            // Add to contact's message list and persist
            if let Some(contact) = self.contacts.get_mut(contact_idx) {
                // Use actual eagle_time and sorted insert for correct chronological order
                contact.insert_message_sorted(ChatMessage::new_with_timestamp(
                    message_text.to_string(),
                    true,                // is_outgoing
                    eagle_time.to_f64(), // Use message's actual eagle_time
                ));
                // Auto-scroll to bottom to show new message
                contact.message_scroll_offset = 0.0;

                // Persist immediately (AGENT.md: every change hits disk)
                if let Some(ref identity_seed) = self.user_identity_seed {
                    let device_secret = self.device_keypair.secret.as_bytes();
                    if let Err(e) = crate::storage::contacts::save_messages(
                        contact,
                        identity_seed,
                        device_secret,
                    ) {
                        crate::log(&format!("STORAGE: Failed to save messages: {}", e));
                    }
                }
            }

            crate::log(&format!(
                "CHAT: Sent message (fid {}...) to {}",
                hex::encode(&friendship_id.as_bytes()[..4]),
                ip
            ));
            return true;
        }

        false
    }

    /// Check for completed avatar downloads and update contacts or user avatar
    /// Returns true if any avatars were updated
    pub fn check_avatar_downloads(&mut self) -> bool {
        let mut changed = false;
        while let Ok(result) = self.contact_avatar_rx.try_recv() {
            // Check if this is the user's own avatar
            if let Some(ref user_handle) = self.user_handle {
                if user_handle == &result.handle {
                    if let Some(pixels) = &result.pixels {
                        let display_pixels =
                            crate::display_profile::DisplayConverter::new().convert_avatar(pixels);
                        self.avatar_pixels = Some(display_pixels.clone());
                        self.avatar_scaled = None; // Force re-scale
                        changed = true;
                        crate::log(&format!(
                            "Avatar: User avatar loaded ({} bytes)",
                            pixels.len()
                        ));
                        // Also update the contact entry for self (if user added themselves)
                        for contact in &mut self.contacts {
                            if contact.handle.as_str() == result.handle {
                                contact.avatar_pixels = Some(display_pixels.clone());
                                contact.avatar_scaled = None;
                                contact.avatar_scaled_diameter = 0;
                                break;
                            }
                        }
                    }
                    continue;
                }
            }

            // Find matching contact by handle
            for contact in &mut self.contacts {
                if contact.handle.as_str() == result.handle {
                    if let Some(pixels) = result.pixels {
                        // Convert to display colorspace and store
                        let display_pixels =
                            crate::display_profile::DisplayConverter::new().convert_avatar(&pixels);
                        contact.avatar_pixels = Some(display_pixels);
                        contact.avatar_scaled = None; // Invalidate scaled cache
                        contact.avatar_scaled_diameter = 0;
                        changed = true;
                        crate::log(&format!(
                            "Avatar: {} loaded ({} bytes)",
                            contact.handle,
                            pixels.len()
                        ));
                    } else {
                        crate::log(&format!("Avatar: {} download failed", contact.handle));
                    }
                    break;
                }
            }
        }
        if changed {
            self.window_dirty = true;
        }
        changed
    }

    /// Spawn background thread to generate CLUTCH keypairs for a contact.
    /// McEliece keygen (~100ms+) is slow, so we do it off the main thread.
    /// ceremony_id is now computed on-demand when ping provenances are available.
    /// Results are received via clutch_keygen_rx and processed in check_clutch_keygens().
    pub fn spawn_clutch_keygen(
        &self,
        contact_id: ContactId,
        _our_handle_hash: [u8; 32],
        _their_handle_hash: [u8; 32],
    ) {
        use crate::crypto::clutch::generate_all_ephemeral_keypairs;

        let tx = self.clutch_keygen_tx.clone();
        #[cfg(not(target_os = "android"))]
        let proxy = self.event_proxy.clone();

        std::thread::spawn(move || {
            #[cfg(feature = "development")]
            #[cfg(feature = "development")]
            crate::log("CLUTCH: Background keypair generation started...");
            let keypairs = generate_all_ephemeral_keypairs();
            crate::log(
                "CLUTCH: Keypairs ready (ceremony_id computed when ping provenances available)",
            );

            let _ = tx.send(ClutchKeygenResult {
                contact_id,
                keypairs,
            });

            // Wake the event loop so it processes the result
            #[cfg(not(target_os = "android"))]
            let _ = proxy.send_event(super::PhotonEvent::ClutchKeygenComplete);
        });
    }

    /// Spawn background thread to perform CLUTCH KEM encapsulation.
    /// The PQ KEMs (~800ms total) are slow, so we do them off the main thread.
    /// Results are received via clutch_kem_encap_rx and processed in check_clutch_kem_encaps().
    #[allow(clippy::too_many_arguments)]
    pub fn spawn_clutch_kem_encap(
        &self,
        contact_id: ContactId,
        their_offer: crate::crypto::clutch::ClutchOfferPayload,
        ceremony_id: [u8; 32],
        conversation_token: [u8; 32],
        peer_addr: std::net::SocketAddr,
    ) {
        use crate::crypto::clutch::ClutchKemResponsePayload;

        let tx = self.clutch_kem_encap_tx.clone();
        #[cfg(not(target_os = "android"))]
        let proxy = self.event_proxy.clone();

        let thread_body = move || {
            #[cfg(feature = "development")]
            #[cfg(feature = "development")]
            crate::log("CLUTCH: Background KEM encapsulation started (low priority)...");
            let (kem_response, local_secrets) =
                ClutchKemResponsePayload::encapsulate_to_peer(&their_offer);
            #[cfg(feature = "development")]
            #[cfg(feature = "development")]
            crate::log("CLUTCH: KEM encapsulation complete");

            let _ = tx.send(ClutchKemEncapResult {
                contact_id,
                kem_response,
                local_secrets,
                ceremony_id,
                conversation_token,
                peer_addr,
            });

            // Wake the event loop so it processes the result
            #[cfg(not(target_os = "android"))]
            let _ = proxy.send_event(super::PhotonEvent::ClutchKemEncapComplete);
        };

        #[cfg(not(target_os = "redox"))]
        {
            use thread_priority::{ThreadBuilderExt, ThreadPriority};
            std::thread::Builder::new()
                .name("clutch-kem-encap".to_string())
                .spawn_with_priority(ThreadPriority::Min, move |_| thread_body())
                .expect("Failed to spawn KEM encap thread");
        }
        #[cfg(target_os = "redox")]
        {
            std::thread::Builder::new()
                .name("clutch-kem-encap".to_string())
                .spawn(thread_body)
                .expect("Failed to spawn KEM encap thread");
        }
    }

    /// Spawn background thread to complete CLUTCH ceremony (avalanche_expand).
    /// The 2MB memory-hard expansion (~850ms) is slow, so we do it off the main thread.
    /// Results are received via clutch_ceremony_rx and processed in check_clutch_ceremonies().
    #[allow(clippy::too_many_arguments)]
    pub fn spawn_clutch_ceremony(
        &self,
        contact_id: ContactId,
        our_handle_hash: [u8; 32],
        their_handle_hash: [u8; 32],
        our_device_pub: [u8; 32],
        their_device_pub: [u8; 32],
        secrets: crate::crypto::clutch::ClutchSharedSecrets,
        ceremony_id: [u8; 32],
        conversation_token: [u8; 32],
        peer_addr: std::net::SocketAddr,
        their_hqc_prefix: [u8; 8],
    ) {
        use crate::crypto::clutch::clutch_complete_full;

        let tx = self.clutch_ceremony_tx.clone();
        #[cfg(not(target_os = "android"))]
        let proxy = self.event_proxy.clone();

        let thread_body = move || {
            #[cfg(feature = "development")]
            #[cfg(feature = "development")]
            crate::log("CLUTCH: Background ceremony completion started (low priority)...");

            // Phase 1: Compute eggs (moderately fast)
            let result = clutch_complete_full(
                &our_device_pub,
                &their_device_pub,
                &our_handle_hash,
                &their_handle_hash,
                &secrets,
            );

            // Phase 2: Expand to 2MB and derive chains (slow - avalanche_expand)
            let friendship_chains = FriendshipChains::from_clutch(
                &[our_handle_hash, their_handle_hash],
                result.eggs.as_slice(),
            );

            #[cfg(feature = "development")]
            #[cfg(feature = "development")]
            crate::log("CLUTCH: Ceremony completion finished");

            let _ = tx.send(ClutchCeremonyResult {
                contact_id,
                friendship_chains,
                eggs_proof: result.proof,
                their_handle_hash,
                ceremony_id,
                conversation_token,
                peer_addr,
                their_hqc_prefix,
            });

            // Wake the event loop so it processes the result
            #[cfg(not(target_os = "android"))]
            let _ = proxy.send_event(super::PhotonEvent::ClutchCeremonyComplete);
        };

        #[cfg(not(target_os = "redox"))]
        {
            use thread_priority::{ThreadBuilderExt, ThreadPriority};
            std::thread::Builder::new()
                .name("clutch-ceremony".to_string())
                .spawn_with_priority(ThreadPriority::Min, move |_| thread_body())
                .expect("Failed to spawn ceremony thread");
        }
        #[cfg(target_os = "redox")]
        {
            std::thread::Builder::new()
                .name("clutch-ceremony".to_string())
                .spawn(thread_body)
                .expect("Failed to spawn ceremony thread");
        }
    }

    /// Process background CLUTCH key generation results.
    ///
    /// Slot-based design: keypairs stored once, slots filled as messages arrive.
    /// Ceremony completes when all slots have offer + both KEM secret directions.
    pub fn check_clutch_keygens(&mut self) -> bool {
        use crate::crypto::clutch::{
            derive_conversation_token, ClutchKemResponsePayload, ClutchKemSharedSecrets,
            ClutchOfferPayload,
        };
        use crate::network::status::{ClutchKemResponseRequest, ClutchOfferRequest};
        use crate::types::CeremonyId;

        let mut changed = false;
        let mut ceremony_completions: Vec<usize> = Vec::new();
        // Deferred KEM encapsulation spawn (to avoid borrow conflict)
        let mut kem_encap_spawn: Option<(
            ContactId,
            ClutchOfferPayload,
            [u8; 32],
            [u8; 32],
            std::net::SocketAddr,
        )> = None;

        // Get our handle_hash for CLUTCH (PRIVATE identity seed)
        let our_handle_hash = match self.user_identity_seed {
            Some(h) => h,
            None => return changed,
        };
        let device_pubkey = *self.device_keypair.public.as_bytes();
        let device_secret = *self.device_keypair.secret.as_bytes();

        while let Ok(result) = self.clutch_keygen_rx.try_recv() {
            let result_id_hex = hex::encode(&result.contact_id.as_bytes()[..4]);
            crate::log(&format!(
                "CLUTCH: Processing keygen result for contact_id {}...",
                result_id_hex,
            ));

            let mut found = false;
            for (idx, contact) in self.contacts.iter_mut().enumerate() {
                if contact.id == result.contact_id {
                    found = true;

                    // Clear the in-progress flag now that keygen is complete
                    contact.clutch_keygen_in_progress = false;

                    // Store keypairs (ceremony_id computed on-demand when provenances available)
                    contact.clutch_our_keypairs = Some(result.keypairs);
                    changed = true;

                    // Persist keypairs to disk immediately (crash recovery)
                    if let Some(ref keypairs) = contact.clutch_our_keypairs {
                        if let Err(e) = crate::storage::contacts::save_clutch_keypairs(
                            keypairs,
                            contact.handle.as_str(),
                            &our_handle_hash,
                            &device_secret,
                        ) {
                            crate::log(&format!(
                                "CLUTCH: Failed to save keypairs for {}: {}",
                                contact.handle, e
                            ));
                        }
                    }

                    // Initialize slots if not done yet (sorted by handle_hash)
                    if contact.clutch_slots.is_empty() {
                        contact.init_clutch_slots(our_handle_hash);
                    }

                    // Check if their slot has an offer (received before keygen completed)
                    let their_slot_has_offer = contact
                        .get_slot(&contact.handle_hash)
                        .map(|s| s.offer.is_some())
                        .unwrap_or(false);

                    // Store local offer in local slot
                    if let Some(ref keypairs) = contact.clutch_our_keypairs {
                        let our_offer = ClutchOfferPayload::from_keypairs(keypairs);
                        if let Some(local_slot) = contact.get_slot_mut(&our_handle_hash) {
                            local_slot.offer = Some(our_offer);
                            crate::log(&format!(
                                "CLUTCH: Stored local offer in local slot for {}",
                                contact.handle
                            ));
                        } else {
                            crate::log(&format!(
                                "CLUTCH: Could not find local slot for {} - handle_hash mismatch?",
                                contact.handle
                            ));
                        }
                    }

                    // Send our offer if not already sent (PT will retry until ACKed)
                    if contact.clutch_offer_transfer_id.is_none() {
                        if let Some(ip) = contact.ip {
                            if let Some(ref keypairs) = contact.clutch_our_keypairs {
                                use crate::network::fgtw::protocol::build_clutch_offer_vsf;

                                let offer = ClutchOfferPayload::from_keypairs(keypairs);
                                let conv_token = derive_conversation_token(&[
                                    our_handle_hash,
                                    contact.handle_hash,
                                ]);

                                // Build VSF and capture our offer_provenance
                                match build_clutch_offer_vsf(
                                    &conv_token,
                                    &offer,
                                    &device_pubkey,
                                    &device_secret,
                                ) {
                                    Ok((vsf_bytes, our_offer_provenance)) => {
                                        // Store our offer provenance (for ceremony_id derivation)
                                        if !contact
                                            .offer_provenances
                                            .contains(&our_offer_provenance)
                                        {
                                            contact.offer_provenances.push(our_offer_provenance);
                                        }

                                        // Persist provenance immediately
                                        if let Err(e) = crate::storage::contacts::save_clutch_slots(
                                            &contact.clutch_slots,
                                            &contact.offer_provenances,
                                            contact.ceremony_id,
                                            contact.handle.as_str(),
                                            &our_handle_hash,
                                            &device_secret,
                                        ) {
                                            crate::log(&format!(
                                                "Failed to persist CLUTCH provenance: {}",
                                                e
                                            ));
                                        }

                                        if let Some(ref checker) = self.status_checker {
                                            checker.send_offer(ClutchOfferRequest {
                                                peer_addr: ip,
                                                vsf_bytes,
                                            });
                                            contact.clutch_offer_transfer_id = Some(0);
                                            crate::log(&format!(
                                                "CLUTCH: Sent offer to {} (prov={}...)",
                                                contact.handle,
                                                hex::encode(&our_offer_provenance[..4])
                                            ));
                                        }
                                    }
                                    Err(e) => {
                                        crate::log(&format!(
                                            "CLUTCH: Failed to build offer VSF for {}: {}",
                                            contact.handle, e
                                        ));
                                    }
                                }
                            }
                        }
                    }

                    // Compute ceremony_id if we have enough offer provenances (2 for DM)
                    let required_provenances = 2;
                    if contact.ceremony_id.is_none()
                        && contact.offer_provenances.len() >= required_provenances
                    {
                        let ceremony_id = *CeremonyId::derive(
                            &[our_handle_hash, contact.handle_hash],
                            &contact.offer_provenances,
                        )
                        .as_bytes();
                        contact.ceremony_id = Some(ceremony_id);
                        crate::log(&format!(
                            "CLUTCH: Computed ceremony_id for {} from {} offer provenances",
                            contact.handle,
                            contact.offer_provenances.len()
                        ));
                    }

                    // Send KEM response if we have ceremony_id and their offer
                    if their_slot_has_offer {
                        let already_sent_kem = contact
                            .get_slot(&our_handle_hash)
                            .map(|s| s.kem_secrets_to_them.is_some())
                            .unwrap_or(false);

                        if !already_sent_kem && !contact.clutch_kem_encap_in_progress {
                            if let Some(ceremony_id) = contact.ceremony_id {
                                if let Some(ip) = contact.ip {
                                    let conv_token = derive_conversation_token(&[
                                        our_handle_hash,
                                        contact.handle_hash,
                                    ]);
                                    let remote_offer = contact
                                        .get_slot(&contact.handle_hash)
                                        .and_then(|s| s.offer.clone());

                                    if let Some(remote_offer) = remote_offer {
                                        // Defer spawn for KEM encapsulation (to avoid borrow conflict)
                                        // (PQ crypto is slow ~800ms, would block UI/network)
                                        contact.clutch_kem_encap_in_progress = true;
                                        kem_encap_spawn = Some((
                                            contact.id.clone(),
                                            remote_offer,
                                            ceremony_id,
                                            conv_token,
                                            ip,
                                        ));
                                        crate::log(&format!(
                                            "CLUTCH: Will spawn KEM encapsulation for {} (post-keygen)",
                                            contact.handle
                                        ));
                                    }
                                }
                            } else {
                                crate::log(&format!(
                                    "CLUTCH: Keypairs ready for {} - need ceremony_id for KEM response (have {} offer provenances)",
                                    contact.handle,
                                    contact.offer_provenances.len()
                                ));
                            }
                        }
                    }

                    // Process any pending KEM response that arrived before keygen completed
                    if let Some(pending_kem) = contact.clutch_pending_kem.take() {
                        crate::log(&format!(
                            "CLUTCH: Processing queued KEM response from {}",
                            contact.handle
                        ));
                        // Decapsulate remote KEM (remote encapsulated to local pubkeys)
                        if let Some(ref local_keys) = contact.clutch_our_keypairs {
                            let remote_secrets = ClutchKemSharedSecrets::decapsulate_from_peer(
                                &pending_kem,
                                local_keys,
                            );
                            // Store remote secrets (from decapsulating FROM remote) in remote slot
                            let remote_hash = contact.handle_hash;
                            if let Some(remote_slot) = contact.get_slot_mut(&remote_hash) {
                                remote_slot.kem_secrets_from_them = Some(remote_secrets);
                            }

                            // Persist slot state after processing pending KEM
                            if let Err(e) = crate::storage::contacts::save_clutch_slots(
                                &contact.clutch_slots,
                                &contact.offer_provenances,
                                contact.ceremony_id,
                                contact.handle.as_str(),
                                &our_handle_hash,
                                &device_secret,
                            ) {
                                crate::log(&format!(
                                    "CLUTCH: Failed to save slots for {}: {}",
                                    contact.handle, e
                                ));
                            }
                        }
                    }

                    // Check if ceremony can complete
                    if contact.all_slots_complete() {
                        crate::log(&format!(
                            "CLUTCH: All slots complete for {} after keygen - triggering ceremony completion",
                            contact.handle
                        ));
                        ceremony_completions.push(idx);
                    }

                    break;
                }
            }

            if !found {
                crate::log(&format!(
                    "CLUTCH: Keygen result contact_id {}... not found in contacts!",
                    result_id_hex
                ));
            }
        }

        // Spawn deferred KEM encapsulation after releasing contacts borrow
        if let Some((contact_id, offer, ceremony_id, conv_token, peer_addr)) = kem_encap_spawn {
            self.spawn_clutch_kem_encap(contact_id, offer, ceremony_id, conv_token, peer_addr);
        }

        // Process deferred ceremony completions (after releasing contacts borrow)
        for idx in ceremony_completions {
            self.complete_clutch_ceremony_by_idx(idx, our_handle_hash);
            changed = true;
        }

        if changed {
            self.window_dirty = true;
        }
        changed
    }

    /// Process background CLUTCH KEM encapsulation results.
    /// When KEM encap completes, store the secrets and send the KEM response.
    pub fn check_clutch_kem_encaps(&mut self) -> bool {
        use crate::network::status::ClutchKemResponseRequest;

        let mut changed = false;
        let mut ceremony_completions: Vec<usize> = Vec::new();
        let our_handle_hash = match self.user_identity_seed {
            Some(h) => h,
            None => return changed,
        };
        let device_pubkey = *self.device_keypair.public.as_bytes();
        let device_secret = *self.device_keypair.secret.as_bytes();

        while let Ok(result) = self.clutch_kem_encap_rx.try_recv() {
            let result_id_hex = hex::encode(&result.contact_id.as_bytes()[..4]);
            crate::log(&format!(
                "CLUTCH: Processing KEM encap result for contact_id {}...",
                result_id_hex,
            ));

            // Find the contact and update state
            let mut found_idx = None;
            for (idx, contact) in self.contacts.iter_mut().enumerate() {
                if contact.id == result.contact_id {
                    found_idx = Some(idx);
                    contact.clutch_kem_encap_in_progress = false;

                    // Store local encapsulation secrets in local slot (local contribution)
                    // Also store the KEM response payload for re-send
                    if let Some(slot) = contact.get_slot_mut(&our_handle_hash) {
                        slot.kem_secrets_to_them = Some(result.local_secrets);
                        slot.kem_response_for_resend = Some(result.kem_response.clone());
                    }

                    // Persist slot state before sending KEM
                    if let Err(e) = crate::storage::contacts::save_clutch_slots(
                        &contact.clutch_slots,
                        &contact.offer_provenances,
                        contact.ceremony_id,
                        contact.handle.as_str(),
                        &our_handle_hash,
                        &device_secret,
                    ) {
                        crate::log(&format!(
                            "CLUTCH: Failed to save slots for {}: {}",
                            contact.handle, e
                        ));
                    }

                    // Send the KEM response
                    if let Some(ref checker) = self.status_checker {
                        checker.send_kem_response(ClutchKemResponseRequest {
                            peer_addr: result.peer_addr,
                            conversation_token: result.conversation_token,
                            ceremony_id: result.ceremony_id,
                            payload: result.kem_response,
                            device_pubkey,
                            device_secret,
                        });
                        crate::log(&format!("CLUTCH: Sent KEM response to {}", contact.handle));
                    }

                    // Check if all slots are complete after storing our KEM encap secrets
                    if contact.all_slots_complete() {
                        crate::log(&format!(
                            "CLUTCH: All slots complete for {} after KEM encap - triggering ceremony",
                            contact.handle
                        ));
                        ceremony_completions.push(idx);
                    }

                    changed = true;
                    break;
                }
            }

            if found_idx.is_none() {
                crate::log(&format!(
                    "CLUTCH: KEM encap result contact_id {}... not found in contacts!",
                    result_id_hex
                ));
            }
        }

        // Process deferred ceremony completions (after releasing contacts borrow)
        for idx in ceremony_completions {
            self.complete_clutch_ceremony_by_idx(idx, our_handle_hash);
            changed = true;
        }

        if changed {
            self.window_dirty = true;
        }
        changed
    }

    /// Process background CLUTCH ceremony completion results.
    /// When ceremony completes, store the friendship chains and send proof.
    pub fn check_clutch_ceremonies(&mut self) -> bool {
        use crate::crypto::clutch::{derive_conversation_token, ClutchCompletePayload};
        use crate::network::status::ClutchCompleteRequest;
        use crate::types::ClutchState;

        let mut changed = false;
        let our_handle_hash = match self.user_identity_seed {
            Some(h) => h,
            None => return changed,
        };
        let device_pubkey = *self.device_keypair.public.as_bytes();
        let device_secret = *self.device_keypair.secret.as_bytes();

        while let Ok(result) = self.clutch_ceremony_rx.try_recv() {
            let result_id_hex = hex::encode(&result.contact_id.as_bytes()[..4]);
            crate::log(&format!(
                "CLUTCH: Processing ceremony result for contact_id {}...",
                result_id_hex,
            ));

            let friendship_id = *result.friendship_chains.id();

            // Save chains to disk first
            if let Some(identity_seed) = self.user_identity_seed.as_ref() {
                crate::log(&format!(
                    "CLUTCH: Saving friendship chains to disk (fid={}...)",
                    hex::encode(&friendship_id.as_bytes()[..8])
                ));
                if let Err(e) = crate::storage::friendship::save_friendship_chains(
                    &result.friendship_chains,
                    identity_seed,
                    &device_secret,
                ) {
                    crate::log(&format!("Failed to save friendship chains: {}", e));
                } else {
                    #[cfg(feature = "development")]
                    #[cfg(feature = "development")]
                    crate::log("CLUTCH: Friendship chains saved successfully");
                }
            } else {
                #[cfg(feature = "development")]
                #[cfg(feature = "development")]
                crate::log("CLUTCH: Cannot save chains - no identity_seed!");
            }

            // Cache chains in memory
            if let Some(entry) = self
                .friendship_chains
                .iter_mut()
                .find(|(id, _)| *id == friendship_id)
            {
                entry.1 = result.friendship_chains;
            } else {
                self.friendship_chains
                    .push((friendship_id, result.friendship_chains));
            }

            // Update sync records for new friendship
            self.update_sync_records();

            // Find the contact and update state
            if let Some(contact) = self.contacts.iter_mut().find(|c| c.id == result.contact_id) {
                let contact_handle = contact.handle.clone();
                contact.clutch_ceremony_in_progress = false;
                contact.friendship_id = Some(friendship_id);

                crate::log(&format!(
                    "CLUTCH: Eggs computed with {}! (proof: {}...)",
                    contact_handle,
                    hex::encode(&result.eggs_proof[..8])
                ));

                // Store our proof for later verification
                contact.clutch_our_eggs_proof = Some(result.eggs_proof);

                // Check if we already received their proof (fast party case)
                let their_early_proof = contact.clutch_their_eggs_proof;

                // Send ClutchComplete proof to peer
                if let Some(ref checker) = self.status_checker {
                    let payload = ClutchCompletePayload {
                        eggs_proof: result.eggs_proof,
                    };

                    checker.send_complete_proof(ClutchCompleteRequest {
                        peer_addr: result.peer_addr,
                        conversation_token: result.conversation_token,
                        ceremony_id: result.ceremony_id,
                        payload,
                        device_pubkey,
                        device_secret,
                    });

                    crate::log(&format!(
                        "CLUTCH: Sent proof to {} via status checker",
                        contact_handle
                    ));
                }

                // Check if they already sent us their proof
                if let Some(their_proof) = their_early_proof {
                    if their_proof == result.eggs_proof {
                        // SUCCESS! Both parties computed same eggs
                        crate::log(&format!(
                            "CLUTCH: Early proof verified with {}! ✓ proof={}...",
                            contact_handle,
                            hex::encode(&result.eggs_proof[..8])
                        ));
                        contact.clutch_state = ClutchState::Complete;
                        self.window_dirty = true; // Force UI redraw to show chat textbox
                        // Store their HQC pub prefix to detect stale offers after restart
                        contact.completed_their_hqc_prefix = Some(result.their_hqc_prefix);
                        contact.clutch_our_eggs_proof = None;
                        contact.clutch_their_eggs_proof = None;
                    } else {
                        // CRYPTOGRAPHIC FAILURE!
                        let our_hex = hex::encode(&result.eggs_proof);
                        let their_hex = hex::encode(&their_proof);
                        crate::log(&format!(
                            "CLUTCH: ⚠ PROOF MISMATCH with {}! ours={}... theirs={}...",
                            contact_handle,
                            &our_hex[..16],
                            &their_hex[..16]
                        ));
                        // Reset to Pending to allow re-keying
                        contact.clutch_state = ClutchState::Pending;
                        contact.clutch_our_eggs_proof = None;
                        contact.clutch_their_eggs_proof = None;
                    }
                } else {
                    // Set state to AwaitingProof - wait for their proof
                    contact.clutch_state = ClutchState::AwaitingProof;
                    crate::log(&format!(
                        "CLUTCH: Awaiting proof from {} (we sent ours)",
                        contact_handle
                    ));
                }

                // Save contact to persist friendship_id and clutch_state
                if let Some(identity_seed) = self.user_identity_seed.as_ref() {
                    if let Err(e) = crate::storage::contacts::save_contact(
                        contact,
                        identity_seed,
                        &device_secret,
                    ) {
                        crate::log(&format!("Failed to save contact after CLUTCH: {}", e));
                    } else {
                        #[cfg(feature = "development")]
                        #[cfg(feature = "development")]
                        crate::log(&format!("CLUTCH: Saved {} state to disk", contact_handle));
                    }

                    // Delete slots file - ceremony is complete, slots no longer needed
                    if let Err(e) =
                        crate::storage::contacts::delete_clutch_slots(contact_handle.as_str())
                    {
                        crate::log(&format!("Failed to delete CLUTCH slots: {}", e));
                    }
                }
                changed = true;
            } else {
                crate::log(&format!(
                    "CLUTCH: Ceremony result contact_id {}... not found in contacts!",
                    result_id_hex
                ));
            }
        }

        if changed {
            self.window_dirty = true;
        }
        changed
    }

    /// Spawn background CLUTCH ceremony completion when all slots are filled.
    /// Extracts data from contact and spawns background thread for heavy crypto.
    ///
    /// Takes contact index to avoid borrow conflicts in the event loop.
    fn complete_clutch_ceremony_by_idx(&mut self, contact_idx: usize, our_handle_hash: [u8; 32]) {
        use crate::crypto::clutch::{derive_conversation_token, ClutchSharedSecrets};

        // Extract data from contact to avoid borrow issues
        let contact = match self.contacts.get_mut(contact_idx) {
            Some(c) => c,
            None => {
                #[cfg(feature = "development")]
                #[cfg(feature = "development")]
                crate::log("CLUTCH: Invalid contact index");
                return;
            }
        };

        // Check if ceremony already in progress
        if contact.clutch_ceremony_in_progress {
            crate::log(&format!(
                "CLUTCH: Ceremony already in progress for {}",
                contact.handle
            ));
            return;
        }

        // Get their slot (the other party)
        let their_handle_hash = contact.handle_hash;
        let contact_id = contact.id.clone();
        let contact_handle = contact.handle.to_string();
        let their_device_pub = *contact.public_identity.as_bytes();

        // Extract all needed data from slots (cloning to release borrow)
        let our_slot = match contact.get_slot(&our_handle_hash) {
            Some(s) => s,
            None => {
                #[cfg(feature = "development")]
                #[cfg(feature = "development")]
                crate::log("CLUTCH: No slot for local party");
                return;
            }
        };
        let their_slot = match contact.get_slot(&their_handle_hash) {
            Some(s) => s,
            None => {
                #[cfg(feature = "development")]
                #[cfg(feature = "development")]
                crate::log("CLUTCH: No slot for remote party");
                return;
            }
        };

        // Local encapsulation secrets from local slot
        let our_kem_secrets = match &our_slot.kem_secrets_to_them {
            Some(s) => s.clone(),
            None => {
                #[cfg(feature = "development")]
                #[cfg(feature = "development")]
                crate::log("CLUTCH: No kem_secrets_to_them in local slot");
                return;
            }
        };
        // Remote encapsulation secrets from remote slot
        let their_kem_secrets = match &their_slot.kem_secrets_from_them {
            Some(s) => s.clone(),
            None => {
                #[cfg(feature = "development")]
                #[cfg(feature = "development")]
                crate::log("CLUTCH: No kem_secrets_from_them in remote slot");
                return;
            }
        };

        // Get their HQC prefix for stale detection
        let their_hqc_prefix: [u8; 8] = their_slot
            .offer
            .as_ref()
            .map(|o| o.hqc256_public[..8].try_into().unwrap_or_default())
            .unwrap_or_default();

        // Get peer address and ceremony_id
        let peer_addr = match contact.ip {
            Some(ip) => ip,
            None => {
                #[cfg(feature = "development")]
                #[cfg(feature = "development")]
                crate::log(&format!("CLUTCH: No IP for {}", contact_handle));
                return;
            }
        };
        let ceremony_id = match contact.ceremony_id {
            Some(c) => c,
            None => {
                #[cfg(feature = "development")]
                #[cfg(feature = "development")]
                crate::log(&format!("CLUTCH: No ceremony_id for {}", contact_handle));
                return;
            }
        };

        let conversation_token = derive_conversation_token(&[our_handle_hash, their_handle_hash]);

        crate::log(&format!(
            "CLUTCH: Spawning ceremony completion for {}",
            contact_handle
        ));

        // Determine low/high ordering by handle hash
        let we_are_low = our_handle_hash < their_handle_hash;

        // Build shared secrets struct with proper ordering
        let secrets = if we_are_low {
            ClutchSharedSecrets {
                low_x25519: our_kem_secrets.x25519,
                high_x25519: their_kem_secrets.x25519,
                low_p384: our_kem_secrets.p384.clone(),
                high_p384: their_kem_secrets.p384.clone(),
                low_secp256k1: our_kem_secrets.secp256k1.clone(),
                high_secp256k1: their_kem_secrets.secp256k1.clone(),
                low_p256: our_kem_secrets.p256.clone(),
                high_p256: their_kem_secrets.p256.clone(),
                low_frodo: our_kem_secrets.frodo.clone(),
                high_frodo: their_kem_secrets.frodo.clone(),
                low_ntru: our_kem_secrets.ntru.clone(),
                high_ntru: their_kem_secrets.ntru.clone(),
                low_mceliece: our_kem_secrets.mceliece.clone(),
                high_mceliece: their_kem_secrets.mceliece.clone(),
                low_hqc: our_kem_secrets.hqc.clone(),
                high_hqc: their_kem_secrets.hqc.clone(),
            }
        } else {
            ClutchSharedSecrets {
                low_x25519: their_kem_secrets.x25519,
                high_x25519: our_kem_secrets.x25519,
                low_p384: their_kem_secrets.p384.clone(),
                high_p384: our_kem_secrets.p384.clone(),
                low_secp256k1: their_kem_secrets.secp256k1.clone(),
                high_secp256k1: our_kem_secrets.secp256k1.clone(),
                low_p256: their_kem_secrets.p256.clone(),
                high_p256: our_kem_secrets.p256.clone(),
                low_frodo: their_kem_secrets.frodo.clone(),
                high_frodo: our_kem_secrets.frodo.clone(),
                low_ntru: their_kem_secrets.ntru.clone(),
                high_ntru: our_kem_secrets.ntru.clone(),
                low_mceliece: their_kem_secrets.mceliece.clone(),
                high_mceliece: our_kem_secrets.mceliece.clone(),
                low_hqc: their_kem_secrets.hqc.clone(),
                high_hqc: our_kem_secrets.hqc.clone(),
            }
        };

        // Mark ceremony in progress and spawn background thread
        contact.clutch_ceremony_in_progress = true;

        let our_device_pub = *self.device_keypair.public.as_bytes();
        self.spawn_clutch_ceremony(
            contact_id,
            our_handle_hash,
            their_handle_hash,
            our_device_pub,
            their_device_pub,
            secrets,
            ceremony_id,
            conversation_token,
            peer_addr,
            their_hqc_prefix,
        );

        self.window_dirty = true;
    }

    /// Ping all contacts that have IP addresses (call periodically)
    pub fn ping_contacts(&mut self) {
        let checker = match &self.status_checker {
            Some(c) => c,
            None => {
                crate::log("Status: No checker available!");
                return;
            }
        };

        let mut pinged = 0;
        for contact in &self.contacts {
            // Prefer local_ip for same-LAN peers (NAT hairpinning workaround)
            let addr = if let (Some(local_ip), Some(local_port)) =
                (contact.local_ip, contact.local_port)
            {
                Some(std::net::SocketAddr::new(
                    std::net::IpAddr::V4(local_ip),
                    local_port,
                ))
            } else {
                contact.ip
            };
            if let Some(ip) = addr {
                checker.ping(ip, contact.public_identity.clone());
                pinged += 1;
            }
        }
        #[cfg(feature = "verbose-network")]
        if pinged > 0 {
            crate::log(&format!("Status: Pinged {} contact(s)", pinged));
        }

        // Send LAN broadcast for local peer discovery (NAT hairpinning workaround)
        // This lets peers on the same LAN discover each other's local IPs
        if let (Some(handle_proof), Some(hq)) = (self.user_handle_proof, &self.handle_query) {
            checker.send_lan_broadcast(handle_proof, hq.port());
        }
    }

    /// Ping a specific contact by index (for entering conversation)
    pub fn ping_contact(&mut self, contact_idx: usize) {
        let checker = match &self.status_checker {
            Some(c) => c,
            None => return,
        };

        if contact_idx < self.contacts.len() {
            let contact = &self.contacts[contact_idx];
            // Prefer local_ip for same-LAN peers (NAT hairpinning workaround)
            let addr = if let (Some(local_ip), Some(local_port)) =
                (contact.local_ip, contact.local_port)
            {
                Some(std::net::SocketAddr::new(
                    std::net::IpAddr::V4(local_ip),
                    local_port,
                ))
            } else {
                contact.ip
            };
            if let Some(ip) = addr {
                checker.ping(ip, contact.public_identity.clone());
                crate::log(&format!(
                    "Status: Pinged {} on conversation enter",
                    contact.handle
                ));
            }
        }
    }

    /// Trigger peer IP refresh (FGTW query + ping all contacts)
    /// Called when returning to contacts screen for snappy updates
    pub fn trigger_peer_refresh(&mut self) {
        crate::log("Network: Triggering peer refresh on screen return");

        // Refresh FGTW peer table
        if let Some(hq) = &self.handle_query {
            let _ = hq.refresh();
        }

        // Ping all contacts to refresh their status
        self.ping_contacts();
    }

    /// Check if it's time to ping contacts and do so
    /// Returns true if pings were sent
    pub fn maybe_ping_contacts(&mut self) -> bool {
        let now = std::time::Instant::now();
        if now >= self.next_status_ping && !self.contacts.is_empty() {
            self.ping_contacts();
            // Ping every 5-15 seconds (randomized to avoid synchronized traffic)
            use rand::Rng;
            let delay = rand::thread_rng().gen_range(5..=15);
            self.next_status_ping = now + std::time::Duration::from_secs(delay);
            true
        } else {
            false
        }
    }

    /// Check if it's time to refresh FGTW and do so
    /// Returns true if refresh was triggered
    pub fn maybe_refresh_fgtw(&mut self) -> bool {
        let now = std::time::Instant::now();
        if now >= self.next_fgtw_refresh && matches!(self.app_state, AppState::Ready) {
            if let Some(hq) = &self.handle_query {
                if hq.refresh() {
                    crate::log("Network: Triggering FGTW refresh");
                    // Refresh every 60-120 seconds (randomized to avoid synchronized traffic)
                    use rand::Rng;
                    let delay = rand::thread_rng().gen_range(60..=120);
                    self.next_fgtw_refresh = now + std::time::Duration::from_secs(delay);
                    return true;
                }
            }
        }
        false
    }

    /// Force an immediate FGTW refresh (called when FCM peer update received)
    pub fn force_fgtw_refresh(&mut self) {
        if matches!(self.app_state, AppState::Ready) {
            if let Some(hq) = &self.handle_query {
                if hq.refresh() {
                    crate::log("Network: FCM-triggered FGTW refresh");
                    // Reset timer so we don't double-refresh
                    self.next_fgtw_refresh =
                        std::time::Instant::now() + std::time::Duration::from_secs(60);
                }
            }
        }
    }

    /// Check for FGTW refresh results and update contact IPs
    /// Returns true if any contacts were updated
    pub fn check_refresh_result(&mut self) -> bool {
        let result = self
            .handle_query
            .as_ref()
            .and_then(|hq| hq.try_recv_refresh());
        let Some(result) = result else { return false };

        if let Some(ref error) = result.error {
            crate::log(&format!("Network: Refresh error: {}", error));
        }

        if result.peers.is_empty() {
            return false;
        }

        crate::log(&format!(
            "Network: Refresh got {} peer(s)",
            result.peers.len()
        ));

        // Get our public IP to detect same-network peers (for local_ip preference)
        // Do this BEFORE pinging so we can use local_ip for same-NAT peers
        let our_public_ip = result.peers.iter()
            .find(|p| p.device_pubkey.as_bytes() == self.device_keypair.public.as_bytes())
            .map(|p| p.ip.ip());
        self.our_public_ip = our_public_ip;

        // Ping all peers - for same-NAT peers, ping BOTH local and public IP
        // (first responder wins - handles both AP isolation and subnet isolation)
        // PRIVACY: Only ping peers who are in our contacts list
        if let Some(ref checker) = self.status_checker {
            let mut pinged_count = 0;
            for peer in &result.peers {
                // Only ping if this peer is in our contacts
                if !self.contacts.iter().any(|c| c.public_identity == peer.device_pubkey) {
                    continue;
                }

                // Always ping public IP
                checker.ping(peer.ip, peer.device_pubkey.clone());
                pinged_count += 1;

                // For same-NAT peers, ALSO ping local IP (covers AP isolation case)
                if let (Some(our_ip), Some(std::net::IpAddr::V4(local_v4))) = (our_public_ip, peer.local_ip) {
                    if peer.ip.ip() == our_ip {
                        let local_addr = std::net::SocketAddr::new(
                            std::net::IpAddr::V4(local_v4),
                            peer.ip.port(),
                        );
                        checker.ping(local_addr, peer.device_pubkey.clone());
                        crate::log(&format!(
                            "Network: Same-NAT peer {} - pinging both {} and {}",
                            hex::encode(&peer.device_pubkey.as_bytes()[..4]),
                            peer.ip, local_addr
                        ));
                    }
                }
            }
            crate::log(&format!(
                "Network: Broadcast ping to {} contact(s)",
                pinged_count
            ));
        }

        // Update contact IPs from fresh peer data
        // Also send CLUTCH offer if we have keys ready but haven't sent yet
        use crate::crypto::clutch::{derive_conversation_token, ClutchOfferPayload};
        use crate::network::fgtw::protocol::build_clutch_offer_vsf;
        use crate::network::status::ClutchOfferRequest;
        use crate::types::ClutchState;

        let mut updated = 0;
        let mut offers_to_send: Vec<ClutchOfferRequest> = Vec::new();

        // Get our handle_hash for CLUTCH (PRIVATE identity seed used in VSF messages)
        let our_handle_hash = match self.user_identity_seed {
            Some(h) => h,
            None => return false,
        };
        let device_pubkey = *self.device_keypair.public.as_bytes();
        let device_secret = *self.device_keypair.secret.as_bytes();

        for peer in &result.peers {
            for contact in &mut self.contacts {
                if contact.public_identity == peer.device_pubkey {
                    let was_none = contact.ip.is_none();
                    if contact.ip != Some(peer.ip) {
                        crate::log(&format!(
                            "Network: Updated {} IP: {:?} -> {}",
                            contact.handle, contact.ip, peer.ip
                        ));
                        contact.ip = Some(peer.ip);
                        updated += 1;
                    }

                    // Update local_ip from FGTW (for hairpin NAT when on same network)
                    if let Some(std::net::IpAddr::V4(local_v4)) = peer.local_ip {
                        if contact.local_ip != Some(local_v4) {
                            crate::log(&format!(
                                "Network: Updated {} local_ip: {:?} -> {}",
                                contact.handle, contact.local_ip, local_v4
                            ));
                            contact.local_ip = Some(local_v4);
                        }

                        // If same public IP (same NAT), use local_ip for all communication
                        // This bypasses AP isolation on hotel/public WiFi
                        if let Some(our_ip) = our_public_ip {
                            if peer.ip.ip() == our_ip {
                                let local_addr = std::net::SocketAddr::new(
                                    std::net::IpAddr::V4(local_v4),
                                    peer.ip.port(),
                                );
                                if contact.ip != Some(local_addr) {
                                    crate::log(&format!(
                                        "Network: Same NAT detected for {} - using local IP {} instead of {}",
                                        contact.handle, local_addr, peer.ip
                                    ));
                                    contact.ip = Some(local_addr);
                                }
                            }
                        }
                    }

                    // If we just got an IP and have keys ready, queue offer to send
                    // Send if Pending, have keypairs, and not already sent (PT will retry)
                    // Note: don't wait for ceremony_id - that comes after offers are exchanged
                    if was_none
                        && contact.clutch_state == ClutchState::Pending
                        && contact.clutch_offer_transfer_id.is_none()
                    {
                        if let Some(ref keypairs) = contact.clutch_our_keypairs {
                            let offer = ClutchOfferPayload::from_keypairs(keypairs);
                            // Compute conversation_token for privacy-preserving wire format
                            let conv_token =
                                derive_conversation_token(&[our_handle_hash, contact.handle_hash]);

                            // Build VSF and capture our offer_provenance
                            match build_clutch_offer_vsf(
                                &conv_token,
                                &offer,
                                &device_pubkey,
                                &device_secret,
                            ) {
                                Ok((vsf_bytes, our_offer_provenance)) => {
                                    // Store our offer provenance (for ceremony_id derivation)
                                    if !contact.offer_provenances.contains(&our_offer_provenance) {
                                        contact.offer_provenances.push(our_offer_provenance);
                                    }

                                    // Persist provenance immediately
                                    if let Err(e) = crate::storage::contacts::save_clutch_slots(
                                        &contact.clutch_slots,
                                        &contact.offer_provenances,
                                        contact.ceremony_id,
                                        contact.handle.as_str(),
                                        &our_handle_hash,
                                        &device_secret,
                                    ) {
                                        crate::log(&format!(
                                            "Failed to persist CLUTCH provenance: {}",
                                            e
                                        ));
                                    }

                                    // Use best_addr for same-NAT detection (local_ip if available)
                                    let target_addr = contact.best_addr(our_public_ip).unwrap_or(peer.ip);
                                    offers_to_send.push(ClutchOfferRequest {
                                        peer_addr: target_addr,
                                        vsf_bytes,
                                    });
                                    contact.clutch_offer_transfer_id = Some(0);
                                    crate::log(&format!(
                                        "CLUTCH: Queueing offer for {} (target={}, prov={}...)",
                                        contact.handle, target_addr,
                                        hex::encode(&our_offer_provenance[..4])
                                    ));
                                }
                                Err(e) => {
                                    crate::log(&format!(
                                        "CLUTCH: Failed to build offer VSF for {}: {}",
                                        contact.handle, e
                                    ));
                                }
                            }
                        }
                    }
                    break;
                }
            }
        }

        // Send queued offers (after releasing mutable borrow on contacts)
        if let Some(ref checker) = self.status_checker {
            for request in offers_to_send {
                checker.send_offer(request);
            }
        }

        if updated > 0 {
            crate::log(&format!("Network: Updated {} contact IP(s)", updated));
        }

        updated > 0
    }

    /// Check for peer updates from WebSocket (real-time IP changes)
    /// Returns true if any contact IP was updated
    #[cfg(not(target_os = "android"))]
    pub fn check_peer_updates(&mut self) -> bool {
        let Some(ref client) = self.peer_update_client else {
            return false;
        };

        let mut updated = false;

        // Process all pending updates
        while let Some(update) = client.try_recv() {
            crate::log(&format!(
                "PeerUpdate: Received IP update for {}:{} (handle_proof: {}...)",
                update.ip,
                update.port,
                &hex::encode(&update.handle_proof[..4])
            ));

            // Update matching contact by device_pubkey
            for contact in &mut self.contacts {
                if contact.public_identity.as_bytes() == &update.device_pubkey {
                    let new_ip = format!("{}:{}", update.ip, update.port)
                        .parse::<std::net::SocketAddr>()
                        .ok();

                    if contact.ip != new_ip {
                        crate::log(&format!(
                            "PeerUpdate: Updated {} IP: {:?} -> {:?}",
                            contact.handle, contact.ip, new_ip
                        ));
                        contact.ip = new_ip;
                        updated = true;
                    }
                    break;
                }
            }
        }

        updated
    }

    // NOTE: initiate_full_clutch was removed - use spawn_clutch_keygen() for background keygen
    // The proper flow is:
    // 1. spawn_clutch_keygen() generates keys + ceremony_id in background (~2s total)
    // 2. check_clutch_keygens() stores results and sends offers when ready
}
