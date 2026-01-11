use crate::ui::{app::*, colour::*, text_rasterizing::*, theme};
use crate::{debug_println, DEBUG_ENABLED};

/// Deploy version as binary (1 byte = u8, 2 bytes = u16 little-endian)
const DEPLOY_VERSION_BYTES: &[u8] = include_bytes!("../../v");

fn get_deploy_version() -> u32 {
    match DEPLOY_VERSION_BYTES.len() {
        1 => DEPLOY_VERSION_BYTES[0] as u32,
        2 => u16::from_le_bytes([DEPLOY_VERSION_BYTES[0], DEPLOY_VERSION_BYTES[1]]) as u32,
        _ => 0,
    }
}

fn decimal_to_dozenal_names(n: u32) -> String {
    const DIGITS: [&str; 12] = [
        "Zil", "Zila", "Zilor", "Ter", "Tera", "Teror",
        "Lun", "Luna", "Lunor", "Stel", "Stela", "Stelor",
    ];

    if n == 0 {
        return DIGITS[0].to_string();
    }

    let mut result = Vec::new();
    let mut remaining = n;

    while remaining > 0 {
        result.push(DIGITS[(remaining % 12) as usize]);
        remaining /= 12;
    }

    result.reverse();
    result.join(" ")
}

impl PhotonApp {
    pub fn render(&mut self) {
        let now = std::time::Instant::now();

        // Increment frame counter (every render() call)
        self.frame_counter += 1;
        // Calculate layout constants (needed by all rendering paths)
        let font_size = self.font_size();
        let box_width = self.textbox_width();
        let box_height = self.textbox_height();
        let center_x = self.width as usize / 2;
        let textbox_y = self.textbox_center_y();
        let ru = self.effective_ru();

        // Update spectrum phase and speckle animation while attesting or searching
        if matches!(
            self.app_state,
            AppState::Launch(LaunchState::Attesting) | AppState::Searching
        ) {
            let delta_time = now.duration_since(self.last_frame_time).as_secs_f32();
            debug_println!(
                "Animating query: delta={:.3}s, phase={:.2}",
                delta_time,
                self.spectrum_phase
            );
            // Spectrum: 2 pi radians per second = 1 full cycle/sec
            self.spectrum_phase += delta_time * std::f32::consts::PI * 2.;
            self.spectrum_phase %= std::f32::consts::TAU; // Wrap phase
                                                          // Speckles: high increment rate creates nice animated effect
            self.speckle_counter += delta_time * (usize::MAX / 64) as f32;
            // Hourglass: stochastic wobble (-12 to +13 degrees per frame)
            use rand::Rng;
            let wobble: f32 = rand::thread_rng().gen_range(-10.6..=15.);
            self.hourglass_angle = (self.hourglass_angle + wobble) % 360.0;
            // Mark window dirty to trigger redraw of animated elements
            self.window_dirty = true;
        }

        // Check if zoom hint should be hidden (timer expired)
        let mut zoom_hint_dirty = false;
        if self.zoom_hint_visible {
            if let Some(hide_time) = self.zoom_hint_hide_time {
                if now >= hide_time {
                    // Timer expired - need to subtract the hint text
                    zoom_hint_dirty = true;
                }
            }
        }

        // Check if text became empty (button disappears, need full redraw to clear the area)
        let current_is_empty = self.current_text_state.chars.is_empty();
        let prev_is_empty = self.previous_text_state.is_empty;
        if !prev_is_empty && current_is_empty {
            match self.app_state {
                AppState::Conversation | AppState::Ready | AppState::Launch(LaunchState::Fresh) => {
                    // Non-empty → Empty: button disappears, need full redraw to clear the area
                    self.window_dirty = true;
                }
                _ => {}
            }
        }

        // Always update scroll to keep blinkey in view except during selection drag
        if self.current_text_state.textbox_focused
            && !self.current_text_state.chars.is_empty()
            && !self.is_mouse_selecting
        {
            self.text_dirty |= self.update_text_scroll();
        }
        if !self.text_dirty {
            if self.current_text_state.width != self.previous_text_state.width
                || self.current_text_state.chars != self.previous_text_state.chars
            {
                self.text_dirty = true;
                self.selection_dirty = true;
            }
        }

        if !self.selection_dirty {
            self.selection_dirty = self.current_text_state.selection_anchor
                != self.previous_text_state.selection_anchor
                || self.current_text_state.blinkey_index != self.previous_text_state.blinkey_index
                || self.current_text_state.scroll_offset != self.previous_text_state.scroll_offset;
        }

        if self.text_dirty
            || self.selection_dirty
            || self.window_dirty
            || self.controls_dirty
            || zoom_hint_dirty
        {
            self.update_counter += 1;

            // Pre-compute scaled avatar before locking buffer (to avoid borrow conflict)
            // Use the same radius calculation as the layout to ensure avatar fits perfectly
            if matches!(self.app_state, AppState::Ready | AppState::Searching) {
                if let Some(ref contacts_block) = self.layout.contacts {
                    let unified = super::app::ContactsUnifiedLayout::new(
                        contacts_block,
                        self.span,
                        self.effective_ru(),
                        0,
                    );
                    let (_, _, avatar_radius) = unified.user_avatar_center_radius();
                    self.update_avatar_scaled((avatar_radius * 2) as usize);
                }
            }

            let eff_ru = self.effective_ru();

            // Pre-compute selection skip rect for hover effect (to avoid borrow conflict with buffer)
            let selection_skip = self.selection_skip_rect();

            let mut buffer = self.renderer.lock_buffer();
            let pixels = buffer.as_mut();
            let indicator_radius = (self.span as f32 * eff_ru / 160.).max(1.) as usize;
            let indicator_x = (self.span as f32 * eff_ru / 40.).max(1.) as usize;
            let indicator_y = indicator_x;

            if self.window_dirty {
                self.redraw_counter += 1;
                self.selection_dirty = false;
                self.text_dirty = false;
                self.hit_test_map.fill(HIT_NONE);
                self.textbox_mask.fill(0);

                // Scroll offset only applies to contacts screen (Ready/Searching)
                let bg_scroll = if matches!(self.app_state, AppState::Ready | AppState::Searching) {
                    self.contacts_scroll_offset
                } else {
                    0
                };
                Self::draw_background_texture(
                    pixels,
                    self.width as usize,
                    self.height as usize,
                    self.speckle_counter as usize,
                    self.is_fullscreen,
                    bg_scroll,
                );

                // Fill header with back button hit area BEFORE window controls
                // so controls can overwrite their portion
                if matches!(self.app_state, AppState::Conversation) {
                    let header_height = self.span as usize / 5;
                    for y in 0..header_height {
                        for x in 0..self.width as usize {
                            let idx = y * self.width as usize + x;
                            self.hit_test_map[idx] = HIT_BACK_HEADER;
                        }
                    }
                }

                // Calculate window control bounds (needed for edges/hairlines)
                // Controls are drawn AFTER content (below)
                #[cfg(not(target_os = "android"))]
                let (start, edges, button_x_start, button_height) =
                    Self::window_controls_bounds(self.width, self.height, eff_ru);

                // Draw FGTW connectivity indicator (small circle in top-left)
                // AA black base circle
                Self::draw_black_circle(
                    pixels,
                    self.width as usize,
                    indicator_x,
                    indicator_y,
                    indicator_radius,
                );
                // Add green if online, grey hairline if offline
                if self.fgtw_online {
                    Self::draw_filled_circle(
                        pixels,
                        self.width as usize,
                        indicator_x,
                        indicator_y,
                        indicator_radius,
                        theme::ONLINE_DOT,
                        true,
                    );
                } else {
                    Self::draw_indicator_hairline(
                        pixels,
                        self.width as usize,
                        indicator_x,
                        indicator_y,
                        indicator_radius,
                        theme::OFFLINE_DOT,
                        true,
                    );
                }
                self.prev_fgtw_online = self.fgtw_online;

                // Show connectivity hint only on very first render
                if !self.hint_was_shown && matches!(self.app_state, AppState::Launch(_)) {
                    self.hint_was_shown = true;
                    let hint_x = (indicator_x + indicator_radius * 2 + 4) as f32;
                    let hint_y = indicator_y as f32 + indicator_radius as f32 * 0.5;
                    let hint_size = font_size * 0.7;
                    self.text_renderer.draw_text_left_u32(
                        pixels,
                        self.width as usize,
                        "<- network",
                        hint_x,
                        hint_y,
                        hint_size,
                        300,                 // Light weight for hint text
                        theme::LABEL_COLOUR, // Same as handle text on attest screen
                        theme::FONT_UI,
                    );
                }

                // Different UI based on app state
                if matches!(self.app_state, AppState::Launch(_)) {
                    // Launch screen: spectrum, logo, handle entry
                    if let Some(ref spectrum_region) = self.layout.logo_spectrum {
                        Self::draw_spectrum(
                            pixels,
                            self.width as usize,
                            spectrum_region,
                            self.spectrum_phase,
                        );
                    }
                    if let Some(ref text_region) = self.layout.photon_text {
                        Self::draw_logo_text(
                            pixels,
                            &mut self.text_renderer,
                            self.width as usize,
                            text_region,
                        );
                    }

                    // Attest block: unified region containing textbox (top), hint (middle), attest (bottom)
                    // Uses AttestBlockLayout for proportional slicing - fiddle with slices in app.rs
                    if let Some(ref block) = self.layout.attest_block {
                        let sub = super::app::AttestBlockLayout::new(block);

                        // Draw textbox - USE TEXT_LAYOUT (single source of truth)
                        let tl = &self.text_layout;
                        Self::draw_textbox(
                            pixels,
                            &mut self.hit_test_map,
                            HIT_HANDLE_TEXTBOX,
                            &mut self.textbox_mask,
                            self.width as usize,
                            tl.center_x,
                            tl.center_y as isize,
                            tl.box_width,
                            tl.box_height,
                        );

                        // Draw "handle" label - font size from hint slice height
                        let hint_font_size = sub.hint.h as f32 * 0.6;
                        self.text_renderer.draw_text_center_u32(
                            pixels,
                            self.width as usize,
                            "handle",
                            (sub.hint.x + sub.hint.w / 2) as f32,
                            (sub.hint.y + sub.hint.h / 2) as f32,
                            hint_font_size,
                            300,
                            theme::LABEL_COLOUR,
                            theme::FONT_UI,
                        );

                        // Always update glow_colour based on state
                        self.glow_colour =
                            if matches!(self.app_state, AppState::Launch(LaunchState::Attesting)) {
                                theme::GLOW_ATTESTING
                            } else if matches!(
                                self.app_state,
                                AppState::Launch(LaunchState::Error(_))
                            ) {
                                theme::GLOW_ERROR
                            } else {
                                theme::GLOW_DEFAULT
                            };

                        if self.current_text_state.textbox_focused {
                            Self::apply_textbox_glow(
                                pixels,
                                &self.textbox_mask,
                                self.width as usize,
                                tl.center_y as isize,
                                tl.box_width,
                                tl.box_height,
                                true,
                                self.glow_colour,
                            );
                        }
                    }
                } else if matches!(self.app_state, AppState::Ready | AppState::Searching) {
                    // Ready/Searching screen uses unified layout:
                    // - user section: avatar + handle + hint + textbox + separator
                    // - contacts rows: scrollable contact list
                    // All elements scale with span-based unit_height

                    if let Some(ref contacts_block) = self.layout.contacts {
                        // Get filter text for contact filtering
                        let filter_text: String = self.current_text_state.chars.iter().collect();
                        let filter_lower = filter_text.to_lowercase();

                        // Compute center offset based on longest VISIBLE contact name
                        let temp_unified = super::app::ContactsUnifiedLayout::new(
                            contacts_block,
                            self.span,
                            ru,
                            0,
                        );
                        let mut longest_name_width = 0.0f32;
                        for contact in self.contacts.iter() {
                            // Skip filtered-out contacts
                            if !filter_lower.is_empty()
                                && !contact.handle.as_str().to_lowercase().contains(&filter_lower)
                            {
                                continue;
                            }
                            let w = self.text_renderer.measure_text_width(
                                contact.handle.as_str(),
                                temp_unified.text_size,
                                theme::FONT_WEIGHT_USER_CONTENT,
                                theme::FONT_USER_CONTENT,
                            );
                            if w > longest_name_width {
                                longest_name_width = w;
                            }
                        }
                        let content_width =
                            temp_unified.text_left_offset as f32 + longest_name_width;
                        let center_offset =
                            ((contacts_block.w as f32 - content_width) / 2.0) as usize;

                        let unified = super::app::ContactsUnifiedLayout::new(
                            contacts_block,
                            self.span,
                            ru,
                            center_offset,
                        );

                        // === USER SECTION ===
                        let scroll = self.contacts_scroll_offset;
                        let (avatar_cx, base_avatar_cy, avatar_radius) =
                            unified.user_avatar_center_radius();
                        let avatar_cy = base_avatar_cy + scroll;

                        // Draw user avatar (no ring, with hit testing)
                        Self::draw_avatar(
                            pixels,
                            Some(&mut self.hit_test_map),
                            self.width as usize,
                            self.height as usize,
                            avatar_cx,
                            avatar_cy,
                            avatar_radius,
                            self.avatar_scaled.as_deref(),
                            None, // no ring
                            self.file_hovering_avatar,
                        );

                        // Draw handle text centered in handle region (with scroll)
                        if let Some(ref handle) = self.user_handle {
                            let (handle_cx, base_handle_cy) = unified.handle_center();
                            let handle_cy = base_handle_cy + scroll;
                            let handle_h = unified.handle.h as isize;
                            // Only draw if visible
                            if handle_cy + handle_h / 2 > 0 && handle_cy - handle_h / 2 < self.height as isize {
                                // Font size from region height (not text_size which is for contact rows)
                                let handle_font = unified.handle.h as f32 * 0.8;
                                self.text_renderer.draw_text_center_u32(
                                    pixels,
                                    self.width as usize,
                                    handle,
                                    handle_cx as f32,
                                    handle_cy as f32,
                                    handle_font,
                                    700,
                                    theme::TEXT_COLOUR,
                                    theme::FONT_USER_CONTENT,
                                );
                            }
                        }

                        // Draw hint text if needed (with scroll)
                        if self.show_avatar_hint {
                            let (hint_cx, base_hint_cy) = unified.hint_center();
                            let hint_cy = base_hint_cy + scroll;
                            let hint_h = unified.hint.h as isize;
                            // Only draw if visible
                            if hint_cy + hint_h / 2 > 0 && hint_cy - hint_h / 2 < self.height as isize {
                                // Font size from region height
                                let hint_font = unified.hint.h as f32 * 0.5;
                                self.text_renderer.draw_text_center_u32(
                                    pixels,
                                    self.width as usize,
                                    "drag and drop an image to upload avatar",
                                    hint_cx as f32,
                                    hint_cy as f32,
                                    hint_font,
                                    300,
                                    theme::LABEL_COLOUR,
                                    theme::FONT_UI,
                                );
                            }
                        }

                        // Draw search textbox (with scroll)
                        // TextLayout provides base positions; we apply scroll offset for rendering
                        let tl = &self.text_layout;
                        let scrolled_textbox_cy = tl.center_y as isize + scroll;
                        let half_box_h = (tl.box_height / 2) as isize;
                        let screen_h = self.height as isize;

                        // Textbox is always drawn (draw_textbox handles partial visibility)
                        // But glow/focus/hover only allowed when FULLY on-screen
                        let textbox_fully_visible = scrolled_textbox_cy >= half_box_h
                            && scrolled_textbox_cy + half_box_h <= screen_h;

                        Self::draw_textbox(
                            pixels,
                            &mut self.hit_test_map,
                            HIT_HANDLE_TEXTBOX,
                            &mut self.textbox_mask,
                            self.width as usize,
                            tl.center_x,
                            scrolled_textbox_cy,  // Already isize
                            tl.box_width,
                            tl.box_height,
                        );

                        // Glow colour: yellow during search, green/red based on result
                        self.glow_colour = if matches!(self.app_state, AppState::Searching) {
                            theme::GLOW_ATTESTING
                        } else {
                            match &self.search_result {
                                Some(SearchResult::Found(_)) => theme::GLOW_SUCCESS,
                                Some(SearchResult::NotFound) => theme::GLOW_ERROR,
                                Some(SearchResult::Error(_)) => theme::GLOW_ERROR,
                                None => theme::GLOW_DEFAULT,
                            }
                        };

                        // Apply glow BEFORE button when focused or searching
                        // Glow function handles partial visibility via per-column bounds checks
                        if self.current_text_state.textbox_focused
                            || matches!(self.app_state, AppState::Searching)
                        {
                            Self::apply_textbox_glow(
                                pixels,
                                &self.textbox_mask,
                                self.width as usize,
                                scrolled_textbox_cy,
                                tl.box_width,
                                tl.box_height,
                                true,
                                self.glow_colour,
                            );
                        }

                        // Draw search button AFTER glow (only when fully visible)
                        if textbox_fully_visible && !self.current_text_state.chars.is_empty() {
                            let button_size = tl.box_height * 7 / 8;
                            let inset = tl.box_height / 16;
                            let button_center_x =
                                tl.center_x + tl.box_width / 2 - inset - button_size / 2;
                            let button_center_y = scrolled_textbox_cy as usize;

                            let button_colour = if matches!(self.app_state, AppState::Searching) {
                                theme::BUTTON_YELLOW
                            } else {
                                theme::BUTTON_BLUE
                            };

                            Self::draw_button(
                                pixels,
                                &mut self.hit_test_map,
                                Some(&mut self.textbox_mask),
                                self.width as usize,
                                self.height as usize,
                                button_center_x,
                                button_center_y,
                                button_size,
                                button_size,
                                HIT_PRIMARY_BUTTON,
                                button_colour,
                                theme::BUTTON_LIGHT_EDGE,
                                theme::BUTTON_SHADOW_EDGE,
                            );

                            let (r, g, b, _a) = unpack_argb(theme::BUTTON_TEXT);
                            if matches!(self.app_state, AppState::Searching) {
                                Self::draw_hourglass_symbol(
                                    pixels,
                                    self.width as usize,
                                    button_center_x,
                                    button_center_y,
                                    button_size * 3 / 4,
                                    self.hourglass_angle,
                                    (r, g, b),
                                );
                            } else {
                                Self::draw_plus_symbol(
                                    pixels,
                                    self.width as usize,
                                    button_center_x,
                                    button_center_y,
                                    button_size * 3 / 4,
                                    (r, g, b),
                                );
                            }
                        }

                        // Show search result in hint region (with scroll)
                        if let Some(ref result) = self.search_result {
                            let (result_cx, base_result_cy) = unified.hint_center();
                            let result_cy = base_result_cy + scroll;
                            let result_h = unified.hint.h as isize;
                            // Only draw if visible
                            if result_cy + result_h / 2 > 0 && result_cy - result_h / 2 < self.height as isize {
                                let (text, colour) = match result {
                                    SearchResult::Found(peer) => {
                                        (format!("added {}", peer.handle), theme::SEARCH_RESULT_ADDED)
                                    }
                                    SearchResult::NotFound => {
                                        ("not found".to_string(), theme::SEARCH_RESULT_NOT_FOUND)
                                    }
                                    SearchResult::Error(e) => {
                                        (format!("error: {}", e), theme::SEARCH_RESULT_NOT_FOUND)
                                    }
                                };
                                self.text_renderer.draw_text_center_u32(
                                    pixels,
                                    self.width as usize,
                                    &text,
                                    result_cx as f32,
                                    result_cy as f32,
                                    unified.hint.h as f32 * 0.5,
                                    500,
                                    colour,
                                    theme::FONT_USER_CONTENT,
                                );
                            }
                        }

                        // Draw separator hairline (with scroll)
                        let base_sep_y = unified.separator_y();
                        let sep_y = base_sep_y + scroll;
                        // Only draw if separator is on screen
                        if sep_y >= 0 && sep_y < self.height as isize {
                            let sep_y_usize = sep_y as usize;
                            for x in unified.separator.x..unified.separator.right() {
                                let idx = sep_y_usize * self.width as usize + x;
                                pixels[idx] =
                                    pixels[idx].wrapping_add(theme::CONTACT_BRIGHTEN_DELTA);
                            }
                        }

                        // === CONTACT ROWS (part of unified layout) ===
                        // Draw contact rows using same unit_height as user section
                        let contact_avatar_radius = unified.avatar_diameter / 2;
                        let contact_avatar_diameter = contact_avatar_radius * 2;
                        let row_height = unified.row_height as isize;
                        let screen_h = self.height as isize;

                        // Track visible row index separately from contact index (filter_lower defined above)
                        let mut visible_row = 0usize;

                        for (contact_idx, contact) in self.contacts.iter_mut().enumerate() {
                            // Filter: skip contacts that don't match search text
                            if !filter_lower.is_empty()
                                && !contact.handle.as_str().to_lowercase().contains(&filter_lower)
                            {
                                continue;
                            }

                            // Use visible_row for positioning (not contact_idx)
                            let (base_avatar_cx, base_avatar_cy) =
                                unified.row_avatar_center(visible_row);
                            let (base_text_x, base_text_y) = unified.row_text_position(visible_row);

                            // Apply scroll offset
                            let avatar_cx = base_avatar_cx;
                            let avatar_cy = base_avatar_cy + scroll;
                            let text_x = base_text_x;
                            let text_y = base_text_y + scroll;

                            // Skip rows entirely below screen (no point continuing)
                            if avatar_cy - (contact_avatar_radius as isize) > screen_h {
                                break;
                            }
                            // Skip rows entirely above screen (but keep iterating)
                            if avatar_cy + (contact_avatar_radius as isize) < 0 {
                                visible_row += 1;
                                continue;
                            }

                            // Cache positions for differential updates (scrolled positions)
                            contact.indicator_x = avatar_cx as usize;
                            contact.indicator_y = avatar_cy as usize;
                            contact.text_x = text_x as f32;
                            contact.text_y = text_y as f32;

                            // Scale contact avatar if needed
                            if contact.avatar_pixels.is_some()
                                && (contact.avatar_scaled.is_none()
                                    || contact.avatar_scaled_diameter != contact_avatar_diameter)
                            {
                                if let Some(scaled) = crate::avatar::scale_avatar(
                                    contact.avatar_pixels.as_ref().unwrap(),
                                    contact_avatar_diameter,
                                ) {
                                    contact.avatar_scaled = Some(scaled);
                                    contact.avatar_scaled_diameter = contact_avatar_diameter;
                                }
                            }

                            // Draw contact avatar with online/offline ring
                            let ring = if contact.is_online {
                                theme::CONTACT_ONLINE
                            } else {
                                theme::CONTACT_OFFLINE
                            };
                            Self::draw_avatar(
                                pixels,
                                None, // no hit testing (handled separately below)
                                self.width as usize,
                                self.height as usize,
                                avatar_cx,
                                avatar_cy,
                                contact_avatar_radius as isize,
                                contact.avatar_scaled.as_deref(),
                                Some(ring),
                                false, // no brighten
                            );
                            contact.prev_is_online = contact.is_online;

                            // Draw handle text - font size derived from row height
                            let is_hovered = self.hovered_contact == Some(contact_idx);
                            let text_color = if is_hovered {
                                theme::CONTACT_NAME
                            } else {
                                theme::CONTACT_NAME_UNHOVERED
                            };

                            self.text_renderer.draw_text_left_u32(
                                pixels,
                                self.width as usize,
                                contact.handle.as_str(),
                                text_x as f32,
                                text_y as f32,
                                unified.text_size,
                                theme::FONT_WEIGHT_USER_CONTENT,
                                text_color,
                                theme::FONT_USER_CONTENT,
                            );

                            // Hit region with scroll offset
                            // Compute row bounds and intersect with screen (use visible_row for positioning)
                            let row_top =
                                unified.rows.y as isize + visible_row as isize * row_height + scroll;
                            let row_bottom = row_top + row_height;

                            // Intersection bounds proof:
                            // WHY: row_top can be negative (scrolled off top) or row_bottom > height (off bottom)
                            // PROOF: .max(0) and .min(height) compute intersection with screen rectangle
                            // PREVENTS: Out-of-bounds hit_test_map access
                            let hit_top = row_top.max(0) as usize;
                            let hit_bottom = row_bottom.min(screen_h) as usize;
                            // Use contact_idx for hit ID (not visible_row) so clicking works correctly
                            let hit_id = HIT_CONTACT_BASE.wrapping_add(contact_idx as u8);

                            for hy in hit_top..hit_bottom {
                                for hx in unified.rows.x..unified.rows.right() {
                                    self.hit_test_map[hy * self.width as usize + hx] = hit_id;
                                }
                            }

                            visible_row += 1;
                        }

                        // Draw version number below last contact row (with padding)
                        let version_y = unified.rows.y as isize
                            + (visible_row as isize + 1) * row_height // +1 for padding row
                            + row_height / 2 // center in the version row
                            + scroll;

                        if version_y > 0 && version_y < screen_h {
                            let version_text = decimal_to_dozenal_names(get_deploy_version());
                            let version_size = unified.text_size * 0.75;
                            let version_width = self.text_renderer.measure_text_width(
                                &version_text,
                                version_size,
                                theme::FONT_WEIGHT_USER_CONTENT,
                                theme::FONT_USER_CONTENT,
                            );
                            let version_x = (self.width as f32 - version_width) / 2.0;

                            self.text_renderer.draw_text_left_u32(
                                pixels,
                                self.width as usize,
                                &version_text,
                                version_x,
                                version_y as f32,
                                version_size,
                                theme::FONT_WEIGHT_USER_CONTENT,
                                theme::VERSION_TEXT,
                                theme::FONT_USER_CONTENT,
                            );
                        }

                        // Cache text size for differential updates
                        self.contact_text_size = unified.text_size;
                        self.prev_hovered_contact = self.hovered_contact;

                        // Redraw window controls AFTER scrolled content (so controls stay on top)
                        #[cfg(not(target_os = "android"))]
                        Self::draw_window_controls(
                            pixels,
                            &mut self.hit_test_map,
                            self.width,
                            self.height,
                            eff_ru,
                        );

                        // Redraw window edges/mask AFTER controls so corners stay transparent
                        #[cfg(not(target_os = "android"))]
                        if !self.is_fullscreen {
                            Self::draw_window_edges_and_mask(
                                pixels,
                                &mut self.hit_test_map,
                                self.width,
                                self.height,
                                start,
                                &edges,
                            );
                        }
                    }
                } else if matches!(self.app_state, AppState::Conversation) {
                    // Conversation view: header with contact name, message area, bottom textbox
                    if let Some(contact_idx) = self.selected_contact {
                        if let Some(contact) = self.contacts.get_mut(contact_idx) {
                            // Layout constants
                            let header_height = self.span as usize / 5; // Contact name header area (1/5 from top)
                            let message_area_top = header_height;
                            let message_area_bottom = textbox_y - box_height;

                            // Avatar circle to left of handle, aligned with "<" arrow
                            let avatar_radius = box_height / 3;
                            let avatar_diameter = avatar_radius * 2;
                            // Position avatar right after the "<" with some spacing
                            let arrow_width = box_height * 3 / 2; // Approximate width of "<" area
                            let avatar_x = arrow_width + avatar_radius;
                            let avatar_y = header_height / 2;

                            // Scale contact avatar if needed
                            if contact.avatar_pixels.is_some()
                                && (contact.avatar_scaled.is_none()
                                    || contact.avatar_scaled_diameter != avatar_diameter)
                            {
                                if let Some(scaled) = crate::avatar::scale_avatar(
                                    contact.avatar_pixels.as_ref().unwrap(),
                                    avatar_diameter,
                                ) {
                                    contact.avatar_scaled = Some(scaled);
                                    contact.avatar_scaled_diameter = avatar_diameter;
                                }
                            }

                            // Draw contact avatar with online/offline ring
                            let ring = if contact.is_online {
                                theme::CONTACT_ONLINE
                            } else {
                                theme::CONTACT_OFFLINE
                            };
                            Self::draw_avatar(
                                pixels,
                                None, // no hit testing
                                self.width as usize,
                                self.height as usize,
                                avatar_x as isize,
                                avatar_y as isize,
                                avatar_radius as isize,
                                contact.avatar_scaled.as_deref(),
                                Some(ring),
                                false, // no brighten
                            );

                            // Draw green "<" back arrow in top-left
                            let arrow_x = box_height; // Inset from left
                            let arrow_y = header_height / 2;
                            self.text_renderer.draw_text_center_u32(
                                pixels,
                                self.width as usize,
                                "<",
                                arrow_x as f32,
                                arrow_y as f32,
                                font_size * 1.4,
                                700,
                                theme::CONTACT_ONLINE, // Green
                                theme::FONT_UI,
                            );

                            // Draw header: contact handle left-aligned next to avatar
                            let handle_x = avatar_x + avatar_radius + avatar_radius / 2; // Right of avatar with small gap
                            self.text_renderer.draw_text_left_u32(
                                pixels,
                                self.width as usize,
                                contact.handle.as_str(),
                                handle_x as f32,
                                (header_height / 2) as f32,
                                font_size * 1.2,
                                500,
                                theme::CONTACT_NAME,
                                theme::FONT_USER_CONTENT,
                            );

                            // Draw separator line below header (additive)
                            let separator_width = box_width;
                            let separator_x = center_x - separator_width / 2;
                            for x in separator_x..(separator_x + separator_width) {
                                let idx = header_height * self.width as usize + x;
                                pixels[idx] =
                                    pixels[idx].wrapping_add(theme::CONTACT_BRIGHTEN_DELTA);
                            }

                            // // Online indicator centered on divider line
                            // let indicator_radius = (self.span / 80).max(1);
                            // let indicator_x = center_x;
                            // let indicator_y = header_height; // On the divider

                            // // Draw base (dark circle)
                            // Self::draw_indicator_base(
                            //     pixels,
                            //     self.width as usize,
                            //     indicator_x,
                            //     indicator_y,
                            //     indicator_radius,
                            // );
                            // if contact.is_online {
                            //     // Online: add green fill
                            //     Self::draw_indicator_colour(
                            //         pixels,
                            //         self.width as usize,
                            //         indicator_x,
                            //         indicator_y,
                            //         indicator_radius,
                            //         theme::ONLINE_DOT,
                            //         true,
                            //     );
                            // } else {
                            //     // Offline: draw hairline ring
                            //     Self::draw_indicator_hairline(
                            //         pixels,
                            //         self.width as usize,
                            //         indicator_x,
                            //         indicator_y,
                            //         indicator_radius,
                            //         theme::CONTACT_OFFLINE,
                            //         true,
                            //     );
                            // }

                            // Draw message area based on CLUTCH state
                            use crate::types::ClutchState;
                            let msg_center_y = (message_area_top + message_area_bottom) / 2;

                            match contact.clutch_state {
                                ClutchState::Complete => {
                                    // CLUTCH complete - can message
                                    if contact.messages.is_empty() {
                                        // Line 1: "no messages yet"
                                        let line1_y = msg_center_y - (font_size as usize);
                                        self.text_renderer.draw_text_center_u32(
                                            pixels,
                                            self.width as usize,
                                            "no messages yet",
                                            center_x as f32,
                                            line1_y as f32,
                                            font_size,
                                            300,
                                            theme::LABEL_COLOUR,
                                            theme::FONT_UI,
                                        );
                                        // Line 2: Success message (green)
                                        let line2_y = msg_center_y + (font_size as usize / 2);
                                        self.text_renderer.draw_text_center_u32(
                                            pixels,
                                            self.width as usize,
                                            "secure channel established",
                                            center_x as f32,
                                            line2_y as f32,
                                            font_size * 0.8,
                                            400,
                                            theme::CONTACT_ONLINE, // Green
                                            theme::FONT_UI,
                                        );
                                    } else {
                                        // Draw messages
                                        let line_height = (font_size as f32 * 1.5) as usize;
                                        let padding = self.span / 32;

                                        // Calculate total height needed for all messages
                                        let total_height =
                                            contact.messages.len() * line_height + padding * 2;
                                        let visible_height =
                                            (message_area_bottom - message_area_top) as usize;

                                        // Start from bottom (most recent messages)
                                        let mut y = message_area_bottom as usize - padding;

                                        // Iterate messages in reverse (newest first at bottom)
                                        for msg in contact.messages.iter().rev() {
                                            y = y.saturating_sub(line_height);

                                            // Apply scroll offset
                                            let scroll_y =
                                                (y as f32 + contact.message_scroll_offset) as usize;

                                            // Skip if above visible area
                                            if scroll_y < message_area_top as usize {
                                                continue;
                                            }
                                            // Stop if below visible area
                                            if scroll_y > message_area_bottom as usize {
                                                break;
                                            }

                                            // Align outgoing (right) vs incoming (left)
                                            if msg.is_outgoing {
                                                // Outgoing: align right with orange color
                                                self.text_renderer.draw_text_right_u32(
                                                    pixels,
                                                    self.width as usize,
                                                    &msg.content,
                                                    (center_x + (box_width / 2) - padding) as f32,
                                                    scroll_y as f32,
                                                    font_size * 0.9,
                                                    theme::FONT_WEIGHT_USER_CONTENT,
                                                    theme::MESSAGE_SENT,
                                                    theme::FONT_USER_CONTENT,
                                                );

                                                // Draw delivery indicator
                                                let indicator =
                                                    if msg.delivered { "✓" } else { "·" };
                                                let indicator_color = if msg.delivered {
                                                    theme::MESSAGE_INDICATOR_ACKD
                                                } else {
                                                    theme::MESSAGE_INDICATOR_SENT
                                                };
                                                self.text_renderer.draw_text_right_u32(
                                                    pixels,
                                                    self.width as usize,
                                                    indicator,
                                                    (center_x + (box_width / 2) - padding * 2)
                                                        as f32,
                                                    scroll_y as f32,
                                                    font_size * 0.7,
                                                    theme::FONT_WEIGHT_USER_CONTENT,
                                                    indicator_color,
                                                    theme::FONT_USER_CONTENT,
                                                );
                                            } else {
                                                // Incoming: align left with cyan color
                                                self.text_renderer.draw_text_left_u32(
                                                    pixels,
                                                    self.width as usize,
                                                    &msg.content,
                                                    (center_x - (box_width / 2) + padding) as f32,
                                                    scroll_y as f32,
                                                    font_size * 0.9,
                                                    theme::FONT_WEIGHT_USER_CONTENT,
                                                    theme::MESSAGE_RECEIVED,
                                                    theme::FONT_USER_CONTENT,
                                                );
                                            }
                                        }
                                    }

                                    // Draw bottom textbox for message input - width from layout, height scales with ru
                                    if let Some(ref tb) = self.layout.textbox {
                                        let tb_height = (self.span as f32 / 8.0 * eff_ru) as usize;
                                        Self::draw_textbox(
                                            pixels,
                                            &mut self.hit_test_map,
                                            HIT_HANDLE_TEXTBOX,
                                            &mut self.textbox_mask,
                                            self.width as usize,
                                            tb.x + tb.w / 2,
                                            (tb.y + tb.h / 2) as isize,
                                            tb.w,
                                            tb_height,
                                        );
                                    }

                                    // Draw send button inset in bottom-right corner of textbox (only if text entered)
                                    if !self.current_text_state.chars.is_empty() {
                                        let send_button_size = box_height * 7 / 8;
                                        let inset = box_height / 16;
                                        let button_center_x =
                                            center_x + box_width / 2 - inset - send_button_size / 2;
                                        let textbox_bottom = textbox_y + box_height / 2;
                                        let button_center_y =
                                            textbox_bottom - inset - send_button_size / 2;
                                        Self::draw_button(
                                            pixels,
                                            &mut self.hit_test_map,
                                            Some(&mut self.textbox_mask),
                                            self.width as usize,
                                            self.height as usize,
                                            button_center_x,
                                            button_center_y,
                                            send_button_size,
                                            send_button_size,
                                            HIT_PRIMARY_BUTTON,
                                            theme::BUTTON_BLUE,
                                            theme::BUTTON_LIGHT_EDGE,
                                            theme::BUTTON_SHADOW_EDGE,
                                        );

                                        // Draw ">" arrow on send button
                                        self.text_renderer.draw_text_center_u32(
                                            pixels,
                                            self.width as usize,
                                            ">",
                                            button_center_x as f32,
                                            button_center_y as f32,
                                            font_size,
                                            700,
                                            theme::BUTTON_TEXT,
                                            theme::FONT_UI,
                                        );
                                    }

                                    // Glow if focused
                                    self.glow_colour = theme::GLOW_DEFAULT;
                                    if self.current_text_state.textbox_focused {
                                        Self::apply_textbox_glow(
                                            pixels,
                                            &self.textbox_mask,
                                            self.width as usize,
                                            textbox_y as isize,
                                            box_width,
                                            box_height,
                                            true,
                                            self.glow_colour,
                                        );
                                    }
                                }
                                _ => {
                                    // CLUTCH in progress - show status, hide textbox
                                    // Line 1: "clutch in progress"
                                    let line1_y = msg_center_y - (font_size as usize / 2);
                                    self.text_renderer.draw_text_center_u32(
                                        pixels,
                                        self.width as usize,
                                        "clutch in progress",
                                        center_x as f32,
                                        line1_y as f32,
                                        font_size,
                                        400,
                                        theme::STATUS_TEXT_ATTESTING, // Yellow with proper alpha
                                        theme::FONT_UI,
                                    );
                                    // Line 2: Hint about what's happening
                                    // Slot-based: only Pending and Complete states
                                    let hint = if contact.clutch_our_keypairs.is_none() {
                                        "generating ephemeral keys..."
                                    } else if contact.clutch_offer_transfer_id.is_none() {
                                        "waiting for network..."
                                    } else if contact.clutch_slots.is_empty() {
                                        "waiting for them to add you back"
                                    } else {
                                        // Count filled slots for progress
                                        let total = contact.clutch_slots.len();
                                        let filled = contact
                                            .clutch_slots
                                            .iter()
                                            .filter(|s| s.is_complete())
                                            .count();
                                        if filled == 0 {
                                            "exchanging key material..."
                                        } else {
                                            "completing ceremony..."
                                        }
                                    };
                                    let line2_y = msg_center_y + (font_size as usize);
                                    self.text_renderer.draw_text_center_u32(
                                        pixels,
                                        self.width as usize,
                                        hint,
                                        center_x as f32,
                                        line2_y as f32,
                                        font_size * 0.7,
                                        300,
                                        theme::LABEL_COLOUR,
                                        theme::FONT_UI,
                                    );
                                    // No textbox drawn - can't message until CLUTCH complete
                                }
                            }
                        }
                    }
                }

                // Draw window controls AFTER content (so controls are on top)
                #[cfg(not(target_os = "android"))]
                Self::draw_window_controls(
                    pixels,
                    &mut self.hit_test_map,
                    self.width,
                    self.height,
                    eff_ru,
                );

                // Draw window edges and button hairlines AFTER controls
                #[cfg(not(target_os = "android"))]
                {
                    if !self.is_fullscreen {
                        Self::draw_window_edges_and_mask(
                            pixels,
                            &mut self.hit_test_map,
                            self.width,
                            self.height,
                            start,
                            &edges,
                        );
                    }

                    Self::draw_button_hairlines(
                        pixels,
                        &mut self.hit_test_map,
                        self.width,
                        self.height,
                        button_x_start,
                        button_height,
                        start,
                        &edges,
                    );
                }

                // Frame counter continues incrementing (not reset)
                self.prev_hovered_button = HoveredButton::None;
            } else {
                // Differential rendering blocks (only if window wasn't fully redrawn)
                // Scroll offset for text rendering (Ready/Searching states only)
                let text_scroll_offset = if matches!(self.app_state, AppState::Ready | AppState::Searching) {
                    self.contacts_scroll_offset
                } else {
                    0
                };

                if self.selection_dirty || self.text_dirty {
                    // Remove old blinkey (if visible)
                    if self.blinkey_visible {
                        Self::undraw_blinkey(
                            pixels,
                            self.width as usize,
                            self.blinkey_pixel_x,
                            self.blinkey_pixel_y,
                            &mut self.blinkey_visible,
                            &mut self.blinkey_wave_top_bright,
                            font_size as usize,
                        );
                    }

                    if self.selection_dirty {
                        // Remove old selection (if present)
                        if let Some(anchor) = self.previous_text_state.selection_anchor {
                            let (sel_start, sel_end) =
                                if anchor < self.previous_text_state.blinkey_index {
                                    (anchor, self.previous_text_state.blinkey_index)
                                } else if anchor > self.previous_text_state.blinkey_index {
                                    (self.previous_text_state.blinkey_index, anchor)
                                } else {
                                    (0, 0)
                                };

                            if sel_start != sel_end {
                                Self::invert_selection(
                                    pixels,
                                    &self.previous_text_state,
                                    self.previous_text_state.scroll_offset,
                                    self.width as usize,
                                    self.height as usize,
                                    sel_start,
                                    sel_end,
                                    &self.text_layout,
                                    &self.hit_test_map,
                                );
                            }
                        }

                        if self.text_dirty {
                            // Remove old text
                            if !self.previous_text_state.chars.is_empty() {
                                Self::render_text_clipped(
                                    pixels,
                                    &self.previous_text_state,
                                    false, // Subtract!
                                    &mut self.text_renderer,
                                    &self.textbox_mask,
                                    self.width as usize,
                                    &self.text_layout,
                                    theme::TEXT_COLOUR,
                                    text_scroll_offset,
                                );
                            } else if !self.previous_text_state.textbox_focused {
                                // Show placeholder when textbox is empty and not focused
                                let placeholder = match self.app_state {
                                    AppState::Launch(_) => Some("∞"),
                                    AppState::Ready | AppState::Searching => Some("search | add"),
                                    _ => None,
                                };
                                if let Some(text) = placeholder {
                                    // Skip only if textbox is completely off-screen
                                    let scrolled_y_isize = self.text_layout.center_y as isize + text_scroll_offset;
                                    let half_h = (self.text_layout.box_height / 2) as isize;
                                    let screen_h = self.height as isize;
                                    if scrolled_y_isize + half_h > 0 && scrolled_y_isize - half_h < screen_h {
                                        let text_width = self.text_renderer.measure_text_width(
                                            text,
                                            self.text_layout.font_size,
                                            500,
                                            theme::FONT_USER_CONTENT,
                                        );
                                        // Center placeholder like empty blinkey: usable_left + (usable_width + button_area) / 2
                                        let empty_center = self.text_layout.usable_left as f32
                                            + (self.text_layout.usable_width
                                                + self.text_layout.button_area)
                                                as f32
                                                / 2.0;
                                        self.text_renderer.draw_text_left_additive_u32(
                                            pixels,
                                            self.width as usize,
                                            text,
                                            empty_center - text_width / 2.0,
                                            scrolled_y_isize as f32,
                                            self.text_layout.font_size,
                                            500,
                                            theme::PLACEHOLDER_TEXT,
                                            theme::FONT_USER_CONTENT,
                                            false, // subtract
                                        );
                                    }
                                }
                            }
                        }
                    }
                }

                // Differential contact hover: add/subtract brightness delta
                if self.hovered_contact != self.prev_hovered_contact {
                    const HOVER_DELTA: u32 = theme::CONTACT_HOVER_DELTA; // 0xFF - 0xA0 = 0x5F per channel

                    // Un-hover previous contact (subtract brightness)
                    if let Some(prev_idx) = self.prev_hovered_contact {
                        if prev_idx < self.contacts.len() {
                            let contact = &self.contacts[prev_idx];
                            self.text_renderer.draw_text_left_additive_u32(
                                pixels,
                                self.width as usize,
                                contact.handle.as_str(),
                                contact.text_x,
                                contact.text_y,
                                self.contact_text_size,
                                theme::FONT_WEIGHT_USER_CONTENT,
                                HOVER_DELTA,
                                theme::FONT_USER_CONTENT,
                                false, // subtract
                            );
                        }
                    }

                    // Hover new contact (add brightness)
                    if let Some(new_idx) = self.hovered_contact {
                        if new_idx < self.contacts.len() {
                            let contact = &self.contacts[new_idx];
                            self.text_renderer.draw_text_left_additive_u32(
                                pixels,
                                self.width as usize,
                                contact.handle.as_str(),
                                contact.text_x,
                                contact.text_y,
                                self.contact_text_size,
                                theme::FONT_WEIGHT_USER_CONTENT,
                                HOVER_DELTA,
                                theme::FONT_USER_CONTENT,
                                true, // add
                            );
                        }
                    }

                    self.prev_hovered_contact = self.hovered_contact;
                }
            }

            if self.text_dirty || self.window_dirty {
                // Scroll offset for text rendering (Ready/Searching states only)
                let text_scroll_offset = if matches!(self.app_state, AppState::Ready | AppState::Searching) {
                    self.contacts_scroll_offset
                } else {
                    0
                };

                // Add new text
                if !self.current_text_state.chars.is_empty() {
                    Self::render_text_clipped(
                        pixels,
                        &self.current_text_state,
                        true, // Add!
                        &mut self.text_renderer,
                        &self.textbox_mask,
                        self.width as usize,
                        &self.text_layout,
                        theme::TEXT_COLOUR,
                        text_scroll_offset,
                    );
                } else if !self.current_text_state.textbox_focused {
                    // Show placeholder when textbox is empty and not focused
                    let placeholder = match self.app_state {
                        AppState::Launch(_) => Some("∞"),
                        AppState::Ready | AppState::Searching => Some("search | add"),
                        _ => None,
                    };
                    if let Some(text) = placeholder {
                        // Skip only if textbox is completely off-screen
                        let scrolled_y_isize = self.text_layout.center_y as isize + text_scroll_offset;
                        let half_h = (self.text_layout.box_height / 2) as isize;
                        let screen_h = self.height as isize;
                        if scrolled_y_isize + half_h > 0 && scrolled_y_isize - half_h < screen_h {
                            let text_width = self.text_renderer.measure_text_width(
                                text,
                                self.text_layout.font_size,
                                500,
                                theme::FONT_USER_CONTENT,
                            );
                            // Center placeholder like empty blinkey: usable_left + (usable_width + button_area) / 2
                            let empty_center = self.text_layout.usable_left as f32
                                + (self.text_layout.usable_width + self.text_layout.button_area) as f32
                                    / 2.0;
                            self.text_renderer.draw_text_left_additive_u32(
                                pixels,
                                self.width as usize,
                                text,
                                empty_center - text_width / 2.0,
                                scrolled_y_isize as f32,
                                self.text_layout.font_size,
                                500,
                                theme::PLACEHOLDER_TEXT,
                                theme::FONT_USER_CONTENT,
                                true, // add
                            );
                        }
                    }
                }
            }

            if self.selection_dirty || self.window_dirty {
                // Invert new selection (if present)
                if let Some(anchor) = self.current_text_state.selection_anchor {
                    let (sel_start, sel_end) = if anchor < self.current_text_state.blinkey_index {
                        (anchor, self.current_text_state.blinkey_index)
                    } else if anchor > self.current_text_state.blinkey_index {
                        (self.current_text_state.blinkey_index, anchor)
                    } else {
                        (0, 0)
                    };

                    if sel_start != sel_end {
                        Self::invert_selection(
                            pixels,
                            &self.current_text_state,
                            self.current_text_state.scroll_offset,
                            self.width as usize,
                            self.height as usize,
                            sel_start,
                            sel_end,
                            &self.text_layout,
                            &self.hit_test_map,
                        );
                    }
                }
            }

            // Draw blinkey (if visible and focused) - only on full redraws or text/selection updates
            if self.blinkey_visible
                && self.current_text_state.textbox_focused
                && (self.window_dirty || self.text_dirty || self.selection_dirty)
            {
                // Recalculate blinkey position using TextLayout (single source of truth)
                self.blinkey_pixel_x = self.text_layout.blinkey_x(&self.current_text_state);
                let base_blinkey_y = self.text_layout.blinkey_y();

                // Apply scroll offset for Ready/Searching states
                let scroll_offset = if matches!(self.app_state, AppState::Ready | AppState::Searching) {
                    self.contacts_scroll_offset
                } else {
                    0
                };
                let scrolled_blinkey_y = base_blinkey_y as isize + scroll_offset;

                // Only draw if blinkey is on-screen
                if scrolled_blinkey_y >= 0 && scrolled_blinkey_y < self.height as isize {
                    self.blinkey_pixel_y = scrolled_blinkey_y as usize;
                    Self::draw_blinkey(
                        pixels,
                        self.width as usize,
                        self.blinkey_pixel_x,
                        self.blinkey_pixel_y,
                        &mut self.blinkey_visible,
                        &mut self.blinkey_wave_top_bright,
                        self.text_layout.font_size as usize,
                    );
                }
            }

            // Draw/remove send button in Conversation view (differential rendering for button appearance)
            if matches!(self.app_state, AppState::Conversation) {
                let current_has_button = !self.current_text_state.chars.is_empty();
                let prev_had_button = !self.previous_text_state.is_empty;

                // Only draw/remove button if state changed OR if doing full redraw
                if (current_has_button != prev_had_button || self.window_dirty)
                    && current_has_button
                {
                    // Button should be visible - draw it
                    let send_button_size = box_height * 7 / 8;
                    let inset = box_height / 16;
                    let button_center_x = center_x + box_width / 2 - inset - send_button_size / 2;
                    let textbox_bottom = textbox_y + box_height / 2;
                    let button_center_y = textbox_bottom - inset - send_button_size / 2;

                    // Differential: button appearing - knockout mask for text clipping
                    Self::draw_button(
                        pixels,
                        &mut self.hit_test_map,
                        Some(&mut self.textbox_mask),
                        self.width as usize,
                        self.height as usize,
                        button_center_x,
                        button_center_y,
                        send_button_size,
                        send_button_size,
                        HIT_PRIMARY_BUTTON,
                        theme::BUTTON_BLUE,
                        theme::BUTTON_LIGHT_EDGE,
                        theme::BUTTON_SHADOW_EDGE,
                    );

                    // Draw ">" arrow on send button
                    self.text_renderer.draw_text_center_u32(
                        pixels,
                        self.width as usize,
                        ">",
                        button_center_x as f32,
                        button_center_y as f32,
                        font_size,
                        700,
                        theme::BUTTON_TEXT,
                        theme::FONT_UI,
                    );
                }
            }

            // Draw/remove search button in Ready view (differential rendering for button appearance)
            if matches!(self.app_state, AppState::Ready | AppState::Searching) {
                let current_has_button = !self.current_text_state.chars.is_empty();
                let prev_had_button = !self.previous_text_state.is_empty;

                // Only draw button if state changed from no-button to button
                if current_has_button && !prev_had_button {
                    let button_size = box_height * 7 / 8;
                    let inset = box_height / 16;
                    let button_center_x = center_x + box_width / 2 - inset - button_size / 2;
                    // WHY: textbox_y is unscrolled, must apply scroll offset for correct position
                    // PROOF: Full redraw path uses scrolled_textbox_cy, differential must match
                    // PREVENTS: Button drawn at wrong Y when contacts are scrolled
                    let button_center_y = (textbox_y as isize + self.contacts_scroll_offset) as usize;

                    let button_colour = if matches!(self.app_state, AppState::Searching) {
                        theme::BUTTON_YELLOW
                    } else {
                        theme::BUTTON_BLUE
                    };

                    // Differential: button appearing - knockout mask for text clipping
                    Self::draw_button(
                        pixels,
                        &mut self.hit_test_map,
                        Some(&mut self.textbox_mask),
                        self.width as usize,
                        self.height as usize,
                        button_center_x,
                        button_center_y,
                        button_size,
                        button_size,
                        HIT_PRIMARY_BUTTON,
                        button_colour,
                        theme::BUTTON_LIGHT_EDGE,
                        theme::BUTTON_SHADOW_EDGE,
                    );

                    // Draw plus icon on add button
                    let (r, g, b, _a) = unpack_argb(theme::BUTTON_TEXT);
                    Self::draw_plus_symbol(
                        pixels,
                        self.width as usize,
                        button_center_x,
                        button_center_y,
                        button_size * 3 / 4,
                        (r, g, b),
                    );
                }
            }

            // Controls dirty - handle hover and focus transitions
            if self.controls_dirty {
                // Handle hover state changes
                if self.prev_hovered_button != self.hovered_button {
                    // Calculate button centers for centerpoint fill
                    // Must match draw_window_controls: span/32 * ru
                    let button_height = (self.span as f32 / 32.0 * eff_ru).ceil() as usize;
                    let button_width = button_height;
                    let total_width = button_width * 7 / 2;
                    let x_start = self.width as usize - total_width;
                    let y_start = 0;
                    let button_center_y = y_start + button_height / 2;

                    // Buttons are offset by button_width / 4 from x_start
                    let button_area_x_start = x_start + button_width / 4;

                    // Minimize: 1px left of left hairline (hairline at button_width from button_area_x_start)
                    let minimize_center_x = button_area_x_start + button_width - 1;
                    // Maximize: center between the two hairlines
                    let maximize_center_x = button_area_x_start + button_width + button_width / 2;
                    // Close: 1px right of right hairline (hairline at button_width * 2 from button_area_x_start)
                    let close_center_x = button_area_x_start + button_width * 2 + 1;

                    // Unhover old button
                    match self.prev_hovered_button {
                        HoveredButton::Close => {
                            Self::draw_hover_centerpoint(
                                pixels,
                                &self.hit_test_map,
                                self.width as usize,
                                self.height as usize,
                                close_center_x,
                                button_center_y,
                                HIT_CLOSE_BUTTON,
                                false,
                                theme::CLOSE_HOVER,
                                self.debug,
                                None,
                            );
                        }
                        HoveredButton::Maximize => {
                            Self::draw_hover_centerpoint(
                                pixels,
                                &self.hit_test_map,
                                self.width as usize,
                                self.height as usize,
                                maximize_center_x,
                                button_center_y,
                                HIT_MAXIMIZE_BUTTON,
                                false,
                                theme::MAXIMIZE_HOVER,
                                self.debug,
                                None,
                            );
                        }
                        HoveredButton::Minimize => {
                            Self::draw_hover_centerpoint(
                                pixels,
                                &self.hit_test_map,
                                self.width as usize,
                                self.height as usize,
                                minimize_center_x,
                                button_center_y,
                                HIT_MINIMIZE_BUTTON,
                                false,
                                theme::MINIMIZE_HOVER,
                                self.debug,
                                None,
                            );
                        }
                        HoveredButton::Textbox => {
                            Self::draw_hover_centerpoint(
                                pixels,
                                &self.hit_test_map,
                                self.width as usize,
                                self.height as usize,
                                center_x,
                                textbox_y,
                                HIT_HANDLE_TEXTBOX,
                                false,
                                theme::TEXTBOX_HOVER,
                                self.debug,
                                selection_skip,
                            );
                        }
                        HoveredButton::QueryButton => {
                            // Button is now inset in textbox bottom-right for all relevant states
                            let send_size = box_height * 7 / 8;
                            let inset = box_height / 16;
                            let button_center_x = center_x + box_width / 2 - inset - send_size / 2;
                            let textbox_bottom = textbox_y + box_height / 2;
                            let query_button_center_y = textbox_bottom - inset - send_size / 2;
                            Self::draw_hover_centerpoint(
                                pixels,
                                &self.hit_test_map,
                                self.width as usize,
                                self.height as usize,
                                button_center_x,
                                query_button_center_y,
                                HIT_PRIMARY_BUTTON,
                                false,
                                theme::QUERY_BUTTON_HOVER,
                                self.debug,
                                None,
                            );
                        }
                        HoveredButton::BackHeader => {
                            // Unhover: subtract header tint
                            Self::apply_back_header_hover(
                                pixels,
                                &self.hit_test_map,
                                self.width as usize,
                                box_height * 2,
                                false,
                            );
                        }
                        HoveredButton::None => {}
                    }

                    // Hover new button
                    match self.hovered_button {
                        HoveredButton::Close => {
                            Self::draw_hover_centerpoint(
                                pixels,
                                &self.hit_test_map,
                                self.width as usize,
                                self.height as usize,
                                close_center_x,
                                button_center_y,
                                HIT_CLOSE_BUTTON,
                                true,
                                theme::CLOSE_HOVER,
                                self.debug,
                                None,
                            );
                        }
                        HoveredButton::Maximize => {
                            Self::draw_hover_centerpoint(
                                pixels,
                                &self.hit_test_map,
                                self.width as usize,
                                self.height as usize,
                                maximize_center_x,
                                button_center_y,
                                HIT_MAXIMIZE_BUTTON,
                                true,
                                theme::MAXIMIZE_HOVER,
                                self.debug,
                                None,
                            );
                        }
                        HoveredButton::Minimize => {
                            Self::draw_hover_centerpoint(
                                pixels,
                                &self.hit_test_map,
                                self.width as usize,
                                self.height as usize,
                                minimize_center_x,
                                button_center_y,
                                HIT_MINIMIZE_BUTTON,
                                true,
                                theme::MINIMIZE_HOVER,
                                self.debug,
                                None,
                            );
                        }
                        HoveredButton::Textbox => {
                            Self::draw_hover_centerpoint(
                                pixels,
                                &self.hit_test_map,
                                self.width as usize,
                                self.height as usize,
                                center_x,
                                textbox_y,
                                HIT_HANDLE_TEXTBOX,
                                true,
                                theme::TEXTBOX_HOVER,
                                self.debug,
                                selection_skip,
                            );
                        }
                        HoveredButton::QueryButton => {
                            // Button is now inset in textbox bottom-right for all relevant states
                            let send_size = box_height * 7 / 8;
                            let inset = box_height / 16;
                            let button_center_x = center_x + box_width / 2 - inset - send_size / 2;
                            let textbox_bottom = textbox_y + box_height / 2;
                            let query_button_center_y = textbox_bottom - inset - send_size / 2;
                            Self::draw_hover_centerpoint(
                                pixels,
                                &self.hit_test_map,
                                self.width as usize,
                                self.height as usize,
                                button_center_x,
                                query_button_center_y,
                                HIT_PRIMARY_BUTTON,
                                true,
                                theme::QUERY_BUTTON_HOVER,
                                self.debug,
                                None,
                            );
                        }
                        HoveredButton::BackHeader => {
                            // Hover: add header tint
                            Self::apply_back_header_hover(
                                pixels,
                                &self.hit_test_map,
                                self.width as usize,
                                box_height * 2,
                                true,
                            );
                        }
                        HoveredButton::None => {}
                    }

                    // Update prev state
                    self.prev_hovered_button = self.hovered_button;
                }
            }

            // Differential update for FGTW connectivity indicator
            if self.fgtw_online != self.prev_fgtw_online {
                if self.fgtw_online {
                    // Going online: subtract grey hairline, add green fill
                    Self::draw_indicator_hairline(
                        pixels,
                        self.width as usize,
                        indicator_x,
                        indicator_y,
                        indicator_radius,
                        theme::OFFLINE_DOT,
                        false,
                    );
                    Self::draw_filled_circle(
                        pixels,
                        self.width as usize,
                        indicator_x,
                        indicator_y,
                        indicator_radius,
                        theme::ONLINE_DOT,
                        true,
                    );
                } else {
                    // Going offline: subtract green fill, add grey hairline
                    Self::draw_filled_circle(
                        pixels,
                        self.width as usize,
                        indicator_x,
                        indicator_y,
                        indicator_radius,
                        theme::ONLINE_DOT,
                        false,
                    );
                    Self::draw_indicator_hairline(
                        pixels,
                        self.width as usize,
                        indicator_x,
                        indicator_y,
                        indicator_radius,
                        theme::OFFLINE_DOT,
                        true,
                    );
                }
                self.prev_fgtw_online = self.fgtw_online;
            }

            // Reapply current hover state after window_dirty redraws
            // (full redraws clear the framebuffer, losing hover overlays)
            // This runs OUTSIDE controls_dirty so it works during animation
            if self.window_dirty && self.hovered_button != HoveredButton::None {
                // Calculate button centers for centerpoint fill
                // Must match draw_window_controls: span/32 * ru
                let button_height = (self.span as f32 / 32.0 * eff_ru).ceil() as usize;
                let button_width = button_height;
                let total_width = button_width * 7 / 2;
                let x_start = self.width as usize - total_width;
                let y_start = 0;
                let button_center_y = y_start + button_height / 2;

                // Buttons are offset by button_width / 4 from x_start
                let button_area_x_start = x_start + button_width / 4;

                // Minimize: 1px left of left hairline
                let minimize_center_x = button_area_x_start + button_width - 1;
                // Maximize: center between the two hairlines
                let maximize_center_x = button_area_x_start + button_width + button_width / 2;
                // Close: 1px right of right hairline
                let close_center_x = button_area_x_start + button_width * 2 + 1;

                // Reapply current hover
                match self.hovered_button {
                    HoveredButton::Close => {
                        Self::draw_hover_centerpoint(
                            pixels,
                            &self.hit_test_map,
                            self.width as usize,
                            self.height as usize,
                            close_center_x,
                            button_center_y,
                            HIT_CLOSE_BUTTON,
                            true,
                            theme::CLOSE_HOVER,
                            self.debug,
                            None,
                        );
                    }
                    HoveredButton::Maximize => {
                        Self::draw_hover_centerpoint(
                            pixels,
                            &self.hit_test_map,
                            self.width as usize,
                            self.height as usize,
                            maximize_center_x,
                            button_center_y,
                            HIT_MAXIMIZE_BUTTON,
                            true,
                            theme::MAXIMIZE_HOVER,
                            self.debug,
                            None,
                        );
                    }
                    HoveredButton::Minimize => {
                        Self::draw_hover_centerpoint(
                            pixels,
                            &self.hit_test_map,
                            self.width as usize,
                            self.height as usize,
                            minimize_center_x,
                            button_center_y,
                            HIT_MINIMIZE_BUTTON,
                            true,
                            theme::MINIMIZE_HOVER,
                            self.debug,
                            None,
                        );
                    }
                    HoveredButton::Textbox => {
                        Self::draw_hover_centerpoint(
                            pixels,
                            &self.hit_test_map,
                            self.width as usize,
                            self.height as usize,
                            center_x,
                            textbox_y,
                            HIT_HANDLE_TEXTBOX,
                            true,
                            theme::TEXTBOX_HOVER,
                            self.debug,
                            selection_skip,
                        );
                    }
                    HoveredButton::QueryButton => {
                        // Button is now inset in textbox bottom-right for all relevant states
                        let send_size = box_height * 7 / 8;
                        let inset = box_height / 16;
                        let button_center_x = center_x + box_width / 2 - inset - send_size / 2;
                        let textbox_bottom = textbox_y + box_height / 2;
                        let query_button_center_y = textbox_bottom - inset - send_size / 2;
                        Self::draw_hover_centerpoint(
                            pixels,
                            &self.hit_test_map,
                            self.width as usize,
                            self.height as usize,
                            button_center_x,
                            query_button_center_y,
                            HIT_PRIMARY_BUTTON,
                            true,
                            theme::QUERY_BUTTON_HOVER,
                            self.debug,
                            None,
                        );
                    }
                    HoveredButton::BackHeader => {
                        // Reapply header hover after window redraw
                        Self::apply_back_header_hover(
                            pixels,
                            &self.hit_test_map,
                            self.width as usize,
                            box_height * 2,
                            true,
                        );
                    }
                    HoveredButton::None => {}
                }
            }
            if self.debug {
                // Draw magenta borders around layout regions
                let magenta = 0xFE_FF_00_FF; // ARGB magenta
                let w = self.width as usize;
                let h = self.height as usize;

                let regions: [Option<&super::app::PixelRegion>; 7] = [
                    self.layout.textbox.as_ref(),
                    self.layout.logo_spectrum.as_ref(),
                    self.layout.photon_text.as_ref(),
                    self.layout.attest_block.as_ref(),
                    self.layout.contacts.as_ref(),
                    self.layout.header.as_ref(),
                    self.layout.message_area.as_ref(),
                ];

                for region in regions.into_iter().flatten() {
                    // Top edge
                    if region.y < h {
                        for x in region.x..region.right().min(w) {
                            pixels[region.y * w + x] = magenta;
                        }
                    }
                    // Bottom edge
                    if region.bottom() > 0 && region.bottom() <= h {
                        let bottom_y = region.bottom() - 1;
                        for x in region.x..region.right().min(w) {
                            pixels[bottom_y * w + x] = magenta;
                        }
                    }
                    // Left edge
                    if region.x < w {
                        for y in region.y..region.bottom().min(h) {
                            pixels[y * w + region.x] = magenta;
                        }
                    }
                    // Right edge
                    if region.right() > 0 && region.right() <= w {
                        let right_x = region.right() - 1;
                        for y in region.y..region.bottom().min(h) {
                            pixels[y * w + right_x] = magenta;
                        }
                    }
                }

                // Draw yellow hairlines for block subdivisions
                let yellow = 0xFE_FF_FF_00; // ARGB yellow

                // Helper to draw region borders
                let draw_region_border =
                    |pixels: &mut [u32], region: &super::app::PixelRegion, color: u32| {
                        // Top edge
                        if region.y < h {
                            for x in region.x..region.right().min(w) {
                                pixels[region.y * w + x] = color;
                            }
                        }
                        // Bottom edge
                        if region.bottom() > 0 && region.bottom() <= h {
                            let bottom_y = region.bottom() - 1;
                            for x in region.x..region.right().min(w) {
                                pixels[bottom_y * w + x] = color;
                            }
                        }
                        // Left edge
                        if region.x < w {
                            for y in region.y..region.bottom().min(h) {
                                pixels[y * w + region.x] = color;
                            }
                        }
                        // Right edge
                        if region.right() > 0 && region.right() <= w {
                            let right_x = region.right() - 1;
                            for y in region.y..region.bottom().min(h) {
                                pixels[y * w + right_x] = color;
                            }
                        }
                    };

                // Attest block subdivisions
                if let Some(ref block) = self.layout.attest_block {
                    let sub = super::app::AttestBlockLayout::new(block);
                    for region in [&sub.error, &sub.textbox, &sub.hint, &sub.attest] {
                        draw_region_border(pixels, region, yellow);
                    }
                }

                // Contacts unified layout subdivisions
                if let Some(ref block) = self.layout.contacts {
                    let sub = super::app::ContactsUnifiedLayout::new(block, self.span, ru, 0);
                    for region in [
                        &sub.user_avatar,
                        &sub.handle,
                        &sub.hint,
                        &sub.textbox,
                        &sub.separator,
                        &sub.rows,
                    ] {
                        draw_region_border(pixels, region, yellow);
                    }
                }

                // Draw black strip at bottom for debug counters
                let counter_size = self.span / 24;
                let strip_height = counter_size * 2;
                let counter_size = counter_size as f32;
                for y in (self.height as usize - strip_height)..self.height as usize {
                    for x in 0..self.width as usize {
                        let idx = y * self.width as usize + x;
                        pixels[idx] = pixels[idx] >> 1 & 0xFF7F7F7F | 0xFF000000;
                    }
                }

                // Draw debug counters (bottom left = redraws, bottom center = updates, bottom right = frames)
                let redraw_text = format!("R:{}", self.redraw_counter);
                let update_text = format!("U:{}", self.update_counter);
                let frame_text = format!("F:{}", self.frame_counter);
                let fps_text = format!("S: {:.1}", self.fps);

                // Bottom left - redraw counter
                self.text_renderer.draw_text_left_u32(
                    pixels,
                    self.width as usize,
                    &redraw_text,
                    counter_size,
                    self.height as f32 - counter_size,
                    counter_size,
                    400,
                    theme::COUNTER_TEXT,
                    "Josefin Slab",
                );

                self.text_renderer.draw_text_center_u32(
                    pixels,
                    self.width as usize,
                    &update_text,
                    self.width as f32 / 3.,
                    self.height as f32 - counter_size,
                    counter_size,
                    400,
                    theme::COUNTER_TEXT,
                    "Josefin Slab",
                );

                // Bottom right center - frame counter
                self.text_renderer.draw_text_center_u32(
                    pixels,
                    self.width as usize,
                    &frame_text,
                    self.width as f32 / 3. * 2.,
                    self.height as f32 - counter_size,
                    counter_size,
                    400,
                    theme::COUNTER_TEXT,
                    "Josefin Slab",
                );

                // Bottom right - FPS counter
                self.text_renderer.draw_text_right_u32(
                    pixels,
                    self.width as usize,
                    &fps_text,
                    self.width as f32 - counter_size,
                    self.height as f32 - counter_size,
                    counter_size,
                    400,
                    theme::COUNTER_TEXT,
                    "Josefin Slab",
                );
            }

            if self.current_text_state.chars.is_empty() != self.previous_text_state.chars.is_empty()
                && !self.current_text_state.chars.is_empty()
                || self.window_dirty && !self.current_text_state.chars.is_empty()
            {
                match &self.app_state {
                    AppState::Launch(launch_state) => match launch_state {
                        LaunchState::Fresh => {
                            // Show "Attest" button in bottom region of attest_block
                            if let Some(ref block) = self.layout.attest_block {
                                let sub = super::app::AttestBlockLayout::new(block);

                                // Button centered in attest region - size from slice
                                let button_center_x = sub.attest.x + sub.attest.w / 2;
                                let button_center_y = sub.attest.y + sub.attest.h / 2;
                                let button_height = sub.attest.h * 3 / 4;
                                const BUTTON_ASPECT: f32 = 2.7;
                                let button_width = (button_height as f32 * BUTTON_ASPECT) as usize;

                                Self::draw_button(
                                    pixels,
                                    &mut self.hit_test_map,
                                    None,
                                    self.width as usize,
                                    self.height as usize,
                                    button_center_x,
                                    button_center_y,
                                    button_width,
                                    button_height,
                                    HIT_PRIMARY_BUTTON,
                                    theme::BUTTON_BLUE,
                                    theme::BUTTON_LIGHT_EDGE,
                                    theme::BUTTON_SHADOW_EDGE,
                                );

                                self.text_renderer.draw_text_center_u32(
                                    pixels,
                                    self.width as usize,
                                    "Attest",
                                    button_center_x as f32,
                                    button_center_y as f32,
                                    button_height as f32 / 2.0,
                                    500,
                                    theme::BUTTON_TEXT,
                                    theme::FONT_USER_CONTENT,
                                );
                            }
                        }
                        LaunchState::Attesting => {
                            // Show "Attesting..." in error region, bottom-aligned
                            if let Some(ref block) = self.layout.attest_block {
                                let sub = super::app::AttestBlockLayout::new(block);
                                let error_center_x = sub.error.x + sub.error.w / 2;
                                // Font size from error slice height
                                let text_size = sub.error.h as f32 * 0.7;
                                // draw_text_center centers on y, so offset up by half text height
                                let error_text_y =
                                    (sub.error.y + sub.error.h) as f32 - text_size / 2.0;
                                self.text_renderer.draw_text_center_u32(
                                    pixels,
                                    self.width as usize,
                                    "Attesting...",
                                    error_center_x as f32,
                                    error_text_y,
                                    text_size,
                                    500,
                                    theme::STATUS_TEXT_ATTESTING,
                                    theme::FONT_USER_CONTENT,
                                );
                            }
                        }
                        LaunchState::Error(ref msg) => {
                            // Show error message in error region, bottom-aligned
                            if let Some(ref block) = self.layout.attest_block {
                                let sub = super::app::AttestBlockLayout::new(block);
                                let error_center_x = sub.error.x + sub.error.w / 2;
                                // Font size from error slice height
                                let text_size = sub.error.h as f32 * 0.7;
                                // draw_text_center centers on y, so offset up by half text height
                                let error_text_y =
                                    (sub.error.y + sub.error.h) as f32 - text_size / 2.0;
                                self.text_renderer.draw_text_center_u32(
                                    pixels,
                                    self.width as usize,
                                    msg,
                                    error_center_x as f32,
                                    error_text_y,
                                    text_size,
                                    500,
                                    theme::STATUS_TEXT_ERROR,
                                    theme::FONT_USER_CONTENT,
                                );
                            }
                        }
                    },
                    AppState::Ready => {
                        // Search button is now drawn inside textbox (above), no separate button needed
                    }
                    _ => {}
                }
            }

            // Zoom hint differential rendering - shows current zoom level at top left third
            // (right third is reserved for window controls)
            if zoom_hint_dirty {
                // Timer expired - subtract the old zoom hint text
                let zoom_text = format!("{:.0}%", self.zoom_hint_ru * 100.0);
                let hint_font_size = font_size * 0.8;
                // Top of text is 1/2 font size from top edge
                let hint_y = hint_font_size / 2.0 + hint_font_size / 2.0; // top edge + half font (for center baseline)
                                                                          // At the 1/3 mark
                let hint_x = self.width as f32 / 3.0;
                let text_width = self.text_renderer.measure_text_width(
                    &zoom_text,
                    hint_font_size,
                    500,
                    theme::FONT_UI,
                );
                self.text_renderer.draw_text_left_additive_u32(
                    pixels,
                    self.width as usize,
                    &zoom_text,
                    hint_x - text_width / 2.0,
                    hint_y,
                    hint_font_size,
                    500,
                    theme::ZOOM_HINT_TEXT,
                    theme::FONT_UI,
                    false, // subtract
                );
                self.zoom_hint_visible = false;
                self.zoom_hint_hide_time = None;
            } else if self.zoom_hint_visible && self.window_dirty {
                // Full redraw while hint is visible - draw it
                let zoom_text = format!("{:.0}%", self.zoom_hint_ru * 100.0);
                let hint_font_size = font_size * 0.8;
                // Top of text is 1/2 font size from top edge
                let hint_y = hint_font_size / 2.0 + hint_font_size / 2.0;
                // At the 1/3 mark
                let hint_x = self.width as f32 / 3.0;
                let text_width = self.text_renderer.measure_text_width(
                    &zoom_text,
                    hint_font_size,
                    500,
                    theme::FONT_UI,
                );
                self.text_renderer.draw_text_left_additive_u32(
                    pixels,
                    self.width as usize,
                    &zoom_text,
                    hint_x - text_width / 2.0,
                    hint_y,
                    hint_font_size,
                    500,
                    theme::ZOOM_HINT_TEXT,
                    theme::FONT_UI,
                    true, // add
                );
            }

            // Debug: overlay hit test map visualization with random colours
            // (drawn last so it covers all UI elements including differentially rendered buttons)
            if self.debug_hit_test {
                for y in 0..self.height as usize {
                    for x in 0..self.width as usize {
                        let hit_idx = y * self.width as usize + x;
                        let element_id = self.hit_test_map[hit_idx];

                        // Show all areas including HIT_NONE (0)
                        if (element_id as usize) < self.debug_hit_colours.len() {
                            let (r, g, b) = self.debug_hit_colours[element_id as usize];
                            pixels[hit_idx] = pack_argb(r, g, b, 255);
                        }
                    }
                }
            }

            // Debug: overlay textbox mask visualization (grayscale alpha)
            if self.show_textbox_mask {
                for y in 0..self.height as usize {
                    for x in 0..self.width as usize {
                        let idx = y * self.width as usize + x;
                        let alpha = self.textbox_mask[idx];
                        // Show mask as white with varying alpha (0=black, 255=white)
                        pixels[idx] = pack_argb(alpha, alpha, alpha, 255);
                    }
                }

                // Render text in green WITHOUT clipping (full window mask)
                if !self.current_text_state.chars.is_empty() {
                    let unclipped_mask = vec![255u8; pixels.len()];
                    Self::render_text_clipped(
                        pixels,
                        &self.current_text_state,
                        true,
                        &mut self.text_renderer,
                        &unclipped_mask,
                        self.width as usize,
                        &self.text_layout,
                        0x00FF00, // Green
                        0,        // No scroll offset for debug view
                    );
                }
            }

            // Always present buffer once per frame
            buffer.present().unwrap();
        } else {
            // macOS with transparent windows + softbuffer doesn't retain buffer contents
            // between frames. Must re-present even when nothing changed or window goes black.
            #[cfg(target_os = "macos")]
            {
                let mut buffer = self.renderer.lock_buffer();
                buffer.present().unwrap();
            }
        }
        self.window_dirty = false;
        self.text_dirty = false;
        self.selection_dirty = false;
        self.controls_dirty = false;
        self.previous_text_state = self.current_text_state.clone();
        self.previous_text_state.is_empty = self.current_text_state.chars.is_empty();

        // Calculate FPS from frame delta times
        let delta_time = now.duration_since(self.last_frame_time).as_secs_f32();
        self.frame_times.push(delta_time);
        if self.frame_times.len() > 60 {
            self.frame_times.remove(0);
        }
        if !self.frame_times.is_empty() {
            let avg_frame_time: f32 =
                self.frame_times.iter().sum::<f32>() / self.frame_times.len() as f32;
            self.fps = 1.0 / avg_frame_time;
        }

        // Update frame time for delta time calculation
        self.last_frame_time = now;
    }

