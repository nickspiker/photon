use crate::ui::{app::*, text_rasterizing::TextRenderer, theme};

impl PhotonApp {
    /// Check if textbox is focused (for event loop control flow)
    pub fn textbox_is_focused(&self) -> bool {
        self.current_text_state.textbox_focused
    }

    /// Get next cursor blink wake time (random interval 0..=125ms)
    pub fn next_blink_wake_time(&self) -> std::time::Instant {
        use rand::Rng;
        let interval_ms = rand::thread_rng().gen_range(0..=300);
        std::time::Instant::now() + std::time::Duration::from_millis(interval_ms)
    }

    /// Toggle cursor blink state and render only the cursor change
    pub fn toggle_cursor_blink(&mut self) {
        if !self.current_text_state.textbox_focused {
            return; // Cursor not visible, nothing to blink
        }

        let font_size = self.font_size();
        let mut buffer = self.renderer.lock_buffer();
        let pixels = buffer.as_mut();

        if self.cursor_wave_top_bright {
            // Currently Bright/Dark → swap to Dark/Bright
            Self::sub_cursor_top(
                pixels,
                self.width as usize,
                self.cursor_pixel_x as f32,
                self.cursor_pixel_y as f32,
                font_size,
            );
            Self::add_cursor_bottom(
                pixels,
                self.width as usize,
                self.cursor_pixel_x as f32,
                self.cursor_pixel_y as f32,
                font_size,
            );
            self.cursor_wave_top_bright = false;
        } else {
            // Currently Dark/Bright → swap to Bright/Dark
            Self::add_cursor_top(
                pixels,
                self.width as usize,
                self.cursor_pixel_x as f32,
                self.cursor_pixel_y as f32,
                font_size,
            );
            Self::sub_cursor_bottom(
                pixels,
                self.width as usize,
                self.cursor_pixel_x as f32,
                self.cursor_pixel_y as f32,
                font_size,
            );
            self.cursor_wave_top_bright = true;
        }

        // Present the buffer to show the cursor change
        buffer.present().unwrap();
    }

    /// Returns the font size for text rendering (min_dim / 16)
    pub fn font_size(&self) -> f32 {
        self.min_dim as f32 / 16.0
    }

    pub fn handle_cursor_left(&mut self) {
        self.hovered_button = HoveredButton::None;
        self.render();
    }
    /// Recalculate all character widths (e.g., after font size change on resize)
    pub fn recalculate_char_widths(&mut self) {
        let font_size = self.font_size();
        let (widths, total_width): (Vec<usize>, usize) = self
            .current_text_state
            .chars
            .iter()
            .map(|ch| {
                self.text_renderer.measure_text_width(
                    &ch.to_string(),
                    font_size,
                    theme::FONT_WEIGHT_USER_CONTENT,
                    theme::FONT_USER_CONTENT,
                ) as usize
            })
            .fold((Vec::new(), 0), |(mut vec, sum), width| {
                vec.push(width);
                (vec, sum + width)
            });

        self.current_text_state.width = total_width;
        self.current_text_state.widths = widths;
    }
    /// Render text with proper clipping to textbox bounds
    pub fn render_text_clipped(
        pixels: &mut [u32],
        text: &TextState,
        add_mode: bool,
        text_renderer: &mut TextRenderer,
        textbox_mask: &[u8],
        window_width: usize,
        window_height: usize,
        min_dim: usize,
        colour: u32,
    ) {
        if text.chars.is_empty() {
            return;
        }

        let margin = min_dim / 8;
        let box_width = window_width - margin * 2;
        let center_x = window_width / 2;
        let center_y = window_height * 4 / 7;

        let text_half = (text.width / 2) as isize;
        let text_start_x = (center_x as isize - text_half + text.scroll_offset) as f32;
        let textbox_left = (center_x - box_width / 2) as f32;
        let textbox_right = (center_x + box_width / 2) as f32;

        let mut x_offset = text_start_x;
        let font_size = min_dim as f32 / 16.0;

        for (i, &ch) in text.chars.iter().enumerate() {
            let char_width = text.widths[i] as f32;
            let char_right = x_offset + char_width;

            // Only render if character is visible within textbox bounds
            if char_right >= textbox_left && x_offset <= textbox_right {
                text_renderer.render_char_additive_u32(
                    pixels,
                    window_width,
                    ch,
                    x_offset,
                    center_y as f32,
                    font_size,
                    500,
                    theme::FONT_USER_CONTENT,
                    colour,
                    textbox_mask,
                    add_mode,
                );
            }

            x_offset += char_width;
        }
    }

