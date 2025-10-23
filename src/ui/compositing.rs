use crate::ui::{app::*, colour::*, text_rasterizing::*, theme};
use crate::{debug_println, DEBUG};

impl PhotonApp {
    pub fn render(&mut self) {
        // Increment frame counter (every render() call)
        self.frame_counter += 1;

        debug_println!("FRAME #{}", self.frame_counter);

        // Calculate layout constants (needed by all rendering paths)
        let font_size = self.font_size();
        let margin = self.min_dim / 8;
        let box_width = self.width as usize - margin * 2;
        let box_height = self.min_dim / 8;
        let center_x = self.width as usize / 2;
        let center_y = self.height as usize * 4 / 7;

        // Check if empty state changed (for button show/hide logic)
        let current_is_empty = self.current_text_state.chars.is_empty();
        let prev_is_empty = self.previous_text_state.is_empty;
        if current_is_empty != prev_is_empty {
            if !current_is_empty {
                // Empty → Non-empty: button will appear (draw it after text updates)
                // No action needed here, we'll draw it after differential rendering
            } else {
                // Non-empty → Empty: button needs to disappear, trigger full redraw
                self.window_dirty = true;
            }
        }

        // Always update scroll to keep cursor in view (but not during selection drag)
        if self.current_text_state.textbox_focused
            && !self.current_text_state.chars.is_empty()
            && !self.is_mouse_selecting
        {
            self.text_dirty |= self.update_text_scroll(box_width);
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
                || self.current_text_state.cursor_index != self.previous_text_state.cursor_index
                || self.current_text_state.scroll_offset != self.previous_text_state.scroll_offset;
        }

        if self.text_dirty || self.selection_dirty || self.window_dirty || self.controls_dirty {
            self.update_counter += 1;
            debug_println!("UPDATE #{}", self.update_counter);
            let mut buffer = self.renderer.lock_buffer();
            let pixels = buffer.as_mut();

            if self.window_dirty {
                self.full_redraw_counter += 1;
                debug_println!("FULL REDRAW #{}", self.full_redraw_counter);
                self.selection_dirty = false;
                self.text_dirty = false;
                self.hit_test_map.fill(HIT_NONE);
                self.textbox_mask.fill(0);

                Self::draw_background_texture(pixels, self.width as usize, self.height as usize);

                let (start, edges, button_x_start, button_height) = Self::draw_window_controls(
                    pixels,
                    &mut self.hit_test_map,
                    self.width,
                    self.height,
                );

                // Skip drawing window edges when fullscreen or maximized
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

                // Needs swaped for centerpoint fill algorithm
                self.minimize_pixels.clear();
                self.maximize_pixels.clear();
                self.close_pixels.clear();

                for idx in 0..self.hit_test_map.len() {
                    match self.hit_test_map[idx] {
                        HIT_MINIMIZE_BUTTON => self.minimize_pixels.push(idx),
                        HIT_MAXIMIZE_BUTTON => self.maximize_pixels.push(idx),
                        HIT_CLOSE_BUTTON => self.close_pixels.push(idx),
                        _ => {}
                    }
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

                // Draw spectrum and logo text
                let logo_center_y = self.height as usize / 4; // Centered in top half
                Self::draw_spectrum(
                    pixels,
                    self.width,
                    self.height,
                    logo_center_y - self.height.min(self.width) as usize / 8,
                );
                Self::draw_logo_text(
                    pixels,
                    &mut self.text_renderer,
                    self.width,
                    self.height,
                    logo_center_y + self.height.min(self.width) as usize / 8,
                );

                // 2. Draw textbox (full width with min_dim/8 margins)
                Self::draw_textbox(
                    pixels,
                    &mut self.hit_test_map,
                    HIT_HANDLE_TEXTBOX,
                    &mut self.textbox_mask,
                    self.width as usize,
                    center_x,
                    center_y,
                    box_width,
                    box_height,
                );

                // Label below the box
                self.text_renderer.draw_text_center_u32(
                    pixels,
                    self.width as usize,
                    "handle",
                    center_x as f32,
                    (center_y + box_height) as f32,
                    font_size,
                    300,
                    theme::FONT_LABEL,
                    theme::FONT_UI,
                );

                // 4. Draw attestation buttons - MOVED to after differential rendering
                // (so buttons are drawn on both full redraws and differential updates)

                /*
                if !self.current_text_state.chars.is_empty() {
                    let button_center_y = center_y + box_height + box_height;
                    let button_height = box_height;

                    match self.handle_status {
                        HandleStatus::Empty => {
                            // Show "Query" button with blue fill
                            let button_width = box_width / 2;
                            Self::draw_button(
                                pixels,
                                &mut self.hit_test_map,
                                self.width as usize,
                                self.height as usize,
                                center_x,
                                button_center_y,
                                button_width,
                                button_height,
                                HIT_PRIMARY_BUTTON,
                                theme::BUTTON_BLUE, // Blue fill
                                theme::BUTTON_LIGHT_EDGE,
                                theme::BUTTON_SHADOW_EDGE,
                            );

                            self.text_renderer.draw_text_center_u32(
                                pixels,
                                self.width as usize,
                                "Query",
                                center_x as f32,
                                button_center_y as f32,
                                font_size,
                                500,
                                0xFF_D0_D0_D0,
                                theme::FONT_USER_CONTENT,
                            );
                        }
                        HandleStatus::Checking => {
                            // Show "Querying..." button with grey fill
                            let button_width = box_width / 2;
                            Self::draw_button(
                                pixels,
                                &mut self.hit_test_map,
                                self.width as usize,
                                self.height as usize,
                                center_x,
                                button_center_y,
                                button_width,
                                button_height,
                                HIT_NONE,           // Not clickable while querying
                                theme::BUTTON_BASE, // Grey fill
                                theme::BUTTON_LIGHT_EDGE,
                                theme::BUTTON_SHADOW_EDGE,
                            );

                            self.text_renderer.draw_text_center_u32(
                                pixels,
                                self.width as usize,
                                "Querying...",
                                center_x as f32,
                                button_center_y as f32,
                                font_size,
                                500,
                                0xFF_80_80_80, // Dimmer text for disabled state
                                theme::FONT_USER_CONTENT,
                            );
                        }
                        HandleStatus::Unattested => {
                            // Show single "Attest" button with dark green fill
                            let button_width = box_width / 2;
                            Self::draw_button(
                                pixels,
                                &mut self.hit_test_map,
                                self.width as usize,
                                self.height as usize,
                                center_x,
                                button_center_y,
                                button_width,
                                button_height,
                                HIT_PRIMARY_BUTTON,
                                theme::BUTTON_GREEN, // Dark green fill
                                theme::BUTTON_LIGHT_EDGE,
                                theme::BUTTON_SHADOW_EDGE,
                            );

                            self.text_renderer.draw_text_center_u32(
                                pixels,
                                self.width as usize,
                                "Attest",
                                center_x as f32,
                                button_center_y as f32,
                                font_size,
                                500,
                                0xFF_D0_D0_D0,
                                theme::FONT_USER_CONTENT,
                            );
                        }
                        HandleStatus::AlreadyAttested => {
                            // Show single "Recover / Challenge" button with dark yellow fill
                            let button_width = box_width / 2;
                            Self::draw_button(
                                pixels,
                                &mut self.hit_test_map,
                                self.width as usize,
                                self.height as usize,
                                center_x,
                                button_center_y,
                                button_width,
                                button_height,
                                HIT_PRIMARY_BUTTON,
                                theme::BUTTON_YELLOW, // Dark yellow fill
                                theme::BUTTON_LIGHT_EDGE,
                                theme::BUTTON_SHADOW_EDGE,
                            );

                            self.text_renderer.draw_text_center_u32(
                                pixels,
                                self.width as usize,
                                "Recover / Challenge",
                                center_x as f32,
                                button_center_y as f32,
                                font_size,
                                500,
                                0xFF_D0_D0_D0,
                                theme::FONT_USER_CONTENT,
                            );
                        }
                        HandleStatus::RecoverOrChallenge => {
                            // Show explanation text
                            let handle_text =
                                self.current_text_state.chars.iter().collect::<String>();
                            let explanation = format!("{} is already attested.", handle_text);

                            let text_y = button_center_y - box_height * 2;
                            self.text_renderer.draw_text_center_u32(
                                pixels,
                                self.width as usize,
                                &explanation,
                                center_x as f32,
                                text_y as f32,
                                font_size * 0.75,
                                400,
                                0xFF_B0_B0_B0,
                                theme::FONT_USER_CONTENT,
                            );

                            let question_y = text_y + (box_height as f32 * 0.75) as usize;
                            self.text_renderer.draw_text_center_u32(
                                pixels,
                                self.width as usize,
                                "Are you recovering your own identity,",
                                center_x as f32,
                                question_y as f32,
                                font_size * 0.75,
                                400,
                                0xFF_B0_B0_B0,
                                theme::FONT_USER_CONTENT,
                            );

                            let question2_y = question_y + (box_height as f32 * 0.6) as usize;
                            self.text_renderer.draw_text_center_u32(
                                pixels,
                                self.width as usize,
                                "or challenging someone else's claim?",
                                center_x as f32,
                                question2_y as f32,
                                font_size * 0.75,
                                400,
                                0xFF_B0_B0_B0,
                                theme::FONT_USER_CONTENT,
                            );

                            // Draw two buttons side by side
                            let button_width = box_width / 4;
                            let spacing = box_width / 8;
                            let recover_x = center_x - spacing - button_width / 2;
                            let challenge_x = center_x + spacing + button_width / 2;

                            // Left button: "Recover"
                            Self::draw_button(
                                pixels,
                                &mut self.hit_test_map,
                                self.width as usize,
                                self.height as usize,
                                recover_x,
                                button_center_y,
                                button_width,
                                button_height,
                                HIT_RECOVER_BUTTON,
                                theme::BUTTON_GREEN, // Green for recover
                                theme::BUTTON_LIGHT_EDGE,
                                theme::BUTTON_SHADOW_EDGE,
                            );

                            self.text_renderer.draw_text_center_u32(
                                pixels,
                                self.width as usize,
                                "Recover",
                                recover_x as f32,
                                button_center_y as f32,
                                font_size * 0.85,
                                500,
                                0xFF_D0_D0_D0,
                                theme::FONT_USER_CONTENT,
                            );

                            // Small subtitle under Recover button
                            let subtitle_y = button_center_y + (box_height as f32 * 0.6) as usize;
                            self.text_renderer.draw_text_center_u32(
                                pixels,
                                self.width as usize,
                                "(I'm you)",
                                recover_x as f32,
                                subtitle_y as f32,
                                font_size * 0.5,
                                300,
                                0xFF_80_80_80,
                                theme::FONT_USER_CONTENT,
                            );

                            // Right button: "Challenge"
                            Self::draw_button(
                                pixels,
                                &mut self.hit_test_map,
                                self.width as usize,
                                self.height as usize,
                                challenge_x,
                                button_center_y,
                                button_width,
                                button_height,
                                HIT_CHALLENGE_BUTTON,
                                theme::BUTTON_YELLOW, // Yellow for challenge
                                theme::BUTTON_LIGHT_EDGE,
                                theme::BUTTON_SHADOW_EDGE,
                            );

                            self.text_renderer.draw_text_center_u32(
                                pixels,
                                self.width as usize,
                                "Challenge",
                                challenge_x as f32,
                                button_center_y as f32,
                                font_size * 0.85,
                                500,
                                0xFF_D0_D0_D0,
                                theme::FONT_USER_CONTENT,
                            );

                            // Small subtitle under Challenge button
                            self.text_renderer.draw_text_center_u32(
                                pixels,
                                self.width as usize,
                                "(They stole this)",
                                challenge_x as f32,
                                subtitle_y as f32,
                                font_size * 0.5,
                                300,
                                0xFF_80_80_80,
                                theme::FONT_USER_CONTENT,
                            );
                        }
                    }
                }
                */

                // Debug: overlay hit test map visualization with random colours
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
                }

                // Frame counter continues incrementing (not reset)
                // After full redraw, pixel lists are rebuilt - reset prev_hovered to force hover reapply
                self.prev_hovered_button = HoveredButton::None;
            } else {
                // Differential rendering blocks (only if window wasn't fully redrawn)
                if self.selection_dirty || self.text_dirty {
                    // 1. Invert old cursor (if visible)
                    if self.cursor_visible {
                        debug_println!(
                            "  DIFF: undraw cursor at ({}, {})",
                            self.cursor_pixel_x,
                            self.cursor_pixel_y
                        );
                        Self::undraw_cursor(
                            pixels,
                            self.width as usize,
                            self.cursor_pixel_x,
                            self.cursor_pixel_y,
                            &mut self.cursor_visible,
                            &mut self.cursor_wave_top_bright,
                            font_size as usize,
                        );
                    }

                    if self.selection_dirty {
                        // 2. Invert old selection (if present)
                        if let Some(anchor) = self.previous_text_state.selection_anchor {
                            let (sel_start, sel_end) =
                                if anchor < self.previous_text_state.cursor_index {
                                    (anchor, self.previous_text_state.cursor_index)
                                } else if anchor > self.previous_text_state.cursor_index {
                                    (self.previous_text_state.cursor_index, anchor)
                                } else {
                                    (0, 0)
                                };

                            if sel_start != sel_end {
                                Self::invert_selection(
                                    pixels,
                                    &self.previous_text_state.widths,
                                    self.previous_text_state.scroll_offset,
                                    self.width as usize,
                                    self.height as usize,
                                    sel_start,
                                    sel_end,
                                    box_width,
                                    font_size,
                                    center_x,
                                    center_y,
                                    &self.hit_test_map,
                                );
                            }
                        }

                        if self.text_dirty {
                            // 3. Remove old text
                            if !self.previous_text_state.chars.is_empty() {
                                Self::render_text_clipped(
                                    pixels,
                                    &self.previous_text_state,
                                    false, // Subtract!
                                    &mut self.text_renderer,
                                    &self.textbox_mask,
                                    self.width as usize,
                                    self.height as usize,
                                    self.min_dim,
                                    theme::TEXT_COLOUR,
                                );
                            } else {
                                if !self.previous_text_state.textbox_focused {
                                    let char_width = self.text_renderer.measure_text_width(
                                        "∞",
                                        font_size,
                                        500,
                                        theme::FONT_USER_CONTENT,
                                    );

                                    self.text_renderer.render_char_additive_u32(
                                        pixels,
                                        self.width as usize,
                                        '∞',
                                        center_x as f32 - char_width / 2.0,
                                        center_y as f32,
                                        font_size,
                                        500,
                                        theme::FONT_USER_CONTENT,
                                        0xFF808080,
                                        &self.textbox_mask,
                                        false,
                                    );
                                }
                            }
                        }
                    }
                }
            }

            if self.text_dirty || self.window_dirty {
                // 4. Add new text
                if !self.current_text_state.chars.is_empty() {
                    Self::render_text_clipped(
                        pixels,
                        &self.current_text_state,
                        true, // Add!
                        &mut self.text_renderer,
                        &self.textbox_mask,
                        self.width as usize,
                        self.height as usize,
                        self.min_dim,
                        theme::TEXT_COLOUR,
                    );
                } else {
                    if !self.current_text_state.textbox_focused {
                        let char_width = self.text_renderer.measure_text_width(
                            "∞",
                            font_size,
                            500,
                            theme::FONT_USER_CONTENT,
                        );

                        self.text_renderer.render_char_additive_u32(
                            pixels,
                            self.width as usize,
                            '∞',
                            center_x as f32 - char_width / 2.0,
                            center_y as f32,
                            font_size,
                            500,
                            theme::FONT_USER_CONTENT,
                            0xFF808080,
                            &self.textbox_mask,
                            true,
                        );
                    }
                }
            }

            if self.selection_dirty || self.window_dirty {
                // 5. Invert new selection (if present)
                if let Some(anchor) = self.current_text_state.selection_anchor {
                    let (sel_start, sel_end) = if anchor < self.current_text_state.cursor_index {
                        (anchor, self.current_text_state.cursor_index)
                    } else if anchor > self.current_text_state.cursor_index {
                        (self.current_text_state.cursor_index, anchor)
                    } else {
                        (0, 0)
                    };

                    if sel_start != sel_end {
                        Self::invert_selection(
                            pixels,
                            &self.current_text_state.widths,
                            self.current_text_state.scroll_offset,
                            self.width as usize,
                            self.height as usize,
                            sel_start,
                            sel_end,
                            box_width,
                            font_size,
                            center_x,
                            center_y,
                            &self.hit_test_map,
                        );
                    }
                }
            }

            // 6. Cursor drawing MOVED to after both rendering paths (with button drawing)
            // This ensures cursor is drawn exactly once per frame on both full redraws and differential updates

            // Controls dirty - handle hover and focus transitions
            if self.controls_dirty {
                // Handle hover state changes
                if self.prev_hovered_button != self.hovered_button {
                    // Calculate button centers for centerpoint fill
                    let smaller_dim = self.width.min(self.height) as f32;
                    let button_height = (smaller_dim / 16.).ceil() as usize;
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
                                self.debug_hit_test,
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
                                self.debug_hit_test,
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
                                self.debug_hit_test,
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
                                self.debug_hit_test,
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
                                self.debug_hit_test,
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
                                self.debug_hit_test,
                            );
                        }
                        HoveredButton::None => {}
                    }

                    // Update prev state
                    self.prev_hovered_button = self.hovered_button;
                }
            }
            if DEBUG {
                // Draw black strip at bottom for debug counters
                let counter_size = self.min_dim / 16;
                let strip_height = counter_size * 3;
                let counter_size = counter_size as f32;
                for y in (self.height as usize - strip_height)..self.height as usize {
                    for x in 0..self.width as usize {
                        let idx = y * self.width as usize + x;
                        pixels[idx] = pixels[idx] >> 1 & 0xFF7F7F7F | 0xFF000000;
                    }
                }

                // Draw debug counters (bottom left = full redraws, bottom center = updates, bottom right = frames)
                let full_redraw_text = format!("FR:{}", self.full_redraw_counter);
                let update_text = format!("U:{}", self.update_counter);
                let frame_text = format!("F:{}", self.frame_counter);

                // Bottom left - full redraw counter
                self.text_renderer.draw_text_left_u32(
                    pixels,
                    self.width as usize,
                    &full_redraw_text,
                    counter_size,
                    self.height as f32 - counter_size * 2.,
                    counter_size,
                    400,
                    0xFFFFFFFF,
                    "Josefin Slab",
                );

                // Bottom center - update counter
                let text_width = self.text_renderer.measure_text_width(
                    &update_text,
                    counter_size,
                    400,
                    "Josefin Slab",
                );
                self.text_renderer.draw_text_left_u32(
                    pixels,
                    self.width as usize,
                    &update_text,
                    self.width as f32 / 2.0 - text_width / 2.0,
                    self.height as f32 - counter_size * 2.,
                    counter_size,
                    400,
                    0xFFFFFFFF,
                    "Josefin Slab",
                );

                // Bottom right - frame counter
                self.text_renderer.draw_text_right_u32(
                    pixels,
                    self.width as usize,
                    &frame_text,
                    self.width as f32 - counter_size,
                    self.height as f32 - counter_size * 2.,
                    counter_size,
                    400,
                    0xFFFFFFFF,
                    "Josefin Slab",
                );
            }

            // Draw cursor (if visible and focused) - must be done on both full redraws and differential updates
            if self.cursor_visible && self.current_text_state.textbox_focused {
                let cursor_pixel_offset: usize = if self.current_text_state.cursor_index > 0 {
                    self.current_text_state.widths[..self.current_text_state.cursor_index]
                        .iter()
                        .sum()
                } else {
                    0
                };
                let total_text_width: usize = self.current_text_state.width;
                let text_half = total_text_width / 2;
                let cursor_x = (center_x as f32 - text_half as f32
                    + self.current_text_state.scroll_offset
                    + cursor_pixel_offset as f32) as usize;
                let cursor_y = (center_y as f32 - box_height as f32 * 0.25) as usize;

                self.cursor_pixel_x = cursor_x;
                self.cursor_pixel_y = cursor_y;
                Self::draw_cursor(
                    pixels,
                    self.width as usize,
                    cursor_x,
                    cursor_y,
                    &mut self.cursor_visible,
                    &mut self.cursor_wave_top_bright,
                    font_size as usize,
                );
            }

            if self.current_text_state.chars.is_empty() != self.previous_text_state.chars.is_empty()
                && !self.current_text_state.chars.is_empty()
                || self.window_dirty && !self.current_text_state.chars.is_empty()
            {
                let button_center_y = center_y + box_height + box_height;
                let button_height = box_height;

                match self.handle_status {
                    HandleStatus::Empty => {
                        // Show "Query" button with blue fill
                        let button_width = box_width / 2;
                        Self::draw_button(
                            pixels,
                            &mut self.hit_test_map,
                            self.width as usize,
                            self.height as usize,
                            center_x,
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
                            "Query",
                            center_x as f32,
                            button_center_y as f32,
                            font_size,
                            500,
                            0xFF_D0_D0_D0,
                            theme::FONT_USER_CONTENT,
                        );
                    }
                    HandleStatus::Checking => {
                        // Show "Querying..." button with grey fill
                        let button_width = box_width / 2;
                        Self::draw_button(
                            pixels,
                            &mut self.hit_test_map,
                            self.width as usize,
                            self.height as usize,
                            center_x,
                            button_center_y,
                            button_width,
                            button_height,
                            HIT_NONE,
                            theme::BUTTON_BASE,
                            theme::BUTTON_LIGHT_EDGE,
                            theme::BUTTON_SHADOW_EDGE,
                        );

                        self.text_renderer.draw_text_center_u32(
                            pixels,
                            self.width as usize,
                            "Querying...",
                            center_x as f32,
                            button_center_y as f32,
                            font_size,
                            500,
                            0xFF_80_80_80,
                            theme::FONT_USER_CONTENT,
                        );
                    }
                    HandleStatus::Unattested => {
                        // Show single "Attest" button with dark green fill
                        let button_width = box_width / 2;
                        Self::draw_button(
                            pixels,
                            &mut self.hit_test_map,
                            self.width as usize,
                            self.height as usize,
                            center_x,
                            button_center_y,
                            button_width,
                            button_height,
                            HIT_PRIMARY_BUTTON,
                            theme::BUTTON_GREEN,
                            theme::BUTTON_LIGHT_EDGE,
                            theme::BUTTON_SHADOW_EDGE,
                        );

                        self.text_renderer.draw_text_center_u32(
                            pixels,
                            self.width as usize,
                            "Attest",
                            center_x as f32,
                            button_center_y as f32,
                            font_size,
                            500,
                            0xFF_D0_D0_D0,
                            theme::FONT_USER_CONTENT,
                        );
                    }
                    HandleStatus::AlreadyAttested => {
                        // Show single "Recover / Challenge" button with dark yellow fill
                        let button_width = box_width / 2;
                        Self::draw_button(
                            pixels,
                            &mut self.hit_test_map,
                            self.width as usize,
                            self.height as usize,
                            center_x,
                            button_center_y,
                            button_width,
                            button_height,
                            HIT_PRIMARY_BUTTON,
                            theme::BUTTON_YELLOW,
                            theme::BUTTON_LIGHT_EDGE,
                            theme::BUTTON_SHADOW_EDGE,
                        );

                        self.text_renderer.draw_text_center_u32(
                            pixels,
                            self.width as usize,
                            "Recover / Challenge",
                            center_x as f32,
                            button_center_y as f32,
                            font_size,
                            500,
                            0xFF_D0_D0_D0,
                            theme::FONT_USER_CONTENT,
                        );
                    }
                    HandleStatus::RecoverOrChallenge => {
                        // Show explanation text
                        let handle_text = self.current_text_state.chars.iter().collect::<String>();
                        let explanation = format!("{} is already attested.", handle_text);

                        let text_y = button_center_y - box_height * 2;
                        self.text_renderer.draw_text_center_u32(
                            pixels,
                            self.width as usize,
                            &explanation,
                            center_x as f32,
                            text_y as f32,
                            font_size * 0.75,
                            400,
                            0xFF_B0_B0_B0,
                            theme::FONT_USER_CONTENT,
                        );

                        let question_y = text_y + (box_height as f32 * 0.75) as usize;
                        self.text_renderer.draw_text_center_u32(
                            pixels,
                            self.width as usize,
                            "Are you recovering your own identity,",
                            center_x as f32,
                            question_y as f32,
                            font_size * 0.75,
                            400,
                            0xFF_B0_B0_B0,
                            theme::FONT_USER_CONTENT,
                        );

                        let question2_y = question_y + (box_height as f32 * 0.6) as usize;
                        self.text_renderer.draw_text_center_u32(
                            pixels,
                            self.width as usize,
                            "or challenging someone else's claim?",
                            center_x as f32,
                            question2_y as f32,
                            font_size * 0.75,
                            400,
                            0xFF_B0_B0_B0,
                            theme::FONT_USER_CONTENT,
                        );

                        // Draw two buttons side by side
                        let button_width = box_width / 4;
                        let spacing = box_width / 8;
                        let recover_x = center_x - spacing - button_width / 2;
                        let challenge_x = center_x + spacing + button_width / 2;

                        // Left button: "Recover"
                        Self::draw_button(
                            pixels,
                            &mut self.hit_test_map,
                            self.width as usize,
                            self.height as usize,
                            recover_x,
                            button_center_y,
                            button_width,
                            button_height,
                            HIT_RECOVER_BUTTON,
                            theme::BUTTON_GREEN,
                            theme::BUTTON_LIGHT_EDGE,
                            theme::BUTTON_SHADOW_EDGE,
                        );

                        self.text_renderer.draw_text_center_u32(
                            pixels,
                            self.width as usize,
                            "Recover",
                            recover_x as f32,
                            button_center_y as f32,
                            font_size * 0.85,
                            500,
                            0xFF_D0_D0_D0,
                            theme::FONT_USER_CONTENT,
                        );

                        // Small subtitle under Recover button
                        let subtitle_y = button_center_y + (box_height as f32 * 0.6) as usize;
                        self.text_renderer.draw_text_center_u32(
                            pixels,
                            self.width as usize,
                            "(I'm you)",
                            recover_x as f32,
                            subtitle_y as f32,
                            font_size * 0.5,
                            300,
                            0xFF_80_80_80,
                            theme::FONT_USER_CONTENT,
                        );

                        // Right button: "Challenge"
                        Self::draw_button(
                            pixels,
                            &mut self.hit_test_map,
                            self.width as usize,
                            self.height as usize,
                            challenge_x,
                            button_center_y,
                            button_width,
                            button_height,
                            HIT_CHALLENGE_BUTTON,
                            theme::BUTTON_YELLOW,
                            theme::BUTTON_LIGHT_EDGE,
                            theme::BUTTON_SHADOW_EDGE,
                        );

                        self.text_renderer.draw_text_center_u32(
                            pixels,
                            self.width as usize,
                            "Challenge",
                            challenge_x as f32,
                            button_center_y as f32,
                            font_size * 0.85,
                            500,
                            0xFF_D0_D0_D0,
                            theme::FONT_USER_CONTENT,
                        );

                        // Small subtitle under Challenge button
                        self.text_renderer.draw_text_center_u32(
                            pixels,
                            self.width as usize,
                            "(They stole this)",
                            challenge_x as f32,
                            subtitle_y as f32,
                            font_size * 0.5,
                            300,
                            0xFF_80_80_80,
                            theme::FONT_USER_CONTENT,
                        );
                    }
                }
            }

            // Always present buffer once per frame
            buffer.present().unwrap();
        }
        self.window_dirty = false;
        self.text_dirty = false;
        self.selection_dirty = false;
        self.controls_dirty = false;
        self.previous_text_state = self.current_text_state.clone();
        self.previous_text_state.is_empty = self.current_text_state.chars.is_empty();
    }

    pub fn draw_window_controls(
        pixels: &mut [u32],
        hit_test_map: &mut [u8],
        window_width: u32,
        window_height: u32,
    ) -> (usize, Vec<(u16, u8, u8)>, usize, usize) {
        let window_width = window_width as usize;
        let window_height = window_height as usize;

        // Calculate button dimensions
        let smaller_dim = window_width.min(window_height) as f32;
        let button_height = (smaller_dim / 16.).ceil() as usize;
        let button_width = button_height;
        let total_width = button_width * 7 / 2;

        // Buttons extend to top-right corner of window
        let mut x_start = window_width - total_width;
        let y_start = 0;

        // Build squircle crossings for bottom-left corner
        // Use same squirdleyness as main window (24)
        let radius = smaller_dim / 2.;
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
    pub fn draw_background_texture(pixels: &mut [u32], width: usize, height: usize) {
        use rayon::prelude::*;

        let middle_rows = &mut pixels[width..(height - 1) * width];

        middle_rows
            .par_chunks_mut(width)
            .enumerate()
            .for_each(|(row_idx, row_pixels)| {
                let mut rng: usize = 0xDEADBEEF01234567
                    ^ ((row_idx.wrapping_sub(height / 2)).wrapping_mul(0x9E3779B94517B397));
                let mask = 0xFF0F071F; // ARGB
                let alpha = 0xFF000000;
                let ones = 0x00010101;
                let base = 0xFF0C140E;
                let mut colour = rng as u32 & mask | alpha;

                // Right half: left-to-right
                for x in width / 2..width - 1 {
                    rng ^= rng.rotate_left(13).wrapping_add(12345678901);
                    let adder = rng as u32 & ones;
                    if rng as u8 == 42 {
                        colour = rng as u32 >> 8 & 0x00_3F_1F_7F | alpha;
                    } else {
                        colour = colour + adder & mask;
                        let subtractor = (rng >> 5) as u32 & ones;
                        colour = colour - subtractor & mask;
                    }
                    row_pixels[x] = colour + base | alpha;
                }

                // Left half: right-to-left (mirror)
                rng = 0xDEADBEEF01234567
                    ^ ((row_idx.wrapping_sub(height / 2)).wrapping_mul(0x9E3779B94517B397));
                colour = rng as u32 & mask | alpha;

                for x in (1..width / 2).rev() {
                    rng ^= rng.rotate_left(13).wrapping_sub(12345678901);
                    let adder = rng as u32 & ones;
                    if rng as u8 == 42 {
                        colour = rng as u32 >> 8 & 0x00_3F_1F_7F | alpha;
                    } else {
                        colour = colour + adder & mask;
                        let subtractor = (rng >> 5) as u32 & ones;
                        colour = colour - subtractor & mask;
                    }
                    row_pixels[x] = colour + base | alpha;
                }
            });
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
            pixels[pixel_idx] = blend_alpha(light_colour, h);
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
            pixels[pixel_idx] = blend_alpha(shadow_colour, h);
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
            pixels[pixel_idx] = blend_alpha(light_colour, h);
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
            pixels[pixel_idx] = blend_alpha(shadow_colour, h);
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
            pixels[pixel_idx] = blend_alpha(light_colour, h);
            if h < 255 {
                hit_test_map[pixel_idx] = HIT_NONE;
            }

            // Top inner edge pixel
            let pixel_idx = (inset as usize + 1) * width as usize + x_left;
            pixels[pixel_idx] = blend_rgb_only(pixels[pixel_idx], light_colour, h, l);

            // Bottom outer edge pixel
            let pixel_idx = (height as usize - 1 - inset as usize) * width as usize + x_left;
            pixels[pixel_idx] = blend_alpha(shadow_colour, h);
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
            pixels[pixel_idx] = blend_alpha(light_colour, h);
            if h < 255 {
                hit_test_map[pixel_idx] = HIT_NONE;
            }

            // Top inner edge pixel
            let pixel_idx = (inset as usize + 1) * width as usize + x_right;
            pixels[pixel_idx] = blend_rgb_only(pixels[pixel_idx], light_colour, h, l);

            // Bottom outer edge pixel
            let pixel_idx = (height as usize - 1 - inset as usize) * width as usize + x_right;
            pixels[pixel_idx] = blend_alpha(shadow_colour, h);
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
        // Get the hover deltas for this button type
        let hover_delta = match button_type {
            HoveredButton::Close => theme::CLOSE_HOVER,
            HoveredButton::Maximize => theme::MAXIMIZE_HOVER,
            HoveredButton::Minimize => theme::MINIMIZE_HOVER,
            HoveredButton::None => [0, 0, 0, 0],
        };

        // Apply deltas (positive for hover, negative for unhover)
        let sign = if hover { 1 } else { -1 };
        let r_delta = hover_delta[0] * sign;
        let g_delta = hover_delta[1] * sign;
        let b_delta = hover_delta[2] * sign;

        // Iterate only over the cached pixels for this button
        for &hit_idx in pixel_list {
            if hit_idx < pixels.len() {
                // Unpack u32 pixel (ARGB format)
                let (r, g, b, a) = unpack_argb(pixels[hit_idx]);

                // Apply deltas with wrapping
                let new_r = r.wrapping_add(r_delta as u8);
                let new_g = g.wrapping_add(g_delta as u8);
                let new_b = b.wrapping_add(b_delta as u8);

                // Pack back to u32
                pixels[hit_idx] = pack_argb(new_r, new_g, new_b, a);
            }
        }
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
        hover_delta: [i8; 4],
        debug_hit_test: bool,
    ) {
        // Debug: draw magenta pixel at centerpoint when hit test map is visible
        // Use alpha=254 so we can skip it in the hover effect loop
        if debug_hit_test {
            let debug_idx = center_y * window_width + center_x;
            if debug_idx < pixels.len() {
                pixels[debug_idx] = 0xFE_FF_00_FF; // Magenta with alpha=254
            }
        }

        // Apply deltas (positive for hover, negative for unhover)
        let sign = if hover { 1 } else { -1 };
        let r_delta = hover_delta[0] * sign;
        let g_delta = hover_delta[1] * sign;
        let b_delta = hover_delta[2] * sign;

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
                let idx = row_start + x;
                if idx < pixels.len() && hit_test_map[idx] == hit_id {
                    // Skip debug pixels (magenta with alpha=254)
                    if pixels[idx] == 0xFE_FF_00_FF {
                        continue;
                    }

                    // Unpack u32 pixel (ARGB format)
                    let (r, g, b, a) = unpack_argb(pixels[idx]);

                    // Apply deltas with wrapping
                    let new_r = r.wrapping_add(r_delta as u8);
                    let new_g = g.wrapping_add(g_delta as u8);
                    let new_b = b.wrapping_add(b_delta as u8);

                    // Pack back to u32
                    pixels[idx] = pack_argb(new_r, new_g, new_b, a);
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

        // Calculate button dimensions (matching draw_window_controls)
        let smaller_dim = window_width.min(window_height) as f32;
        let button_width = (smaller_dim / 16.).ceil() as usize;

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
        center_y: usize,
        box_width: usize,
        box_height: usize,
    ) {
        // Convert from center coordinates to top-left
        let x = center_x - box_width / 2;
        let y = center_y - box_height / 2;

        let light_colour = theme::TEXTBOX_LIGHT_EDGE;
        let shadow_colour = theme::TEXTBOX_SHADOW_EDGE;
        let fill_colour = theme::TEXTBOX_FILL;
        let radius = (box_width.min(box_height) / 2) as f32;
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
            textbox_mask[idx] = h;

            // Fill horizontally to the diagonal (where horizontal edge would be)
            let diag_x = (x + radius as usize - i).min(window_width);
            for fill_x in (px + 2)..=diag_x {
                let idx = py * window_width + fill_x;
                pixels[idx] = fill_colour;
                hit_test_map[idx] = hit_id;
                textbox_mask[idx] = 255;
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
            textbox_mask[idx] = h;

            // Fill vertically down from horizontal edge to diagonal
            // Diagonal is where the vertical edge is at this same iteration
            let diag_y = y + radius as usize - i;
            for fill_y in (hy + 2)..diag_y {
                let idx = fill_y * window_width + hx;
                pixels[idx] = fill_colour;
                hit_test_map[idx] = hit_id;
                textbox_mask[idx] = 255;
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
            textbox_mask[idx] = h;

            // Fill horizontally to the diagonal
            let diag_x = x + box_width - 1 - radius as usize + i;
            for fill_x in diag_x..(px - 1) {
                let idx = py * window_width + fill_x;
                pixels[idx] = fill_colour;
                hit_test_map[idx] = hit_id;
                textbox_mask[idx] = 255;
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
            textbox_mask[idx] = h;

            // Fill vertically down from horizontal edge to diagonal
            let diag_y = y + radius as usize - i;
            for fill_y in (hy + 2)..diag_y {
                let idx = fill_y * window_width + hx;
                pixels[idx] = fill_colour;
                hit_test_map[idx] = hit_id;
                textbox_mask[idx] = 255;
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
            textbox_mask[idx] = h;

            // Fill horizontally to the diagonal
            let diag_x = (x + radius as usize - i).min(window_width);
            for fill_x in (px + 2)..=diag_x {
                let idx = py * window_width + fill_x;
                pixels[idx] = fill_colour;
                hit_test_map[idx] = hit_id;
                textbox_mask[idx] = 255;
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
            textbox_mask[idx] = h;

            // Fill vertically up from horizontal edge to diagonal
            let diag_y = y + box_height - radius as usize + i;
            for fill_y in (diag_y + 1)..(hy - 1) {
                let idx = fill_y * window_width + hx;
                pixels[idx] = fill_colour;
                hit_test_map[idx] = hit_id;
                textbox_mask[idx] = 255;
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
            textbox_mask[idx] = h;

            // Fill horizontally to the diagonal
            let diag_x = x + box_width - 1 - radius as usize + i;
            for fill_x in diag_x..(px - 1) {
                let idx = py * window_width + fill_x;
                pixels[idx] = fill_colour;
                hit_test_map[idx] = hit_id;
                textbox_mask[idx] = 255;
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
            textbox_mask[idx] = h;

            // Fill vertically up from horizontal edge to diagonal
            let diag_y = y + box_height - radius as usize + i;
            for fill_y in (diag_y + 1)..(hy - 1) {
                let idx = fill_y * window_width + hx;
                pixels[idx] = fill_colour;
                hit_test_map[idx] = hit_id;
                textbox_mask[idx] = 255;
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
                    textbox_mask[idx] = 255;
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
                    textbox_mask[idx] = 255;
                }
            }
        }
    }

    pub fn draw_button(
        pixels: &mut [u32],
        hit_test_map: &mut [u8],

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

        let radius = (box_width.min(box_height) / 2) as f32;
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

            // Fill horizontally to the diagonal (where horizontal edge would be)
            let diag_x = (x + radius as usize - i).min(window_width);
            for fill_x in (px + 2)..=diag_x {
                let idx = py * window_width + fill_x;
                pixels[idx] = fill_colour;
                hit_test_map[idx] = hit_id;
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

            // Fill vertically down from horizontal edge to diagonal
            // Diagonal is where the vertical edge is at this same iteration
            let diag_y = y + radius as usize - i;
            for fill_y in (hy + 2)..diag_y {
                let idx = fill_y * window_width + hx;
                pixels[idx] = fill_colour;
                hit_test_map[idx] = hit_id;
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

            // Fill horizontally to the diagonal
            let diag_x = x + box_width - 1 - radius as usize + i;
            for fill_x in diag_x..(px - 1) {
                let idx = py * window_width + fill_x;
                pixels[idx] = fill_colour;
                hit_test_map[idx] = hit_id;
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

            // Fill vertically down from horizontal edge to diagonal
            let diag_y = y + radius as usize - i;
            for fill_y in (hy + 2)..diag_y {
                let idx = fill_y * window_width + hx;
                pixels[idx] = fill_colour;
                hit_test_map[idx] = hit_id;
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

            // Fill horizontally to the diagonal
            let diag_x = (x + radius as usize - i).min(window_width);
            for fill_x in (px + 2)..=diag_x {
                let idx = py * window_width + fill_x;
                pixels[idx] = fill_colour;
                hit_test_map[idx] = hit_id;
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

            // Fill vertically up from horizontal edge to diagonal
            let diag_y = y + box_height - radius as usize + i;
            for fill_y in (diag_y + 1)..(hy - 1) {
                let idx = fill_y * window_width + hx;
                pixels[idx] = fill_colour;
                hit_test_map[idx] = hit_id;
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

            // Fill horizontally to the diagonal
            let diag_x = x + box_width - 1 - radius as usize + i;
            for fill_x in diag_x..(px - 1) {
                let idx = py * window_width + fill_x;
                pixels[idx] = fill_colour;
                hit_test_map[idx] = hit_id;
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

            // Fill vertically up from horizontal edge to diagonal
            let diag_y = y + box_height - radius as usize + i;
            for fill_y in (diag_y + 1)..(hy - 1) {
                let idx = fill_y * window_width + hx;
                pixels[idx] = fill_colour;
                hit_test_map[idx] = hit_id;
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
                }
            }
        }
    }

    pub fn draw_spectrum(
        pixels: &mut [u32],
        window_width: u32,
        window_height: u32,
        vertical_center_px: usize, // Vertical center position in pixels
    ) {
        let window_width = window_width as usize;
        let _window_height = window_height as usize;
        let smaller_dim = window_width.min(window_height as usize) as f32;

        // Size the spectrum relative to window dimensions
        let logo_width = (smaller_dim / 1.5) as usize;
        let logo_height = (smaller_dim / 5.) as usize;

        // Position horizontally centered, vertically at specified position
        let x_start: usize = (window_width - logo_width) / 2;
        let y_offset = vertical_center_px.saturating_sub(logo_height);

        // Draw horizontal spectrum rainbow
        for y in 0..logo_height * 2 {
            for x in 0..logo_width {
                // Flip x for wave calculations to match flipped spectrum
                let x_flipped = logo_width - 1 - x;
                let x_norm = x_flipped as f32 / logo_width as f32;
                let amplitude = logo_height as f32 / (1. + 12. * x_norm);

                let wave_phase = (logo_width as f32 / (x_flipped + logo_width / 2) as f32) * 55.;
                let wave_offset = wave_phase.sin() * amplitude;

                let mut scale = (y as f32 + wave_offset - logo_height as f32) / logo_height as f32;
                scale = ((logo_height * 2 - y) as f32 / logo_height as f32)
                    * (y as f32 / logo_height as f32)
                    * 32000.
                    / (scale.abs() + amplitude / smaller_dim * 0.25);
                let px = x_start + x;

                // Map x position to wavelength index (0-480), flipped left-right
                let wavelength_idx = ((logo_width - 1 - x) * 480) / logo_width;
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

                // Write pixel (with y_offset for vertical positioning)
                let idx = (y + y_offset) * window_width + px - logo_width / 16;
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
        window_width: u32,
        window_height: u32,
        vertical_center_px: usize, // Vertical center position in pixels
    ) {
        let window_width = window_width as usize;
        let window_height = window_height as usize;
        let smaller_dim = window_width.min(window_height) as f32;

        // Calculate text position
        let text_x = window_width as f32 / 2.;
        let text_y = vertical_center_px as f32;
        let text_size = smaller_dim / 8. * 1.18;

        // Virtual buffer region (only process where text lives with glow padding)
        let text_height_estimate = (text_size * 1.5) as usize; // Text + glow padding
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
}

// Helper functions for u32 packed pixel manipulation (ARGB format: 0xAARRGGBB)
#[inline]
fn pack_argb(r: u8, g: u8, b: u8, a: u8) -> u32 {
    ((a as u32) << 24) | ((r as u32) << 16) | ((g as u32) << 8) | (b as u32)
}

#[inline]
fn unpack_argb(pixel: u32) -> (u8, u8, u8, u8) {
    let a = (pixel >> 24) as u8;
    let r = (pixel >> 16) as u8;
    let g = (pixel >> 8) as u8;
    let b = pixel as u8;
    (r, g, b, a)
}

/// Blend two packed ARGB colours with alpha transparency.
/// Used for outer edge pixels with variable alpha.
/// Formula: (bg_colour * (255 - alpha) + fg_colour * alpha) >> 8
#[inline]
fn blend_alpha(fg_colour: u32, alpha: u8) -> u32 {
    let mut fg = fg_colour as u64;
    fg = (fg | (fg << 16)) & 0x0000FFFF0000FFFF;
    fg = (fg | (fg << 8)) & 0x00FF00FF00FF00FF;

    let mut blended = fg * alpha as u64;
    blended = (blended >> 8) & 0x00FF00FF00FF00FF;
    blended = (blended | (blended >> 8)) & 0x0000FFFF0000FFFF;
    blended = blended | (blended >> 16);

    blended as u32
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