    /// Calculate window control bounds without drawing.
    /// Returns (start, crossings, button_x_start, button_height) needed for edges/hairlines.
    pub fn window_controls_bounds(
        window_width: u32,
        window_height: u32,
        ru: f32,
    ) -> (usize, Vec<(u16, u8, u8)>, usize, usize) {
        let window_width = window_width as usize;
        let window_height = window_height as usize;

        // Calculate button dimensions using harmonic mean (span) scaled by ru
        let span = 2.0 * window_width as f32 * window_height as f32
            / (window_width as f32 + window_height as f32);
        let button_height = (span / 32.0 * ru).ceil() as usize;
        let button_width = button_height;
        let total_width = button_width * 7 / 2;

        let x_start = window_width - total_width;

        // Build squircle crossings for bottom-left corner
        let radius = span * ru / 4.;
        let squirdleyness = 24;

        let mut crossings: Vec<(u16, u8, u8)> = Vec::new();
        let mut y = 1f32;
        loop {
            let y_norm = y / radius;
            let x_norm = (1.0 - y_norm.powi(squirdleyness)).powf(1.0 / squirdleyness as f32);
            let x = x_norm * radius;
            let inset = radius - x;
            if inset > 0. {
                crossings.push((
                    inset as u16,
                    (inset.fract().sqrt() * 256.) as u8,
                    ((1. - inset.fract()).sqrt() * 256.) as u8,
                ));
            }
            if x < y {
                break;
            }
            y += 1.;
        }
        let start = (radius - y) as usize;
        let crossings: Vec<(u16, u8, u8)> = crossings.into_iter().rev().collect();

        // Return button_x_start with the offset applied (matches draw_window_controls)
        (start, crossings, x_start + button_width / 4, button_height)
    }