    pub fn update_text_scroll(&mut self, textbox_width: usize) -> bool {
        if self.current_text_state.chars.is_empty() {
            self.current_text_state.scroll_offset = 0;
            return false;
        }

        let total_text_width: usize = self.current_text_state.width;

        if total_text_width <= textbox_width {
            self.current_text_state.scroll_offset = 0;
            return false;
        }

        let cursor_pixel_offset: usize = self.current_text_state.widths
            [..self.current_text_state.cursor_index]
            .iter()
            .sum();

        let margin = textbox_width / 40;
        let textbox_half = (textbox_width / 2) as isize;
        let text_half = (total_text_width / 2) as isize;

        let cursor_pos_in_centered_text = cursor_pixel_offset as isize - text_half;
        let cursor_pos_in_view =
            cursor_pos_in_centered_text + self.current_text_state.scroll_offset;

        if cursor_pos_in_view < -textbox_half + margin as isize {
            self.current_text_state.scroll_offset =
                -textbox_half + margin as isize - cursor_pos_in_centered_text;
            return true;
        } else if cursor_pos_in_view > textbox_half - margin as isize {
            self.current_text_state.scroll_offset =
                textbox_half - margin as isize - cursor_pos_in_centered_text;
            return true;
        }
        false
    }

    /// Get the selection range as (start, end) where start < end, or None if no selection
    pub fn get_selection_range(&self) -> Option<(usize, usize)> {
        self.current_text_state.selection_anchor.and_then(|anchor| {
            if anchor < self.current_text_state.cursor_index {
                Some((anchor, self.current_text_state.cursor_index))
            } else if anchor > self.current_text_state.cursor_index {
                Some((self.current_text_state.cursor_index, anchor))
            } else {
                // Anchor equals cursor - no selection
                None
            }
        })
    }

    /// Delete the currently selected text and clear selection
    pub fn delete_selection(&mut self) {
        if let Some((start, end)) = self.get_selection_range() {
            self.current_text_state.delete_range(start..end);
            self.current_text_state.cursor_index = start;
            self.current_text_state.selection_anchor = None;

            // Reset scroll offset if text is now empty
            if self.current_text_state.chars.is_empty() {
                self.current_text_state.scroll_offset = 0;
            }
        }
    }

    /// Get the selected text
    pub fn get_selected_text(&self) -> Option<String> {
        self.get_selection_range()
            .map(|(start, end)| self.current_text_state.chars[start..end].iter().collect())
    }

    pub fn add_cursor_top(
        pixels: &mut [u32],
        window_width: usize,
        cursor_x: f32,
        cursor_top: f32,
        cursor_height: f32,
    ) {
        let x = cursor_x as usize;
        let y_start = cursor_top as usize;
        let y_end = (cursor_top + cursor_height) as usize;
        let half_height = cursor_height / 2.;

        for y in y_start..y_end {
            let idx = y * window_width + x;

            // Map to [-1, 1] range for full cursor
            let t = (y as f32 - cursor_top - half_height) / half_height;

            let wave = (1. - t * t) * (1. + t) * (1. + t) * theme::CURSOR_BRIGHTNESS;
            for x in -7..=7isize {
                pixels[idx + x as usize] += 0x00010101 * (wave as u32 >> x.abs());
            }
        }
    }

