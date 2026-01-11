use crate::debug_println;
#[cfg(target_os = "android")]
use crate::ui::renderer_android::Renderer;
#[cfg(any(target_os = "linux", target_os = "redox"))]
use crate::ui::renderer_linux::Renderer;
#[cfg(target_os = "macos")]
use crate::ui::renderer_macos::Renderer;
#[cfg(target_os = "windows")]
use crate::ui::renderer_windows::Renderer;
use crate::ui::{app::*, text_rasterizing::TextRenderer, theme};
use rand::Rng;
impl PhotonApp {
    /// Check if textbox is focused (for event loop control flow)
    pub fn textbox_is_focused(&self) -> bool {
        self.current_text_state.textbox_focused
    }

    /// Returns the font size for text rendering (from text_layout)
    pub fn font_size(&self) -> f32 {
        self.text_layout.font_size
    }

    /// Returns the textbox width (from text_layout)
    pub fn textbox_width(&self) -> usize {
        self.text_layout.box_width
    }

    /// Returns the textbox height (from text_layout)
    pub fn textbox_height(&self) -> usize {
        self.text_layout.box_height
    }

    /// Returns the textbox center Y position (from text_layout)
    pub fn textbox_center_y(&self) -> usize {
        self.text_layout.center_y
    }

    /// Returns the left x edge of the textbox (from text_layout)
    pub fn textbox_left(&self) -> usize {
        self.text_layout.center_x - self.text_layout.box_width / 2
    }