    pub fn draw_window_controls(
        pixels: &mut [u32],
        hit_test_map: &mut [u8],
        window_width: u32,
        window_height: u32,
        ru: f32,
    ) -> (usize, Vec<(u16, u8, u8)>, usize, usize) {
        let window_width = window_width as usize;
        let window_height = window_height as usize;

        // Calculate button dimensions using harmonic mean (span) scaled by ru
        // span = 2wh/(w+h), base button size = span/32, scaled by ru (zoom multiplier)
        let span = 2.0 * window_width as f32 * window_height as f32
            / (window_width as f32 + window_height as f32);
        let button_height = (span / 32.0 * ru).ceil() as usize;
        let button_width = button_height;
        let total_width = button_width * 7 / 2;

        // Buttons extend to top-right corner of window
        let mut x_start = window_width - total_width;
        let y_start = 0;

        // Build squircle crossings for bottom-left corner
        let radius = span * ru / 4.;
        let squirdleyness = 24;

        let mut crossings: Vec<(u16, u8, u8)> = Vec::new();
        let mut y = 1f32;
        loop {
            let y_norm = y / radius;
            let x_norm = (1.0 - y_norm.powi(squirdleyness)).powf(1.0 / squirdleyness as f32);
            let x = x_norm * radius;
            let inset = radius - x;
            if inset > 0. {
                crossings.push((
                    inset as u16,
                    (inset.fract().sqrt() * 256.) as u8,
                    ((1. - inset.fract()).sqrt() * 256.) as u8,
                ));
            }
            if x < y {
                break;
            }
            y += 1.;
        }
        let start = (radius - y) as usize;
        let crossings: Vec<(u16, u8, u8)> = crossings.into_iter().rev().collect();

        let edge_colour = theme::WINDOW_LIGHT_EDGE;
        let bg_colour = theme::WINDOW_CONTROLS_BG;

        // Left edge (vertical) - draw light hairline following squircle curve
        let mut y_offset = start;
        for (inset, l, h) in &crossings {
            if y_offset >= button_height {
                break;
            }
            let py = y_start + button_height - 1 - y_offset;

            // Fill grey to the right of the curve and populate hit test map
            let col_end = total_width.min(window_width - x_start);
            for col in (*inset as usize + 2)..col_end - 1 {
                let px = x_start + col;
                let pixel_idx = (py * window_width + px) as usize;

                // Write packed ARGB colour directly
                pixels[pixel_idx] = bg_colour;

                // Determine which button this pixel belongs to
                // Button widths: minimize (0-1), maximize (1-2), close (2-3.5)
                // Buttons are drawn with a button_width / 4 offset
                let button_area_x_start = x_start + button_width / 4;

                // Determine button ID based on x position
                // Handle the case where px might be before button_area_x_start
                let button_id = if px < button_area_x_start {
                    HIT_MINIMIZE_BUTTON // Left edge before offset belongs to minimize
                } else {
                    let x_in_button_area = px - button_area_x_start;
                    if x_in_button_area < button_width {
                        HIT_MINIMIZE_BUTTON
                    } else if x_in_button_area < button_width * 2 {
                        HIT_MAXIMIZE_BUTTON
                    } else {
                        HIT_CLOSE_BUTTON
                    }
                };
                hit_test_map[pixel_idx] = button_id;
            }

            let px = x_start + *inset as usize;
            let pixel_idx = (py * window_width + px) as usize;
            pixels[pixel_idx] = blend_rgb_only(pixels[pixel_idx], edge_colour, *l, *h);

            let px = x_start + *inset as usize + 1;
            let pixel_idx = (py * window_width + px) as usize;
            pixels[pixel_idx] = blend_rgb_only(bg_colour, edge_colour, *h, *l);

            // Populate hit test map for inner edge pixel
            let button_area_x_start = x_start + button_width / 4;

            let button_id = if px < button_area_x_start {
                HIT_MINIMIZE_BUTTON
            } else {
                let x_in_button_area = px - button_area_x_start;
                if x_in_button_area < button_width {
                    HIT_MINIMIZE_BUTTON
                } else if x_in_button_area < button_width * 2 {
                    HIT_MAXIMIZE_BUTTON
                } else {
                    HIT_CLOSE_BUTTON
                }
            };
            hit_test_map[pixel_idx] = button_id;

            y_offset += 1;
        }

        // Bottom edge (horizontal)
        let mut x_offset = start;
        let crossing_limit = crossings.len().min(window_width - (x_start + start));
        for &(inset, l, h) in &crossings[..crossing_limit] {
            let i = inset as usize;
            let px = x_start + x_offset;

            // Outer edge pixel (blend hairline with background texture behind)
            let py = y_start + button_height - 1 - i;
            let pixel_idx = py * window_width + px;
            pixels[pixel_idx] = blend_rgb_only(pixels[pixel_idx], edge_colour, l, h);

            // Fill grey above the curve (towards center of buttons) and populate hit test
            for row in (i + 2)..start {
                let py = y_start + button_height - 1 - row;
                let pixel_idx = py * window_width + px;

                pixels[pixel_idx] = bg_colour;

                // Determine which button this pixel belongs to
                // Buttons are drawn with a button_width / 4 offset
                let button_area_x_start = x_start + button_width / 4;

                // Handle the case where px might be before button_area_x_start
                let button_id = if px < button_area_x_start {
                    HIT_MINIMIZE_BUTTON // Left edge before offset belongs to minimize
                } else {
                    let x_in_button_area = px - button_area_x_start;
                    if x_in_button_area < button_width {
                        HIT_MINIMIZE_BUTTON
                    } else if x_in_button_area < button_width * 2 {
                        HIT_MAXIMIZE_BUTTON
                    } else {
                        HIT_CLOSE_BUTTON
                    }
                };
                hit_test_map[pixel_idx] = button_id;
            }

            let py = y_start + button_height - 1 - (i + 1);
            let pixel_idx = py * window_width + px;
            pixels[pixel_idx] = blend_rgb_only(bg_colour, edge_colour, h, l);

            // Populate hit test map for inner edge pixel
            let button_area_x_start = x_start + button_width / 4;

            let button_id = if px < button_area_x_start {
                HIT_MINIMIZE_BUTTON
            } else {
                let x_in_button_area = px - button_area_x_start;
                if x_in_button_area < button_width {
                    HIT_MINIMIZE_BUTTON
                } else if x_in_button_area < button_width * 2 {
                    HIT_MAXIMIZE_BUTTON
                } else {
                    HIT_CLOSE_BUTTON
                }
            };
            hit_test_map[pixel_idx] = button_id;

            x_offset += 1;
        }

        // Continue bottom edge linearly from where squircle ends to window edge
        let linear_start_x = x_start + start + crossings.len();
        let edge_y = y_start + button_height - 1;

        for px in linear_start_x..window_width {
            // Draw edge pixel at bottom of button area
            let pixel_idx = edge_y * window_width + px;
            pixels[pixel_idx] = edge_colour;

            // Fill grey above the edge (from edge to top of button area)
            for row in 1..start {
                let py = edge_y - row;
                let pixel_idx = py * window_width + px;
                pixels[pixel_idx] = bg_colour;

                // All pixels past the squircle belong to close button
                hit_test_map[pixel_idx] = HIT_CLOSE_BUTTON;
            }
        }

        x_start += button_width / 4;

        // Draw button symbols using glyph colours
        let (r, g, b, _a) = unpack_argb(theme::MINIMIZE_GLYPH);
        let minimize_colour = (r, g, b);
        Self::draw_minimize_symbol(
            pixels,
            window_width,
            x_start + button_width / 2,
            y_start + button_width / 2,
            button_width / 4,
            minimize_colour,
        );

        let (r, g, b, _a) = unpack_argb(theme::MAXIMIZE_GLYPH);
        let maximize_colour = (r, g, b);
        let (r, g, b, _a) = unpack_argb(theme::MAXIMIZE_GLYPH_INTERIOR);
        let maximize_interior = (r, g, b);
        Self::draw_maximize_symbol(
            pixels,
            window_width,
            x_start + button_width + button_width / 2,
            y_start + button_width / 2,
            button_width / 4,
            maximize_colour,
            maximize_interior,
        );

        let (r, g, b, _a) = unpack_argb(theme::CLOSE_GLYPH);
        let close_colour = (r, g, b);
        Self::draw_close_symbol(
            pixels,
            window_width,
            x_start + button_width * 2 + button_width / 2,
            y_start + button_width / 2,
            button_width / 4,
            close_colour,
        );
        (start, crossings, x_start, button_height)
    }