    pub fn add_cursor_bottom(
        pixels: &mut [u32],
        window_width: usize,
        cursor_x: f32,
        cursor_top: f32,
        cursor_height: f32,
    ) {
        let x = cursor_x as usize;
        let y_start = cursor_top as usize;
        let y_end = (cursor_top + cursor_height) as usize;
        let half_height = cursor_height / 2.;

        for y in y_start..y_end {
            let idx = y * window_width + x;

            // Map to [-1, 1] range for full cursor
            let t = (y as f32 - cursor_top - half_height) / half_height;

            let wave = (1. - t * t) * (1. - t) * (1. - t) * theme::CURSOR_BRIGHTNESS;
            for x in -7..=7isize {
                pixels[idx + x as usize] += 0x00010101 * (wave as u32 >> x.abs());
            }
        }
    }

    pub fn sub_cursor_top(
        pixels: &mut [u32],
        window_width: usize,
        cursor_x: f32,
        cursor_top: f32,
        cursor_height: f32,
    ) {
        let x = cursor_x as usize;
        let y_start = cursor_top as usize;
        let y_end = (cursor_top + cursor_height) as usize;
        let half_height = cursor_height / 2.;

        for y in y_start..y_end {
            let idx = y * window_width + x;
            let t = (y as f32 - cursor_top - half_height) / half_height;
            let wave = (1. - t * t) * (1. + t) * (1. + t) * theme::CURSOR_BRIGHTNESS;
            for x in -7..=7isize {
                pixels[idx + x as usize] -= 0x00010101 * (wave as u32 >> x.abs());
            }
        }
    }

    pub fn sub_cursor_bottom(
        pixels: &mut [u32],
        window_width: usize,
        cursor_x: f32,
        cursor_top: f32,
        cursor_height: f32,
    ) {
        let x = cursor_x as usize;
        let y_start = cursor_top as usize;
        let y_end = (cursor_top + cursor_height) as usize;
        let half_height = cursor_height / 2.;

        for y in y_start..y_end {
            let idx = y * window_width + x;
            let t = (y as f32 - cursor_top - half_height) / half_height;
            let wave = (1. - t * t) * (1. - t) * (1. - t) * theme::CURSOR_BRIGHTNESS;
            for x in -7..=7isize {
                pixels[idx + x as usize] -= 0x00010101 * (wave as u32 >> x.abs());
            }
        }
    }

    // Invert RGB for selection highlight (reversible: 255 - (255 - x) = x)
    pub fn invert_selection(
        pixels: &mut [u32],
        char_widths: &[usize],
        scroll_offset: isize,
        window_width: usize,
        window_height: usize,
        sel_start: usize,
        sel_end: usize,
        box_width: usize,
        font_size: f32,
        center_x: usize,
        center_y: usize,
        hit_test_map: &[u8],
    ) {
        if sel_start >= sel_end || sel_start >= char_widths.len() {
            return;
        }

        let sel_start_px: usize = char_widths[..sel_start].iter().sum();
        let sel_end_px: usize = char_widths[..sel_end.min(char_widths.len())].iter().sum();

        let total_text_width: usize = char_widths.iter().sum();
        let text_half = (total_text_width / 2) as isize;
        let text_start_x = center_x as isize - text_half + scroll_offset;
        let sel_x_start = text_start_x + sel_start_px as isize;
        let sel_x_end = text_start_x + sel_end_px as isize;

        let sel_y_top = center_y as isize - (font_size / 2.0) as isize;
        let sel_y_bottom = center_y as isize + (font_size / 2.0) as isize;

        let textbox_left = (center_x - box_width / 2) as isize;
        let textbox_right = (center_x + box_width / 2) as isize;

        for y in sel_y_top.max(0)..sel_y_bottom.min(window_height as isize) {
            for x in sel_x_start.max(textbox_left)..sel_x_end.min(textbox_right) {
                let idx = y as usize * window_width + x as usize;
                // Only invert pixels inside the textbox (hard clip to hit test map)
                if hit_test_map[idx] == HIT_HANDLE_TEXTBOX {
                    pixels[idx] ^= 0x00FFFFFF;
                }
            }
        }
    }
}