    /// Returns the right x edge of the textbox (from text_layout)
    pub fn textbox_right(&self) -> usize {
        self.text_layout.center_x + self.text_layout.box_width / 2
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
    /// y_offset: additional Y offset to apply (for scroll)
    pub fn render_text_clipped(
        pixels: &mut [u32],
        text: &TextState,
        add_mode: bool,
        text_renderer: &mut TextRenderer,
        textbox_mask: &[u8],
        window_width: usize,
        layout: &TextLayout,
        colour: u32,
        y_offset: isize,
    ) {
        if text.chars.is_empty() {
            return;
        }

        // Apply Y offset for scroll - skip only if textbox is completely off-screen
        // WHY: Text can be partially visible when center is near edge
        // PROOF: Glyph rendering clips per-pixel, so partial visibility is safe
        // PREVENTS: Early return when text is still partially visible
        let render_y_isize = layout.center_y as isize + y_offset;
        let height = pixels.len() / window_width;
        let half_h = (layout.box_height / 2) as isize;
        // Skip only when textbox is COMPLETELY off-screen (center ± half_height)
        if render_y_isize + half_h < 0 || render_y_isize - half_h >= height as isize {
            return;
        }
        let render_y = render_y_isize as f32;

        let text_start_x = layout.text_start_x(text);
        // Use full textbox bounds for rendering - text can flow under button area
        let textbox_left = (layout.center_x - layout.box_width / 2) as f32;
        let textbox_right = (layout.center_x + layout.box_width / 2) as f32;

        let mut x_offset = text_start_x;

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
                    render_y,
                    layout.font_size,
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

    /// Update scroll offset to keep blinkey visible within usable text area.
    /// Uses symmetric margins since text is now centered in usable area.
    pub fn update_text_scroll(&mut self) -> bool {
        if self.current_text_state.chars.is_empty() {
            self.current_text_state.scroll_offset = 0.0;
            return false;
        }

        let layout = &self.text_layout;
        let total_text_width: usize = self.current_text_state.width;

        if total_text_width <= layout.usable_width {
            self.current_text_state.scroll_offset = 0.0;
            return false;
        }

        let blinkey_pixel_offset: usize = self.current_text_state.widths
            [..self.current_text_state.blinkey_index]
            .iter()
            .sum();

        // Symmetric margins - text is centered in usable area
        let margin = layout.margin as f32;
        let usable_half = (layout.usable_width / 2) as f32;
        let text_half = (total_text_width / 2) as f32;

        // First: clamp scroll so text doesn't leave empty space
        // Max scroll right (negative): right edge of text at right margin
        let max_scroll_right = usable_half - margin - text_half;
        // Max scroll left (positive): left edge of text at left margin
        let max_scroll_left = text_half - usable_half + margin;

        let old_offset = self.current_text_state.scroll_offset;
        self.current_text_state.scroll_offset = old_offset.clamp(max_scroll_right, max_scroll_left);

        // Second: ensure blinkey is visible within margins
        let blinkey_pos_in_centered_text = blinkey_pixel_offset as f32 - text_half;
        let blinkey_pos_in_view =
            blinkey_pos_in_centered_text + self.current_text_state.scroll_offset;

        if blinkey_pos_in_view < -usable_half + margin {
            self.current_text_state.scroll_offset =
                -usable_half + margin - blinkey_pos_in_centered_text;
        } else if blinkey_pos_in_view > usable_half - margin {
            self.current_text_state.scroll_offset =
                usable_half - margin - blinkey_pos_in_centered_text;
        }

        self.current_text_state.scroll_offset != old_offset
    }

    /// Update scroll offset during selection drag (called every frame)
    /// Returns true if scroll was modified and a redraw is needed
    pub fn update_selection_scroll(&mut self) -> bool {
        if !self.current_text_state.textbox_focused || self.current_text_state.chars.is_empty() {
            return false;
        }

        let total_text_width = self.current_text_state.width;

        // If text fits in textbox, no need to scroll during selection
        if total_text_width <= self.text_layout.usable_width {
            self.current_text_state.scroll_offset = 0.0;
            return false;
        }

        let now = std::time::Instant::now();
        let box_left = self.text_layout.usable_left;
        let box_right = self.text_layout.usable_right;
        let mouse_x = self.mouse_x as f32;

        // Calculate time delta
        let time_delta = if let Some(last_time) = self.selection_last_update_time {
            now.duration_since(last_time).as_secs_f32()
        } else {
            0.0
        };
        self.selection_last_update_time = Some(now);

        // Calculate signed distance outside textbox (negative = left, positive = right)
        let distance_outside = if mouse_x < box_left as f32 {
            box_left as f32 - mouse_x // Positive when outside left
        } else if mouse_x > box_right as f32 {
            mouse_x - box_right as f32 // Positive when outside right
        } else {
            0.0
        };

        // If outside, apply time-based scroll with bounds checking
        if distance_outside > 0. {
            debug_println!(
                "SCROLL: mouse outside by {:.1}px, box_left={}, box_right={}, mouse_x={:.1}",
                distance_outside,
                box_left,
                box_right,
                mouse_x
            );
            let base_speed = 1000.; // scroll offset units per second
            let usable_width = self.text_layout.usable_width as f32;
            let speed_ratio = distance_outside / usable_width;
            let scroll_speed = base_speed * speed_ratio;
            let scroll_delta = scroll_speed * time_delta;
            debug_println!(
                "  speed_ratio={:.2}, scroll_delta={:.2}",
                speed_ratio,
                scroll_delta
            );

            let total_text_width = self.current_text_state.width as f32;
            let textbox_half = usable_width / 2.0;

            // Calculate scroll limits:
            // Stop at 3/4 width from center instead of at the edge (leaves padding for selection)
            let scroll_limit_distance = (textbox_half * 3.0) / 4.0;

            // Max scroll LEFT (positive): first char at 3/4 from left edge
            let max_scroll_left = (total_text_width / 2.0) - scroll_limit_distance;

            // Max scroll RIGHT (negative): last char at 3/4 from right edge
            let max_scroll_right = scroll_limit_distance - (total_text_width / 2.0);

            debug_println!(
                "  current_offset={:.1}, max_left={:.1}, max_right={:.1}, text_width={:.0}",
                self.current_text_state.scroll_offset,
                max_scroll_left,
                max_scroll_right,
                total_text_width
            );

            // Apply scroll with bounds
            let old_offset = self.current_text_state.scroll_offset;
            if mouse_x < box_left as f32 {
                // Scrolling to show BEGINNING (offset increases, text moves right)
                let new_offset = self.current_text_state.scroll_offset + scroll_delta;
                self.current_text_state.scroll_offset = new_offset.min(max_scroll_left);
            } else {
                // Scrolling to show END (offset decreases, text moves left)
                let new_offset = self.current_text_state.scroll_offset - scroll_delta;
                self.current_text_state.scroll_offset = new_offset.max(max_scroll_right);
            }

            // Only mark dirty and request redraw if offset actually changed
            if self.current_text_state.scroll_offset != old_offset {
                debug_println!(
                    "  Scroll offset changed: {} -> {}",
                    old_offset,
                    self.current_text_state.scroll_offset
                );
                self.text_dirty = true;
                return true;
            } else {
                debug_println!("  Scroll offset unchanged (hit limit)");
            }
        }
        false
    }

    /// Get the selection range as (start, end) where start < end, or None if no selection
    pub fn get_selection_range(&self) -> Option<(usize, usize)> {
        self.current_text_state.selection_anchor.and_then(|anchor| {
            if anchor < self.current_text_state.blinkey_index {
                Some((anchor, self.current_text_state.blinkey_index))
            } else if anchor > self.current_text_state.blinkey_index {
                Some((self.current_text_state.blinkey_index, anchor))
            } else {
                // Anchor equals blinkey - no selection
                None
            }
        })
    }

    /// Delete the currently selected text and clear selection
    pub fn delete_selection(&mut self) {
        if let Some((start, end)) = self.get_selection_range() {
            self.current_text_state.delete_range(start..end);
            self.current_text_state.blinkey_index = start;
            self.current_text_state.selection_anchor = None;

            // Reset scroll offset if text is now empty
            if self.current_text_state.chars.is_empty() {
                self.current_text_state.scroll_offset = 0.0;
            }
        }
    }

    /// Get the selected text
    pub fn get_selected_text(&self) -> Option<String> {
        self.get_selection_range()
            .map(|(start, end)| self.current_text_state.chars[start..end].iter().collect())
    }

    /// Paste text at current cursor position
    pub fn paste_text(&mut self, text: &str) {
        use super::app::{AppState, LaunchState};
        use super::theme;

        // Delete selection if it exists
        if self.current_text_state.selection_anchor.is_some() {
            self.delete_selection();
        }

        // Calculate widths for pasted text
        let font_size = self.font_size();
        let widths: Vec<usize> = text
            .chars()
            .map(|ch| {
                self.text_renderer.measure_text_width(
                    &ch.to_string(),
                    font_size,
                    theme::FONT_WEIGHT_USER_CONTENT,
                    theme::FONT_USER_CONTENT,
                ) as usize
            })
            .collect();

        // Insert pasted text at blinkey
        let insert_idx = self.current_text_state.blinkey_index;
        self.current_text_state
            .insert_str(insert_idx, text, &widths);
        self.current_text_state.blinkey_index += widths.len();
        if matches!(self.app_state, AppState::Launch(_)) {
            self.set_launch_state(LaunchState::Fresh);
        }
        if self.search_result.is_some() {
            self.window_dirty = true;
        }
        self.text_dirty = true;
        self.glow_colour = theme::GLOW_DEFAULT;
        self.search_result = None;
        self.controls_dirty = true;
    }

    pub fn handle_blinkey_left(&mut self) {
        self.hovered_button = HoveredButton::None;
        self.render();
    }

    /// Get next blinkey blink wake time (random interval 0..=125ms)
    pub fn next_blink_wake_time(&self) -> std::time::Instant {
        let interval_ms = rand::thread_rng().gen_range(0..=300);
        std::time::Instant::now() + std::time::Duration::from_millis(interval_ms)
    }

    pub fn start_blinkey(
        pixels: &mut [u32],
        width: usize,
        blinkey_pixel_x: usize,
        blinkey_pixel_y: usize,
        blinkey_visible: &mut bool,
        blinkey_wave_top_bright: &mut bool,
        font_size: usize,
    ) {
        if *blinkey_visible {
            panic!("Cursor already visible when starting blinkey!");
        }
        *blinkey_wave_top_bright = rand::thread_rng().gen();
        if *blinkey_wave_top_bright {
            Self::add_blinkey_top(pixels, width, blinkey_pixel_x, blinkey_pixel_y, font_size);
        } else {
            Self::add_blinkey_bottom(pixels, width, blinkey_pixel_x, blinkey_pixel_y, font_size);
        }
        *blinkey_visible = true;
    }

    pub fn stop_blinkey(
        pixels: &mut [u32],
        width: usize,
        blinkey_pixel_x: usize,
        blinkey_pixel_y: usize,
        blinkey_visible: &mut bool,
        blinkey_wave_top_bright: &mut bool,
        font_size: usize,
    ) {
        if !*blinkey_visible {
            panic!("Cursor not visible when stopping blinkey!");
        }
        if *blinkey_wave_top_bright {
            Self::subtract_blinkey_top(pixels, width, blinkey_pixel_x, blinkey_pixel_y, font_size);
        } else {
            Self::subtract_blinkey_bottom(
                pixels,
                width,
                blinkey_pixel_x,
                blinkey_pixel_y,
                font_size,
            );
        }
        *blinkey_visible = false;
    }

    pub fn undraw_blinkey(
        pixels: &mut [u32],
        width: usize,
        blinkey_pixel_x: usize,
        blinkey_pixel_y: usize,
        blinkey_visible: &mut bool,
        blinkey_wave_top_bright: &mut bool,
        font_size: usize,
    ) {
        if !*blinkey_visible {
            panic!("Cursor not visible when redrawing blinkey!");
        }
        if *blinkey_wave_top_bright {
            Self::subtract_blinkey_top(pixels, width, blinkey_pixel_x, blinkey_pixel_y, font_size);
        } else {
            Self::subtract_blinkey_bottom(
                pixels,
                width,
                blinkey_pixel_x,
                blinkey_pixel_y,
                font_size,
            );
        }
    }

    pub fn draw_blinkey(
        pixels: &mut [u32],
        width: usize,
        blinkey_pixel_x: usize,
        blinkey_pixel_y: usize,
        blinkey_visible: &mut bool,
        blinkey_wave_top_bright: &mut bool,
        font_size: usize,
    ) {
        if !*blinkey_visible {
            panic!("Cursor not visible when redrawing blinkey!");
        }
        if *blinkey_wave_top_bright {
            Self::add_blinkey_top(pixels, width, blinkey_pixel_x, blinkey_pixel_y, font_size);
        } else {
            Self::add_blinkey_bottom(pixels, width, blinkey_pixel_x, blinkey_pixel_y, font_size);
        }
    }

    pub fn flip_blinkey(
        renderer: &mut Renderer,
        width: usize,
        blinkey_pixel_x: usize,
        blinkey_pixel_y: usize,
        blinkey_visible: &mut bool,
        blinkey_wave_top_bright: &mut bool,
        font_size: usize,
        is_mouse_selecting: bool,
    ) {
        if *blinkey_visible && !is_mouse_selecting {
            let font_size = font_size as usize;
            let mut buffer = renderer.lock_buffer();
            let pixels = buffer.as_mut();
            if *blinkey_wave_top_bright {
                Self::subtract_blinkey_top(
                    pixels,
                    width as usize,
                    blinkey_pixel_x,
                    blinkey_pixel_y,
                    font_size,
                );
                Self::add_blinkey_bottom(
                    pixels,
                    width as usize,
                    blinkey_pixel_x,
                    blinkey_pixel_y,
                    font_size,
                );
                *blinkey_wave_top_bright = false;
            } else {
                Self::add_blinkey_top(
                    pixels,
                    width as usize,
                    blinkey_pixel_x,
                    blinkey_pixel_y,
                    font_size,
                );
                Self::subtract_blinkey_bottom(
                    pixels,
                    width as usize,
                    blinkey_pixel_x,
                    blinkey_pixel_y,
                    font_size,
                );
                *blinkey_wave_top_bright = true;
            }
            buffer.present().unwrap();
        }
    }

    pub fn add_blinkey_top(
        pixels: &mut [u32],
        window_width: usize,
        blinkey_x: usize,
        blinkey_top: usize,
        blinkey_height: usize,
    ) {
        let y_end = blinkey_top + blinkey_height;
        let half_height = blinkey_height / 2;

        for y in blinkey_top..y_end {
            let idx = y * window_width + blinkey_x;

            // Map to [-1, 1] range for full blinkey
            let t = (y - blinkey_top - half_height) as isize as f32 / half_height as f32;

            let wave = (1. - t * t) * (1. - t) * (1. - t) * theme::CURSOR_BRIGHTNESS;

            for x in -7..=7isize {
                pixels[idx + x as usize] += 0x00010101 * (wave as u32 >> x.abs());
            }
        }
    }

    pub fn add_blinkey_bottom(
        pixels: &mut [u32],
        window_width: usize,
        blinkey_x: usize,
        blinkey_top: usize,
        blinkey_height: usize,
    ) {
        let y_end = blinkey_top + blinkey_height;
        let half_height = blinkey_height / 2;

        for y in blinkey_top..y_end {
            let idx = y * window_width + blinkey_x;

            // Map to [-1, 1] range for full blinkey
            let t = (y - blinkey_top - half_height) as isize as f32 / half_height as f32;

            let wave = (1. - t * t) * (1. + t) * (1. + t) * theme::CURSOR_BRIGHTNESS;

            for x in -7..=7isize {
                pixels[idx + x as usize] += 0x00010101 * (wave as u32 >> x.abs());
            }
        }
    }

    pub fn subtract_blinkey_top(
        pixels: &mut [u32],
        window_width: usize,
        blinkey_x: usize,
        blinkey_top: usize,
        blinkey_height: usize,
    ) {
        let y_end = blinkey_top + blinkey_height;
        let half_height = blinkey_height / 2;

        for y in blinkey_top..y_end {
            let idx = y * window_width + blinkey_x;

            // Map to [-1, 1] range for full blinkey
            let t = (y - blinkey_top - half_height) as isize as f32 / half_height as f32;

            let wave = (1. - t * t) * (1. - t) * (1. - t) * theme::CURSOR_BRIGHTNESS;
            for x in -7..=7isize {
                pixels[idx + x as usize] -= 0x00010101 * (wave as u32 >> x.abs());
            }
        }
    }

    pub fn subtract_blinkey_bottom(
        pixels: &mut [u32],
        window_width: usize,
        blinkey_x: usize,
        blinkey_top: usize,
        blinkey_height: usize,
    ) {
        let y_end = blinkey_top + blinkey_height;
        let half_height = blinkey_height / 2;

        for y in blinkey_top..y_end {
            let idx = y * window_width + blinkey_x;

            // Map to [-1, 1] range for full blinkey
            let t = (y - blinkey_top - half_height) as isize as f32 / half_height as f32;

            let wave = (1. - t * t) * (1. + t) * (1. + t) * theme::CURSOR_BRIGHTNESS;
            for x in -7..=7isize {
                pixels[idx + x as usize] -= 0x00010101 * (wave as u32 >> x.abs());
            }
        }
    }

    // Invert RGB for selection highlight (reversible: 255 - (255 - x) = x)
    pub fn invert_selection(
        pixels: &mut [u32],
        text: &TextState,
        scroll_offset: f32,
        window_width: usize,
        window_height: usize,
        sel_start: usize,
        sel_end: usize,
        layout: &TextLayout,
        hit_test_map: &[u8],
    ) {
        if sel_start >= sel_end || sel_start >= text.widths.len() {
            return;
        }

        let sel_start_px: usize = text.widths[..sel_start].iter().sum();
        let sel_end_px: usize = text.widths[..sel_end.min(text.widths.len())].iter().sum();

        let text_half = (text.width / 2) as f32;
        let text_start_x = layout.usable_center as f32 - text_half + scroll_offset;
        let sel_x_start = (text_start_x + sel_start_px as f32) as isize;
        let sel_x_end = (text_start_x + sel_end_px as f32) as isize;

        let sel_y_top = (layout.center_y as f32 - layout.font_size / 2.0) as isize;
        let sel_y_bottom = (layout.center_y as f32 + layout.font_size / 2.0) as isize;

        let textbox_left = layout.usable_left as isize;
        let textbox_right = layout.usable_right as isize;

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