    pub fn draw_minimize_symbol(
        pixels: &mut [u32],
        width: usize,
        x: usize,
        y: usize,
        r: usize,
        stroke_colour: (u8, u8, u8),
    ) {
        let r = r + 1;
        let r_render = r / 4 + 1;
        let r_2 = r_render * r_render;
        let r_4 = r_2 * r_2;
        let r_3 = r_render * r_render * r_render;

        let stroke_packed = pack_argb(stroke_colour.0, stroke_colour.1, stroke_colour.2, 255);

        for h in -(r_render as isize)..=(r_render as isize) {
            for w in -(r as isize)..=(r as isize) {
                // Regular squircle: h^4 + w^4
                let h2 = h * h;
                let h4 = h2 * h2;
                let a = (w.abs() - (r * 3 / 4) as isize).max(0);
                let w2 = a * a;
                let w4 = w2 * w2;
                let dist_4 = (h4 + w4) as usize;

                if dist_4 <= r_4 {
                    let px = (x as isize + w) as usize;
                    let py = (y as isize + h + (r / 2) as isize) as usize;
                    let idx = py * width + px;
                    let gradient = ((r_4 - dist_4) << 8) / (r_3 << 2);
                    if gradient > 255 {
                        pixels[idx] = stroke_packed;
                    } else {
                        // Blend background towards stroke_colour using packed SIMD
                        let alpha = gradient as u64;
                        let inv_alpha = 256 - alpha;

                        // Widen bg pixel to packed channels
                        let mut bg = pixels[idx] as u64;
                        bg = (bg | (bg << 16)) & 0x0000FFFF0000FFFF;
                        bg = (bg | (bg << 8)) & 0x00FF00FF00FF00FF;

                        // Widen stroke colour to packed channels
                        let mut stroke = stroke_packed as u64;
                        stroke = (stroke | (stroke << 16)) & 0x0000FFFF0000FFFF;
                        stroke = (stroke | (stroke << 8)) & 0x00FF00FF00FF00FF;

                        // Blend: bg * inv_alpha + stroke * alpha
                        let mut blended = bg * inv_alpha + stroke * alpha;

                        // Contract back to u32
                        blended = (blended >> 8) & 0x00FF00FF00FF00FF;
                        blended = (blended | (blended >> 8)) & 0x0000FFFF0000FFFF;
                        blended = blended | (blended >> 16);
                        pixels[idx] = blended as u32;
                    }
                }
            }
        }
    }

    pub fn draw_maximize_symbol(
        pixels: &mut [u32],
        width: usize,
        x: usize,
        y: usize,
        r: usize,
        stroke_colour: (u8, u8, u8),
        fill_colour: (u8, u8, u8),
    ) {
        let r = r + 1;
        let mut r_4 = r * r;
        r_4 *= r_4;
        let r_3 = r * r * r;

        // Inner radius (inset by r/6)
        let r_inner = r * 4 / 5;
        let mut r_inner_4 = r_inner * r_inner;
        r_inner_4 *= r_inner_4;
        let r_inner_3 = r_inner * r_inner * r_inner;

        // Edge threshold: gradient spans approximately 4r^3 worth of dist_4 change
        let outer_edge_threshold = r_3 << 2;
        let inner_edge_threshold = r_inner_3 << 2;

        let stroke_packed = pack_argb(stroke_colour.0, stroke_colour.1, stroke_colour.2, 255);
        let fill_packed = pack_argb(fill_colour.0, fill_colour.1, fill_colour.2, 255);

        for h in -(r as isize)..=r as isize {
            for w in -(r as isize)..=r as isize {
                let h2 = h * h;
                let h4 = h2 * h2;
                let w2 = w * w;
                let w4 = w2 * w2;
                let dist_4 = (h4 + w4) as usize;

                if dist_4 <= r_4 {
                    let px = (x as isize + w) as usize;
                    let py = (y as isize + h) as usize;
                    let idx = py * width + px;

                    // Determine which zone we're in
                    let dist_from_outer = r_4 - dist_4;

                    if dist_4 <= r_inner_4 {
                        let dist_from_inner = r_inner_4 - dist_4;

                        // Inside inner squircle
                        if dist_from_inner <= inner_edge_threshold {
                            // Inner edge: blend from stroke to fill using packed SIMD
                            let gradient = ((dist_from_inner) << 8) / inner_edge_threshold;
                            let alpha = gradient as u64;
                            let inv_alpha = 256 - alpha;

                            let mut stroke = stroke_packed as u64;
                            stroke = (stroke | (stroke << 16)) & 0x0000FFFF0000FFFF;
                            stroke = (stroke | (stroke << 8)) & 0x00FF00FF00FF00FF;

                            let mut fill = fill_packed as u64;
                            fill = (fill | (fill << 16)) & 0x0000FFFF0000FFFF;
                            fill = (fill | (fill << 8)) & 0x00FF00FF00FF00FF;

                            let mut blended = stroke * inv_alpha + fill * alpha;
                            blended = (blended >> 8) & 0x00FF00FF00FF00FF;
                            blended = (blended | (blended >> 8)) & 0x0000FFFF0000FFFF;
                            blended = blended | (blended >> 16);
                            pixels[idx] = blended as u32;
                        } else {
                            // Solid fill center
                            pixels[idx] = fill_packed;
                        }
                    } else {
                        // Between inner and outer: stroke ring
                        if dist_from_outer <= outer_edge_threshold {
                            // Outer edge: blend from background to stroke using packed SIMD
                            let gradient = ((dist_from_outer) << 8) / outer_edge_threshold;
                            let alpha = gradient as u64;
                            let inv_alpha = 256 - alpha;

                            let mut bg = pixels[idx] as u64;
                            bg = (bg | (bg << 16)) & 0x0000FFFF0000FFFF;
                            bg = (bg | (bg << 8)) & 0x00FF00FF00FF00FF;

                            let mut stroke = stroke_packed as u64;
                            stroke = (stroke | (stroke << 16)) & 0x0000FFFF0000FFFF;
                            stroke = (stroke | (stroke << 8)) & 0x00FF00FF00FF00FF;

                            let mut blended = bg * inv_alpha + stroke * alpha;
                            blended = (blended >> 8) & 0x00FF00FF00FF00FF;
                            blended = (blended | (blended >> 8)) & 0x0000FFFF0000FFFF;
                            blended = blended | (blended >> 16);
                            pixels[idx] = blended as u32;
                        } else {
                            // Solid stroke ring
                            pixels[idx] = stroke_packed;
                        }
                    }
                }
            }
        }
    }

    pub fn draw_close_symbol(
        pixels: &mut [u32],
        width: usize,
        x: usize,
        y: usize,
        r: usize,
        stroke_colour: (u8, u8, u8),
    ) {
        let r = r + 1;
        // Draw X with antialiased rounded-end diagonals (capsule/pill shaped)
        let thickness = (r / 3).max(1) as f32;
        let radius = thickness / 2.;
        let size = (r * 2) as f32; // X spans diameter, not radius
        let cxf = x as f32;
        let cyf = y as f32;

        let end = size / 3.;

        // Define the two diagonal line segments
        // Diagonal 1: top-left to bottom-right
        let x1_start = cxf - end;
        let y1_start = cyf - end;
        let x1_end = cxf + end;
        let y1_end = cyf + end;

        // Diagonal 2: top-right to bottom-left
        let x2_start = cxf + end;
        let y2_start = cyf - end;
        let x2_end = cxf - end;
        let y2_end = cyf + end;

        // Pack stroke colour once
        let stroke_packed = pack_argb(stroke_colour.0, stroke_colour.1, stroke_colour.2, 255);

        // Scan the bounding box and render both capsules
        let min_x = ((x as i32) - (r as i32)).max(0);
        let max_x = ((x as i32) + (r as i32)).min(width as i32);
        let min_y = ((y as i32) - (r as i32)).max(0);
        let max_y = ((y as i32) + (r as i32)).min(width as i32);

        let cxi = x as i32;
        let cyi = y as i32;

        // Quadrant 1: top-left (diagonal 1)
        for py in min_y..cyi {
            for px in min_x..cxi {
                let px_f = px as f32 + 0.5;
                let py_f = py as f32 + 0.5;

                let dist = Self::distance_to_capsule(
                    px_f, py_f, x1_start, y1_start, x1_end, y1_end, radius,
                );

                let alpha_f = if dist < -0.5 {
                    1.
                } else if dist < 0.5 {
                    0.5 - dist
                } else {
                    0.
                };

                if alpha_f > 0. {
                    let idx = py as usize * width + px as usize;
                    let alpha = (alpha_f * 256.0) as u64;
                    let inv_alpha = 256 - alpha;

                    let mut bg = pixels[idx] as u64;
                    bg = (bg | (bg << 16)) & 0x0000FFFF0000FFFF;
                    bg = (bg | (bg << 8)) & 0x00FF00FF00FF00FF;

                    let mut stroke = stroke_packed as u64;
                    stroke = (stroke | (stroke << 16)) & 0x0000FFFF0000FFFF;
                    stroke = (stroke | (stroke << 8)) & 0x00FF00FF00FF00FF;

                    let mut blended = bg * inv_alpha + stroke * alpha;
                    blended = (blended >> 8) & 0x00FF00FF00FF00FF;
                    blended = (blended | (blended >> 8)) & 0x0000FFFF0000FFFF;
                    blended = blended | (blended >> 16);
                    pixels[idx] = blended as u32;
                }
            }
        }

        // Quadrant 2: top-right (diagonal 2)
        for py in min_y..cyi {
            for px in cxi..max_x {
                let px_f = px as f32 + 0.5;
                let py_f = py as f32 + 0.5;

                let dist = Self::distance_to_capsule(
                    px_f, py_f, x2_start, y2_start, x2_end, y2_end, radius,
                );

                let alpha_f = if dist < -0.5 {
                    1.
                } else if dist < 0.5 {
                    0.5 - dist
                } else {
                    0.
                };

                if alpha_f > 0. {
                    let idx = py as usize * width + px as usize;
                    let alpha = (alpha_f * 256.0) as u64;
                    let inv_alpha = 256 - alpha;

                    let mut bg = pixels[idx] as u64;
                    bg = (bg | (bg << 16)) & 0x0000FFFF0000FFFF;
                    bg = (bg | (bg << 8)) & 0x00FF00FF00FF00FF;

                    let mut stroke = stroke_packed as u64;
                    stroke = (stroke | (stroke << 16)) & 0x0000FFFF0000FFFF;
                    stroke = (stroke | (stroke << 8)) & 0x00FF00FF00FF00FF;

                    let mut blended = bg * inv_alpha + stroke * alpha;
                    blended = (blended >> 8) & 0x00FF00FF00FF00FF;
                    blended = (blended | (blended >> 8)) & 0x0000FFFF0000FFFF;
                    blended = blended | (blended >> 16);
                    pixels[idx] = blended as u32;
                }
            }
        }

        // Quadrant 3: bottom-left (diagonal 2)
        for py in cyi..max_y {
            for px in min_x..cxi {
                let px_f = px as f32 + 0.5;
                let py_f = py as f32 + 0.5;

                let dist = Self::distance_to_capsule(
                    px_f, py_f, x2_start, y2_start, x2_end, y2_end, radius,
                );

                let alpha_f = if dist < -0.5 {
                    1.
                } else if dist < 0.5 {
                    0.5 - dist
                } else {
                    0.
                };

                if alpha_f > 0. {
                    let idx = py as usize * width + px as usize;
                    let alpha = (alpha_f * 256.0) as u64;
                    let inv_alpha = 256 - alpha;

                    let mut bg = pixels[idx] as u64;
                    bg = (bg | (bg << 16)) & 0x0000FFFF0000FFFF;
                    bg = (bg | (bg << 8)) & 0x00FF00FF00FF00FF;

                    let mut stroke = stroke_packed as u64;
                    stroke = (stroke | (stroke << 16)) & 0x0000FFFF0000FFFF;
                    stroke = (stroke | (stroke << 8)) & 0x00FF00FF00FF00FF;

                    let mut blended = bg * inv_alpha + stroke * alpha;
                    blended = (blended >> 8) & 0x00FF00FF00FF00FF;
                    blended = (blended | (blended >> 8)) & 0x0000FFFF0000FFFF;
                    blended = blended | (blended >> 16);
                    pixels[idx] = blended as u32;
                }
            }
        }

        // Quadrant 4: bottom-right (diagonal 1)
        for py in cyi..max_y {
            for px in cxi..max_x {
                let px_f = px as f32 + 0.5;
                let py_f = py as f32 + 0.5;

                let dist = Self::distance_to_capsule(
                    px_f, py_f, x1_start, y1_start, x1_end, y1_end, radius,
                );

                let alpha_f = if dist < -0.5 {
                    1.
                } else if dist < 0.5 {
                    0.5 - dist
                } else {
                    0.
                };

                if alpha_f > 0. {
                    let idx = py as usize * width + px as usize;
                    let alpha = (alpha_f * 256.0) as u64;
                    let inv_alpha = 256 - alpha;

                    let mut bg = pixels[idx] as u64;
                    bg = (bg | (bg << 16)) & 0x0000FFFF0000FFFF;
                    bg = (bg | (bg << 8)) & 0x00FF00FF00FF00FF;

                    let mut stroke = stroke_packed as u64;
                    stroke = (stroke | (stroke << 16)) & 0x0000FFFF0000FFFF;
                    stroke = (stroke | (stroke << 8)) & 0x00FF00FF00FF00FF;

                    let mut blended = bg * inv_alpha + stroke * alpha;
                    blended = (blended >> 8) & 0x00FF00FF00FF00FF;
                    blended = (blended | (blended >> 8)) & 0x0000FFFF0000FFFF;
                    blended = blended | (blended >> 16);
                    pixels[idx] = blended as u32;
                }
            }
        }
    }

    /// Draw magnifying glass icon (circle with diagonal handle)
    pub fn _draw_magnify_symbol(
        pixels: &mut [u32],
        width: usize,
        cx: usize,
        cy: usize,
        size: usize,
        stroke_colour: (u8, u8, u8),
    ) {
        // Geometry based on magnify.svg (1000x1000 viewbox):
        // Circle center at ~(417, 417), radius 292, stroke 83
        // Handle from (625, 625) to (875, 875)
        // Normalize to our size parameter

        let scale = size as f32 / 1000.0;
        let stroke_width = 83.0 * scale;
        let radius = stroke_width / 2.0;

        // Circle parameters (offset from center towards top-left)
        let circle_cx = cx as f32 - 125.0 * scale;
        let circle_cy = cy as f32 - 125.0 * scale;
        let circle_r = 292.0 * scale;

        // Handle endpoints (45° diagonal from bottom-right of circle)
        let handle_start_x = cx as f32 + 83.0 * scale;
        let handle_start_y = cy as f32 + 83.0 * scale;
        let handle_end_x = cx as f32 + 333.0 * scale;
        let handle_end_y = cy as f32 + 333.0 * scale;

        let stroke_packed = pack_argb(stroke_colour.0, stroke_colour.1, stroke_colour.2, 255);

        // Bounding box
        let half_size = (size / 2 + 2) as isize;
        let min_x = (cx as isize - half_size) as usize;
        let max_x = (cx as isize + half_size) as usize;
        let min_y = (cy as isize - half_size) as usize;
        let max_y = (cy as isize + half_size) as usize;

        for py in min_y..max_y {
            for px in min_x..max_x {
                let px_f = px as f32 + 0.5;
                let py_f = py as f32 + 0.5;

                // Distance to circle ring (absolute distance to circle edge minus stroke radius)
                let dx = px_f - circle_cx;
                let dy = py_f - circle_cy;
                let dist_to_center = (dx * dx + dy * dy).sqrt();
                let dist_to_ring = (dist_to_center - circle_r).abs() - radius;

                // Distance to handle capsule
                let dist_to_handle = Self::distance_to_capsule(
                    px_f,
                    py_f,
                    handle_start_x,
                    handle_start_y,
                    handle_end_x,
                    handle_end_y,
                    radius,
                );

                // Use minimum distance (union of shapes)
                let dist = dist_to_ring.min(dist_to_handle);

                // Antialiased rendering
                let alpha_f = if dist < -0.5 {
                    1.0
                } else if dist < 0.5 {
                    0.5 - dist
                } else {
                    0.0
                };

                if alpha_f > 0.0 {
                    let idx = py * width + px;
                    let alpha = (alpha_f * 256.0) as u64;
                    let inv_alpha = 256 - alpha;

                    let mut bg = pixels[idx] as u64;
                    bg = (bg | (bg << 16)) & 0x0000FFFF0000FFFF;
                    bg = (bg | (bg << 8)) & 0x00FF00FF00FF00FF;

                    let mut stroke = stroke_packed as u64;
                    stroke = (stroke | (stroke << 16)) & 0x0000FFFF0000FFFF;
                    stroke = (stroke | (stroke << 8)) & 0x00FF00FF00FF00FF;

                    let mut blended = bg * inv_alpha + stroke * alpha;
                    blended = (blended >> 8) & 0x00FF00FF00FF00FF;
                    blended = (blended | (blended >> 8)) & 0x0000FFFF0000FFFF;
                    blended = blended | (blended >> 16);
                    pixels[idx] = blended as u32;
                }
            }
        }
    }

    /// Draw plus icon (horizontal + vertical bars)
    pub fn draw_plus_symbol(
        pixels: &mut [u32],
        width: usize,
        cx: usize,
        cy: usize,
        size: usize,
        stroke_colour: (u8, u8, u8),
    ) {
        let scale = size as f32 / 1000.0;
        let stroke_width = 120.0 * scale; // Slightly thicker than magnify
        let radius = stroke_width / 2.0;
        let arm_length = 350.0 * scale; // Length from center to end

        let cxf = cx as f32;
        let cyf = cy as f32;

        // Horizontal bar endpoints
        let h_start_x = cxf - arm_length;
        let h_end_x = cxf + arm_length;

        // Vertical bar endpoints
        let v_start_y = cyf - arm_length;
        let v_end_y = cyf + arm_length;

        let stroke_packed = pack_argb(stroke_colour.0, stroke_colour.1, stroke_colour.2, 255);

        // Bounding box
        let half_size = (size / 2 + 2) as isize;
        let min_x = (cx as isize - half_size).max(0) as usize;
        let max_x = (cx as isize + half_size).min(width as isize) as usize;
        let min_y = (cy as isize - half_size).max(0) as usize;
        let max_y = (cy as isize + half_size).min((pixels.len() / width) as isize) as usize;

        for py in min_y..max_y {
            for px in min_x..max_x {
                let px_f = px as f32 + 0.5;
                let py_f = py as f32 + 0.5;

                // Distance to horizontal capsule
                let dist_h = Self::distance_to_capsule(
                    px_f, py_f, h_start_x, cyf, h_end_x, cyf, radius,
                );

                // Distance to vertical capsule
                let dist_v = Self::distance_to_capsule(
                    px_f, py_f, cxf, v_start_y, cxf, v_end_y, radius,
                );

                // Use minimum distance (union of shapes)
                let dist = dist_h.min(dist_v);

                // Antialiased rendering
                let alpha_f = if dist < -0.5 {
                    1.0
                } else if dist < 0.5 {
                    0.5 - dist
                } else {
                    0.0
                };

                if alpha_f > 0.0 {
                    let idx = py * width + px;
                    let alpha = (alpha_f * 256.0) as u64;
                    let inv_alpha = 256 - alpha;

                    let mut bg = pixels[idx] as u64;
                    bg = (bg | (bg << 16)) & 0x0000FFFF0000FFFF;
                    bg = (bg | (bg << 8)) & 0x00FF00FF00FF00FF;

                    let mut stroke = stroke_packed as u64;
                    stroke = (stroke | (stroke << 16)) & 0x0000FFFF0000FFFF;
                    stroke = (stroke | (stroke << 8)) & 0x00FF00FF00FF00FF;

                    let mut blended = bg * inv_alpha + stroke * alpha;
                    blended = (blended >> 8) & 0x00FF00FF00FF00FF;
                    blended = (blended | (blended >> 8)) & 0x0000FFFF0000FFFF;
                    blended = blended | (blended >> 16);
                    pixels[idx] = blended as u32;
                }
            }
        }
    }

    /// Draw hourglass icon (two triangles meeting at center point)
    /// angle_degrees: rotation angle in degrees (stochastic wobble during search)
    pub fn draw_hourglass_symbol(
        pixels: &mut [u32],
        width: usize,
        cx: usize,
        cy: usize,
        size: usize,
        angle_degrees: f32,
        stroke_colour: (u8, u8, u8),
    ) {
        let scale = size as f32 / 1000.0;
        let stroke_width = 83.0 * scale;
        let radius = stroke_width / 2.0;

        let half_h = 400.0 * scale; // Half height of hourglass
        let half_w = 300.0 * scale; // Half width at top/bottom

        // Precompute rotation (inverse rotation for sample point transform)
        let angle_rad = -angle_degrees.to_radians();
        let cos_a = angle_rad.cos();
        let sin_a = angle_rad.sin();
        let cx_f = cx as f32;
        let cy_f = cy as f32;

        // Hourglass vertices in local coords (center at origin)
        // Top triangle: apex at center, base at top
        let top_apex = (0.0_f32, 0.0_f32);
        let top_left = (-half_w, -half_h);
        let top_right = (half_w, -half_h);
        // Bottom triangle: apex at center, base at bottom
        let bot_left = (-half_w, half_h);
        let bot_right = (half_w, half_h);

        let stroke_packed = pack_argb(stroke_colour.0, stroke_colour.1, stroke_colour.2, 255);

        // Bounding box (expanded for rotation)
        let half_size = (size / 2 + 2) as isize;
        let min_x = (cx as isize - half_size).max(0) as usize;
        let max_x = (cx as isize + half_size) as usize;
        let min_y = (cy as isize - half_size).max(0) as usize;
        let max_y = (cy as isize + half_size) as usize;

        for py in min_y..max_y {
            for px in min_x..max_x {
                // Rotate sample point into hourglass local space (inverse rotation)
                let dx = px as f32 + 0.5 - cx_f;
                let dy = py as f32 + 0.5 - cy_f;
                let lx = dx * cos_a - dy * sin_a;
                let ly = dx * sin_a + dy * cos_a;

                // Distance to each line segment of the hourglass (6 edges total)
                // Top triangle edges
                let d1 = Self::distance_to_capsule_local(
                    lx,
                    ly,
                    top_left.0,
                    top_left.1,
                    top_right.0,
                    top_right.1,
                    radius,
                );
                let d2 = Self::distance_to_capsule_local(
                    lx, ly, top_left.0, top_left.1, top_apex.0, top_apex.1, radius,
                );
                let d3 = Self::distance_to_capsule_local(
                    lx,
                    ly,
                    top_right.0,
                    top_right.1,
                    top_apex.0,
                    top_apex.1,
                    radius,
                );

                // Bottom triangle edges
                let d4 = Self::distance_to_capsule_local(
                    lx,
                    ly,
                    bot_left.0,
                    bot_left.1,
                    bot_right.0,
                    bot_right.1,
                    radius,
                );
                let d5 = Self::distance_to_capsule_local(
                    lx, ly, bot_left.0, bot_left.1, top_apex.0, top_apex.1, radius,
                );
                let d6 = Self::distance_to_capsule_local(
                    lx,
                    ly,
                    bot_right.0,
                    bot_right.1,
                    top_apex.0,
                    top_apex.1,
                    radius,
                );

                // Minimum distance (union of all edges)
                let dist = d1.min(d2).min(d3).min(d4).min(d5).min(d6);

                // Antialiased rendering
                let alpha_f = if dist < -0.5 {
                    1.0
                } else if dist < 0.5 {
                    0.5 - dist
                } else {
                    0.0
                };

                if alpha_f > 0.0 {
                    let idx = py * width + px;
                    let alpha = (alpha_f * 256.0) as u64;
                    let inv_alpha = 256 - alpha;

                    let mut bg = pixels[idx] as u64;
                    bg = (bg | (bg << 16)) & 0x0000FFFF0000FFFF;
                    bg = (bg | (bg << 8)) & 0x00FF00FF00FF00FF;

                    let mut stroke = stroke_packed as u64;
                    stroke = (stroke | (stroke << 16)) & 0x0000FFFF0000FFFF;
                    stroke = (stroke | (stroke << 8)) & 0x00FF00FF00FF00FF;

                    let mut blended = bg * inv_alpha + stroke * alpha;
                    blended = (blended >> 8) & 0x00FF00FF00FF00FF;
                    blended = (blended | (blended >> 8)) & 0x0000FFFF0000FFFF;
                    blended = blended | (blended >> 16);
                    pixels[idx] = blended as u32;
                }
            }
        }
    }

    // Helper: distance to capsule in local coords (no center offset needed)
    #[inline]
    fn distance_to_capsule_local(
        px: f32,
        py: f32,
        x1: f32,
        y1: f32,
        x2: f32,
        y2: f32,
        radius: f32,
    ) -> f32 {
        let dx = x2 - x1;
        let dy = y2 - y1;
        let len_sq = dx * dx + dy * dy;

        let t = if len_sq > 0.0 {
            ((px - x1) * dx + (py - y1) * dy) / len_sq
        } else {
            0.0
        };
        let t = t.clamp(0.0, 1.0);

        let closest_x = x1 + t * dx;
        let closest_y = y1 + t * dy;
        let dist_x = px - closest_x;
        let dist_y = py - closest_y;

        (dist_x * dist_x + dist_y * dist_y).sqrt() - radius
    }

    // Helper function: distance from point to capsule (line segment with rounded ends)
    pub fn distance_to_capsule(
        px: f32,
        py: f32,
        x1: f32,
        y1: f32,
        x2: f32,
        y2: f32,
        radius: f32,
    ) -> f32 {
        // Vector from start to end
        let dx = x2 - x1;
        let dy = y2 - y1;
        let len_sq = dx * dx + dy * dy;

        // Project point onto line segment (clamped to [0, 1])
        let t = ((px - x1) * dx + (py - y1) * dy) / len_sq;
        let t_clamped = t.clamp(0., 1.);

        // Closest point on line segment
        let closest_x = x1 + t_clamped * dx;
        let closest_y = y1 + t_clamped * dy;

        // Distance to closest point minus radius
        let dist_x = px - closest_x;
        let dist_y = py - closest_y;
        (dist_x * dist_x + dist_y * dist_y).sqrt() - radius
    }

    // Motion triggered by network action, motion speed dependent on latency
    // Delegates to shared drawing module
    pub fn draw_background_texture(
        pixels: &mut [u32],
        width: usize,
        height: usize,
        speckle: usize,
        fullscreen: bool,
        scroll_offset: isize,
    ) {
        super::drawing::draw_background_texture(pixels, width, height, speckle, fullscreen, scroll_offset);
    }

    /// Draw window edge hairlines and apply squircle alpha mask
    pub fn draw_window_edges_and_mask(
        pixels: &mut [u32],
        hit_test_map: &mut [u8],
        width: u32,
        height: u32,
        start: usize,
        crossings: &[(u16, u8, u8)],
    ) {
        let light_colour = theme::WINDOW_LIGHT_EDGE;
        let shadow_colour = theme::WINDOW_SHADOW_EDGE;

        // Fill all four edges with white before squircle clipping
        // Top edge
        for x in 0..width {
            let idx = 0 * width + x;
            pixels[idx as usize] = light_colour;
        }

        // Bottom edge
        for x in 0..width {
            let idx = (height - 1) * width + x;
            pixels[idx as usize] = shadow_colour;
        }

        // Left edge
        for y in 0..height {
            let idx = y * width + 0;
            pixels[idx as usize] = light_colour;
        }

        // Right edge
        for y in 0..height {
            let idx = y * width + (width - 1);
            pixels[idx as usize] = shadow_colour;
        }

        // Fill four corner squares and clear hitmap
        for row in 0..start {
            for col in 0..start {
                let idx = row * width as usize + col;
                pixels[idx] = 0;
                hit_test_map[idx] = HIT_NONE;
            }
        }
        for row in 0..start {
            for col in (width as usize - start)..width as usize {
                let idx = row * width as usize + col;
                pixels[idx] = 0;
                hit_test_map[idx] = HIT_NONE;
            }
        }
        for row in (height as usize - start)..height as usize {
            for col in 0..start {
                let idx = row * width as usize + col;
                pixels[idx] = 0;
                hit_test_map[idx] = HIT_NONE;
            }
        }
        for row in (height as usize - start)..height as usize {
            for col in (width as usize - start)..width as usize {
                let idx = row * width as usize + col;
                pixels[idx] = 0;
                hit_test_map[idx] = HIT_NONE;
            }
        }

        // Top left/right edges
        let mut y_top = start;
        for crossing in 0..crossings.len() {
            let (inset, l, h) = crossings[crossing];
            // Left edge fill
            for idx in y_top * width as usize..y_top * width as usize + inset as usize {
                pixels[idx] = 0;
                hit_test_map[idx] = HIT_NONE;
            }

            // Left edge outer pixel
            let pixel_idx = y_top * width as usize + inset as usize;
            pixels[pixel_idx] = scale_alpha(light_colour, h);
            if h < 255 {
                hit_test_map[pixel_idx] = HIT_NONE; // NEEDS FIXED!!!
            }

            // Left edge inner pixel
            let pixel_idx = pixel_idx + 1;
            pixels[pixel_idx] = blend_rgb_only(pixels[pixel_idx], light_colour, h, l);

            // Right edge inner pixel
            let pixel_idx = y_top * width as usize + width as usize - 2 - inset as usize;
            pixels[pixel_idx] = blend_rgb_only(pixels[pixel_idx], shadow_colour, h, l);

            // Right edge outer pixel
            let pixel_idx = pixel_idx + 1;
            pixels[pixel_idx] = scale_alpha(shadow_colour, h);
            if h < 255 {
                hit_test_map[pixel_idx] = HIT_NONE;
            }

            // Right edge fill
            for idx in (y_top * width as usize + width as usize - inset as usize)
                ..((y_top + 1) * width as usize)
            {
                pixels[idx] = 0;
                hit_test_map[idx] = HIT_NONE;
            }
            y_top += 1;
        }

        // Bottom left/right edges
        let mut y_bottom = height as usize - start - 1;
        for crossing in 0..crossings.len() {
            let (inset, l, h) = crossings[crossing];

            // Left edge fill
            for idx in y_bottom * width as usize..y_bottom * width as usize + inset as usize {
                pixels[idx] = 0;
                hit_test_map[idx] = HIT_NONE;
            }

            // Left outer edge pixel
            let pixel_idx = y_bottom * width as usize + inset as usize;
            pixels[pixel_idx] = scale_alpha(light_colour, h);
            if h < 255 {
                hit_test_map[pixel_idx] = HIT_NONE;
            }

            // Left inner edge pixel
            let pixel_idx = pixel_idx + 1;
            pixels[pixel_idx] = blend_rgb_only(pixels[pixel_idx], light_colour, h, l);

            // Right edge inner pixel
            let pixel_idx = y_bottom * width as usize + width as usize - 2 - inset as usize;
            pixels[pixel_idx] = blend_rgb_only(pixels[pixel_idx], shadow_colour, h, l);

            // Right edge outer pixel
            let pixel_idx = pixel_idx + 1;
            pixels[pixel_idx] = scale_alpha(shadow_colour, h);
            if h < 255 {
                hit_test_map[pixel_idx] = HIT_NONE;
            }

            // Right edge fill
            for idx in (y_bottom * width as usize + width as usize - inset as usize)
                ..((y_bottom + 1) * width as usize)
            {
                pixels[idx] = 0;
                hit_test_map[idx] = HIT_NONE;
            }

            y_bottom -= 1;
        }

        // Left side top/bottom edges
        let mut x_left = start;
        for crossing in 0..crossings.len() {
            let (inset, l, h) = crossings[crossing];

            // Top edge fill
            for row in 0..inset as usize {
                let idx = row * width as usize + x_left;
                pixels[idx] = 0;
                hit_test_map[idx] = HIT_NONE;
            }

            // Top outer edge pixel
            let pixel_idx = inset as usize * width as usize + x_left;
            pixels[pixel_idx] = scale_alpha(light_colour, h);
            if h < 255 {
                hit_test_map[pixel_idx] = HIT_NONE;
            }

            // Top inner edge pixel
            let pixel_idx = (inset as usize + 1) * width as usize + x_left;
            pixels[pixel_idx] = blend_rgb_only(pixels[pixel_idx], light_colour, h, l);

            // Bottom outer edge pixel
            let pixel_idx = (height as usize - 1 - inset as usize) * width as usize + x_left;
            pixels[pixel_idx] = scale_alpha(shadow_colour, h);
            if h < 255 {
                hit_test_map[pixel_idx] = HIT_NONE;
            }

            // Bottom inner edge pixel
            let pixel_idx = (height as usize - 2 - inset as usize) * width as usize + x_left;
            pixels[pixel_idx] = blend_rgb_only(pixels[pixel_idx], shadow_colour, h, l);

            // Bottom edge fill
            for row in (height as usize - inset as usize)..height as usize {
                let idx = row * width as usize + x_left;
                pixels[idx] = 0;
                hit_test_map[idx] = HIT_NONE;
            }

            x_left += 1;
        }

        // Right side top/bottom edges
        let mut x_right = width as usize - start - 1;
        for crossing in 0..crossings.len() {
            let (inset, l, h) = crossings[crossing];

            // Top edge fill
            for row in 0..inset as usize {
                let idx = row * width as usize + x_right;
                pixels[idx] = 0;
                hit_test_map[idx] = HIT_NONE;
            }

            // Top outer edge pixel
            let pixel_idx = inset as usize * width as usize + x_right;
            pixels[pixel_idx] = scale_alpha(light_colour, h);
            if h < 255 {
                hit_test_map[pixel_idx] = HIT_NONE;
            }

            // Top inner edge pixel
            let pixel_idx = (inset as usize + 1) * width as usize + x_right;
            pixels[pixel_idx] = blend_rgb_only(pixels[pixel_idx], light_colour, h, l);

            // Bottom outer edge pixel
            let pixel_idx = (height as usize - 1 - inset as usize) * width as usize + x_right;
            pixels[pixel_idx] = scale_alpha(shadow_colour, h);
            if h < 255 {
                hit_test_map[pixel_idx] = HIT_NONE;
            }

            // Bottom inner edge pixel
            let pixel_idx = (height as usize - 2 - inset as usize) * width as usize + x_right;
            pixels[pixel_idx] = blend_rgb_only(pixels[pixel_idx], shadow_colour, h, l);

            // Bottom edge fill
            for row in (height as usize - inset as usize)..height as usize {
                let idx = row * width as usize + x_right;
                pixels[idx] = 0;
                hit_test_map[idx] = HIT_NONE;
            }

            x_right -= 1;
        }
    }

    /// Apply hover effect to button using cached pixel list
    pub fn draw_button_hover_by_pixels(
        pixels: &mut [u32],
        pixel_list: &[usize],
        hover: bool,
        button_type: HoveredButton,
    ) {
        // Get the hover deltas for this button type (packed u32, already platform-adjusted via fmt())
        let hover_delta = match button_type {
            HoveredButton::Close => theme::CLOSE_HOVER,
            HoveredButton::Maximize => theme::MAXIMIZE_HOVER,
            HoveredButton::Minimize => theme::MINIMIZE_HOVER,
            HoveredButton::Textbox => theme::TEXTBOX_HOVER,
            HoveredButton::QueryButton => theme::QUERY_BUTTON_HOVER,
            HoveredButton::BackHeader => theme::BACK_HEADER_HOVER,
            HoveredButton::None => 0,
        };

        // Add/sub directly on packed u32 (deltas chosen to never overflow)
        for &hit_idx in pixel_list {
            pixels[hit_idx] = if hover {
                pixels[hit_idx].wrapping_add(hover_delta)
            } else {
                pixels[hit_idx].wrapping_sub(hover_delta)
            };
        }
    }

    /// Calculate selection rectangle bounds for hover skip logic
    /// Returns (x_start, x_end, y_top, y_bottom) or None if no selection
    fn selection_skip_rect(&self) -> Option<(usize, usize, usize, usize)> {
        let anchor = self.current_text_state.selection_anchor?;
        let (sel_start, sel_end) = if anchor < self.current_text_state.blinkey_index {
            (anchor, self.current_text_state.blinkey_index)
        } else if anchor > self.current_text_state.blinkey_index {
            (self.current_text_state.blinkey_index, anchor)
        } else {
            return None; // No selection (anchor == cursor)
        };

        if sel_start >= sel_end || sel_start >= self.current_text_state.widths.len() {
            return None;
        }

        let text = &self.current_text_state;
        let layout = &self.text_layout;

        let sel_start_px: usize = text.widths[..sel_start].iter().sum();
        let sel_end_px: usize = text.widths[..sel_end.min(text.widths.len())].iter().sum();

        let text_half = (text.width / 2) as f32;
        let text_start_x = layout.usable_center as f32 - text_half + text.scroll_offset;
        let sel_x_start = (text_start_x + sel_start_px as f32).max(0.0) as usize;
        let sel_x_end = (text_start_x + sel_end_px as f32).max(0.0) as usize;

        let sel_y_top = (layout.center_y as f32 - layout.font_size / 2.0).max(0.0) as usize;
        let sel_y_bottom = (layout.center_y as f32 + layout.font_size / 2.0).max(0.0) as usize;

        Some((sel_x_start, sel_x_end, sel_y_top, sel_y_bottom))
    }

    /// Apply hover effect using centerpoint fill algorithm
    /// Starts from element center, scans vertically then horizontally based on hit test map
    pub fn draw_hover_centerpoint(
        pixels: &mut [u32],
        hit_test_map: &[u8],
        window_width: usize,
        window_height: usize,
        center_x: usize,
        center_y: usize,
        hit_id: u8,
        hover: bool,
        hover_delta: u32,
        debug: bool,
        skip_rect: Option<(usize, usize, usize, usize)>, // (x_start, x_end, y_top, y_bottom)
    ) {
        // Debug: draw magenta pixel at centerpoint
        // Use alpha=254 so we can distinguish it from actual magenta UI elements
        if debug {
            let debug_idx = center_y * window_width + center_x;
            pixels[debug_idx] = 0xFE_FF_00_FF; // Magenta with alpha=254
        }

        // 1. Find vertical extent by scanning up/down from center
        let mut top_y = center_y;
        let mut bottom_y = center_y;

        // Scan upward
        while top_y > 0 {
            let idx = top_y * window_width + center_x;
            if hit_test_map[idx] != hit_id {
                top_y += 1; // Back up one
                break;
            }
            top_y -= 1;
        }

        // Scan downward
        while bottom_y < window_height - 1 {
            let idx = bottom_y * window_width + center_x;
            if hit_test_map[idx] != hit_id {
                bottom_y -= 1; // Back up one
                break;
            }
            bottom_y += 1;
        }

        // 2. For each row in vertical range, scan left/right and apply hover effect
        for y in top_y..=bottom_y {
            let row_start = y * window_width;

            // Find left extent
            let mut left_x = center_x;
            while left_x > 0 {
                let idx = row_start + left_x;
                if hit_test_map[idx] != hit_id {
                    left_x += 1; // Back up one
                    break;
                }
                left_x -= 1;
            }

            // Find right extent
            let mut right_x = center_x;
            while right_x < window_width - 1 {
                let idx = row_start + right_x;
                if hit_test_map[idx] != hit_id {
                    right_x -= 1; // Back up one
                    break;
                }
                right_x += 1;
            }

            // Apply hover effect to this row
            for x in left_x..=right_x {
                // Skip pixels inside selection rect (avoid double-XOR with selection invert)
                if let Some((sel_x_start, sel_x_end, sel_y_top, sel_y_bottom)) = skip_rect {
                    if x >= sel_x_start && x < sel_x_end && y >= sel_y_top && y < sel_y_bottom {
                        continue;
                    }
                }

                let idx = row_start + x;
                if hit_test_map[idx] == hit_id {
                    if debug {
                        if pixels[idx] == 0xFE_FF_00_FF {
                            continue;
                        }
                    }

                    // Add/sub directly on packed u32 (deltas have 0xFF alpha to absorb RGB carry)
                    pixels[idx] = if hover {
                        pixels[idx].wrapping_add(hover_delta)
                    } else {
                        pixels[idx].wrapping_sub(hover_delta)
                    };
                }
            }
        }
    }

    /// Apply hover effect to conversation back header
    /// Adds/subtracts brightness to header area
    pub fn apply_back_header_hover(
        pixels: &mut [u32],
        hit_test_map: &[u8],
        width: usize,
        header_height: usize,
        hover: bool,
    ) {
        // Add/sub directly on packed u32 (delta chosen to never overflow)
        let delta = theme::BACK_HEADER_HOVER;

        for y in 0..header_height {
            for x in 0..width {
                let idx = y * width + x;
                if hit_test_map[idx] == HIT_BACK_HEADER {
                    pixels[idx] = if hover {
                        pixels[idx].wrapping_add(delta)
                    } else {
                        pixels[idx].wrapping_sub(delta)
                    };
                }
            }
        }
    }

    /// Draw vertical hairlines between buttons
    pub fn draw_button_hairlines(
        pixels: &mut [u32],
        hit_test_map: &mut [u8],
        window_width: u32,
        window_height: u32,
        button_x_start: usize,
        button_height: usize,
        _start: usize,
        _crossings: &[(u16, u8, u8)],
    ) {
        let width = window_width as usize;
        let y_start = 0;

        // button_width equals button_height (passed in, already scaled with span * ru)
        let button_width = button_height;

        // Two hairlines: at 1.0 and 2.0 button widths from button area start
        // Left hairline between minimize and maximize
        let left_px = button_x_start + button_width;
        // Right hairline between maximize and close
        let right_px = button_x_start + button_width * 2;

        // Start from vertical center and draw upward until we hit transparency
        let center_y = y_start + button_height / 2;

        // Edge/hairline colour
        let edge_colour = theme::WINDOW_CONTROLS_HAIRLINE;

        // Draw left hairline
        // Draw upward from center until colour changes
        let center_colour = pixels[center_y * width + left_px];
        for py in (y_start..=center_y).rev() {
            let idx = py * width + left_px;
            let diff = pixels[idx] != center_colour;
            pixels[idx] = edge_colour;
            hit_test_map[idx] = HIT_NONE;
            if diff {
                break;
            }
        }

        // Draw downward from center+1 until colour changes
        for py in (center_y + 1)..(y_start + button_height) {
            let idx = py * width + left_px;
            let diff = pixels[idx] != center_colour;
            pixels[idx] = edge_colour;
            hit_test_map[idx] = HIT_NONE;
            if diff {
                break;
            }
        }

        // Draw right hairline
        // Draw upward from center until colour changes
        let center_colour_right = pixels[center_y * width + right_px];
        for py in (y_start..=center_y).rev() {
            let idx = py * width + right_px;
            let diff = pixels[idx] != center_colour_right;
            pixels[idx] = edge_colour;
            hit_test_map[idx] = HIT_NONE;
            if diff {
                break;
            }
        }

        // Draw downward from center+1 until colour changes
        for py in (center_y + 1)..(y_start + button_height) {
            let idx = py * width + right_px;
            let diff = pixels[idx] != center_colour_right;
            pixels[idx] = edge_colour;
            hit_test_map[idx] = HIT_NONE;
            if diff {
                break;
            }
        }
    }

    pub fn draw_textbox(
        pixels: &mut [u32],
        hit_test_map: &mut [u8],
        hit_id: u8,
        textbox_mask: &mut [u8],
        window_width: usize,
        center_x: usize,
        center_y: isize,  // Signed to handle negative scroll positions
        box_width: usize,
        box_height: usize,
    ) {
        // Buffer length for bounds checking (scroll can push textbox partially off-screen)
        let buf_len = pixels.len();
        let height = buf_len / window_width;
        let height_signed = height as isize;

        // Convert from center coordinates to top-left (signed for correct off-screen handling)
        let x = center_x.wrapping_sub(box_width / 2);
        let y_signed = center_y - (box_height as isize / 2);
        // Wrapped version for existing code that still uses usize (edge pixels)
        let y = if y_signed >= 0 { y_signed as usize } else { 0usize.wrapping_sub((-y_signed) as usize) };

        // WHY: Check if textbox overlaps visible region [0, height) using signed math
        // PROOF: top = y_signed, bottom = y_signed + box_height; visible if top < height AND bottom > 0
        // PREVENTS: Drawing when entirely off-screen, while allowing partial visibility
        let box_top = y_signed;
        let box_bottom = y_signed + box_height as isize;
        if box_bottom <= 0 || box_top >= height_signed {
            return; // Entirely off-screen
        }

        let light_colour = theme::TEXTBOX_LIGHT_EDGE;
        let shadow_colour = theme::TEXTBOX_SHADOW_EDGE;
        let fill_colour = theme::TEXTBOX_FILL;
        // Pill-shaped: radius = height/2 gives semicircular ends
        let radius = box_height as f32 / 2.;
        let squirdleyness = 3;

        // Generate crossings from edge (radius/12 o'clock) toward diagonal (1:30)
        let mut crossings: Vec<(u16, u8, u8)> = Vec::new();
        let mut offset = 0f32;

        loop {
            let y_norm = offset / radius;
            let x_norm = (1. - y_norm.powi(squirdleyness)).powf(1. / squirdleyness as f32);
            let x = x_norm * radius;
            let inset = radius - x;

            if inset >= 0. {
                let l = (inset.fract().sqrt() * 256.) as u8;
                let h = ((1. - inset.fract()).sqrt() * 256.) as u8;
                crossings.push((inset as u16, l, h));
            }

            // Stop at 45-degree diagonal (when x < offset)
            if x < offset {
                break;
            }

            offset += 1.;
        }

        // (height already computed above for early-out check)

        // Top-left corner - vertical edge with diagonal fill
        // WHY bounds checks: Scroll can push textbox partially off-screen (negative Y wraps to huge usize,
        // or Y exceeds height). X could also exceed width on narrow windows.
        // PROOF: wrapping_add/wrapping_sub produce wrapped coordinates when textbox is off-screen.
        // PREVENTS: Out-of-bounds pixel buffer access when textbox is partially visible.
        for (i, &(inset, l, h)) in crossings.iter().enumerate() {
            // Stop at diagonal - when inset exceeds i, we've gone past the 45-degree point
            if inset as usize > i {
                break;
            }

            let py = y.wrapping_add(radius as usize).wrapping_sub(i); // Start at horizontal center, go up
            let px = x.wrapping_add(inset as usize);

            // Outer antialiased pixel (bounds check justified above)
            if py < height && px < window_width {
                let idx = py * window_width + px;
                pixels[idx] = blend_rgb_only(pixels[idx], light_colour, l, h);
            }

            // Inner antialiased pixel
            let px1 = px.wrapping_add(1);
            if py < height && px1 < window_width {
                let idx = py * window_width + px1;
                pixels[idx] = blend_rgb_only(light_colour, fill_colour, l, h);
                hit_test_map[idx] = hit_id;
                textbox_mask[idx] = h;
            }

            // Fill horizontally to the diagonal (where horizontal edge would be)
            if py < height {
                let diag_x = x.wrapping_add(radius as usize).wrapping_sub(i).min(window_width);
                for fill_x in (px.wrapping_add(2))..=diag_x {
                    if fill_x >= window_width { continue; }
                    let idx = py * window_width + fill_x;
                    pixels[idx] = fill_colour;
                    hit_test_map[idx] = hit_id;
                    textbox_mask[idx] = 255;
                }
            }

            // Horizontal edge - Outer antialiased pixel
            let hx = x.wrapping_add(radius as usize).wrapping_sub(i); // Start at vertical center, go left
            let hy = y.wrapping_add(inset as usize); // Distance from top edge

            if hy < height && hx < window_width {
                let idx = hy * window_width + hx;
                pixels[idx] = blend_rgb_only(pixels[idx], light_colour, l, h);
            }

            // Horizontal edge - Inner antialiased pixel (below the outer)
            let hy1 = hy.wrapping_add(1);
            if hy1 < height && hx < window_width {
                let idx = hy1 * window_width + hx;
                pixels[idx] = blend_rgb_only(light_colour, fill_colour, l, h);
                hit_test_map[idx] = hit_id;
                textbox_mask[idx] = h;
            }

            // Fill vertically between horizontal edge and diagonal
            // WHY: Use signed arithmetic to handle negative Y when scrolled off top
            // PROOF: y_signed can be negative, clamping to [0, height) gives visible portion
            // PREVENTS: Infinite loop from wrapped usize, fills only visible pixels
            let hy_signed = y_signed + inset as isize;
            let diag_y_signed = y_signed + radius as isize - i as isize;
            let fill_start = (hy_signed + 2).max(0).min(height_signed) as usize;
            let fill_end = diag_y_signed.max(0).min(height_signed) as usize;
            if hx < window_width && fill_start < fill_end {
                for fill_y in fill_start..fill_end {
                    let idx = fill_y * window_width + hx;
                    pixels[idx] = fill_colour;
                    hit_test_map[idx] = hit_id;
                    textbox_mask[idx] = 255;
                }
            }
        }

        // Top-right corner - mirror of top-left (flip x)
        // (bounds checks justified in top-left comment: scroll causes partial visibility)
        for (i, &(inset, l, h)) in crossings.iter().enumerate() {
            if inset as usize > i {
                break;
            }

            let py = y.wrapping_add(radius as usize).wrapping_sub(i);
            let px = x.wrapping_add(box_width).wrapping_sub(1).wrapping_sub(inset as usize);

            // Vertical edge - Outer antialiased pixel
            if py < height && px < window_width {
                let idx = py * window_width + px;
                pixels[idx] = blend_rgb_only(pixels[idx], shadow_colour, l, h);
            }

            // Vertical edge - Inner antialiased pixel
            let px1 = px.wrapping_sub(1);
            if py < height && px1 < window_width {
                let idx = py * window_width + px1;
                pixels[idx] = blend_rgb_only(shadow_colour, fill_colour, l, h);
                hit_test_map[idx] = hit_id;
                textbox_mask[idx] = h;
            }

            // Fill horizontally to the diagonal
            if py < height {
                let diag_x = x.wrapping_add(box_width).wrapping_sub(1).wrapping_sub(radius as usize).wrapping_add(i);
                for fill_x in diag_x..px.wrapping_sub(1) {
                    if fill_x >= window_width { continue; }
                    let idx = py * window_width + fill_x;
                    pixels[idx] = fill_colour;
                    hit_test_map[idx] = hit_id;
                    textbox_mask[idx] = 255;
                }
            }

            // Horizontal edge - Outer antialiased pixel
            let hx = x.wrapping_add(box_width).wrapping_sub(1).wrapping_sub(radius as usize).wrapping_add(i);
            let hy = y.wrapping_add(inset as usize);

            if hy < height && hx < window_width {
                let idx = hy * window_width + hx;
                pixels[idx] = blend_rgb_only(pixels[idx], light_colour, l, h);
            }

            // Horizontal edge - Inner antialiased pixel
            let hy1 = hy.wrapping_add(1);
            if hy1 < height && hx < window_width {
                let idx = hy1 * window_width + hx;
                pixels[idx] = blend_rgb_only(light_colour, fill_colour, l, h);
                hit_test_map[idx] = hit_id;
                textbox_mask[idx] = h;
            }

            // Fill vertically between horizontal edge and diagonal
            // WHY: Use signed arithmetic to handle negative Y when scrolled off top
            // PROOF: y_signed can be negative, clamping to [0, height) gives visible portion
            // PREVENTS: Infinite loop from wrapped usize, fills only visible pixels
            let hy_signed = y_signed + inset as isize;
            let diag_y_signed = y_signed + radius as isize - i as isize;
            let fill_start = (hy_signed + 2).max(0).min(height_signed) as usize;
            let fill_end = diag_y_signed.max(0).min(height_signed) as usize;
            if hx < window_width && fill_start < fill_end {
                for fill_y in fill_start..fill_end {
                    let idx = fill_y * window_width + hx;
                    pixels[idx] = fill_colour;
                    hit_test_map[idx] = hit_id;
                    textbox_mask[idx] = 255;
                }
            }
        }

        // Bottom-left corner - mirror of top-left (flip y), shifted down 1 to avoid overlap
        // (bounds checks justified in top-left comment: scroll causes partial visibility)
        for (i, &(inset, l, h)) in crossings.iter().enumerate() {
            if inset as usize > i {
                break;
            }

            let py = y.wrapping_add(box_height).wrapping_sub(radius as usize).wrapping_add(i);
            let px = x.wrapping_add(inset as usize);

            // Vertical edge - Outer antialiased pixel
            if py < height && px < window_width {
                let idx = py * window_width + px;
                pixels[idx] = blend_rgb_only(pixels[idx], light_colour, l, h);
            }

            // Vertical edge - Inner antialiased pixel
            let px1 = px.wrapping_add(1);
            if py < height && px1 < window_width {
                let idx = py * window_width + px1;
                pixels[idx] = blend_rgb_only(light_colour, fill_colour, l, h);
                hit_test_map[idx] = hit_id;
                textbox_mask[idx] = h;
            }

            // Fill horizontally to the diagonal
            if py < height {
                let diag_x = x.wrapping_add(radius as usize).wrapping_sub(i).min(window_width);
                for fill_x in (px.wrapping_add(2))..=diag_x {
                    if fill_x >= window_width { continue; }
                    let idx = py * window_width + fill_x;
                    pixels[idx] = fill_colour;
                    hit_test_map[idx] = hit_id;
                    textbox_mask[idx] = 255;
                }
            }

            // Horizontal edge - Outer antialiased pixel
            let hx = x.wrapping_add(radius as usize).wrapping_sub(i);
            let hy = y.wrapping_add(box_height).wrapping_sub(inset as usize);

            if hy < height && hx < window_width {
                let idx = hy * window_width + hx;
                pixels[idx] = blend_rgb_only(pixels[idx], shadow_colour, l, h);
            }

            // Horizontal edge - Inner antialiased pixel
            let hy1 = hy.wrapping_sub(1);
            if hy1 < height && hx < window_width {
                let idx = hy1 * window_width + hx;
                pixels[idx] = blend_rgb_only(shadow_colour, fill_colour, l, h);
                hit_test_map[idx] = hit_id;
                textbox_mask[idx] = h;
            }

            // Fill vertically between diagonal and horizontal edge
            // WHY: Use signed arithmetic to handle off-screen scroll
            // PROOF: y_signed can be negative or exceed height, clamping gives visible portion
            // PREVENTS: Infinite loop from wrapped usize, fills only visible pixels
            let diag_y_signed = y_signed + box_height as isize - radius as isize + i as isize;
            let hy_signed = y_signed + box_height as isize - inset as isize;
            let fill_start = (diag_y_signed + 1).max(0).min(height_signed) as usize;
            let fill_end = (hy_signed - 1).max(0).min(height_signed) as usize;
            if hx < window_width && fill_start < fill_end {
                for fill_y in fill_start..fill_end {
                    let idx = fill_y * window_width + hx;
                    pixels[idx] = fill_colour;
                    hit_test_map[idx] = hit_id;
                    textbox_mask[idx] = 255;
                }
            }
        }

        // Bottom-right corner - mirror of top-left (flip both x and y), shifted down 1 to avoid overlap
        // (bounds checks justified in top-left comment: scroll causes partial visibility)
        for (i, &(inset, l, h)) in crossings.iter().enumerate() {
            if inset as usize > i {
                break;
            }

            let py = y.wrapping_add(box_height).wrapping_sub(radius as usize).wrapping_add(i);
            let px = x.wrapping_add(box_width).wrapping_sub(1).wrapping_sub(inset as usize);

            // Vertical edge - Outer antialiased pixel
            if py < height && px < window_width {
                let idx = py * window_width + px;
                pixels[idx] = blend_rgb_only(pixels[idx], shadow_colour, l, h);
            }

            // Vertical edge - Inner antialiased pixel
            let px1 = px.wrapping_sub(1);
            if py < height && px1 < window_width {
                let idx = py * window_width + px1;
                pixels[idx] = blend_rgb_only(shadow_colour, fill_colour, l, h);
                hit_test_map[idx] = hit_id;
                textbox_mask[idx] = h;
            }

            // Fill horizontally to the diagonal
            if py < height {
                let diag_x = x.wrapping_add(box_width).wrapping_sub(1).wrapping_sub(radius as usize).wrapping_add(i);
                for fill_x in diag_x..px.wrapping_sub(1) {
                    if fill_x >= window_width { continue; }
                    let idx = py * window_width + fill_x;
                    pixels[idx] = fill_colour;
                    hit_test_map[idx] = hit_id;
                    textbox_mask[idx] = 255;
                }
            }

            // Horizontal edge - Outer antialiased pixel
            let hx = x.wrapping_add(box_width).wrapping_sub(1).wrapping_sub(radius as usize).wrapping_add(i);
            let hy = y.wrapping_add(box_height).wrapping_sub(inset as usize);

            if hy < height && hx < window_width {
                let idx = hy * window_width + hx;
                pixels[idx] = blend_rgb_only(pixels[idx], shadow_colour, l, h);
            }

            // Horizontal edge - Inner antialiased pixel
            let hy1 = hy.wrapping_sub(1);
            if hy1 < height && hx < window_width {
                let idx = hy1 * window_width + hx;
                pixels[idx] = blend_rgb_only(shadow_colour, fill_colour, l, h);
                hit_test_map[idx] = hit_id;
                textbox_mask[idx] = h;
            }

            // Fill vertically between diagonal and horizontal edge
            // WHY: Use signed arithmetic to handle off-screen scroll
            // PROOF: y_signed can be negative or exceed height, clamping gives visible portion
            // PREVENTS: Infinite loop from wrapped usize, fills only visible pixels
            let diag_y_signed = y_signed + box_height as isize - radius as isize + i as isize;
            let hy_signed = y_signed + box_height as isize - inset as isize;
            let fill_start = (diag_y_signed + 1).max(0).min(height_signed) as usize;
            let fill_end = (hy_signed - 1).max(0).min(height_signed) as usize;
            if hx < window_width && fill_start < fill_end {
                for fill_y in fill_start..fill_end {
                    let idx = fill_y * window_width + hx;
                    pixels[idx] = fill_colour;
                    hit_test_map[idx] = hit_id;
                    textbox_mask[idx] = 255;
                }
            }
        }

        // Fill center and straight edges
        // Use signed arithmetic to clamp Y ranges to visible portion
        let radius_int = radius as isize;

        if box_width > box_height {
            // Fat box: draw top and bottom straight edges
            let left_edge = x.wrapping_add(radius as usize);
            let right_edge = x.wrapping_add(box_width).wrapping_sub(radius as usize);

            // Top edge (horizontal hairline) - just outer pixel
            let top_y = y_signed.max(0).min(height_signed) as usize;
            if y_signed >= 0 && y_signed < height_signed {
                for px in left_edge..right_edge {
                    if px >= window_width { continue; }
                    let idx = top_y * window_width + px;
                    pixels[idx] = light_colour;
                }
            }

            // Bottom edge (horizontal hairline) - just outer pixel, shifted down 1
            let bottom_y_signed = y_signed + box_height as isize;
            if bottom_y_signed >= 0 && bottom_y_signed < height_signed {
                let bottom_y = bottom_y_signed as usize;
                for px in left_edge..right_edge {
                    if px >= window_width { continue; }
                    let idx = bottom_y * window_width + px;
                    pixels[idx] = shadow_colour;
                }
            }

            // Fill center rectangle - clamp Y range to visible portion
            let fill_top = (y_signed + 1).max(0).min(height_signed) as usize;
            let fill_bottom = (y_signed + box_height as isize).max(0).min(height_signed) as usize;
            for py in fill_top..fill_bottom {
                for px in left_edge..right_edge {
                    if px >= window_width { continue; }
                    let idx = py * window_width + px;
                    pixels[idx] = fill_colour;
                    hit_test_map[idx] = hit_id;
                    textbox_mask[idx] = 255;
                }
            }
        } else {
            // Skinny box: draw left and right straight edges
            let top_edge = (y_signed + radius_int).max(0).min(height_signed) as usize;
            let bottom_edge = (y_signed + box_height as isize - radius_int).max(0).min(height_signed) as usize;

            // Left edge (vertical hairline) - just outer pixel
            if x < window_width {
                for py in top_edge..bottom_edge {
                    let idx = py * window_width + x;
                    pixels[idx] = light_colour;
                }
            }

            // Right edge (vertical hairline) - just outer pixel
            let right_x = x.wrapping_add(box_width);
            if right_x < window_width {
                for py in top_edge..bottom_edge {
                    let idx = py * window_width + right_x;
                    pixels[idx] = shadow_colour;
                }
            }

            // Fill center rectangle
            for py in top_edge..bottom_edge {
                for px in (x.wrapping_add(1))..(x.wrapping_add(box_width).wrapping_sub(1)) {
                    if px >= window_width { continue; }
                    let idx = py * window_width + px;
                    pixels[idx] = fill_colour;
                    hit_test_map[idx] = hit_id;
                    textbox_mask[idx] = 255;
                }
            }
        }
    }

    /// Generate textbox glow mask by blurring textbox_mask left/right and knocking out center
    /// glow_colour is 0x00RRGGBB format (no alpha), or 0x00010101 for white/gray
    pub fn apply_textbox_glow(
        pixels: &mut [u32],
        textbox_mask: &[u8],
        window_width: usize,
        center_y: isize,
        box_width: usize,
        box_height: usize,
        add: bool,
        glow_colour: u32,
    ) {
        // Blur radii (how far to blur in each direction)
        let blur_radius_horiz = 32;
        let blur_radius_vert = 16;

        // WHY: center_y can be negative or huge when scrolled off-screen
        // PROOF: y_top/y_bottom computed from center_y - need center on-screen for valid usize
        // PREVENTS: Underflow when computing y_top = center_y - box_height/2
        // NOTE: Cast to usize wraps negatives to huge values, failing >= height check
        let height = pixels.len() / window_width;
        let half_h = (box_height / 2) as isize;
        if (center_y - half_h) as usize >= height || (center_y + half_h) as usize >= height {
            return;
        }
        let center_y = center_y as usize;

        // Textbox bounds (guard above ensures these + blur_radius stay in bounds)
        let y_top = center_y - box_height / 2;
        let y_bottom = center_y + box_height / 2;

        // Find horizontal bounds of textbox (left/right edges)
        let center_x = window_width / 2;
        let mut x_left = center_x;
        let mut x_right = center_x;

        // Scan from center to find textbox edges
        let scan_y = center_y * window_width;
        for x in (0..center_x).rev() {
            if textbox_mask[scan_y + x] > 0 {
                x_left = x;
            } else {
                break;
            }
        }
        for x in center_x..window_width {
            if textbox_mask[scan_y + x] > 0 {
                x_right = x;
            } else {
                break;
            }
        }

        // Corner radius for skipping rounded corners (harmonic mean for smooth scaling)
        let corner_radius = 2 * box_width * box_height / (box_width + box_height);
        let x_vert_start = x_left + corner_radius;
        let x_vert_end = x_right - corner_radius;
        let y_horiz_start = y_top + corner_radius;
        let y_horiz_end = y_bottom - corner_radius;

        let mut adder;
        if add {
            // Horizontal blur pass - right from right edge (skip rounded corners)
            for y in y_top..y_bottom {
                adder = 0;
                for x in x_right
                    - (y_horiz_start as isize - y as isize).max(0) as usize
                    - (y as isize - y_horiz_end as isize).max(0) as usize
                    ..x_right + blur_radius_horiz
                {
                    let idx = y * window_width + x;
                    if x > 0 && textbox_mask[idx] < textbox_mask[idx - 1] {
                        adder += (textbox_mask[idx - 1] - textbox_mask[idx]) as u32;
                    }
                    adder = (adder * 15 >> 4).min(71);
                    let intensity = (adder * (255 - textbox_mask[idx]) as u32) >> 8;
                    let r = ((glow_colour >> 16) & 0xFF) * intensity >> 8;
                    let g = ((glow_colour >> 8) & 0xFF) * intensity >> 8;
                    let b = (glow_colour & 0xFF) * intensity >> 8;
                    pixels[idx] += (r << 16) | (g << 8) | b;
                }
            }

            // Horizontal blur pass - left from left edge (with diagonal corner fill)
            for y in y_top..y_bottom {
                adder = 0;
                // PROOF saturating_sub: blur_radius_horiz could exceed x_left
                // Prevents underflow when blurring near left edge, saturating at 0
                for x in (x_left.saturating_sub(blur_radius_horiz)
                    ..=x_left
                        + (y_horiz_start as isize - y as isize).max(0) as usize
                        + (y as isize - y_horiz_end as isize).max(0) as usize)
                    .rev()
                {
                    let idx = y * window_width + x;
                    if x + 1 < window_width && textbox_mask[idx] < textbox_mask[idx + 1] {
                        adder += (textbox_mask[idx + 1] - textbox_mask[idx]) as u32;
                    }
                    adder = (adder * 15 >> 4).min(71);
                    let intensity = (adder * (255 - textbox_mask[idx]) as u32) >> 8;
                    let r = ((glow_colour >> 16) & 0xFF) * intensity >> 8;
                    let g = ((glow_colour >> 8) & 0xFF) * intensity >> 8;
                    let b = (glow_colour & 0xFF) * intensity >> 8;
                    pixels[idx] += (r << 16) | (g << 8) | b;
                }
            }

            // Vertical blur pass - down from bottom edge (with diagonal corner fill)
            for x in x_left..x_right {
                adder = 0;
                for y in y_bottom
                    - (x_vert_start as isize - x as isize).max(0) as usize
                    - (x as isize - x_vert_end as isize).max(0) as usize
                    ..y_bottom + blur_radius_vert
                {
                    // WHY: Glow extends blur_radius_vert below textbox, may exceed screen
                    // PROOF: Loop scans outward (increasing y), once y >= height all remaining y's also >=
                    // PREVENTS: Out-of-bounds pixel access when glow extends past bottom edge
                    if y >= height {
                        break;
                    }
                    let idx = y * window_width + x;
                    if y > 0 {
                        let idx_above = (y - 1) * window_width + x;
                        if textbox_mask[idx] < textbox_mask[idx_above] {
                            adder += (textbox_mask[idx_above] - textbox_mask[idx]) as u32;
                        }
                    }
                    adder = (adder * 3 >> 2).min(70);
                    let intensity = (adder * (255 - textbox_mask[idx]) as u32) >> 8;
                    let r = ((glow_colour >> 16) & 0xFF) * intensity >> 8;
                    let g = ((glow_colour >> 8) & 0xFF) * intensity >> 8;
                    let b = (glow_colour & 0xFF) * intensity >> 8;
                    pixels[idx] += (r << 16) | (g << 8) | b;
                }
            }

            // Vertical blur pass - up from top edge (with diagonal corner fill)
            for x in x_left..x_right {
                adder = 0;
                for y in (0..=y_top
                    + (x_vert_start as isize - x as isize).max(0) as usize
                    + (x as isize - x_vert_end as isize).max(0) as usize)
                    .rev()
                {
                    // WHY: Glow extends blur_radius_vert above textbox, may go negative (wrapped)
                    // PROOF: Loop scans outward (decreasing y), once past y_top - blur_radius_vert we stop
                    // PREVENTS: Processing pixels above glow region or wrapped negative values
                    if y + blur_radius_vert < y_top {
                        break;
                    }
                    // WHY: y_top + corner_adjust can exceed height when textbox is near bottom
                    // PROOF: Loop starts at y_top + corner_adjust, which depends on textbox position
                    // PREVENTS: Out-of-bounds access to textbox_mask[idx] and pixels[idx]
                    if y >= height {
                        continue;
                    }
                    let idx = y * window_width + x;
                    if y + 1 < height {
                        let idx_below = (y + 1) * window_width + x;
                        if textbox_mask[idx] < textbox_mask[idx_below] {
                            adder += (textbox_mask[idx_below] - textbox_mask[idx]) as u32;
                        }
                    }
                    adder = (adder * 3 >> 2).min(70);
                    let intensity = (adder * (255 - textbox_mask[idx]) as u32) >> 8;
                    let r = ((glow_colour >> 16) & 0xFF) * intensity >> 8;
                    let g = ((glow_colour >> 8) & 0xFF) * intensity >> 8;
                    let b = (glow_colour & 0xFF) * intensity >> 8;
                    pixels[idx] += (r << 16) | (g << 8) | b;
                }
            }
        } else {
            // Horizontal blur pass - right from right edge (skip rounded corners)
            for y in y_top..y_bottom {
                adder = 0;
                for x in x_right
                    - (y_horiz_start as isize - y as isize).max(0) as usize
                    - (y as isize - y_horiz_end as isize).max(0) as usize
                    ..x_right + blur_radius_horiz
                {
                    let idx = y * window_width + x;
                    if x > 0 && textbox_mask[idx] < textbox_mask[idx - 1] {
                        adder += (textbox_mask[idx - 1] - textbox_mask[idx]) as u32;
                    }
                    adder = (adder * 15 >> 4).min(71);
                    let intensity = (adder * (255 - textbox_mask[idx]) as u32) >> 8;
                    let r = ((glow_colour >> 16) & 0xFF) * intensity >> 8;
                    let g = ((glow_colour >> 8) & 0xFF) * intensity >> 8;
                    let b = (glow_colour & 0xFF) * intensity >> 8;
                    pixels[idx] -= (r << 16) | (g << 8) | b;
                }
            }

            // Horizontal blur pass - left from left edge (with diagonal corner fill)
            for y in y_top..y_bottom {
                adder = 0;
                // PROOF saturating_sub: blur_radius_horiz could exceed x_left
                // Prevents underflow when blurring near left edge, saturating at 0
                for x in (x_left.saturating_sub(blur_radius_horiz)
                    ..=x_left
                        + (y_horiz_start as isize - y as isize).max(0) as usize
                        + (y as isize - y_horiz_end as isize).max(0) as usize)
                    .rev()
                {
                    let idx = y * window_width + x;
                    if x + 1 < window_width && textbox_mask[idx] < textbox_mask[idx + 1] {
                        adder += (textbox_mask[idx + 1] - textbox_mask[idx]) as u32;
                    }
                    adder = (adder * 15 >> 4).min(71);
                    let intensity = (adder * (255 - textbox_mask[idx]) as u32) >> 8;
                    let r = ((glow_colour >> 16) & 0xFF) * intensity >> 8;
                    let g = ((glow_colour >> 8) & 0xFF) * intensity >> 8;
                    let b = (glow_colour & 0xFF) * intensity >> 8;
                    pixels[idx] -= (r << 16) | (g << 8) | b;
                }
            }

            // Vertical blur pass - down from bottom edge (with diagonal corner fill)
            for x in x_left..x_right {
                adder = 0;
                for y in y_bottom
                    - (x_vert_start as isize - x as isize).max(0) as usize
                    - (x as isize - x_vert_end as isize).max(0) as usize
                    ..y_bottom + blur_radius_vert
                {
                    // WHY: Glow extends blur_radius_vert below textbox, may exceed screen
                    // PROOF: Loop scans outward (increasing y), once y >= height all remaining y's also >=
                    // PREVENTS: Out-of-bounds pixel access when glow extends past bottom edge
                    if y >= height {
                        break;
                    }
                    let idx = y * window_width + x;
                    if y > 0 {
                        let idx_above = (y - 1) * window_width + x;
                        if textbox_mask[idx] < textbox_mask[idx_above] {
                            adder += (textbox_mask[idx_above] - textbox_mask[idx]) as u32;
                        }
                    }
                    adder = (adder * 3 >> 2).min(70);
                    let intensity = (adder * (255 - textbox_mask[idx]) as u32) >> 8;
                    let r = ((glow_colour >> 16) & 0xFF) * intensity >> 8;
                    let g = ((glow_colour >> 8) & 0xFF) * intensity >> 8;
                    let b = (glow_colour & 0xFF) * intensity >> 8;
                    pixels[idx] -= (r << 16) | (g << 8) | b;
                }
            }

            // Vertical blur pass - up from top edge (with diagonal corner fill)
            for x in x_left..x_right {
                adder = 0;
                for y in (0..=y_top
                    + (x_vert_start as isize - x as isize).max(0) as usize
                    + (x as isize - x_vert_end as isize).max(0) as usize)
                    .rev()
                {
                    // WHY: Glow extends blur_radius_vert above textbox, may go negative (wrapped)
                    // PROOF: Loop scans outward (decreasing y), once past y_top - blur_radius_vert we stop
                    // PREVENTS: Processing pixels above glow region or wrapped negative values
                    if y + blur_radius_vert < y_top {
                        break;
                    }
                    // WHY: y_top + corner_adjust can exceed height when textbox is near bottom
                    // PROOF: Loop starts at y_top + corner_adjust, which depends on textbox position
                    // PREVENTS: Out-of-bounds access to textbox_mask[idx] and pixels[idx]
                    if y >= height {
                        continue;
                    }
                    let idx = y * window_width + x;
                    if y + 1 < height {
                        let idx_below = (y + 1) * window_width + x;
                        if textbox_mask[idx] < textbox_mask[idx_below] {
                            adder += (textbox_mask[idx_below] - textbox_mask[idx]) as u32;
                        }
                    }
                    adder = (adder * 3 >> 2).min(70);
                    let intensity = (adder * (255 - textbox_mask[idx]) as u32) >> 8;
                    let r = ((glow_colour >> 16) & 0xFF) * intensity >> 8;
                    let g = ((glow_colour >> 8) & 0xFF) * intensity >> 8;
                    let b = (glow_colour & 0xFF) * intensity >> 8;
                    pixels[idx] -= (r << 16) | (g << 8) | b;
                }
            }
        }
    }

    pub fn draw_button(
        pixels: &mut [u32],
        hit_test_map: &mut [u8],
        mut textbox_mask: Option<&mut [u8]>,
        window_width: usize,
        _window_height: usize,
        center_x: usize,
        center_y: usize,
        box_width: usize,
        box_height: usize,
        hit_id: u8,
        fill_colour: u32,
        light_colour: u32,
        shadow_colour: u32,
    ) {
        // Convert from center coordinates to top-left
        let x = center_x - box_width / 2;
        let y = center_y - box_height / 2;

        // Pill-shaped: radius = height/2 gives semicircular ends (same as textbox)
        let radius = box_height as f32 / 2.;
        let squirdleyness = 3;

        // Generate crossings from edge (radius/12 o'clock) toward diagonal (1:30)
        let mut crossings: Vec<(u16, u8, u8)> = Vec::new();
        let mut offset = 0f32;

        loop {
            let y_norm = offset / radius;
            let x_norm = (1. - y_norm.powi(squirdleyness)).powf(1. / squirdleyness as f32);
            let x = x_norm * radius;
            let inset = radius - x;

            if inset >= 0. {
                let l = (inset.fract().sqrt() * 256.) as u8;
                let h = ((1. - inset.fract()).sqrt() * 256.) as u8;
                crossings.push((inset as u16, l, h));
            }

            // Stop at 45-degree diagonal (when x < offset)
            if x < offset {
                break;
            }

            offset += 1.;
        }

        // Top-left corner - vertical edge with diagonal fill
        for (i, &(inset, l, h)) in crossings.iter().enumerate() {
            // Stop at diagonal - when inset exceeds i, we've gone past the 45-degree point
            if inset as usize > i {
                break;
            }

            let py = y + radius as usize - i; // Start at horizontal center, go up
            let px = x + inset as usize;

            // Outer antialiased pixel
            let idx = py * window_width + px;
            pixels[idx] = blend_rgb_only(pixels[idx], light_colour, l, h);

            // Inner antialiased pixel
            let idx = idx + 1;
            pixels[idx] = blend_rgb_only(light_colour, fill_colour, l, h);
            hit_test_map[idx] = hit_id;
            if let Some(ref mut mask) = textbox_mask {
                mask[idx] = 255 - h;
            }

            // Fill horizontally to the diagonal (where horizontal edge would be)
            let diag_x = (x + radius as usize - i).min(window_width);
            for fill_x in (px + 2)..=diag_x {
                let idx = py * window_width + fill_x;
                pixels[idx] = fill_colour;
                hit_test_map[idx] = hit_id;
                if let Some(ref mut mask) = textbox_mask {
                    mask[idx] = 0;
                }
            }

            // Horizontal edge - Outer antialiased pixel
            let hx = x + radius as usize - i; // Start at vertical center, go left
            let hy = y + inset as usize; // Distance from top edge

            let idx = hy * window_width + hx;
            pixels[idx] = blend_rgb_only(pixels[idx], light_colour, l, h);

            // Horizontal edge - Inner antialiased pixel (below the outer)
            let idx = (hy + 1) * window_width + hx;
            pixels[idx] = blend_rgb_only(light_colour, fill_colour, l, h);
            hit_test_map[idx] = hit_id;
            if let Some(ref mut mask) = textbox_mask {
                mask[idx] = 255 - h;
            }

            // Fill vertically down from horizontal edge to diagonal
            // Diagonal is where the vertical edge is at this same iteration
            let diag_y = y + radius as usize - i;
            for fill_y in (hy + 2)..diag_y {
                let idx = fill_y * window_width + hx;
                pixels[idx] = fill_colour;
                hit_test_map[idx] = hit_id;
                if let Some(ref mut mask) = textbox_mask {
                    mask[idx] = 0;
                }
            }
        }

        // Top-right corner - mirror of top-left (flip x)
        for (i, &(inset, l, h)) in crossings.iter().enumerate() {
            if inset as usize > i {
                break;
            }

            let py = y + radius as usize - i;
            let px = x + box_width - 1 - inset as usize;

            // Vertical edge - Outer antialiased pixel
            let idx = py * window_width + px;
            pixels[idx] = blend_rgb_only(pixels[idx], shadow_colour, l, h);

            // Vertical edge - Inner antialiased pixel
            let idx = idx - 1;
            pixels[idx] = blend_rgb_only(shadow_colour, fill_colour, l, h);
            hit_test_map[idx] = hit_id;
            if let Some(ref mut mask) = textbox_mask {
                mask[idx] = 255 - h;
            }

            // Fill horizontally to the diagonal
            let diag_x = x + box_width - 1 - radius as usize + i;
            for fill_x in diag_x..(px - 1) {
                let idx = py * window_width + fill_x;
                pixels[idx] = fill_colour;
                hit_test_map[idx] = hit_id;
                if let Some(ref mut mask) = textbox_mask {
                    mask[idx] = 0;
                }
            }

            // Horizontal edge - Outer antialiased pixel
            let hx = x + box_width - 1 - radius as usize + i;
            let hy = y + inset as usize;

            let idx = hy * window_width + hx;
            pixels[idx] = blend_rgb_only(pixels[idx], light_colour, l, h);

            // Horizontal edge - Inner antialiased pixel
            let idx = (hy + 1) * window_width + hx;
            pixels[idx] = blend_rgb_only(light_colour, fill_colour, l, h);
            hit_test_map[idx] = hit_id;
            if let Some(ref mut mask) = textbox_mask {
                mask[idx] = 255 - h;
            }

            // Fill vertically down from horizontal edge to diagonal
            let diag_y = y + radius as usize - i;
            for fill_y in (hy + 2)..diag_y {
                let idx = fill_y * window_width + hx;
                pixels[idx] = fill_colour;
                hit_test_map[idx] = hit_id;
                if let Some(ref mut mask) = textbox_mask {
                    mask[idx] = 0;
                }
            }
        }

        // Bottom-left corner - mirror of top-left (flip y), shifted down 1 to avoid overlap
        for (i, &(inset, l, h)) in crossings.iter().enumerate() {
            if inset as usize > i {
                break;
            }

            let py = y + box_height - radius as usize + i;
            let px = x + inset as usize;

            // Vertical edge - Outer antialiased pixel
            let idx = py * window_width + px;
            pixels[idx] = blend_rgb_only(pixels[idx], light_colour, l, h);

            // Vertical edge - Inner antialiased pixel
            let idx = idx + 1;
            pixels[idx] = blend_rgb_only(light_colour, fill_colour, l, h);
            hit_test_map[idx] = hit_id;
            if let Some(ref mut mask) = textbox_mask {
                mask[idx] = 255 - h;
            }

            // Fill horizontally to the diagonal
            let diag_x = (x + radius as usize - i).min(window_width);
            for fill_x in (px + 2)..=diag_x {
                let idx = py * window_width + fill_x;
                pixels[idx] = fill_colour;
                hit_test_map[idx] = hit_id;
                if let Some(ref mut mask) = textbox_mask {
                    mask[idx] = 0;
                }
            }

            // Horizontal edge - Outer antialiased pixel
            let hx = x + radius as usize - i;
            let hy = y + box_height - inset as usize;

            let idx = hy * window_width + hx;
            pixels[idx] = blend_rgb_only(pixels[idx], shadow_colour, l, h);

            // Horizontal edge - Inner antialiased pixel
            let idx = (hy - 1) * window_width + hx;
            pixels[idx] = blend_rgb_only(shadow_colour, fill_colour, l, h);
            hit_test_map[idx] = hit_id;
            if let Some(ref mut mask) = textbox_mask {
                mask[idx] = 255 - h;
            }

            // Fill vertically up from horizontal edge to diagonal
            let diag_y = y + box_height - radius as usize + i;
            for fill_y in (diag_y + 1)..(hy - 1) {
                let idx = fill_y * window_width + hx;
                pixels[idx] = fill_colour;
                hit_test_map[idx] = hit_id;
                if let Some(ref mut mask) = textbox_mask {
                    mask[idx] = 0;
                }
            }
        }

        // Bottom-right corner - mirror of top-left (flip both x and y), shifted down 1 to avoid overlap
        for (i, &(inset, l, h)) in crossings.iter().enumerate() {
            if inset as usize > i {
                break;
            }

            let py = y + box_height - radius as usize + i;
            let px = x + box_width - 1 - inset as usize;

            // Vertical edge - Outer antialiased pixel
            let idx = py * window_width + px;
            pixels[idx] = blend_rgb_only(pixels[idx], shadow_colour, l, h);

            // Vertical edge - Inner antialiased pixel
            let idx = idx - 1;
            pixels[idx] = blend_rgb_only(shadow_colour, fill_colour, l, h);
            hit_test_map[idx] = hit_id;
            if let Some(ref mut mask) = textbox_mask {
                mask[idx] = 255 - h;
            }

            // Fill horizontally to the diagonal
            let diag_x = x + box_width - 1 - radius as usize + i;
            for fill_x in diag_x..(px - 1) {
                let idx = py * window_width + fill_x;
                pixels[idx] = fill_colour;
                hit_test_map[idx] = hit_id;
                if let Some(ref mut mask) = textbox_mask {
                    mask[idx] = 0;
                }
            }

            // Horizontal edge - Outer antialiased pixel
            let hx = x + box_width - 1 - radius as usize + i;
            let hy = y + box_height - inset as usize;

            let idx = hy * window_width + hx;
            pixels[idx] = blend_rgb_only(pixels[idx], shadow_colour, l, h);

            // Horizontal edge - Inner antialiased pixel
            let idx = (hy - 1) * window_width + hx;
            pixels[idx] = blend_rgb_only(shadow_colour, fill_colour, l, h);
            hit_test_map[idx] = hit_id;
            if let Some(ref mut mask) = textbox_mask {
                mask[idx] = 255 - h;
            }

            // Fill vertically up from horizontal edge to diagonal
            let diag_y = y + box_height - radius as usize + i;
            for fill_y in (diag_y + 1)..(hy - 1) {
                let idx = fill_y * window_width + hx;
                pixels[idx] = fill_colour;
                hit_test_map[idx] = hit_id;
                if let Some(ref mut mask) = textbox_mask {
                    mask[idx] = 0;
                }
            }
        }

        // Fill center and straight edges
        let radius_int = radius as usize;

        if box_width > box_height {
            // Fat box: draw top and bottom straight edges
            let left_edge = x + radius_int;
            let right_edge = x + box_width - radius_int;

            // Top edge (horizontal hairline) - just outer pixel
            for px in left_edge..right_edge {
                let idx = y * window_width + px;
                pixels[idx] = light_colour;
            }

            // Bottom edge (horizontal hairline) - just outer pixel, shifted down 1
            let bottom_y = y + box_height;
            for px in left_edge..right_edge {
                let idx = bottom_y * window_width + px;
                pixels[idx] = shadow_colour;
            }

            // Fill center rectangle
            for py in (y + 1)..(y + box_height) {
                for px in left_edge..right_edge {
                    let idx = py * window_width + px;
                    pixels[idx] = fill_colour;
                    hit_test_map[idx] = hit_id;
                    if let Some(ref mut mask) = textbox_mask {
                        mask[idx] = 0;
                    }
                }
            }
        } else {
            // Skinny box: draw left and right straight edges
            let top_edge = y + radius_int;
            let bottom_edge = y + box_height - radius_int;

            // Left edge (vertical hairline) - just outer pixel
            for py in top_edge..bottom_edge {
                let idx = py * window_width + x;
                pixels[idx] = light_colour;
            }

            // Right edge (vertical hairline) - just outer pixel
            let right_x = x + box_width;
            for py in top_edge..bottom_edge {
                let idx = py * window_width + right_x;
                pixels[idx] = shadow_colour;
            }

            // Fill center rectangle
            for py in top_edge..bottom_edge {
                for px in (x + 1)..(x + box_width - 1) {
                    let idx = py * window_width + px;
                    pixels[idx] = fill_colour;
                    hit_test_map[idx] = hit_id;
                    if let Some(ref mut mask) = textbox_mask {
                        mask[idx] = 0;
                    }
                }
            }
        }
    }

    pub fn draw_spectrum(
        pixels: &mut [u32],
        window_width: usize,
        region: &super::app::PixelRegion,
        phase_offset: f32, // Sine wave phase offset in radians
    ) {
        // Spectrum fills full region width, wave amplitude based on height
        let logo_width = region.w;
        let logo_height = region.h / 2; // Wave oscillates around center
                                        // Harmonic mean of region for brightness scaling (matches old span behavior)
        let region_span =
            2.0 * region.w as f32 * region.h as f32 / (region.w as f32 + region.h as f32);

        // Wave period tied to height - aspect ratio determines wave count
        let waves_per_region = region.w as f32 / region.h as f32 * 2.;

        // Loop invariant: y in 0..region.h → py = region.y + y is in region.y..region.bottom()
        // Loop invariant: x in 0..region.w → px = region.x + x is in region.x..region.right()
        for y in 0..region.h {
            let py = region.y + y;
            for x in 0..logo_width {
                let px = region.x + x;

                // Flip x for wave calculations to match flipped spectrum
                let x_flipped = logo_width - 1 - x;
                let x_norm = x_flipped as f32 / logo_width as f32;
                // Amplitude ramps up toward blue (high x_flipped)
                let amplitude = logo_height as f32 / (1. + 12. * x_norm);

                // Frequency ramps up toward blue end (logarithmic)
                let freq_ramp = 2f32.powf(-x_norm + 2.); // ~1x at red, ~3x at blue
                let wave_phase =
                    x_norm * std::f32::consts::TAU * waves_per_region * freq_ramp - phase_offset;
                let wave_offset = wave_phase.sin() * amplitude;

                let mut scale = (y as f32 + wave_offset - logo_height as f32) / logo_height as f32;
                scale = ((logo_height * 2 - y) as f32 / logo_height as f32)
                    * (y as f32 / logo_height as f32)
                    * 32000.
                    / (scale.abs() + amplitude / region_span * 0.25);

                // Map x position to wavelength index, flipped left-right
                // LMS2006SO covers 350-830nm in 1nm steps
                const START_NM: usize = 350;
                const LAMBDA_START: usize = 350 - START_NM;
                const LAMBDA_END: usize = 750 - START_NM;
                let wavelength_idx = LAMBDA_START
                    + ((logo_width - 1 - x) * (LAMBDA_END - LAMBDA_START)) / logo_width.max(1);
                let lms_idx = wavelength_idx * 3;

                // Extract L, M, S from LMS2006SO array
                let l = LMS2006SO[lms_idx];
                let m = LMS2006SO[lms_idx + 1];
                let s = LMS2006SO[lms_idx + 2];

                // Convert LMS to REC2020 magic 9
                let r =
                    3.168241098811690000 * l + -2.156882856491830000 * m + 0.096456879211209600 * s;
                let g = -0.266362510245695000 * l
                    + 1.404945732577530000 * m
                    + -0.175554801656117000 * s;
                let b =
                    0.003891529873740330 * l + -0.020567680031394800 * m + 0.945832607950864000 * s;

                // idx proven in-bounds by loop invariants
                let idx = py * window_width + px;
                let (r_bg, g_bg, b_bg, a) = unpack_argb(pixels[idx]);
                let r_b = r_bg as f32 * r_bg as f32;
                let g_b = g_bg as f32 * g_bg as f32;
                let b_b = b_bg as f32 * b_bg as f32;
                let r_new = (r * scale + r_b).sqrt() as u8;
                let g_new = (g * scale + g_b).sqrt() as u8;
                let b_new = (b * scale + b_b).sqrt() as u8;
                pixels[idx] = pack_argb(r_new, g_new, b_new, a);
            }
        }
    }

    pub fn draw_logo_text(
        pixels: &mut [u32],
        text_renderer: &mut TextRenderer,
        window_width: usize,
        region: &super::app::PixelRegion,
    ) {
        const TEXT_ASPECT: f32 = 6.;

        // Size constrained to fit region using harmonic mean (smooth derivative)
        let max_size_by_width = region.w as f32 / TEXT_ASPECT;
        let max_size_by_height = region.h as f32;
        // Harmonic mean: 2*a*b/(a+b) - smoothly blends constraints
        let text_size =
            2.0 * max_size_by_width * max_size_by_height / (max_size_by_width + max_size_by_height);

        let text_x = (region.x + region.w / 2) as f32;
        let text_y = (region.y + region.h / 2) as f32;

        // Virtual buffer region (only process where text lives with glow padding)
        let text_height_estimate = (text_size * 1.5) as usize;
        let window_height = pixels.len() / window_width;
        let start = (text_y as usize).saturating_sub(text_height_estimate);
        let stop = (text_y as usize + text_height_estimate).min(window_height);
        let virtual_height = stop - start;
        let buffer_size = window_width * virtual_height;

        let mut glow_buffer = vec![0; buffer_size];
        text_renderer.draw_text_center(
            &mut glow_buffer,
            window_width as u32,
            virtual_height as u32,
            "Photon",
            text_x,
            text_y - start as f32, // Adjust y for virtual buffer
            text_size,
            800, // weight
            vec![theme::LOGO_GLOW_GRAY],
            0, // rotation
            theme::FONT_LOGO,
        );

        let mut highlight_buffer = vec![0; buffer_size];
        text_renderer.draw_text_center(
            &mut highlight_buffer,
            window_width as u32,
            virtual_height as u32,
            "Photon",
            text_x,
            text_y - start as f32,
            text_size,
            800, // weight
            vec![theme::LOGO_HIGHLIGHT_GRAY],
            0, // rotation
            theme::FONT_LOGO,
        );
        text_renderer.draw_text_center(
            &mut highlight_buffer,
            window_width as u32,
            virtual_height as u32,
            "Photon",
            text_x + 1.,
            text_y - start as f32,
            text_size,
            800, // weight
            vec![0],
            0, // rotation
            theme::FONT_LOGO,
        );
        text_renderer.draw_text_center(
            &mut highlight_buffer,
            window_width as u32,
            virtual_height as u32,
            "Photon",
            text_x,
            text_y - start as f32 + 1.,
            text_size,
            800, // weight
            vec![0],
            0, // rotation
            theme::FONT_LOGO,
        );

        let mut prev = highlight_buffer[0];
        for glow_idx in 1..highlight_buffer.len() {
            prev = (((highlight_buffer[glow_idx] as u16 + prev as u16 * 15) >> 4) as u8)
                .max(highlight_buffer[glow_idx]);
            highlight_buffer[glow_idx] = prev;
        }
        let mut prev = highlight_buffer[highlight_buffer.len() - 1];
        for glow_idx in (0..highlight_buffer.len()).rev() {
            prev = (((highlight_buffer[glow_idx] as u16 + prev as u16 * 15) >> 4) as u8)
                .max(highlight_buffer[glow_idx]);
            highlight_buffer[glow_idx] = prev;
        }

        // // Vertical pass: top to bottom
        // for x in 0..window_width as usize {
        //     let mut prev = highlight_buffer[x]; // y=0, x=x
        //     for y in 1..window_height as usize {
        //         let glow_idx = y * window_width as usize + x;
        //         prev = (((highlight_buffer[glow_idx] as u16 + prev as u16 * 3) >> 2) as u8)
        //             .max(highlight_buffer[glow_idx]);
        //         highlight_buffer[glow_idx] = prev;
        //     }
        // }

        // // Vertical pass: bottom to top
        // for x in 0..window_width as usize {
        //     let mut prev =
        //         highlight_buffer[(window_height as usize - 1) * window_width as usize + x];
        //     for y in (0..window_height as usize - 1).rev() {
        //         let glow_idx = y * window_width as usize + x;
        //         prev = (((highlight_buffer[glow_idx] as u16 + prev as u16 * 3) >> 2) as u8)
        //             .max(highlight_buffer[glow_idx]);
        //         highlight_buffer[glow_idx] = prev;
        //     }
        // }

        let mut prev = glow_buffer[0];
        for glow_idx in 1..glow_buffer.len() {
            prev = (((glow_buffer[glow_idx] as u16 + prev as u16 * 3) >> 2) as u8)
                .max(glow_buffer[glow_idx]);
            glow_buffer[glow_idx] = prev;
        }
        let mut prev = glow_buffer[glow_buffer.len() - 1];
        for glow_idx in (0..glow_buffer.len()).rev() {
            prev = (((glow_buffer[glow_idx] as u16 + prev as u16 * 3) >> 2) as u8)
                .max(glow_buffer[glow_idx]);
            glow_buffer[glow_idx] = prev;
        }

        // Vertical pass: top to bottom
        for x in 0..window_width as usize {
            let mut prev = glow_buffer[x]; // y=0, x=x
            for y in 1..virtual_height as usize {
                let glow_idx = y * window_width as usize + x;
                prev = (((glow_buffer[glow_idx] as u16 + prev as u16) >> 1) as u8)
                    .max(glow_buffer[glow_idx]);
                glow_buffer[glow_idx] = prev;
            }
        }

        // Vertical pass: bottom to top
        for x in 0..window_width as usize {
            let mut prev = glow_buffer[(virtual_height as usize - 1) * window_width as usize + x];
            for y in (0..virtual_height as usize - 1).rev() {
                let glow_idx = y * window_width as usize + x;
                prev = (((glow_buffer[glow_idx] as u16 + prev as u16) >> 1) as u8)
                    .max(glow_buffer[glow_idx]);
                glow_buffer[glow_idx] = prev;
            }
        }

        // Composite glow buffer to screen with offset
        for glow_idx in 0..glow_buffer.len() {
            let pixel_idx = glow_idx + start * window_width;
            let grey = glow_buffer[glow_idx];
            let (r, g, b, a) = unpack_argb(pixels[pixel_idx]);
            pixels[pixel_idx] = pack_argb(
                r.wrapping_add(grey),
                g.wrapping_add(grey),
                b.wrapping_add(grey),
                a,
            );
        }
        text_renderer.draw_text_center_u32(
            pixels,
            window_width,
            "Photon",
            text_x,
            text_y,
            text_size,
            800, // weight
            theme::LOGO_TEXT,
            theme::FONT_LOGO,
        );

        // Composite highlight buffer to screen with offset
        for glow_idx in 0..highlight_buffer.len() {
            let pixel_idx = glow_idx + start * window_width;
            let grey = highlight_buffer[glow_idx];
            let (r, g, b, a) = unpack_argb(pixels[pixel_idx]);
            pixels[pixel_idx] = pack_argb(
                r.wrapping_add(grey),
                g.wrapping_add(grey),
                b.wrapping_add(grey),
                a,
            );
        }
    }

    /// Draw an anti-aliased filled circle blending toward black
    /// Used as the base layer for the connectivity indicator
    fn draw_black_circle(pixels: &mut [u32], width: usize, cx: usize, cy: usize, radius: usize) {
        let r_outer = radius as isize;
        let r_outer2 = r_outer * r_outer;
        let r_inner = (radius - 1) as isize;
        let r_inner2 = r_inner * r_inner;
        let edge_range = r_outer2 - r_inner2; // Width of the AA edge band

        for dy in -r_outer..=r_outer {
            let y = cy as isize + dy;
            if y < 0 || y >= (pixels.len() / width) as isize {
                continue;
            }
            let dy2 = dy * dy;

            for dx in -r_outer..=r_outer {
                let dist2 = dx * dx + dy2;
                if dist2 > r_outer2 {
                    continue;
                }

                let x = (cx as isize + dx) as usize;

                let idx = y as usize * width + x;
                // Calculate alpha: 255 inside (full darken), 0 at edge (no darken)
                let inv_alpha = if dist2 <= r_inner2 {
                    0
                } else {
                    // Linear gradient from inner edge (0) to outer edge (255)
                    (((dist2 - r_inner2) << 8) / edge_range) as u32
                };

                let mut pixel = pixels[idx] as u64;
                pixel = (pixel | (pixel << 16)) & 0x0000FFFF0000FFFF;
                pixel = (pixel | (pixel << 8)) & 0x00FF00FF00FF00FF;
                pixel *= inv_alpha as u64; // Multiply by inv_alpha, not alpha
                pixel = (pixel >> 8) & 0x00FF00FF00FF00FF;
                pixel = (pixel | (pixel >> 8)) & 0x0000FFFF0000FFFF;
                pixel = pixel | (pixel >> 16);
                pixels[idx] = (pixel as u32) | 0xFF000000;
            }
        }
    }

    /// Add or subtract colour from an anti-aliased circle region
    /// Used for the green overlay on the connectivity indicator
    fn draw_filled_circle(
        pixels: &mut [u32],
        width: usize,
        cx: usize,
        cy: usize,
        radius: usize,
        colour: u32,
        add: bool,
    ) {
        let r_outer = radius as isize;
        let r_outer2 = r_outer * r_outer;
        let r_inner = (radius - 1) as isize;
        let r_inner2 = r_inner * r_inner;
        let edge_range = r_outer2 - r_inner2;

        // Widen the color once
        let mut colour_wide = colour as u64;
        colour_wide = (colour_wide | (colour_wide << 16)) & 0x0000FFFF0000FFFF;
        colour_wide = (colour_wide | (colour_wide << 8)) & 0x00FF00FF00FF00FF;

        for dy in -r_outer..=r_outer {
            let y = cy as isize + dy;
            if y < 0 || y >= (pixels.len() / width) as isize {
                continue;
            }
            let dy2 = dy * dy;

            for dx in -r_outer..=r_outer {
                let dist2 = dx * dx + dy2;
                if dist2 > r_outer2 {
                    continue;
                }
                let x = cx as isize + dx;
                if x < 0 || x >= width as isize {
                    continue;
                }
                let idx = y as usize * width + x as usize;
                // Calculate alpha: 255 inside, 0 at edge
                let alpha = if dist2 <= r_inner2 {
                    255
                } else {
                    (((r_outer2 - dist2) << 8) / edge_range) as u32
                };

                // Scale the color by alpha
                let mut scaled_colour = colour_wide * alpha as u64;
                scaled_colour = (scaled_colour >> 8) & 0x00FF00FF00FF00FF;

                // Narrow back to u32
                scaled_colour = (scaled_colour | (scaled_colour >> 8)) & 0x0000FFFF0000FFFF;
                scaled_colour = scaled_colour | (scaled_colour >> 16);
                let scaled_colour_u32 = scaled_colour as u32;

                // Add or subtract directly on u32
                pixels[idx] = if add {
                    pixels[idx].wrapping_add(scaled_colour_u32)
                } else {
                    pixels[idx].wrapping_sub(scaled_colour_u32)
                };
            }
        }
    }

    /// Add or subtract a single-pixel hairline circle (anti-aliased ring)
    /// Used for the grey outline on offline indicators
    /// Draws at the outer edge of the circle (same edge as draw_indicator_base AA zone)
    fn draw_indicator_hairline(
        pixels: &mut [u32],
        width: usize,
        cx: usize,
        cy: usize,
        radius: usize,
        colour: u32,
        add: bool,
    ) {
        let r_outer = radius as isize;
        let r_outer2 = r_outer * r_outer;
        let r_inner = (radius - 2) as isize;
        let r_inner2 = r_inner * r_inner;
        let edge_range = r_outer2 - r_inner2;

        // Widen the color once
        let mut colour_wide = colour as u64;
        colour_wide = (colour_wide | (colour_wide << 16)) & 0x0000FFFF0000FFFF;
        colour_wide = (colour_wide | (colour_wide << 8)) & 0x00FF00FF00FF00FF;

        for dy in -r_outer..=r_outer {
            let y = cy as isize + dy;
            if y < 0 || y >= (pixels.len() / width) as isize {
                continue;
            }
            let dy2 = dy * dy;

            for dx in -r_outer..=r_outer {
                let dist2 = dx * dx + dy2;
                if dist2 > r_outer2 {
                    continue;
                }
                let x = cx as isize + dx;
                if x < 0 || x >= width as isize {
                    continue;
                }
                let idx = y as usize * width + x as usize;
                // Calculate alpha: 255 inside, 0 at edge
                let alpha = if dist2 <= r_inner2 {
                    continue;
                } else {
                    ((r_outer2 - dist2).min(dist2 - r_inner2) << 9) / edge_range
                };

                // Scale the color by alpha
                let mut scaled_colour = colour_wide * alpha as u64;
                scaled_colour = (scaled_colour >> 8) & 0x00FF00FF00FF00FF;

                // Narrow back to u32
                scaled_colour = (scaled_colour | (scaled_colour >> 8)) & 0x0000FFFF0000FFFF;
                scaled_colour = scaled_colour | (scaled_colour >> 16);
                let scaled_colour_u32 = scaled_colour as u32;

                // Add or subtract directly on u32
                pixels[idx] = if add {
                    pixels[idx].wrapping_add(scaled_colour_u32)
                } else {
                    pixels[idx].wrapping_sub(scaled_colour_u32)
                };
            }
        }
    }

    /// Update cached scaled avatar if diameter changed
    pub fn update_avatar_scaled(&mut self, diameter: usize) {
        // Skip if no avatar or already at correct size
        if self.avatar_pixels.is_none() {
            return;
        }
        if self.avatar_scaled.is_some() && self.avatar_scaled_diameter == diameter {
            return;
        }

        let src = self.avatar_pixels.as_ref().unwrap();
        let src_size = crate::avatar::AVATAR_SIZE;

        // Use resize crate with Mitchell filter on RGB8 data
        use resize::Pixel::RGB8;
        use resize::Type::Mitchell;

        let mut resizer = resize::new(src_size, src_size, diameter, diameter, RGB8, Mitchell)
            .expect("Failed to create resizer");

        let mut dst = vec![0u8; diameter * diameter * 3];

        // Convert slices to rgb::RGB<u8> slices
        let src_rgb: &[rgb::RGB8] = unsafe {
            std::slice::from_raw_parts(src.as_ptr() as *const rgb::RGB8, src_size * src_size)
        };
        let dst_rgb: &mut [rgb::RGB8] = unsafe {
            std::slice::from_raw_parts_mut(dst.as_mut_ptr() as *mut rgb::RGB8, diameter * diameter)
        };

        resizer.resize(src_rgb, dst_rgb).expect("Resize failed");

        self.avatar_scaled = Some(dst);
        self.avatar_scaled_diameter = diameter;
    }

    /// Unified avatar drawing function
    /// - hit_test_map: Some = fill with HIT_AVATAR, None = skip hit testing
    /// - ring_colour: Some = draw status ring, None = no ring
    /// - brighten: brighten avatar when file hovering (self avatar only)
    /// avatar_scaled must be pre-scaled to diameter×diameter (diameter = radius * 2)
    ///
    /// Coordinates are isize to support scrolling (can be partially/fully off-screen).
    /// Computes intersection of avatar bounds with screen - loop bounds prove safety.
    pub fn draw_avatar(
        pixels: &mut [u32],
        mut hit_test_map: Option<&mut [u8]>,
        width: usize,
        height: usize,
        cx: isize,
        cy: isize,
        radius: isize,
        avatar_scaled: Option<&[u8]>,
        ring_colour: Option<u32>,
        brighten: bool,
    ) {
        let r = radius;
        let diameter = (radius * 2) as usize;
        let stroke_width = radius / 16;

        // Ring radii: 1px inside + 1px outside = 2px total
        let r_inner = r - 1;
        let r_inner2 = r_inner * r_inner;
        let r_inner_inner = r - 2;
        let r_inner_inner2 = r_inner_inner * r_inner_inner;
        let r_outer = r + stroke_width;
        let r_outer2 = r_outer * r_outer;
        let r_outer_outer = r_outer + 1;
        let r_outer_outer2 = r_outer_outer * r_outer_outer;

        // AA diff for outer ring edge: maps [r_outer2, r_outer_outer2) to [255, 0]
        let diff_outer = r_outer_outer2 - r_outer2;
        // AA diff for inner edge (no-ring case): maps [r_inner_inner2, r_inner2) to [255, 0]
        let diff_inner = r_inner2 - r_inner_inner2;

        // Intersection bounds:
        // WHY: cx/cy can be negative or exceed screen bounds due to scroll offset
        // PROOF: We compute intersection of circle bounding box with screen (0..width, 0..height)
        //        If intersection is empty (y_max <= y_min), avatar is off-screen, return early
        //        Otherwise cast to usize is safe since values are in [0, width/height]
        // PREVENTS: Negative isize cast to usize would wrap to huge value causing infinite loop

        if let Some(ring) = ring_colour {
            // === WITH RING ===
            // Compute intersection of outer ring bounds with screen (keep as isize)
            let y_min_i = (cy - r_outer_outer).max(0);
            let y_max_i = (cy + r_outer_outer + 1).min(height as isize);
            let x_min_i = (cx - r_outer_outer).max(0);
            let x_max_i = (cx + r_outer_outer + 1).min(width as isize);

            // Empty intersection = entirely off-screen
            if y_max_i <= y_min_i || x_max_i <= x_min_i {
                return;
            }

            // Safe to cast - values are in [0, width/height]
            let y_min = y_min_i as usize;
            let y_max = y_max_i as usize;
            let x_min = x_min_i as usize;
            let x_max = x_max_i as usize;

            for y in y_min..y_max {
                let dy = y as isize - cy;
                let dy2 = dy * dy;

                for x in x_min..x_max {
                    let dx = x as isize - cx;
                    let dist2 = dx * dx + dy2;
                    let idx = y * width + x;

                    // Hit test covers ring area (not the AA fringe)
                    if let Some(htm) = hit_test_map.as_mut() {
                        if dist2 <= r_outer2 {
                            htm[idx] = HIT_AVATAR;
                        }
                    }

                    if dist2 <= r_inner_inner2 {
                        // Inside inner AA edge - avatar only
                        let colour = sample_avatar(avatar_scaled, dx, dy, r, diameter, brighten);
                        pixels[idx] = 0xFF000000 | colour;
                    } else if dist2 < r_inner2 {
                        // Inner AA edge - blend ring over avatar
                        let colour = sample_avatar(avatar_scaled, dx, dy, r, diameter, brighten);
                        // PROOF: dist2 ∈ (r_inner_inner2, r_inner2), so numerator ∈ (0, diff_inner<<8)
                        // Division maps to (0, 256), cast to u8 is safe (max 255)
                        let alpha = (((dist2 - r_inner_inner2) << 8) / diff_inner) as u8;
                        pixels[idx] = blend_rgb_only(0xFF000000 | colour, ring, 255 - alpha, alpha);
                    } else if dist2 <= r_outer2 {
                        // Solid ring (r_inner to r_outer)
                        pixels[idx] = 0xFF000000 | ring;
                    } else if dist2 <= r_outer_outer2 {
                        // Outer AA edge (r_outer to r_outer_outer) - blend ring to background
                        // PROOF: dist2 ∈ (r_outer2, r_outer_outer2], so numerator ∈ [0, diff_outer<<8)
                        // Division maps to [0, 256), cast to u8 is safe (max 255)
                        let alpha = (((r_outer_outer2 - dist2) << 8) / diff_outer) as u8;
                        pixels[idx] = blend_rgb_only(pixels[idx], ring, 255 - alpha, alpha);
                    }
                }
            }
        } else {
            // === NO RING ===
            // Compute intersection of inner circle bounds with screen (keep as isize)
            let y_min_i = (cy - r_inner).max(0);
            let y_max_i = (cy + r_inner + 1).min(height as isize);
            let x_min_i = (cx - r_inner).max(0);
            let x_max_i = (cx + r_inner + 1).min(width as isize);

            // Empty intersection = entirely off-screen
            if y_max_i <= y_min_i || x_max_i <= x_min_i {
                return;
            }

            // Safe to cast - values are in [0, width/height]
            let y_min = y_min_i as usize;
            let y_max = y_max_i as usize;
            let x_min = x_min_i as usize;
            let x_max = x_max_i as usize;

            for y in y_min..y_max {
                let dy = y as isize - cy;
                let dy2 = dy * dy;

                for x in x_min..x_max {
                    let dx = x as isize - cx;
                    let idx = y * width + x;
                    let dist2 = dx * dx + dy2;

                    // DEBUG: Left half = raw square texture, right half = AA circle
                    if DEBUG_ENABLED.load(std::sync::atomic::Ordering::Relaxed) && dx.is_negative()
                    {
                        let colour = sample_avatar(avatar_scaled, dx, dy, r, diameter, brighten);
                        pixels[idx] = 0xFF000000 | colour;
                        continue;
                    }

                    // Circle check - loop is square bounding box, clip to circle
                    if dist2 > r_inner2 {
                        continue;
                    }

                    // Hit test (trimmed radius) - already inside circle
                    if let Some(htm) = hit_test_map.as_mut() {
                        htm[idx] = HIT_AVATAR;
                    }

                    if dist2 <= r_inner_inner2 {
                        let colour = sample_avatar(avatar_scaled, dx, dy, r, diameter, brighten);
                        pixels[idx] = 0xFF000000 | colour;
                    } else {
                        // AA edge - blend avatar to background
                        let colour = sample_avatar(avatar_scaled, dx, dy, r, diameter, brighten);
                        // PROOF: dist2 ∈ (r_inner_inner2, r_inner2], so numerator ∈ [0, diff_inner<<8)
                        // Division maps to [0, 256), cast to u8 is safe (max 255)
                        let alpha = (((r_inner2 - dist2) << 8) / diff_inner) as u8;
                        pixels[idx] = blend_rgb_only(pixels[idx], colour, 255 - alpha, alpha);
                    }
                }
            }
        }
    }
}

/// Sample avatar texture at offset (dx, dy) from center
/// Texture is diameter×diameter, centered at (r, r)
#[inline]
fn sample_avatar(
    avatar_data: Option<&[u8]>,
    dx: isize,
    dy: isize,
    r: isize,
    diameter: usize,
    brighten: bool,
) -> u32 {
    if let Some(data) = avatar_data {
        let tex_x = (dx + r) as usize;
        let tex_y = (dy + r) as usize;
        let tex_idx = (tex_y * diameter + tex_x) * 3;
        let mut red = data[tex_idx] as u32;
        let mut green = data[tex_idx + 1] as u32;
        let mut blue = data[tex_idx + 2] as u32;
        if brighten {
            // PROOF: red/green/blue are u8 (0-255), multiplied by 3/2 gives max 382.
            // .min(255) is REQUIRED to prevent overflow when packing to u32 RGB.
            red = (red * 3 / 2).min(255);
            green = (green * 3 / 2).min(255);
            blue = (blue * 3 / 2).min(255);
        }
        (red << 16) | (green << 8) | blue
    } else {
        if brighten {
            0x404040
        } else {
            0x202020
        }
    }
}

// Helper functions for u32 packed pixel manipulation
// Desktop: ARGB format (0xAARRGGBB)
// Android: ABGR format (0xAABBGGRR)
#[inline]
#[cfg(not(target_os = "android"))]
fn pack_argb(r: u8, g: u8, b: u8, a: u8) -> u32 {
    ((a as u32) << 24) | ((r as u32) << 16) | ((g as u32) << 8) | (b as u32)
}

#[inline]
#[cfg(target_os = "android")]
fn pack_argb(r: u8, g: u8, b: u8, a: u8) -> u32 {
    ((a as u32) << 24) | ((b as u32) << 16) | ((g as u32) << 8) | (r as u32)
}

#[inline]
#[cfg(not(target_os = "android"))]
fn unpack_argb(pixel: u32) -> (u8, u8, u8, u8) {
    let a = (pixel >> 24) as u8;
    let r = (pixel >> 16) as u8;
    let g = (pixel >> 8) as u8;
    let b = pixel as u8;
    (r, g, b, a)
}

#[inline]
#[cfg(target_os = "android")]
fn unpack_argb(pixel: u32) -> (u8, u8, u8, u8) {
    let a = (pixel >> 24) as u8;
    let b = (pixel >> 16) as u8;
    let g = (pixel >> 8) as u8;
    let r = pixel as u8;
    (r, g, b, a)
}

/// Scale a packed ARGB colour by alpha (premultiply).
/// Formula: colour * alpha >> 8
#[inline]
fn scale_alpha(colour: u32, alpha: u8) -> u32 {
    let mut c = colour as u64;
    c = (c | (c << 16)) & 0x0000FFFF0000FFFF;
    c = (c | (c << 8)) & 0x00FF00FF00FF00FF;

    let mut scaled = c * alpha as u64;
    scaled = (scaled >> 8) & 0x00FF00FF00FF00FF;
    scaled = (scaled | (scaled >> 8)) & 0x0000FFFF0000FFFF;
    scaled = scaled | (scaled >> 16);

    scaled as u32
}

#[inline]
fn blend_rgb_only(bg_colour: u32, fg_colour: u32, weight_bg: u8, weight_fg: u8) -> u32 {
    let mut bg = bg_colour as u64;
    bg = (bg | (bg << 16)) & 0x0000FFFF0000FFFF;
    bg = (bg | (bg << 8)) & 0x00FF00FF00FF00FF;

    let mut fg = fg_colour as u64;
    fg = (fg | (fg << 16)) & 0x0000FFFF0000FFFF;
    fg = (fg | (fg << 8)) & 0x00FF00FF00FF00FF;

    // Blend all 4 channels (including alpha)
    let mut blended = bg * weight_bg as u64 + fg * weight_fg as u64;
    blended = (blended >> 8) & 0x00FF00FF00FF00FF;
    blended = (blended | (blended >> 8)) & 0x0000FFFF0000FFFF;
    blended = blended | (blended >> 16) | 0xFF000000;

    blended as u32
}
