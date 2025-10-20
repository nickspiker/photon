use super::renderer::Renderer;
use super::text::TextRenderer;
use super::theme;
use winit::{
    dpi::PhysicalSize,
    event::{ElementState, KeyEvent, MouseButton},
    keyboard::{Key, ModifiersState, NamedKey},
    window::{CursorIcon, Window},
};

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

pub struct PhotonApp {
    renderer: Renderer,
    #[allow(dead_code)] // Will be used for drawing text/logos soon
    text_renderer: TextRenderer,
    width: u32,
    height: u32,
    needs_redraw: bool, // True when dimensions change or content updates

    // Universal scaling units (cached for performance)
    min_dim: usize,     // min(width, height) - universal scaling unit
    perimeter: usize,   // width + height
    diagonal_sq: usize, // width² + height² (diagonal squared, for distance calculations)

    // Launch screen state
    username_input: String,
    character_widths: Vec<usize>, // Width in pixels of each rendered character
    cursor_char_index: usize,     // Cursor position in characters (0 = before first char)
    text_scroll_char_index: usize, // First visible character index (for horizontal scrolling)
    cursor_blink_rate_ms: u64,    // System cursor blink rate in milliseconds (max for random range)
    cursor_wave_top_bright: bool, // True=top is bright, False=top is dark
    cursor_needs_init: bool,      // True when textbox just got focus, apply initial wave
    cursor_needs_undo: bool,      // True when textbox just lost focus, undo wave by inverting ops
    cursor_pixel_x: usize,        // Cursor x position in pixels
    cursor_pixel_y: usize,        // Cursor y position in pixels
    cursor_height: f32,           // Cursor height in pixels
    username_available: Option<bool>, // None = checking, Some(true) = available, Some(false) = taken
    textbox_focused: bool,            // True when textbox is clicked and accepting input
    textbox_mask: Vec<u8>, // Single-channel alpha mask for textbox (0=outside, 255=inside, faded at edges)
    textbox_bounds: (usize, usize, usize, usize), // (x, y, width, height) of textbox
    show_textbox_mask: bool, // Debug: show textbox mask visualization (Ctrl+T)
    redraw_counter: usize, // Count of full redraws for debugging
    render_counter: usize, // Count of render() calls

    // Input state
    mouse_x: f32,
    mouse_y: f32,
    is_dragging_resize: bool,
    is_dragging_move: bool,
    resize_edge: ResizeEdge,
    drag_start_cursor_screen_pos: (f64, f64), // Global screen position when drag starts
    drag_start_size: (u32, u32),
    drag_start_window_pos: (i32, i32),
    modifiers: ModifiersState,
    is_fullscreen: bool, // True when window is fullscreen

    // Window control buttons
    close_button_bounds: (f32, f32, f32, f32), // (x, y, width, height)
    maximize_button_bounds: (f32, f32, f32, f32),
    minimize_button_bounds: (f32, f32, f32, f32),
    hovered_button: HoveredButton,
    prev_hovered_button: HoveredButton, // Previous hover state to detect changes

    // Button rendering data (cached from last render)
    button_x_start: usize,
    button_height: usize,
    button_curve_start: usize,
    button_crossings: Vec<(u16, u8, u8)>,

    // Cached button pixel coordinates for fast hover effects
    minimize_pixels: Vec<usize>,
    maximize_pixels: Vec<usize>,
    close_pixels: Vec<usize>,

    // Hit test bitmap (one byte per pixel, element ID)
    hit_test_map: Vec<u8>,
    debug_hit_test: bool,
    debug_hit_colours: Vec<(u8, u8, u8)>, // Random colours for each hit area ID
}

// Hit test element IDs
const HIT_NONE: u8 = 0;
const HIT_MINIMIZE_BUTTON: u8 = 1;
const HIT_MAXIMIZE_BUTTON: u8 = 2;
const HIT_CLOSE_BUTTON: u8 = 3;
const HIT_HANDLE_TEXTBOX: u8 = 4;

// Button hover colour deltas are now in theme module

#[derive(Debug, Clone, Copy, PartialEq)]
enum HoveredButton {
    None,
    Close,
    Maximize,
    Minimize,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum ResizeEdge {
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
    #[cfg(target_os = "linux")]
    pub async fn new(window: &Window, cursor_blink_rate_ms: u64) -> Self {
        let size = window.inner_size();
        let renderer = Renderer::new(window, size.width, size.height).await;
        let text_renderer = TextRenderer::new();

        // Check initial fullscreen/maximized state
        let is_fullscreen = window.fullscreen().is_some() || window.is_maximized();

        let w = size.width as usize;
        let h = size.height as usize;
        let app = Self {
            renderer,
            text_renderer,
            width: size.width,
            height: size.height,
            needs_redraw: true,
            min_dim: w.min(h),
            perimeter: w + h,
            diagonal_sq: w * w + h * h,
            username_input: String::new(),
            character_widths: Vec::new(),
            cursor_char_index: 0,
            text_scroll_char_index: 0,
            cursor_blink_rate_ms,
            cursor_wave_top_bright: false,
            cursor_needs_init: false,
            cursor_needs_undo: false,
            cursor_pixel_x: 0,
            cursor_pixel_y: 0,
            cursor_height: 0.0,
            username_available: None,
            textbox_focused: false,
            textbox_mask: Vec::new(),
            textbox_bounds: (0, 0, 0, 0),
            show_textbox_mask: false,
            redraw_counter: 0,
            render_counter: 0,
            mouse_x: 0.0,
            mouse_y: 0.0,
            is_dragging_resize: false,
            is_dragging_move: false,
            resize_edge: ResizeEdge::None,
            drag_start_cursor_screen_pos: (0.0, 0.0),
            drag_start_size: (0, 0),
            drag_start_window_pos: (0, 0),
            modifiers: ModifiersState::empty(),
            close_button_bounds: (0.0, 0.0, 0.0, 0.0),
            maximize_button_bounds: (0.0, 0.0, 0.0, 0.0),
            minimize_button_bounds: (0.0, 0.0, 0.0, 0.0),
            hovered_button: HoveredButton::None,
            prev_hovered_button: HoveredButton::None,
            button_x_start: 0,
            button_height: 0,
            button_curve_start: 0,
            button_crossings: Vec::new(),
            minimize_pixels: Vec::new(),
            maximize_pixels: Vec::new(),
            close_pixels: Vec::new(),
            hit_test_map: vec![0; (size.width * size.height) as usize],
            debug_hit_test: false,
            debug_hit_colours: Vec::new(),
            is_fullscreen,
        };
        app
    }

    #[cfg(target_os = "windows")]
    pub fn new(
        window: &Window,
        screen_width: u32,
        screen_height: u32,
        cursor_blink_rate_ms: u64,
    ) -> Self {
        let size = window.inner_size();
        let renderer = Renderer::new(window, size.width, size.height);
        let text_renderer = TextRenderer::new();

        // Check initial fullscreen/maximized state
        let is_fullscreen = window.fullscreen().is_some() || window.is_maximized();

        let w = size.width as usize;
        let h = size.height as usize;
        let mut app = Self {
            renderer,
            text_renderer,
            window_width: size.width,
            window_height: size.height,
            screen_width,
            screen_height,
            needs_redraw: true, // Initial draw needed
            min_dim: w.min(h),
            perimeter: w + h,
            diagonal_sq: w * w + h * h,
            username_input: String::new(),
            character_widths: Vec::new(),
            cursor_char_index: 0,
            text_scroll_char_index: 0,
            cursor_blink_rate_ms,
            cursor_wave_top_bright: false,
            cursor_needs_init: false,
            cursor_needs_undo: false,
            cursor_pixel_x: 0,
            cursor_pixel_y: 0,
            cursor_height: 0.0,
            username_available: None,
            textbox_focused: false,
            textbox_mask: Vec::new(),
            textbox_bounds: (0, 0, 0, 0),
            show_textbox_mask: false,
            redraw_counter: 0,
            render_counter: 0,
            mouse_x: 0.0,
            mouse_y: 0.0,
            is_dragging_resize: false,
            is_dragging_move: false,
            resize_edge: ResizeEdge::None,
            drag_start_cursor_screen_pos: (0.0, 0.0),
            drag_start_size: (0, 0),
            drag_start_window_pos: (0, 0),
            modifiers: ModifiersState::empty(),
            close_button_bounds: (0.0, 0.0, 0.0, 0.0),
            maximize_button_bounds: (0.0, 0.0, 0.0, 0.0),
            minimize_button_bounds: (0.0, 0.0, 0.0, 0.0),
            hovered_button: HoveredButton::None,
            prev_hovered_button: HoveredButton::None,
            button_x_start: 0,
            button_height: 0,
            button_curve_start: 0,
            button_crossings: Vec::new(),
            minimize_pixels: Vec::new(),
            maximize_pixels: Vec::new(),
            close_pixels: Vec::new(),
            hit_test_map: vec![0; (size.width * size.height) as usize],
            debug_hit_test: false,
            debug_hit_colours: Vec::new(),
            is_fullscreen,
        };
        app.update_button_bounds();
        app
    }

    /// Update the fullscreen/maximized state
    /// When true, window edges are not drawn
    pub fn set_fullscreen(&mut self, is_fullscreen: bool) {
        if self.is_fullscreen != is_fullscreen {
            self.is_fullscreen = is_fullscreen;
        }
    }

    /// Check if textbox is focused (for event loop control flow)
    pub fn textbox_is_focused(&self) -> bool {
        self.textbox_focused
    }

    /// Get next cursor blink wake time (random interval 0..=125ms)
    pub fn next_blink_wake_time(&self) -> std::time::Instant {
        use rand::Rng;
        let interval_ms = rand::thread_rng().gen_range(0..=125);
        std::time::Instant::now() + std::time::Duration::from_millis(interval_ms)
    }

    pub fn resize(&mut self, size: PhysicalSize<u32>) {
        let w = size.width as usize;
        let h = size.height as usize;

        self.width = size.width;
        self.height = size.height;

        // Update cached scaling units
        self.min_dim = w.min(h);
        self.perimeter = w + h;
        self.diagonal_sq = w * w + h * h;

        self.renderer.resize(size.width, size.height);
        self.hit_test_map
            .resize((size.width * size.height) as usize, 0);

        // Clear hover state on resize since button positions/sizes change
        self.hovered_button = HoveredButton::None;

        self.needs_redraw = true; // Dimensions changed, need to redraw
    }

    pub fn update_modifiers(&mut self, modifiers: ModifiersState) {
        self.modifiers = modifiers;
    }

    pub fn handle_keyboard(&mut self, event: KeyEvent) {
        if event.state == ElementState::Pressed {
            // Toggle debug visualizations with Ctrl shortcuts
            if self.modifiers.control_key() {
                if let Key::Character(ref c) = event.logical_key {
                    // Ctrl+H: Toggle hit test map
                    if c.eq_ignore_ascii_case("h") {
                        self.debug_hit_test = !self.debug_hit_test;
                        self.show_textbox_mask = false; // Clear other debug state
                        self.needs_redraw = true; // Force redraw to show visualization
                                                  // Generate new random colours for each hit area
                        use std::collections::hash_map::RandomState;
                        use std::hash::{BuildHasher, Hash, Hasher};
                        let random_state = RandomState::new();
                        self.debug_hit_colours.clear();
                        for id in 0..=255u8 {
                            let mut hasher = random_state.build_hasher();
                            id.hash(&mut hasher);
                            std::time::SystemTime::now().hash(&mut hasher);
                            let hash = hasher.finish();
                            let r = ((hash >> 0) & 0xFF) as u8;
                            let g = ((hash >> 8) & 0xFF) as u8;
                            let b = ((hash >> 16) & 0xFF) as u8;
                            self.debug_hit_colours.push((r, g, b));
                        }
                        return; // Don't process as regular input
                    }

                    // Ctrl+T: Toggle textbox mask visualization
                    if c.eq_ignore_ascii_case("t") {
                        self.show_textbox_mask = !self.show_textbox_mask;
                        self.debug_hit_test = false; // Clear other debug state
                        self.needs_redraw = true; // Force redraw to show visualization
                        return; // Don't process as regular input
                    }
                }
                // If Ctrl is held, don't process any other input
                return;
            }

            // All text editing requires textbox focus
            if !self.textbox_focused {
                return;
            }

            // Track whether text content changes (not just cursor position)
            let mut text_changed = false;
            let mut cursor_moved = false;
            let old_text = self.username_input.clone();
            let old_cursor_index = self.cursor_char_index;

            match event.logical_key {
                Key::Named(NamedKey::ArrowLeft) => {
                    if self.cursor_char_index > 0 {
                        self.cursor_char_index -= 1;
                        cursor_moved = true;
                    }
                }
                Key::Named(NamedKey::ArrowRight) => {
                    if self.cursor_char_index < self.username_input.len() {
                        self.cursor_char_index += 1;
                        cursor_moved = true;
                    }
                }
                Key::Named(NamedKey::Home) => {
                    if self.cursor_char_index != 0 {
                        self.cursor_char_index = 0;
                        cursor_moved = true;
                    }
                }
                Key::Named(NamedKey::End) => {
                    let end_pos = self.username_input.len();
                    if self.cursor_char_index != end_pos {
                        self.cursor_char_index = end_pos;
                        cursor_moved = true;
                    }
                }
                Key::Named(NamedKey::Backspace) => {
                    if self.cursor_char_index > 0 {
                        self.username_input.remove(self.cursor_char_index - 1);
                        self.cursor_char_index -= 1;
                        text_changed = true;
                        self.username_available = None;
                    }
                }
                Key::Named(NamedKey::Delete) => {
                    if self.cursor_char_index < self.username_input.len() {
                        self.username_input.remove(self.cursor_char_index);
                        text_changed = true;
                        self.username_available = None;
                    }
                }
                Key::Named(NamedKey::Enter) => {
                    if !self.username_input.is_empty() {
                        self.submit_username();
                    }
                }
                _ => {}
            }

            // Handle text input using event.text field (includes space and all printable chars)
            if let Some(text) = &event.text {
                for ch in text.chars() {
                    self.username_input.insert(self.cursor_char_index, ch);
                    self.cursor_char_index += 1;
                }
                text_changed = true;
                self.username_available = None;
            }

            // If text changed, do differential rendering
            if text_changed {
                eprintln!("TEXT CHANGED: '{}' -> '{}'", old_text, self.username_input);

                // Calculate OLD cursor position before changing anything
                let margin = self.min_dim / 8;
                let box_width = self.width as usize - margin * 2;
                let box_height = self.min_dim / 8;
                let center_x = self.width as usize / 2;
                let center_y = self.height as usize * 4 / 7;

                // Old cursor position (using old character_widths which still matches old_text)
                let old_cursor_pixel_x =
                    if !old_text.is_empty() && old_cursor_index <= self.character_widths.len() {
                        let old_cursor_pixel_offset: usize =
                            self.character_widths[..old_cursor_index].iter().sum();
                        let old_scroll_pixel_offset: usize = self.character_widths
                            [..self.text_scroll_char_index]
                            .iter()
                            .sum();
                        let textbox_left = center_x - box_width / 2;
                        textbox_left + old_cursor_pixel_offset - old_scroll_pixel_offset
                    } else {
                        center_x
                    };
                let old_cursor_pixel_y = (center_y as f32 - box_height as f32 * 0.25) as usize;
                let old_cursor_height = box_height as f32 * 0.5;

                // Recalculate char widths for new text
                self.recalculate_char_widths();

                // Update scroll if needed
                self.update_text_scroll(box_width);

                // Update cursor pixel position
                self.update_cursor_position();

                // Prepare data needed for rendering (before locking buffer)
                let center_x = self.width as usize / 2;
                let center_y = self.height as usize * 4 / 7;
                let font_size = box_height as f32 * 0.5;
                let text_scroll_index = self.text_scroll_char_index;
                let char_widths = self.character_widths.clone();
                let textbox_mask = self.textbox_mask.clone();
                let new_text = self.username_input.clone();
                let width = self.width as usize;
                let height = self.height as usize;

                // Helper closure to render text (captures prepared data)
                let render_text =
                    |pixels: &mut [u32],
                     text: &str,
                     add_mode: bool,
                     text_renderer: &mut TextRenderer| {
                        if text.is_empty() {
                            return;
                        }

                        let chars: Vec<char> = text.chars().collect();
                        let char_widths: Vec<usize> = chars
                            .iter()
                            .map(|&ch| {
                                text_renderer.measure_text_width(
                                    &ch.to_string(),
                                    font_size,
                                    500,
                                    theme::FONT_USER_CONTENT,
                                ) as usize
                            })
                            .collect();

                        let total_width: usize = char_widths.iter().sum();
                        let text_start_x = if total_width <= box_width {
                            center_x as f32 - (total_width as f32 / 2.0)
                        } else {
                            let scroll_pixel_offset: usize = char_widths
                                [..text_scroll_index.min(char_widths.len())]
                                .iter()
                                .sum();
                            let textbox_left = center_x - box_width / 2;
                            textbox_left as f32 - scroll_pixel_offset as f32
                        };

                        let mut x_offset = text_start_x;

                        for &ch in &chars {
                            let actual_width = text_renderer.render_char_additive_u32(
                                pixels,
                                width,
                                ch,
                                x_offset,
                                center_y as f32,
                                font_size,
                                500,
                                theme::FONT_USER_CONTENT,
                                theme::TEXT_BRIGHTNESS,
                                &textbox_mask,
                                add_mode,
                            );
                            x_offset += actual_width;
                        }
                    };

                // Now lock buffer and do rendering
                let mut buffer = self.renderer.lock_buffer();
                let pixels = buffer.as_mut();

                // Subtract old cursor first (if it exists)
                if self.cursor_wave_top_bright {
                    Self::sub_cursor_top(
                        pixels,
                        self.width as usize,
                        old_cursor_pixel_x as f32,
                        old_cursor_pixel_y as f32,
                        old_cursor_height,
                    );
                } else {
                    Self::sub_cursor_bottom(
                        pixels,
                        self.width as usize,
                        old_cursor_pixel_x as f32,
                        old_cursor_pixel_y as f32,
                        old_cursor_height,
                    );
                }

                // Subtract old text
                render_text(pixels, &old_text, false, &mut self.text_renderer);

                // Add new text
                render_text(pixels, &new_text, true, &mut self.text_renderer);

                // Add new cursor at new position (same blink state)
                if self.cursor_wave_top_bright {
                    Self::add_cursor_top(
                        pixels,
                        self.width as usize,
                        self.cursor_pixel_x as f32,
                        self.cursor_pixel_y as f32,
                        self.cursor_height,
                    );
                } else {
                    Self::add_cursor_bottom(
                        pixels,
                        self.width as usize,
                        self.cursor_pixel_x as f32,
                        self.cursor_pixel_y as f32,
                        self.cursor_height,
                    );
                }

                // Present buffer
                buffer.present().unwrap();
            } else if cursor_moved && !text_changed {
                // Cursor moved but text didn't change
                eprintln!(
                    "CURSOR MOVED: {} -> {}",
                    old_cursor_index, self.cursor_char_index
                );

                // Calculate old cursor position
                let margin = self.min_dim / 8;
                let box_width = self.width as usize - margin * 2;
                let box_height = self.min_dim / 8;
                let center_x = self.width as usize / 2;
                let center_y = self.height as usize * 4 / 7;

                let old_cursor_pixel_offset: usize = if !self.username_input.is_empty()
                    && old_cursor_index <= self.character_widths.len()
                {
                    self.character_widths[..old_cursor_index].iter().sum()
                } else {
                    0
                };
                let scroll_pixel_offset: usize = self.character_widths
                    [..self.text_scroll_char_index]
                    .iter()
                    .sum();
                let textbox_left = center_x - box_width / 2;
                let old_cursor_pixel_x = if !self.username_input.is_empty() {
                    textbox_left + old_cursor_pixel_offset - scroll_pixel_offset
                } else {
                    center_x
                };
                let cursor_pixel_y = (center_y as f32 - box_height as f32 * 0.25) as usize;
                let cursor_height = box_height as f32 * 0.5;

                // Update text scroll and cursor position for new location
                self.update_text_scroll(box_width);
                self.update_cursor_position();

                // Lock buffer and move cursor
                let mut buffer = self.renderer.lock_buffer();
                let pixels = buffer.as_mut();

                // Subtract cursor at old position (maintaining blink state)
                if self.cursor_wave_top_bright {
                    Self::sub_cursor_top(
                        pixels,
                        self.width as usize,
                        old_cursor_pixel_x as f32,
                        cursor_pixel_y as f32,
                        cursor_height,
                    );
                } else {
                    Self::sub_cursor_bottom(
                        pixels,
                        self.width as usize,
                        old_cursor_pixel_x as f32,
                        cursor_pixel_y as f32,
                        cursor_height,
                    );
                }

                // Add cursor at new position (same blink state)
                if self.cursor_wave_top_bright {
                    Self::add_cursor_top(
                        pixels,
                        self.width as usize,
                        self.cursor_pixel_x as f32,
                        self.cursor_pixel_y as f32,
                        self.cursor_height,
                    );
                } else {
                    Self::add_cursor_bottom(
                        pixels,
                        self.width as usize,
                        self.cursor_pixel_x as f32,
                        self.cursor_pixel_y as f32,
                        self.cursor_height,
                    );
                }

                // Present buffer
                buffer.present().unwrap();
            }
        }
    }

    pub fn handle_mouse_click(
        &mut self,
        window: &Window,
        state: ElementState,
        _button: MouseButton,
    ) {
        match state {
            ElementState::Pressed => {
                // Check window control buttons using hitmap
                let mouse_x = self.mouse_x as usize;
                let mouse_y = self.mouse_y as usize;

                if mouse_x < self.width as usize && mouse_y < self.height as usize {
                    let hit_idx = mouse_y * self.width as usize + mouse_x;
                    let element_id = self.hit_test_map[hit_idx];

                    match element_id {
                        HIT_CLOSE_BUTTON => {
                            std::process::exit(0);
                        }
                        HIT_MINIMIZE_BUTTON => {
                            window.set_minimized(true);
                            return;
                        }
                        HIT_MAXIMIZE_BUTTON => {
                            window.set_maximized(!window.is_maximized());
                            return;
                        }
                        HIT_HANDLE_TEXTBOX => {
                            // Focus the textbox
                            self.textbox_focused = true;
                            self.cursor_needs_init = true;
                            self.cursor_char_index = self.username_input.len();
                            self.text_scroll_char_index = 0;
                            if !self.username_input.is_empty() {
                                self.recalculate_char_widths();
                            }
                            return;
                        }
                        _ => {
                            // Clicked outside textbox, unfocus it
                            if self.textbox_focused {
                                self.textbox_focused = false;
                                self.cursor_needs_undo = true;
                                // Don't need full redraw, just present when cursor changes
                            }
                        }
                    }
                }

                let edge = self.get_resize_edge(self.mouse_x, self.mouse_y);
                if edge != ResizeEdge::None {
                    self.is_dragging_resize = true;
                    self.resize_edge = edge;
                    self.drag_start_size = (self.width, self.height);

                    // Store the window position and global cursor position at drag start
                    if let Some(window_pos) = window.outer_position().ok() {
                        self.drag_start_window_pos = (window_pos.x, window_pos.y);

                        // Calculate global cursor position from window-relative position
                        let cursor_screen_x = window_pos.x as f64 + self.mouse_x as f64;
                        let cursor_screen_y = window_pos.y as f64 + self.mouse_y as f64;
                        self.drag_start_cursor_screen_pos = (cursor_screen_x, cursor_screen_y);
                    }
                } else {
                    // Not on a resize edge, start window drag
                    self.is_dragging_move = true;

                    // Store the window position and global cursor position at drag start
                    if let Some(window_pos) = window.outer_position().ok() {
                        self.drag_start_window_pos = (window_pos.x, window_pos.y);

                        // Calculate global cursor position from window-relative position
                        let cursor_screen_x = window_pos.x as f64 + self.mouse_x as f64;
                        let cursor_screen_y = window_pos.y as f64 + self.mouse_y as f64;
                        self.drag_start_cursor_screen_pos = (cursor_screen_x, cursor_screen_y);
                    }
                }
            }
            ElementState::Released => {
                self.is_dragging_resize = false;
                self.is_dragging_move = false;
                self.resize_edge = ResizeEdge::None;
            }
        }
    }

    pub fn handle_cursor_left(&mut self) {
        // Clear hover state when cursor leaves window
        // Hover change will be handled in render()
        self.hovered_button = HoveredButton::None;
    }

    fn recalculate_char_widths(&mut self) {
        self.character_widths.clear();

        let box_height = self.min_dim as f32 / 12.0;
        let font_size = box_height * 0.5;

        for ch in self.username_input.chars() {
            let width = self.text_renderer.measure_text_width(
                &ch.to_string(),
                font_size,
                500,
                theme::FONT_USER_CONTENT,
            );
            self.character_widths.push(width as usize);
        }
    }

    fn update_text_scroll(&mut self, textbox_width: usize) {
        if self.username_input.is_empty() {
            self.text_scroll_char_index = 0;
            return;
        }

        let cursor_pixel_offset: usize =
            self.character_widths[..self.cursor_char_index].iter().sum();
        let scroll_pixel_offset: usize = self.character_widths[..self.text_scroll_char_index]
            .iter()
            .sum();

        let margin = textbox_width / 40;
        let visible_width = textbox_width.saturating_sub(margin * 2);

        let cursor_in_view = cursor_pixel_offset - scroll_pixel_offset;

        if cursor_in_view < margin {
            while self.text_scroll_char_index > 0 {
                let scroll_offset: usize = self.character_widths[..self.text_scroll_char_index - 1]
                    .iter()
                    .sum();
                let new_cursor_in_view = cursor_pixel_offset - scroll_offset;
                if new_cursor_in_view >= margin {
                    break;
                }
                self.text_scroll_char_index -= 1;
            }
        } else if cursor_in_view > visible_width {
            while self.text_scroll_char_index < self.username_input.len() {
                self.text_scroll_char_index += 1;
                let scroll_offset: usize = self.character_widths[..self.text_scroll_char_index]
                    .iter()
                    .sum();
                let new_cursor_in_view = cursor_pixel_offset - scroll_offset;
                if new_cursor_in_view <= visible_width {
                    break;
                }
            }
        }
    }

    fn update_cursor_position(&mut self) {
        let margin = self.min_dim / 8;
        let box_width = self.width as usize - margin * 2;
        let box_height = self.min_dim / 8;
        let center_x = self.width as usize / 2;
        let center_y = self.height as usize * 4 / 7;

        if !self.username_input.is_empty() {
            let cursor_pixel_offset: usize =
                self.character_widths[..self.cursor_char_index].iter().sum();
            let scroll_pixel_offset: usize = self.character_widths[..self.text_scroll_char_index]
                .iter()
                .sum();
            let textbox_left = center_x - box_width / 2;
            self.cursor_pixel_x = textbox_left + cursor_pixel_offset - scroll_pixel_offset;
        } else {
            self.cursor_pixel_x = center_x;
        }
        self.cursor_pixel_y = (center_y as f32 - box_height as f32 * 0.25) as usize;
        self.cursor_height = box_height as f32 * 0.5;
    }

    pub fn handle_mouse_move(
        &mut self,
        window: &Window,
        position: winit::dpi::PhysicalPosition<f64>,
    ) -> bool {
        self.mouse_x = position.x as f32;
        self.mouse_y = position.y as f32;

        // Handle window move dragging
        if self.is_dragging_move {
            // Get current global cursor position
            if let Some(window_pos) = window.outer_position().ok() {
                let current_cursor_screen_x = window_pos.x as f64 + self.mouse_x as f64;
                let current_cursor_screen_y = window_pos.y as f64 + self.mouse_y as f64;

                // Calculate delta in global screen space
                let dx = (current_cursor_screen_x - self.drag_start_cursor_screen_pos.0) as i32;
                let dy = (current_cursor_screen_y - self.drag_start_cursor_screen_pos.1) as i32;

                // Move window
                let new_x = self.drag_start_window_pos.0 + dx;
                let new_y = self.drag_start_window_pos.1 + dy;
                let _ = window.set_outer_position(winit::dpi::PhysicalPosition::new(new_x, new_y));
            }
            false // No redraw needed for window move
        } else if self.is_dragging_resize {
            // Get current global cursor position
            if let Some(window_pos) = window.outer_position().ok() {
                let current_cursor_screen_x = window_pos.x as f64 + self.mouse_x as f64;
                let current_cursor_screen_y = window_pos.y as f64 + self.mouse_y as f64;

                // Calculate delta in global screen space
                let dx = (current_cursor_screen_x - self.drag_start_cursor_screen_pos.0) as f32;
                let dy = (current_cursor_screen_y - self.drag_start_cursor_screen_pos.1) as f32;

                // Minimum window dimension: 32 pixels
                let min_size = 128.;

                let (new_width, new_height, should_move, new_x, new_y) = match self.resize_edge {
                    ResizeEdge::Right => {
                        let w = ((self.drag_start_size.0 as f32 + dx).max(min_size) as u32)
                            .max(min_size as u32);
                        let h = self.drag_start_size.1.max(min_size as u32);
                        (w, h, false, 0, 0)
                    }
                    ResizeEdge::Left => {
                        let w = ((self.drag_start_size.0 as f32 - dx).max(min_size) as u32)
                            .max(min_size as u32);
                        let h = self.drag_start_size.1.max(min_size as u32);
                        let width_change = self.drag_start_size.0 as i32 - w as i32;
                        let new_x = self.drag_start_window_pos.0 + width_change;
                        (w, h, true, new_x, self.drag_start_window_pos.1)
                    }
                    ResizeEdge::Bottom => {
                        let w = self.drag_start_size.0.max(min_size as u32);
                        let h = ((self.drag_start_size.1 as f32 + dy).max(min_size) as u32)
                            .max(min_size as u32);
                        (w, h, false, 0, 0)
                    }
                    ResizeEdge::Top => {
                        let w = self.drag_start_size.0.max(min_size as u32);
                        let h = ((self.drag_start_size.1 as f32 - dy).max(min_size) as u32)
                            .max(min_size as u32);
                        let height_change = self.drag_start_size.1 as i32 - h as i32;
                        let new_y = self.drag_start_window_pos.1 + height_change;
                        (w, h, true, self.drag_start_window_pos.0, new_y)
                    }
                    ResizeEdge::TopRight => {
                        let w = ((self.drag_start_size.0 as f32 + dx).max(min_size) as u32)
                            .max(min_size as u32);
                        let h = ((self.drag_start_size.1 as f32 - dy).max(min_size) as u32)
                            .max(min_size as u32);
                        let height_change = self.drag_start_size.1 as i32 - h as i32;
                        let new_y = self.drag_start_window_pos.1 + height_change;
                        (w, h, true, self.drag_start_window_pos.0, new_y)
                    }
                    ResizeEdge::TopLeft => {
                        let w = ((self.drag_start_size.0 as f32 - dx).max(min_size) as u32)
                            .max(min_size as u32);
                        let h = ((self.drag_start_size.1 as f32 - dy).max(min_size) as u32)
                            .max(min_size as u32);
                        let width_change = self.drag_start_size.0 as i32 - w as i32;
                        let height_change = self.drag_start_size.1 as i32 - h as i32;
                        let new_x = self.drag_start_window_pos.0 + width_change;
                        let new_y = self.drag_start_window_pos.1 + height_change;
                        (w, h, true, new_x, new_y)
                    }
                    ResizeEdge::BottomRight => {
                        let w = ((self.drag_start_size.0 as f32 + dx).max(min_size) as u32)
                            .max(min_size as u32);
                        let h = ((self.drag_start_size.1 as f32 + dy).max(min_size) as u32)
                            .max(min_size as u32);
                        (w, h, false, 0, 0)
                    }
                    ResizeEdge::BottomLeft => {
                        let w = ((self.drag_start_size.0 as f32 - dx).max(min_size) as u32)
                            .max(min_size as u32);
                        let h = ((self.drag_start_size.1 as f32 + dy).max(min_size) as u32)
                            .max(min_size as u32);
                        let width_change = self.drag_start_size.0 as i32 - w as i32;
                        let new_x = self.drag_start_window_pos.0 + width_change;
                        (w, h, true, new_x, self.drag_start_window_pos.1)
                    }
                    _ => (self.drag_start_size.0, self.drag_start_size.1, false, 0, 0),
                };

                // Move window if resizing from left/top
                if should_move {
                    let _ =
                        window.set_outer_position(winit::dpi::PhysicalPosition::new(new_x, new_y));
                }

                let _ =
                    window.request_inner_size(winit::dpi::PhysicalSize::new(new_width, new_height));
            }
            false // No redraw needed for window resize (resize event handles it)
        } else {
            // Check button hover state using hitmap
            let old_hovered = self.hovered_button;

            // Get hit test value at mouse position
            let mouse_x = self.mouse_x as usize;
            let mouse_y = self.mouse_y as usize;
            let element_id = if mouse_x < self.width as usize && mouse_y < self.height as usize {
                let hit_idx = mouse_y * self.width as usize + mouse_x;
                let element_id = self.hit_test_map[hit_idx];

                self.hovered_button = match element_id {
                    HIT_CLOSE_BUTTON => HoveredButton::Close,
                    HIT_MAXIMIZE_BUTTON => HoveredButton::Maximize,
                    HIT_MINIMIZE_BUTTON => HoveredButton::Minimize,
                    _ => HoveredButton::None,
                };
                element_id
            } else {
                self.hovered_button = HoveredButton::None;
                HIT_NONE
            };

            // Update cursor icon based on hover position
            // Check what we're hovering over
            let cursor = if self.hovered_button != HoveredButton::None {
                CursorIcon::Pointer
            } else if element_id == HIT_HANDLE_TEXTBOX {
                CursorIcon::Text
            } else {
                let edge = self.get_resize_edge(self.mouse_x, self.mouse_y);
                match edge {
                    ResizeEdge::None => CursorIcon::Default,
                    ResizeEdge::Top | ResizeEdge::Bottom => CursorIcon::NsResize,
                    ResizeEdge::Left | ResizeEdge::Right => CursorIcon::EwResize,
                    ResizeEdge::TopLeft | ResizeEdge::BottomRight => CursorIcon::NwseResize,
                    ResizeEdge::TopRight | ResizeEdge::BottomLeft => CursorIcon::NeswResize,
                }
            };
            window.set_cursor(cursor);

            // Return true if hover state changed
            old_hovered != self.hovered_button
        }
    }

    fn get_resize_edge(&self, x: f32, y: f32) -> ResizeEdge {
        let resize_border = ((self.width.min(self.height) as f32) / 32.0).ceil();

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

    fn submit_username(&mut self) {
        // TODO: Implement semantic embedding for discovery
        // Username -> Hash (identity/keys) + Embedding vector (semantic search, opt-in)
        // Embedding platform: SentenceTransformers (all-MiniLM-L6-v2, ~80MB, 384-dim)
        // Store on DHT: hash=identity, embedding=public discovery vector
        log::warn!(
            "⚠️  SEMANTIC EMBEDDING NOT IMPLEMENTED - Username discovery will be hash-only!"
        );
        log::warn!("⚠️  TODO: Add semantic embedding for fuzzy username search");

        // TODO: Query DHT for username availability
        log::info!("Submitting username: {}", self.username_input);

        // Placeholder - simulate DHT query
        self.username_available = Some(true); // For now, always available
    }

    pub fn render(&mut self) {
        // Increment render counter
        self.render_counter += 1;

        // Update text scroll before locking buffer (needs mutable self)
        if self.needs_redraw && !self.username_input.is_empty() {
            let margin = self.min_dim / 8;
            let box_width = self.width as usize - margin * 2;
            self.update_text_scroll(box_width);
        }

        // Always lock buffer once per frame
        let mut buffer = self.renderer.lock_buffer();
        let pixels = buffer.as_mut();

        // Only redraw full content if dimensions changed or content is dirty
        if self.needs_redraw {
            self.redraw_counter += 1;

            // Clear hitmap to HIT_NONE before drawing
            self.hit_test_map.fill(HIT_NONE);

            Self::draw_background_texture(pixels, self.width as usize, self.height as usize);

            let (start, edges, button_x_start, button_height) =
                Self::draw_window_controls(pixels, &mut self.hit_test_map, self.width, self.height);

            // Cache button rendering data for hover effects
            self.button_x_start = button_x_start;
            self.button_height = button_height;
            self.button_curve_start = start;
            self.button_crossings = edges.clone();

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

            // Build button pixel lists from hitmap AFTER masking for fast hover effects
            // This ensures we only capture pixels inside the squircle curves
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

            // Initialize textbox_mask buffer to screen dimensions
            let buffer_size = self.width as usize * self.height as usize;
            if self.textbox_mask.len() != buffer_size {
                self.textbox_mask.resize(buffer_size, 0);
            }

            // 2. Draw textbox (full width with min_dim/8 margins)
            let margin = self.min_dim / 8;
            let box_width = self.width as usize - margin * 2;
            let box_height = self.min_dim / 8;
            let center_x = self.width as usize / 2;
            let center_y = self.height as usize * 4 / 7;

            // Draw textbox (fills pixels, hit_test_map, and textbox_mask in one pass)
            Self::draw_textbox(
                pixels,
                &mut self.hit_test_map,
                &mut self.textbox_mask,
                self.width as usize,
                self.height as usize,
                center_x,
                center_y,
                box_width,
                box_height,
            );

            // 3. Draw text
            // Infinity placeholder inside the box (only if username is empty AND not focused)
            if self.username_input.is_empty() && !self.textbox_focused {
                self.text_renderer.draw_text_center_u32(
                    pixels,
                    self.width as usize,
                    self.height as usize,
                    "∞",
                    center_x as f32,
                    center_y as f32,
                    box_height as f32 * 0.5,
                    500, // Thin weight
                    theme::FONT_HINT,
                    theme::FONT_USER_CONTENT,
                );
            } else if !self.username_input.is_empty() {
                // Calculate total text width for centering
                let total_text_width: usize = self.character_widths.iter().sum();

                // Keep text centered until it exceeds textbox width
                let text_start_x = if total_text_width <= box_width {
                    // Center text
                    center_x as f32 - (total_text_width as f32 / 2.0)
                } else {
                    // Text overflow - use scroll offset
                    let scroll_pixel_offset: usize = self.character_widths
                        [..self.text_scroll_char_index]
                        .iter()
                        .sum();
                    let textbox_left = center_x - box_width / 2;
                    textbox_left as f32 - scroll_pixel_offset as f32
                };

                // Draw all characters using additive compositing
                let chars: Vec<char> = self.username_input.chars().collect();
                let mut x_offset = text_start_x;
                let font_size = box_height as f32 * 0.5;

                for i in 0..chars.len() {
                    let actual_width = self.text_renderer.render_char_additive_u32(
                        pixels,
                        self.width as usize,
                        chars[i],
                        x_offset,
                        center_y as f32,
                        font_size,
                        500,
                        theme::FONT_USER_CONTENT,
                        theme::TEXT_BRIGHTNESS,
                        &self.textbox_mask,
                        true,
                    );
                    x_offset += actual_width;
                }
            }

            // Update cursor position for blinking
            if !self.username_input.is_empty() {
                let cursor_pixel_offset: usize =
                    self.character_widths[..self.cursor_char_index].iter().sum();
                let scroll_pixel_offset: usize = self.character_widths
                    [..self.text_scroll_char_index]
                    .iter()
                    .sum();
                let textbox_left = center_x - box_width / 2;
                self.cursor_pixel_x = textbox_left + cursor_pixel_offset - scroll_pixel_offset;
            } else {
                self.cursor_pixel_x = center_x;
            }
            self.cursor_pixel_y = (center_y as f32 - box_height as f32 * 0.25) as usize;
            self.cursor_height = box_height as f32 * 0.5;

            // Label below the box
            self.text_renderer.draw_text_center_u32(
                pixels,
                self.width as usize,
                self.height as usize,
                "handle",
                center_x as f32,
                (center_y + box_height) as f32,
                box_height as f32 * 0.5,
                300, // Thin weight
                theme::FONT_LABEL,
                theme::FONT_UI,
            );

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

            self.needs_redraw = false;
            self.render_counter = 0; // Reset frame counter after full redraw

            // After full redraw, pixel lists are rebuilt - reset prev_hovered to force hover reapply
            self.prev_hovered_button = HoveredButton::None;
        }

        // Handle hover state changes
        if self.prev_hovered_button != self.hovered_button {
            // Unhover old button
            match self.prev_hovered_button {
                HoveredButton::Close => {
                    Self::draw_button_hover_by_pixels(
                        pixels,
                        &self.close_pixels,
                        false,
                        HoveredButton::Close,
                    );
                }
                HoveredButton::Maximize => {
                    Self::draw_button_hover_by_pixels(
                        pixels,
                        &self.maximize_pixels,
                        false,
                        HoveredButton::Maximize,
                    );
                }
                HoveredButton::Minimize => {
                    Self::draw_button_hover_by_pixels(
                        pixels,
                        &self.minimize_pixels,
                        false,
                        HoveredButton::Minimize,
                    );
                }
                HoveredButton::None => {}
            }

            // Hover new button
            match self.hovered_button {
                HoveredButton::Close => {
                    Self::draw_button_hover_by_pixels(
                        pixels,
                        &self.close_pixels,
                        true,
                        HoveredButton::Close,
                    );
                }
                HoveredButton::Maximize => {
                    Self::draw_button_hover_by_pixels(
                        pixels,
                        &self.maximize_pixels,
                        true,
                        HoveredButton::Maximize,
                    );
                }
                HoveredButton::Minimize => {
                    Self::draw_button_hover_by_pixels(
                        pixels,
                        &self.minimize_pixels,
                        true,
                        HoveredButton::Minimize,
                    );
                }
                HoveredButton::None => {}
            }

            // Update prev state
            self.prev_hovered_button = self.hovered_button;
        }

        // Handle cursor initialization (when textbox gains focus)
        if self.cursor_needs_init {
            self.cursor_needs_init = false;
            self.cursor_wave_top_bright = true;
            // First blink: Dark/Dark → Bright/Dark
            Self::add_cursor_top(
                pixels,
                self.width as usize,
                self.cursor_pixel_x as f32,
                self.cursor_pixel_y as f32,
                self.cursor_height,
            );
        }

        // Handle cursor undo (when textbox loses focus - return to Dark/Dark)
        let did_undo = self.cursor_needs_undo;
        if self.cursor_needs_undo {
            self.cursor_needs_undo = false;
            // The flag indicates which half is currently bright after the last blink
            // We need to undo whichever one is bright
            if self.cursor_wave_top_bright {
                // Top is bright, subtract it
                Self::sub_cursor_top(
                    pixels,
                    self.width as usize,
                    self.cursor_pixel_x as f32,
                    self.cursor_pixel_y as f32,
                    self.cursor_height,
                );
            } else {
                // Bottom is bright, subtract it
                Self::sub_cursor_bottom(
                    pixels,
                    self.width as usize,
                    self.cursor_pixel_x as f32,
                    self.cursor_pixel_y as f32,
                    self.cursor_height,
                );
            }
        }

        // Handle cursor blinking - wake timer already fired, just blink
        // Don't blink on the same frame we undo to avoid re-dirtying the screen
        if !did_undo && self.textbox_focused {
            if self.cursor_wave_top_bright {
                // Currently Bright/Dark → swap to Dark/Bright
                Self::sub_cursor_top(
                    pixels,
                    self.width as usize,
                    self.cursor_pixel_x as f32,
                    self.cursor_pixel_y as f32,
                    self.cursor_height,
                );
                Self::add_cursor_bottom(
                    pixels,
                    self.width as usize,
                    self.cursor_pixel_x as f32,
                    self.cursor_pixel_y as f32,
                    self.cursor_height,
                );
                self.cursor_wave_top_bright = false;
            } else {
                // Currently Dark/Bright → swap to Bright/Dark
                Self::add_cursor_top(
                    pixels,
                    self.width as usize,
                    self.cursor_pixel_x as f32,
                    self.cursor_pixel_y as f32,
                    self.cursor_height,
                );
                Self::sub_cursor_bottom(
                    pixels,
                    self.width as usize,
                    self.cursor_pixel_x as f32,
                    self.cursor_pixel_y as f32,
                    self.cursor_height,
                );
                self.cursor_wave_top_bright = true;
            }
        }

        // Draw debug counters (bottom left = redraw, bottom right = render)
        let redraw_text = format!("R:{}", self.redraw_counter);
        let render_text = format!("F:{}", self.render_counter);
        let counter_size = (self.min_dim / 16) as f32;

        // Bottom left - redraw counter
        self.text_renderer.draw_text_left_u32(
            pixels,
            self.width as usize,
            self.height as usize,
            &redraw_text,
            counter_size,
            self.height as f32 - counter_size * 2.,
            counter_size,
            400,
            0xFFFFFFFF,
            "Josefin Slab",
        );

        // Bottom right - render counter
        self.text_renderer.draw_text_right_u32(
            pixels,
            self.width as usize,
            self.height as usize,
            &render_text,
            self.width as f32 - counter_size,
            self.height as f32 - counter_size * 2.,
            counter_size,
            400,
            0xFFFFFFFF,
            "Josefin Slab",
        );

        // Always present buffer once per frame
        buffer.present().unwrap();
    }

    fn add_cursor_top(
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
            pixels[idx] += 0x00010101 * wave as u32;
        }
    }

    fn add_cursor_bottom(
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
            pixels[idx] += 0x00010101 * wave as u32;
        }
    }

    fn sub_cursor_top(
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
            pixels[idx] -= 0x00010101 * wave as u32;
        }
    }

    fn sub_cursor_bottom(
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
            pixels[idx] -= 0x00010101 * wave as u32;
        }
    }

    /// Render text string at textbox position using additive/subtractive rendering
    fn render_text_differential(&mut self, pixels: &mut [u32], text: &str, add_mode: bool) {
        if text.is_empty() {
            return;
        }

        let margin = self.min_dim / 8;
        let box_width = self.width as usize - margin * 2;
        let box_height = self.min_dim / 8;
        let center_x = self.width as usize / 2;
        let center_y = self.height as usize * 4 / 7;

        let chars: Vec<char> = text.chars().collect();
        let char_widths: Vec<usize> = chars
            .iter()
            .map(|&ch| {
                self.text_renderer.measure_text_width(
                    &ch.to_string(),
                    box_height as f32 * 0.5,
                    500,
                    theme::FONT_USER_CONTENT,
                ) as usize
            })
            .collect();

        let total_width: usize = char_widths.iter().sum();
        let text_start_x = if total_width <= box_width {
            center_x as f32 - (total_width as f32 / 2.0)
        } else {
            let scroll_pixel_offset: usize =
                char_widths[..self.text_scroll_char_index].iter().sum();
            let textbox_left = center_x - box_width / 2;
            textbox_left as f32 - scroll_pixel_offset as f32
        };

        let mut x_offset = text_start_x;
        let font_size = box_height as f32 * 0.5;

        for &ch in &chars {
            let actual_width = self.text_renderer.render_char_additive_u32(
                pixels,
                self.width as usize,
                ch,
                x_offset,
                center_y as f32,
                font_size,
                500,
                theme::FONT_USER_CONTENT,
                theme::TEXT_BRIGHTNESS,
                &self.textbox_mask,
                add_mode,
            );
            x_offset += actual_width;
        }
    }

    fn draw_window_controls(
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
        let radius = smaller_dim / 2.0;
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

    fn draw_minimize_symbol(
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

    fn draw_maximize_symbol(
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

    fn draw_close_symbol(
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
    fn distance_to_capsule(
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

    fn draw_background_texture(pixels: &mut [u32], width: usize, height: usize) {
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
                    colour = colour + adder & mask;
                    let subtractor = (rng >> 5) as u32 & ones;
                    colour = colour - subtractor & mask;
                    row_pixels[x] = colour + base | alpha;
                }

                // Left half: right-to-left (mirror)
                rng = 0xDEADBEEF01234567
                    ^ ((row_idx.wrapping_sub(height / 2)).wrapping_mul(0x9E3779B94517B397));
                colour = rng as u32 & mask | alpha;

                for x in (1..width / 2).rev() {
                    rng ^= rng.rotate_left(13).wrapping_sub(12345678901);
                    let adder = rng as u32 & ones;
                    colour = colour + adder & mask;
                    let subtractor = (rng >> 5) as u32 & ones;
                    colour = colour - subtractor & mask;
                    row_pixels[x] = colour + base | alpha;
                }
            });
    }

    /// Draw window edge hairlines and apply squircle alpha mask
    fn draw_window_edges_and_mask(
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
    fn draw_button_hover_by_pixels(
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

    /// Draw vertical hairlines between buttons
    fn draw_button_hairlines(
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

    fn draw_textbox(
        pixels: &mut [u32],
        hit_test_map: &mut [u8],
        textbox_mask: &mut [u8],
        window_width: usize,
        _window_height: usize,
        center_x: usize,
        center_y: usize,
        box_width: usize,
        box_height: usize,
    ) {
        // Clear mask first
        textbox_mask.fill(0);

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
            hit_test_map[idx] = HIT_HANDLE_TEXTBOX;
            textbox_mask[idx] = h;

            // Fill horizontally to the diagonal (where horizontal edge would be)
            let diag_x = (x + radius as usize - i).min(window_width);
            for fill_x in (px + 2)..=diag_x {
                let idx = py * window_width + fill_x;
                pixels[idx] = fill_colour;
                hit_test_map[idx] = HIT_HANDLE_TEXTBOX;
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
            hit_test_map[idx] = HIT_HANDLE_TEXTBOX;
            textbox_mask[idx] = h;

            // Fill vertically down from horizontal edge to diagonal
            // Diagonal is where the vertical edge is at this same iteration
            let diag_y = y + radius as usize - i;
            for fill_y in (hy + 2)..diag_y {
                let idx = fill_y * window_width + hx;
                pixels[idx] = fill_colour;
                hit_test_map[idx] = HIT_HANDLE_TEXTBOX;
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
            hit_test_map[idx] = HIT_HANDLE_TEXTBOX;
            textbox_mask[idx] = h;

            // Fill horizontally to the diagonal
            let diag_x = x + box_width - 1 - radius as usize + i;
            for fill_x in diag_x..(px - 1) {
                let idx = py * window_width + fill_x;
                pixels[idx] = fill_colour;
                hit_test_map[idx] = HIT_HANDLE_TEXTBOX;
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
            hit_test_map[idx] = HIT_HANDLE_TEXTBOX;
            textbox_mask[idx] = h;

            // Fill vertically down from horizontal edge to diagonal
            let diag_y = y + radius as usize - i;
            for fill_y in (hy + 2)..diag_y {
                let idx = fill_y * window_width + hx;
                pixels[idx] = fill_colour;
                hit_test_map[idx] = HIT_HANDLE_TEXTBOX;
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
            hit_test_map[idx] = HIT_HANDLE_TEXTBOX;
            textbox_mask[idx] = h;

            // Fill horizontally to the diagonal
            let diag_x = (x + radius as usize - i).min(window_width);
            for fill_x in (px + 2)..=diag_x {
                let idx = py * window_width + fill_x;
                pixels[idx] = fill_colour;
                hit_test_map[idx] = HIT_HANDLE_TEXTBOX;
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
            hit_test_map[idx] = HIT_HANDLE_TEXTBOX;
            textbox_mask[idx] = h;

            // Fill vertically up from horizontal edge to diagonal
            let diag_y = y + box_height - radius as usize + i;
            for fill_y in (diag_y + 1)..(hy - 1) {
                let idx = fill_y * window_width + hx;
                pixels[idx] = fill_colour;
                hit_test_map[idx] = HIT_HANDLE_TEXTBOX;
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
            hit_test_map[idx] = HIT_HANDLE_TEXTBOX;
            textbox_mask[idx] = h;

            // Fill horizontally to the diagonal
            let diag_x = x + box_width - 1 - radius as usize + i;
            for fill_x in diag_x..(px - 1) {
                let idx = py * window_width + fill_x;
                pixels[idx] = fill_colour;
                hit_test_map[idx] = HIT_HANDLE_TEXTBOX;
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
            hit_test_map[idx] = HIT_HANDLE_TEXTBOX;
            textbox_mask[idx] = h;

            // Fill vertically up from horizontal edge to diagonal
            let diag_y = y + box_height - radius as usize + i;
            for fill_y in (diag_y + 1)..(hy - 1) {
                let idx = fill_y * window_width + hx;
                pixels[idx] = fill_colour;
                hit_test_map[idx] = HIT_HANDLE_TEXTBOX;
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
                    hit_test_map[idx] = HIT_HANDLE_TEXTBOX;
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
            let right_x = x + box_width - 1;
            for py in top_edge..bottom_edge {
                let idx = py * window_width + right_x;
                pixels[idx] = shadow_colour;
            }

            // Fill center rectangle
            for py in top_edge..bottom_edge {
                for px in (x + 1)..(x + box_width - 1) {
                    let idx = py * window_width + px;
                    pixels[idx] = fill_colour;
                    hit_test_map[idx] = HIT_HANDLE_TEXTBOX;
                    textbox_mask[idx] = 255;
                }
            }
        }
    }

    fn draw_spectrum(
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

    fn draw_logo_text(
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
            window_height,
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

// Colour conversion matrices (commented out - using inline calculations instead)
// static LMS2XYZ: Matrix3<f32> = Matrix3::new(
//     1.82320417830601000000000E+00,
//     -1.08438051449034000000000E+00,
//     2.13853269260661000000000E-01,
//     6.45919065585675000000000E-01,
//     2.67038878594950000000000E-01,
//     0.00000000000000000000000E+00,
//     0.00000000000000000000000E+00,
//     0.00000000000000000000000E+00,
//     1.13436512274304000000000E+00,
// );
// static LMS2REC601L525: Matrix3<f32> = Matrix3::new(
//     5.222296891,
//     -4.229101727,
//     0.1314491941,
//     -0.6657302686,
//     1.67263331,
//     -0.1870707488,
//     -0.02435691107,
//     -0.1126634117,
//     1.192543673,
// );
// static LMS2REC601L625: Matrix3<f32> = Matrix3::new(
//     4.667263996,
//     -3.679863594,
//     0.1149124113,
//     -0.5532896364,
//     1.546071719,
//     -0.1595271493,
//     -0.02396958555,
//     -0.1341723126,
//     1.222574152,
// );
// static LMS2REC709: Matrix3<f32> = Matrix3::new(
//     4.91540997355389,
//     -3.92456541543308,
//     0.127391461208755,
//     -0.554971925469863,
//     1.55158938196015,
//     -0.160112970001834,
//     -0.0302101619921962,
//     -0.114892521632605,
//     1.21093312316247,
// );
// static LMS2REC2020: Matrix3<f32> = Matrix3::new(
//     3.168241098811690000,
//     -2.156882856491830000,
//     0.096456879211209600,
//     -0.266362510245695000,
//     1.404945732577530000,
//     -0.175554801656117000,
//     0.003891529873740330,
//     -0.020567680031394800,
//     0.945832607950864000,
// );
// static LMS2PROLAB: Matrix4<f32> = Matrix4::new(
//     4.9539022897099000E+00,
//     8.1268366793707400E+00,
//     2.3456174013944800E+00,
//     4.2460828656884400E+00,
//     5.2566821114546600E-01,
//     -9.0688381648312700E+00,
//     -3.9617221338614400E-03,
//     2.3348763516690700E-01,
//     1.8177764547009900E+00,
//     9.4256253638629600E-01,
//     -2.3435688323380700E+00,
//     1.8177764547009900E+00,
//     0.0000000000000000E+00,
//     0.0000000000000000E+00,
//     0.0000000000000000E+00,
//     1.0000000000000000E+00,
// );
// static LMS2PHOTOPIC: Matrix1x3<f32> = Matrix1x3::new(0.707822839629681, 0.292177180002215, 0f32);
static _LMS2006SO_SCALE: f32 = 1.7102058280935600E-02;
static LMS2006SO: [f32; 1443] = [
    // 2006 LMS 2° Standard Observer interleaved [L,M,S,L,M,S,...] starting from 350 and going to 830nm in 1nm steps
    0.0000000000000000E+00,
    0.0000000000000000E+00,
    0.0000000000000000E+00,
    2.0337836422542300E-09,
    1.8753040036051000E-09,
    1.2341569183685300E-07,
    2.4633104541619600E-09,
    2.2808900506789800E-09,
    1.4839201499748600E-07,
    2.9835515772257800E-09,
    2.7741952308986300E-09,
    1.7842293623507200E-07,
    3.6136655040482200E-09,
    3.3741912184016400E-09,
    2.1453138280576600E-07,
    4.3768569227452300E-09,
    4.1039528334315600E-09,
    2.5794729747030800E-07,
    5.3012312569390500E-09,
    4.9915454604879200E-09,
    3.1014953337842100E-07,
    6.4208296811130200E-09,
    6.0711043950483800E-09,
    3.7291622745504700E-07,
    7.7768827269887000E-09,
    7.3841476286931100E-09,
    4.4838536813023800E-07,
    9.4193286464579400E-09,
    8.9811725601038000E-09,
    5.3912762049896100E-07,
    1.1408652446574600E-08,
    1.0923598038713300E-08,
    6.4823388951542800E-07,
    1.3818113321237200E-08,
    1.3286126428685600E-08,
    7.7942060384033100E-07,
    1.6736442507360600E-08,
    1.6159616534169900E-08,
    9.3715630656852200E-07,
    2.0271110917268600E-08,
    1.9654577873624300E-08,
    1.1268138648296100E-06,
    2.4552286881724400E-08,
    2.3905420687026500E-08,
    1.3548534828959300E-06,
    2.9737629752150000E-08,
    2.9075625123987200E-08,
    1.6290427526757500E-06,
    3.6018095892086000E-08,
    3.5364028410904800E-08,
    1.9587212370544000E-06,
    4.3624970870373700E-08,
    4.3012471790865600E-08,
    2.3551185984445300E-06,
    5.2838386824860500E-08,
    5.2315101324529000E-08,
    2.8317371087886300E-06,
    6.3997638658587600E-08,
    6.3629680244904200E-08,
    3.4048115702481700E-06,
    7.7513678974546500E-08,
    7.7391347919848400E-08,
    4.0938623126124200E-06,
    9.3884251889703400E-08,
    9.4129354568471100E-08,
    4.9223601038828400E-06,
    1.1371222304883300E-07,
    1.1448741532003400E-07,
    5.9185256225278100E-06,
    1.3772778086252900E-07,
    1.3924846639766900E-07,
    7.1162907234045500E-06,
    1.6681532655615000E-07,
    1.6936477550742300E-07,
    8.5564542404371000E-06,
    2.0204604328745000E-07,
    2.0599456442674400E-07,
    1.0288071695540200E-05,
    2.4471734372904400E-07,
    2.5054655223455300E-07,
    1.2370126250703700E-05,
    2.9640064881943100E-07,
    3.0473413223942200E-07,
    1.4873537819986500E-05,
    3.5899925719140200E-07,
    3.7064126615790900E-07,
    1.7883578776730900E-05,
    4.3481843638774500E-07,
    4.5080262972054000E-07,
    2.1502778540945100E-05,
    5.2665031705589800E-07,
    5.4830109196846000E-07,
    2.5854415984262500E-05,
    6.3787671645032200E-07,
    6.6688627712791700E-07,
    3.1086718612408100E-05,
    7.7259367783929000E-07,
    8.1111876874962600E-07,
    3.7377911559683900E-05,
    9.3576231212654400E-07,
    9.8654550195777000E-07,
    4.4942288376680000E-05,
    1.1333914966083400E-06,
    1.1999130890947700E-06,
    5.4037510397216800E-05,
    1.3727591589629900E-06,
    1.4594272829015200E-06,
    6.4973383318960900E-05,
    1.6626802955166100E-06,
    1.7750685557436200E-06,
    7.8122409949701400E-05,
    2.0138315938737400E-06,
    2.1589759315211900E-06,
    9.3932478571854900E-05,
    2.4391446145237100E-06,
    2.6259138317816100E-06,
    1.1294211912219300E-04,
    2.9542820107990800E-06,
    3.1938398901388100E-06,
    1.3579884684990900E-04,
    3.5782143245472700E-06,
    3.8845955721711900E-06,
    1.6328121828326200E-04,
    4.3339185986960300E-06,
    4.7247461608590900E-06,
    1.9632535078543700E-04,
    5.2367973714709300E-06,
    5.7366371468408800E-06,
    2.3598799590550100E-04,
    6.3101084002406200E-06,
    6.9496085178640100E-06,
    2.8346445746583500E-04,
    7.5789545030599800E-06,
    8.3958604377359300E-06,
    3.4011381446016400E-04,
    9.0698187857220900E-06,
    1.0109969625710600E-05,
    4.0746347257208000E-04,
    1.0809862006062600E-05,
    1.2128081055187200E-05,
    4.8720710394924000E-04,
    1.2825943721985800E-05,
    1.4486710482141700E-05,
    5.8119366533868000E-04,
    1.5143335002909900E-05,
    1.7221095328195800E-05,
    6.9140454821561700E-04,
    1.7784111393798700E-05,
    2.0363053430379400E-05,
    8.1991620899600800E-04,
    2.0765224939789900E-05,
    2.3938339435757900E-05,
    9.6884522263573700E-04,
    2.4100765106789000E-05,
    2.7968980799656100E-05,
    1.1403119671432700E-03,
    2.7818427699331000E-05,
    3.2493162373884400E-05,
    1.3365141838675200E-03,
    3.1955103606322500E-05,
    3.7560030028414400E-05,
    1.5596071304818900E-03,
    3.6555447938592800E-05,
    4.3227923158882700E-05,
    1.8115893217266800E-03,
    4.1674129791457700E-05,
    4.9567063503456100E-05,
    2.0942008045531300E-03,
    4.7364402162782100E-05,
    5.6651413559879300E-05,
    2.4087637796160100E-03,
    5.3623854744473500E-05,
    6.4515677277540900E-05,
    2.7558728868772800E-03,
    6.0409328745535200E-05,
    7.3167359250956700E-05,
    3.1352858476453500E-03,
    6.7640928421307900E-05,
    8.2590061746496100E-05,
    3.5457816823548800E-03,
    7.5195915292173800E-05,
    9.2737881528527800E-05,
    3.9849937271614000E-03,
    8.2934503680324500E-05,
    1.0354710488912400E-04,
    4.4497488767803900E-03,
    9.0811357953765700E-05,
    1.1499784468930400E-04,
    4.9378413490314200E-03,
    9.8826687766067800E-05,
    1.2708776168093000E-04,
    5.4473034894766500E-03,
    1.0700465471985900E-04,
    1.3982065774342400E-04,
    5.9761006336156200E-03,
    1.1539598803052400E-04,
    1.5320885284716700E-04,
    6.5222157689263900E-03,
    1.2404681273761000E-04,
    1.6725904809685000E-04,
    7.0827894561964700E-03,
    1.3288100680007900E-04,
    1.8191350882756300E-04,
    7.6507489632019900E-03,
    1.4176863569918300E-04,
    1.9708029823337900E-04,
    8.2166444023236100E-03,
    1.5055491221426500E-04,
    2.1264625608849400E-04,
    8.7695314938396100E-03,
    1.5906139975922500E-04,
    2.2847668599972700E-04,
    9.2971560838091900E-03,
    1.6714204441331800E-04,
    2.4445611236274300E-04,
    9.7893850719131500E-03,
    1.7486668617159300E-04,
    2.6063184044886800E-04,
    1.0249179095185900E-02,
    1.8239658381433200E-04,
    2.7713169677741400E-04,
    1.0685312848110700E-02,
    1.8993385804864500E-04,
    2.9413160365977900E-04,
    1.1109282536808100E-02,
    1.9772060923388600E-04,
    3.1185917151066300E-04,
    1.1535102875110300E-02,
    2.0597944801059100E-04,
    3.3054999947184000E-04,
    1.1974844762903600E-02,
    2.1471372675069300E-04,
    3.5028495206138600E-04,
    1.2424189936378300E-02,
    2.2385768074147500E-04,
    3.7110021913750600E-04,
    1.2873392539682900E-02,
    2.3333118677929600E-04,
    3.9303034338983200E-04,
    1.3311336497213900E-02,
    2.4303815519518800E-04,
    4.1610778469097200E-04,
    1.3725589059993600E-02,
    2.5289010105779800E-04,
    4.4034221310950500E-04,
    1.4106337558269000E-02,
    2.6288730846765800E-04,
    4.6565022892748200E-04,
    1.4458723815908100E-02,
    2.7305809748206700E-04,
    4.9189764212188000E-04,
    1.4792883673216900E-02,
    2.8343622816381800E-04,
    5.1891411104375200E-04,
    1.5120204746752100E-02,
    2.9406121680233400E-04,
    5.4649027862683000E-04,
    1.5453229059945400E-02,
    3.0493942003453000E-04,
    5.7437980360830300E-04,
    1.5799925970567200E-02,
    3.1591525662758400E-04,
    6.0231215948755100E-04,
    1.6145579302807000E-02,
    3.2676892544734400E-04,
    6.2998326192580200E-04,
    1.6468204906967900E-02,
    3.3725309970391600E-04,
    6.5705304273402700E-04,
    1.6744192649099900E-02,
    3.4709477813212300E-04,
    6.8314834540200000E-04,
    1.6948792474393200E-02,
    3.5608482000696900E-04,
    7.0798107835507400E-04,
    1.7064139078523700E-02,
    3.6435601204008800E-04,
    7.3172524848300800E-04,
    1.7102058280935600E-02,
    3.7215490723797600E-04,
    7.5473006527409200E-04,
    1.7082885976550600E-02,
    3.7975645093924300E-04,
    7.7740963848838900E-04,
    1.7027602062647600E-02,
    3.8746136130094100E-04,
    8.0024010135499300E-04,
    1.6957257383648700E-02,
    3.9552218655354900E-04,
    8.2361811632414700E-04,
    1.6886489025555200E-02,
    4.0391163167423300E-04,
    8.4740724124294200E-04,
    1.6805115129367500E-02,
    4.1252279345881900E-04,
    8.7130522982613500E-04,
    1.6697060550542900E-02,
    4.2123903576427000E-04,
    8.9497614740367700E-04,
    1.6546759021439900E-02,
    4.2993354329850100E-04,
    9.1805087908712100E-04,
    1.6339481542685500E-02,
    4.3853762472022000E-04,
    9.4031964801481800E-04,
    1.6067649967939600E-02,
    4.4725672560739000E-04,
    9.6234277655779100E-04,
    1.5747986075195900E-02,
    4.5638447241033100E-04,
    9.8494012608982100E-04,
    1.5402259000510700E-02,
    4.6623859352048100E-04,
    1.0090109302170600E-03,
    1.5050769758243400E-02,
    4.7716481240831400E-04,
    1.0355430083643900E-03,
    1.4712147558696000E-02,
    4.8950820979424000E-04,
    1.0654726859923400E-03,
    1.4400727819751900E-02,
    5.0350968657658700E-04,
    1.0991978007835400E-03,
    1.4119226398814600E-02,
    5.1940484927265300E-04,
    1.1370030529397900E-03,
    1.3867632755849100E-02,
    5.3746289856775300E-04,
    1.1792137669688600E-03,
    1.3646006446327600E-02,
    5.5799374418718100E-04,
    1.2262004207245600E-03,
    1.3454498422492500E-02,
    5.8124441106727900E-04,
    1.2782154740462100E-03,
    1.3290569232921300E-02,
    6.0703278866830700E-04,
    1.3348338174929600E-03,
    1.3140852546421900E-02,
    6.3501056359934900E-04,
    1.3953413382956300E-03,
    1.2989897138823500E-02,
    6.6474752644349300E-04,
    1.4588595771798500E-03,
    1.2822964268679100E-02,
    6.9571444394804500E-04,
    1.5243190977147100E-03,
    1.2626123474505700E-02,
    7.2737463115808100E-04,
    1.5906236330866200E-03,
    1.2388772049531900E-02,
    7.5952754741933300E-04,
    1.6572517672231000E-03,
    1.2110401511659000E-02,
    7.9205498193598000E-04,
    1.7238107741288100E-03,
    1.1793310203930800E-02,
    8.2482489415711300E-04,
    1.7898690187474700E-03,
    1.1440227004612200E-02,
    8.5769127066651100E-04,
    1.8549577012122600E-03,
    1.1054272395332100E-02,
    8.9054256056293200E-04,
    1.9187074895752100E-03,
    1.0639590477681000E-02,
    9.2346053459243200E-04,
    1.9812901516404400E-03,
    1.0203114306017500E-02,
    9.5660130714470900E-04,
    2.0430774519123500E-03,
    9.7520370391188900E-03,
    9.9015124977462900E-04,
    2.1045146862801400E-03,
    9.2929656553955800E-03,
    1.0243280133500200E-03,
    2.1661193942175300E-03,
    8.8318442592468600E-03,
    1.0593377527552100E-03,
    2.2283833292617400E-03,
    8.3741281394234900E-03,
    1.0952333256011000E-03,
    2.2914573631931600E-03,
    7.9253352447513500E-03,
    1.1320263083000600E-03,
    2.3554081899733100E-03,
    7.4902382520014300E-03,
    1.1697280919944400E-03,
    2.4203115360300200E-03,
    7.0726748378552700E-03,
    1.2083500099102700E-03,
    2.4862523166600200E-03,
    6.6756249952792500E-03,
    1.2478888089253100E-03,
    2.5532658748657700E-03,
    6.3004046317485300E-03,
    1.2882799862645000E-03,
    2.6211481040714700E-03,
    5.9444711287507500E-03,
    1.3294341175622100E-03,
    2.6896107431230200E-03,
    5.6048529404018800E-03,
    1.3712495244732400E-03,
    2.7583370366182200E-03,
    5.2790942060201800E-03,
    1.4136120220790100E-03,
    2.8269814633075800E-03,
    4.9652002902236000E-03,
    1.4565571182866800E-03,
    2.8955130778395600E-03,
    4.6625567636111900E-03,
    1.5007972663264800E-03,
    2.9653129266579900E-03,
    4.3743027942454000E-03,
    1.5473051462425700E-03,
    3.0382601735460700E-03,
    4.1036292062322400E-03,
    1.5971715292570500E-03,
    3.1164189808909000E-03,
    3.8528238362962300E-03,
    1.6516233342035600E-03,
    3.2020654218304900E-03,
    3.6234242963694900E-03,
    1.7117724405455700E-03,
    3.2971847371488600E-03,
    3.4156595931444800E-03,
    1.7777323683505900E-03,
    3.4017565587676400E-03,
    3.2267723147611200E-03,
    1.8493167360159000E-03,
    3.5151705477202200E-03,
    3.0537434756128300E-03,
    1.9262826810789100E-03,
    3.6367142711091200E-03,
    2.8940058994974400E-03,
    2.0083148981790900E-03,
    3.7655457700717200E-03,
    2.7453709386045700E-03,
    2.0951109659019800E-03,
    3.9009022772939900E-03,
    2.6058574355066200E-03,
    2.1867069737159600E-03,
    4.0428463005881400E-03,
    2.4733517372392700E-03,
    2.2832239427812100E-03,
    4.1916702551081900E-03,
    2.3459860325681200E-03,
    2.3847718135483200E-03,
    4.3476781916348300E-03,
    2.2222246572253700E-03,
    2.4914465813396200E-03,
    4.5111861766765900E-03,
    2.1008362477132400E-03,
    2.6033126576567100E-03,
    4.6824139050486300E-03,
    1.9810289951800800E-03,
    2.7203501005748500E-03,
    4.8611254706099900E-03,
    1.8628266486595200E-03,
    2.8424853168750300E-03,
    5.0468924598579100E-03,
    1.7464639766101300E-03,
    2.9695994901622000E-03,
    5.2391839961015900E-03,
    1.6322060079336300E-03,
    3.1015229561587400E-03,
    5.4373571557014100E-03,
    1.5203411559473700E-03,
    3.2380460438801200E-03,
    5.6406903675367500E-03,
    1.4115320776060000E-03,
    3.3789675417504300E-03,
    5.8485201537045600E-03,
    1.3076698243005000E-03,
    3.5240515281642800E-03,
    6.0601247001246700E-03,
    1.2104516370036000E-03,
    3.6730087249240000E-03,
    6.2746786780533800E-03,
    1.1210451706037200E-03,
    3.8254928093903700E-03,
    6.4912496293221400E-03,
    1.0401822105765200E-03,
    3.9811070891574300E-03,
    6.7089377281013100E-03,
    9.6785543258678100E-04,
    4.1394324509038200E-03,
    6.9273405915223200E-03,
    9.0242172517637000E-04,
    4.3000025188152200E-03,
    7.1461898673367400E-03,
    8.4218901432984200E-04,
    4.4622930693377700E-03,
    7.3652197795778400E-03,
    7.8580278214391700E-04,
    4.6257210552768300E-03,
    7.5841681070881100E-03,
    7.3219122898030600E-04,
    4.7895320082413600E-03,
    7.8023653190252600E-03,
    6.8069763562474500E-04,
    4.9524338115545200E-03,
    8.0174112438585900E-03,
    6.3146556421143900E-04,
    5.1128816491629400E-03,
    8.2262541102424500E-03,
    5.8474510117509300E-04,
    5.2691887993266600E-03,
    8.4255979314452100E-03,
    5.4070544385815200E-04,
    5.4195375534235400E-03,
    8.6119305888513000E-03,
    4.9944525574876500E-04,
    5.5624710023117600E-03,
    8.7824917560981200E-03,
    4.6096984051805000E-04,
    5.6984317351882700E-03,
    8.9382286621939400E-03,
    4.2512241296986600E-04,
    5.8285041492524100E-03,
    9.0812660834398300E-03,
    3.9172426296971200E-04,
    5.9539494072415600E-03,
    9.2139811790001300E-03,
    3.6061187342457200E-04,
    6.0761927192947400E-03,
    9.3389697957710500E-03,
    3.3163531564847600E-04,
    6.1964023499988100E-03,
    9.4583798377984100E-03,
    3.0467025923895400E-04,
    6.3141856948493700E-03,
    9.5719120784204500E-03,
    2.7964831193493500E-04,
    6.4287119962502900E-03,
    9.6786220456429400E-03,
    2.5650197422355900E-04,
    6.5391101040573800E-03,
    9.7775474176276500E-03,
    2.3515225192474900E-04,
    6.6444739023932600E-03,
    9.8677157526709800E-03,
    2.1551177412812100E-04,
    6.7443009261699700E-03,
    9.9486511094620100E-03,
    1.9747411769875200E-04,
    6.8398490922512600E-03,
    1.0021918364800000E-02,
    1.8088473255835900E-04,
    6.9329193796189400E-03,
    1.0089663928963100E-02,
    1.6559672827708800E-04,
    7.0254251318532700E-03,
    1.0154108760223200E-02,
    1.5148347950422300E-04,
    7.1193884549185500E-03,
    1.0217538923042800E-02,
    1.3843568120761800E-04,
    7.2162317135828500E-03,
    1.0281455041916200E-02,
    1.2636767121445500E-04,
    7.3145765607656700E-03,
    1.0343997354558900E-02,
    1.1523552429599900E-04,
    7.4122377323502300E-03,
    1.0402406018357300E-02,
    1.0499941323040600E-04,
    7.5069141625719000E-03,
    1.0453864119815200E-02,
    9.5614795851519900E-05,
    7.5961936900162200E-03,
    1.0495511547746300E-02,
    8.7033951574619000E-05,
    7.6779747575263900E-03,
    1.0524944655230000E-02,
    7.9204424385833000E-05,
    7.7517445032889900E-03,
    1.0541682719412600E-02,
    7.2063528352844900E-05,
    7.8174202594831300E-03,
    1.0545765035034100E-02,
    6.5550997092213800E-05,
    7.8749508150489400E-03,
    1.0537277825691600E-02,
    5.9611920295291200E-05,
    7.9243153484450800E-03,
    1.0516350948066700E-02,
    5.4196258068304000E-05,
    7.9659689629581900E-03,
    1.0483753494890900E-02,
    4.9258355881751900E-05,
    8.0021961014981000E-03,
    1.0442654052592100E-02,
    4.4756467553159600E-05,
    8.0357894672522500E-03,
    1.0396789733260100E-02,
    4.0652524676066400E-05,
    8.0695955806438900E-03,
    1.0349844642191700E-02,
    3.6911836382522300E-05,
    8.1065099477963700E-03,
    1.0305439565749500E-02,
    3.3502772023542600E-05,
    8.1485697989691500E-03,
    1.0265865283116000E-02,
    3.0397245575012300E-05,
    8.1941873620305100E-03,
    1.0228323627785600E-02,
    2.7572175369152000E-05,
    8.2408204137383300E-03,
    1.0188801018600200E-02,
    2.5006156340999000E-05,
    8.2858697093040700E-03,
    1.0143346019431800E-02,
    2.2678678904241000E-05,
    8.3266799085684500E-03,
    1.0088086346153000E-02,
    2.0570234946998200E-05,
    8.3612638813631700E-03,
    1.0020416670424400E-02,
    1.8661815097855200E-05,
    8.3904831578603600E-03,
    9.9424453110133700E-03,
    1.6933652065210200E-05,
    8.4159444450982100E-03,
    9.8573982025070000E-03,
    1.5367567950284700E-05,
    8.4392808501511200E-03,
    9.7684188005141900E-03,
    1.3947393442490500E-05,
    8.4621465046726100E-03,
    9.6785509540398500E-03,
    1.2658725781533900E-05,
    8.4857872646781200E-03,
    9.5898490419472000E-03,
    1.1488897427882200E-05,
    8.5097472261495500E-03,
    9.5007983568004300E-03,
    1.0427195881208400E-05,
    8.5331362467911100E-03,
    9.4090743605151300E-03,
    9.4639769113951600E-06,
    8.5550535035668900E-03,
    9.3124452147482300E-03,
    8.5903902184168100E-06,
    8.5745903328563200E-03,
    9.2087815502328600E-03,
    7.7983253839758000E-06,
    8.5909713354766600E-03,
    9.0965770157917300E-03,
    7.0803246582343300E-06,
    8.6039807484743700E-03,
    8.9764327341728100E-03,
    6.4294603455640200E-06,
    8.6135462499022300E-03,
    8.8494502240711900E-03,
    5.8394175679174000E-06,
    8.6195989579280500E-03,
    8.7167036436928000E-03,
    5.3044743304607600E-06,
    8.6220744776145400E-03,
    8.5792341454466200E-03,
    4.8194460395932600E-06,
    8.6207271557013700E-03,
    8.4376347339839200E-03,
    4.3796393798336700E-06,
    8.6145758644324100E-03,
    8.2908752555614100E-03,
    3.9808147889238800E-06,
    8.6024639234627500E-03,
    8.1376300233174700E-03,
    3.6191304896260300E-06,
    8.5832523279784100E-03,
    7.9767106907101200E-03,
    3.2911029067865200E-06,
    8.5558284870459600E-03,
    7.8070778635451300E-03,
    2.9935734064882700E-06,
    8.5199259463956300E-03,
    7.6286164627687700E-03,
    2.7236769915650800E-06,
    8.4785327094285800E-03,
    7.4442846455476200E-03,
    2.4788114988000000E-06,
    8.4353980316922900E-03,
    7.2575624376092300E-03,
    2.2566193723655300E-06,
    8.3942003380215500E-03,
    7.0716363706962500E-03,
    2.0549663849645900E-06,
    8.3585475993522300E-03,
    6.8893950837779700E-03,
    1.8719206796882600E-06,
    8.3308006065215700E-03,
    6.7127572268603900E-03,
    1.7057331197043800E-06,
    8.3085458337513000E-03,
    6.5407852558521600E-03,
    1.5548192255527000E-06,
    8.2882232374923200E-03,
    6.3719783392746100E-03,
    1.4177449601102200E-06,
    8.2663109893767700E-03,
    6.2049819258348100E-03,
    1.2932124354350600E-06,
    8.2393264426956400E-03,
    6.0385814812437200E-03,
    1.1800471819722300E-06,
    8.2045517748891100E-03,
    5.8718114408152200E-03,
    1.0771859548018800E-06,
    8.1621734590327600E-03,
    5.7042622115768700E-03,
    9.8366536956749900E-07,
    8.1131028201526700E-03,
    5.5357020997996100E-03,
    8.9861344481337800E-07,
    8.0582496867452600E-03,
    5.3659602453678300E-03,
    8.2124122493747600E-07,
    7.9985165909010100E-03,
    5.1949260627613300E-03,
    7.5083467695385500E-07,
    7.9346416931721800E-03,
    5.0227001361295100E-03,
    6.8674738138841900E-07,
    7.8667547308601800E-03,
    4.8500082885624400E-03,
    6.2839359836718800E-07,
    7.7948427795129500E-03,
    4.6776696858382600E-03,
    5.7524310538114600E-07,
    7.7189036504495800E-03,
    4.5064337500542200E-03,
    5.2681589761552200E-07,
    7.6389458237955600E-03,
    4.3369788156326400E-03,
    4.8267738601359000E-07,
    7.5550795229195400E-03,
    4.1698331803749600E-03,
    4.4243393686520600E-07,
    7.4677788785309800E-03,
    4.0051612969922600E-03,
    4.0572866511300000E-07,
    7.3775958397715600E-03,
    3.8430484595205000E-03,
    3.7223827084280300E-07,
    7.2850656528623900E-03,
    3.6835858057757100E-03,
    3.4166988547846700E-07,
    7.1907052958628900E-03,
    3.5268693412508500E-03,
    3.1375815468557100E-07,
    7.0947990626899000E-03,
    3.3730497813994400E-03,
    2.8826252874444700E-07,
    6.9967846944364200E-03,
    3.2224673872613000E-03,
    2.6496475083538900E-07,
    6.8959307244391800E-03,
    3.0754657213024500E-03,
    2.4366690784822800E-07,
    6.7915584073980500E-03,
    2.9323378811238700E-03,
    2.2418954837671600E-07,
    6.6830443004428000E-03,
    2.7933290039844800E-03,
    2.0636988972583100E-07,
    6.5700305649047900E-03,
    2.6586304883794000E-03,
    1.9006020444937500E-07,
    6.4530180512627300E-03,
    2.5283622696895700E-03,
    1.7512626368251600E-07,
    6.3326968702578100E-03,
    2.4026046916633800E-03,
    1.6144620547528500E-07,
    6.2097304658331000E-03,
    2.2814098947508100E-03,
    1.4890940287056000E-07,
    6.0847525304237300E-03,
    2.1648039954364000E-03,
    1.3741538915481500E-07,
    5.9582693212275100E-03,
    2.0527669214165300E-03,
    1.2687288984181600E-07,
    5.8303858906152400E-03,
    1.9451767089822100E-03,
    1.1719895844719700E-07,
    5.7011228569076200E-03,
    1.8418941172024200E-03,
    1.0831818695081300E-07,
    5.5705150251767300E-03,
    1.7427867153530900E-03,
    1.0016199407484600E-07,
    5.4386122927452300E-03,
    1.6477284136624200E-03,
    9.2667989781055700E-08,
    5.3056230337377700E-03,
    1.5566085113976600E-03,
    8.5734678201843600E-08,
    5.1723165949908900E-03,
    1.4693566431282300E-03,
    7.9320108958232000E-08,
    5.0395399225841800E-03,
    1.3859050932036500E-03,
    7.3385470349972300E-08,
    4.9080662917423300E-03,
    1.3061788305550300E-03,
    6.7894854523239300E-08,
    4.7785964923815600E-03,
    1.2300962111036100E-03,
    6.2815040208208900E-08,
    4.6513763458261300E-03,
    1.1575850806257800E-03,
    5.8115291711956500E-08,
    4.5251290585576900E-03,
    1.0886215589834400E-03,
    5.3767172950474900E-08,
    4.3983533873218400E-03,
    1.0231742476159700E-03,
    4.9744375394600400E-08,
    4.2697282014747200E-03,
    9.6119028096353300E-04,
    4.6022558888826100E-08,
    4.1381153263301600E-03,
    9.0259871481131200E-04,
    4.2579204379866000E-08,
    4.0029937310918700E-03,
    8.4729175597008000E-04,
    3.9393477663897900E-08,
    3.8656327319559100E-03,
    7.9507310135722400E-04,
    3.6446103328079800E-08,
    3.7276010893274400E-03,
    7.4574216771032400E-04,
    3.3719248123615300E-08,
    3.5903041409002400E-03,
    6.9911670713773600E-04,
    3.1196413064711500E-08,
    3.4549822456418400E-03,
    6.5503085958410100E-04,
    2.8862333600567700E-08,
    3.3225859171857600E-03,
    6.1334738895293000E-04,
    2.6702887256380200E-08,
    3.1934375672421300E-03,
    5.7399065366334700E-04,
    2.4705008184540200E-08,
    3.0676913374603600E-03,
    5.3689249206002400E-04,
    2.2856608109011400E-08,
    2.9454736368092900E-03,
    5.0197804265192000E-04,
    2.1146503184558700E-08,
    2.8268846002306600E-03,
    4.6916721977791700E-04,
    1.9564346328283400E-08,
    2.7120130186808300E-03,
    4.3835269485309900E-04,
    1.8100564614035700E-08,
    2.6009720181180700E-03,
    4.0934333243520000E-04,
    1.6746301350903700E-08,
    2.4938556962401100E-03,
    3.8195445581945300E-04,
    1.5493362495323500E-08,
    2.3907282311071700E-03,
    3.5603038245042700E-04,
    1.4334167072572300E-08,
    2.2916266807707200E-03,
    3.3144077882446300E-04,
    1.3261701307668600E-08,
    2.1963994180922200E-03,
    3.0812720237525600E-04,
    1.2269476188145100E-08,
    2.1042700308363200E-03,
    2.8622085901441900E-04,
    1.1351488201925400E-08,
    2.0144233179940300E-03,
    2.6584689410195500E-04,
    1.0502183012747900E-08,
    1.9261741491696500E-03,
    2.4707733070716500E-04,
    9.7164218533515800E-09,
    1.8389600303160500E-03,
    2.2994138918040600E-04,
    8.9894504330853300E-09,
    1.7525058242828600E-03,
    2.1438298361666100E-04,
    8.3168701718136500E-09,
    1.6672620693745900E-03,
    2.0014032966368800E-04,
    7.6946115860681400E-09,
    1.5837782334616500E-03,
    1.8695036215883600E-04,
    7.1189096664163500E-09,
    1.5025186105313500E-03,
    1.7459897941965000E-04,
    6.5862810970673700E-09,
    1.4238662301475000E-03,
    1.6291348001739600E-04,
    6.0935031798800600E-09,
    1.3481027666407200E-03,
    1.5178140924483200E-04,
    5.6375943352526300E-09,
    1.2753484538032300E-03,
    1.4120587320879300E-04,
    5.2157960619129500E-09,
    1.2056634905737500E-03,
    1.3120668132376000E-04,
    4.8255562464566000E-09,
    1.1390795892794000E-03,
    1.2179362613059000E-04,
    4.4645137216457700E-09,
    1.0756032108102900E-03,
    1.1296788054973900E-04,
    4.1304839800384300E-09,
    1.0152009604904300E-03,
    1.0472077094771200E-04,
    3.8214459565071900E-09,
    9.5775826841767100E-04,
    9.7028791177179800E-05,
    3.5355297996747700E-09,
    9.0314684676743700E-04,
    8.9864888708624700E-05,
    3.2710055582765200E-09,
    8.5124381609339200E-04,
    8.3201817016047500E-05,
    3.0262727139961100E-09,
    8.0193158679979600E-04,
    7.7012398746155400E-05,
    2.7998504974424000E-09,
    7.5510003426697400E-04,
    7.1271741507081000E-05,
    2.5903689286736700E-09,
    7.1065301020913700E-04,
    6.5962213488852200E-05,
    2.3965605280594200E-09,
    6.6849818115851100E-04,
    6.1065867464697600E-05,
    2.2172526473258900E-09,
    6.2854442687437800E-04,
    5.6562900392135100E-05,
    2.0513603743839200E-09,
    5.9070201180770400E-04,
    5.2432306262326100E-05,
    1.8978799690092400E-09,
    5.5488085413641500E-04,
    4.8648534364648200E-05,
    1.7558827896577100E-09,
    5.2098586436575200E-04,
    4.5172420350754100E-05,
    1.6245096746690600E-09,
    4.8892435328361500E-04,
    4.1966625638251200E-05,
    1.5029657438625700E-09,
    4.5860788768963500E-04,
    3.8999240631605400E-05,
    1.3905155890712400E-09,
    4.2995211532235300E-04,
    3.6242982621527900E-05,
    1.2864788245146700E-09,
    4.0287636500452100E-04,
    3.3676009301585800E-05,
    1.1902259700878800E-09,
    3.7730295796269800E-04,
    3.1284926630283400E-05,
    1.1011746426577000E-09,
    3.5315774898355600E-04,
    2.9058641751627700E-05,
    1.0187860323219000E-09,
    3.3037014197571800E-04,
    2.6986649020846600E-05,
    9.4256164231057200E-10,
    3.0887285326442700E-04,
    2.5059011809994900E-05,
    8.7204027280430200E-10,
    2.8860412223020200E-04,
    2.3266251095514000E-05,
    8.0679523041956500E-10,
    2.6951334857135500E-04,
    2.1599109729545200E-05,
    7.4643174647718800E-10,
    2.5155193361040200E-04,
    2.0048879278261000E-05,
    6.9058458843297900E-10,
    2.3467094379593400E-04,
    1.8607454718479800E-05,
    6.3891585001833900E-10,
    2.1882146433852400E-04,
    1.7267293890935200E-05,
    5.9111290672000700E-10,
    2.0394866176145700E-04,
    1.6021040592856400E-05,
    5.4688652422841800E-10,
    1.8997674834020600E-04,
    1.4860671730565600E-05,
    5.0596910840978900E-10,
    1.7683317309379000E-04,
    1.3778769999963300E-05,
    4.6811308621324000E-10,
    1.6445449936088600E-04,
    1.2768789892300300E-05,
    4.3308940771658500E-10,
    1.5278530305891800E-04,
    1.1824950755232500E-05,
    4.0068616024731300E-10,
    1.4179181116757700E-04,
    1.0943105616566000E-05,
    3.7070728619343100E-10,
    1.3149648379311400E-04,
    1.0122975850773800E-05,
    3.4297139674621300E-10,
    1.2192004378876300E-04,
    9.3642976767373600E-06,
    3.1731067439734900E-10,
    1.1306772396423300E-04,
    8.6659203053712500E-06,
    2.9356985754997100E-10,
    1.0493233328372600E-04,
    8.0260095249768700E-06,
    2.7160530109991900E-10,
    9.7484635006159700E-05,
    7.4414724939296100E-06,
    2.5128410730321800E-10,
    9.0645439701165900E-05,
    6.9062729857209900E-06,
    2.3248332167105200E-10,
    8.4335404051022800E-05,
    6.4144780125124000E-06,
    2.1508918902693300E-10,
    7.8487285645226900E-05,
    5.9609946328872800E-06,
    1.9899646522481700E-10,
    7.3044131554317100E-05,
    5.5414453020757500E-06,
    1.8410778036367700E-10,
    6.7961945085525700E-05,
    5.1522228669591000E-06,
    1.7033304964561100E-10,
    6.3218605237974100E-05,
    4.7907885266547100E-06,
    1.5758892831287500E-10,
    5.8796766102063400E-05,
    4.4549915107829900E-06,
    1.4579830736589300E-10,
    5.4679199761610000E-05,
    4.1428675685257100E-06,
    1.3488984701105100E-10,
    5.0848928988467300E-05,
    3.8526209399351000E-06,
    1.2479754501540500E-10,
    4.7287551793272600E-05,
    3.5825646046405100E-06,
    1.1546033735656900E-10,
    4.3970979487948900E-05,
    3.3309993719254800E-06,
    1.0682172875153300E-10,
    4.0876365141685100E-05,
    3.0963696419132700E-06,
    9.8829450828871900E-11,
    3.7983744849923000E-05,
    2.8772905596752800E-06,
    9.1435145876126600E-11,
    3.5275665029303600E-05,
    2.6725280418911400E-06,
    8.4594074248828700E-11,
    3.2738487394606100E-05,
    2.4810942988755600E-06,
    7.8264843670849200E-11,
    3.0366020436604500E-05,
    2.3025074326494500E-06,
    7.2409158788179300E-11,
    2.8152984419495900E-05,
    2.1363369928371300E-06,
    6.6991589460806900E-11,
    2.6093340405619600E-05,
    1.9820912339193600E-06,
    6.1979356390726800E-11,
    2.4180485139435200E-05,
    1.8392335092434300E-06,
    5.7342132789014500E-11,
    2.2407338243597100E-05,
    1.7071583271707800E-06,
    5.3051860881939200E-11,
    2.0766295994232800E-05,
    1.5851104561732600E-06,
    4.9082582145877700E-11,
    1.9249672271347100E-05,
    1.4723441014576000E-06,
    4.5410280243854300E-11,
    1.7849876266602600E-05,
    1.3681647938679800E-06,
    4.2012735713387400E-11,
    1.6559478284480700E-05,
    1.2719266612610200E-06,
    3.8869391526422400E-11,
    1.5370567057659200E-05,
    1.1830015422383700E-06,
    3.5961228707915100E-11,
    1.4273085586242400E-05,
    1.1007080452207800E-06,
    3.3270651260488200E-11,
    1.3257563623102700E-05,
    1.0244159241834000E-06,
    3.0781379698891900E-11,
    1.2315742215459900E-05,
    9.5356789524838300E-07,
    2.8478352550092300E-11,
    1.1440410817901700E-05,
    8.8767052782636100E-07,
    2.6347635223009300E-11,
    1.0625774619764200E-05,
    8.2631611168326100E-07,
    2.4376335696515300E-11,
    9.8685646364983400E-06,
    7.6924859341175900E-07,
    2.2552526515559400E-11,
    9.1659800989718400E-06,
    7.1624347870351200E-07,
    2.0865172623452100E-11,
    8.5151532787775500E-06,
    6.6707639149099400E-07,
    1.9304064593654100E-11,
    7.9131937621750800E-06,
    6.2152528417306000E-07,
    1.7859756857085500E-11,
    7.3569583940704800E-06,
    5.7935285410878800E-07,
    1.6523510551195900E-11,
    6.8424316830968200E-06,
    5.4026004547226400E-07,
    1.5287240645001600E-11,
    6.3657725913472800E-06,
    5.0396166349964400E-07,
    1.4143467020165000E-11,
    5.9235632485779500E-06,
    4.7020439136768400E-07,
    1.3085269212132200E-11,
    5.5127589115633900E-06,
    4.3876299346864400E-07,
    1.2106244537485300E-11,
    5.1306600509748700E-06,
    4.0944154827706200E-07,
    1.1200469354157800E-11,
    4.7749142855919900E-06,
    3.8208194930618300E-07,
    1.0362463220116600E-11,
    4.4434190229562500E-06,
    3.5654371952270600E-07,
    9.5871557336485500E-12,
    4.1342770820464100E-06,
    3.3269761748026300E-07,
    8.8698558546195000E-12,
    3.8457738316494900E-06,
    3.1042464333257100E-07,
    8.2062235210804200E-12,
    3.5765405223014000E-06,
    2.8962737679554800E-07,
    7.5922433894865600E-12,
    3.3259804335637400E-06,
    2.7025931446266200E-07,
    7.0242005396427800E-12,
    3.0935517822824700E-06,
    2.5227745480569600E-07,
    6.4986579973767900E-12,
    2.8785930332169900E-06,
    2.3563076910612400E-07,
    6.0124359389399100E-12,
    2.6803552588664800E-06,
    2.2026241713016300E-07,
    5.5625924513104100E-12,
    2.4978221355355200E-06,
    2.0609361676611100E-07,
    5.1464057319887700E-12,
    2.3292223031137000E-06,
    1.9298012385754300E-07,
    4.7613576205834600E-12,
    2.1728396614761400E-06,
    1.8078172280816000E-07,
    4.4051183625445300E-12,
    2.0272101872072900E-06,
    1.6937930276650000E-07,
    4.0755325128569300E-12,
    1.8910859585599700E-06,
    1.5867190314922600E-07,
    3.7706058944031500E-12,
    1.7635271087235900E-06,
    1.4858563711503900E-07,
    3.4884935320860400E-12,
    1.6441749227276200E-06,
    1.3909997783948900E-07,
    3.2274884897066300E-12,
    1.5327646648847900E-06,
    1.3020278710997900E-07,
    2.9860115420537500E-12,
    1.4289963515684500E-06,
    1.2187821100989100E-07,
    2.7626016197159800E-12,
    1.3325437908463800E-06,
    1.1410744754584700E-07,
    2.5559069688016600E-12,
    1.2430258489724400E-06,
    1.0686655286067100E-07,
    2.3646769720784000E-12,
    1.1599217710882400E-06,
    1.0011980401490500E-07,
    2.1877545820455100E-12,
    1.0827187555604900E-06,
    9.3831189258442400E-08,
    2.0240693201550900E-12,
    1.0109497750829700E-06,
    8.7967486049513600E-08,
    1.8726307998233500E-12,
    9.4418918147828500E-07,
    8.2498019134885700E-08,
    1.7325227340427000E-12,
    8.8204249687365800E-07,
    7.7393144198348400E-08,
    1.6028973913373300E-12,
    8.2412762104123700E-07,
    7.2620713375094900E-08,
    1.4829704665178200E-12,
    7.7009504667531100E-07,
    6.8150900832724600E-08,
    1.3720163351998700E-12,
    7.1963038114271400E-07,
    6.3957245418787300E-08,
    1.2693636633745200E-12,
    6.7245056059676800E-07,
    6.0016265881398900E-08,
    1.1743913454651800E-12,
    6.2832044278153200E-07,
    5.6309302890111300E-08,
    1.0865247462945400E-12,
    5.8709939112669300E-07,
    5.2827716776107800E-08,
    1.0052322242231900E-12,
    5.4866237611374300E-07,
    4.9564357498557000E-08,
    9.3002191442289000E-13,
    5.1287948758723100E-07,
    4.6511303481222300E-08,
    8.6043875282173500E-13,
    4.7961810627222900E-07,
    4.3660065287802400E-08,
    7.9606172271417600E-13,
    4.4872152190405500E-07,
    4.0999775699850300E-08,
    7.3650130737655800E-13,
    4.1995264407388400E-07,
    3.8512339446311800E-08,
    6.8139713327497600E-13,
    3.9308306160063500E-07,
    3.6180012352781100E-08,
    6.3041578960560800E-13,
    3.6791495380802400E-07,
    3.3987323151108500E-08,
    5.8324881097450000E-13,
    3.4427709518359200E-07,
    3.1920793655984900E-08,
    5.3961081101091400E-13,
    3.2204470278743500E-07,
    2.9970731350904300E-08,
    4.9923775562156600E-13,
    3.0119394023108300E-07,
    2.8136102069416700E-08,
    4.6188536543797500E-13,
    2.8171130144447400E-07,
    2.6416719259420300E-08,
    4.2732763779085500E-13,
    2.6357014164980900E-07,
    2.4811189788591300E-08,
    3.9535547926866300E-13,
    2.4673343783257700E-07,
    2.3317150765498300E-08,
    3.6577544058653800E-13,
    2.3113947897661800E-07,
    2.1929877251657500E-08,
    3.3840854611087300E-13,
    2.1666220064530100E-07,
    2.0638423066640600E-08,
    3.1308921095751100E-13,
    2.0317589427887600E-07,
    1.9431727020056800E-08,
    2.8966423911138600E-13,
    1.9057166570738000E-07,
    1.8300201041257300E-08,
    2.6799189650570600E-13,
    1.7875521468008100E-07,
    1.7235539846590200E-08,
    2.4794105345226300E-13,
    1.6765230159881700E-07,
    1.6231277426052000E-08,
    2.2939039123412000E-13,
    1.5722554903779100E-07,
    1.5284468133097900E-08,
    2.1222766806011700E-13,
    1.4744531579993000E-07,
    1.4392890814065400E-08,
    1.9634903993980300E-13,
    1.3828165222891100E-07,
    1.3554267450177100E-08,
    1.8165843236972100E-13,
    1.2970457947557500E-07,
    1.2766287799197600E-08,
    1.6806695902939800E-13,
    1.2168198057917400E-07,
    1.2026400175814600E-08,
    1.5549238396982700E-13,
    1.1417389049778900E-07,
    1.1331241112398900E-08,
    1.4385862404037900E-13,
    1.0714152903625000E-07,
    1.0677513047823000E-08,
    1.3309528854356600E-13,
    1.0054959831520100E-07,
    1.0062213797890700E-08,
    1.2313725333229200E-13,
    9.4365929051194400E-08,
    9.4826074126988400E-09,
    1.1392426677265800E-13,
    8.8562775782436200E-08,
    8.9364179206199900E-09,
    1.0540058518817200E-13,
    8.3120512053981200E-08,
    8.4223665959192600E-09,
    9.7514635579601700E-14,
    7.8021419948406400E-08,
    7.9393315893070300E-09,
    9.0218703579737300E-14,
    7.3248004888125500E-08,
    7.4861244192220900E-09,
    8.3468644755014700E-14,
    6.8783095999378000E-08,
    7.0615106883810100E-09,
    7.7223617507219500E-14,
    6.4609183099514200E-08,
    6.6640631027870000E-09,
    7.1445835959173800E-14,
    6.0706616772823000E-08,
    6.2917569975791900E-09,
    6.6100341329230900E-14,
    5.7056442540316800E-08,
    5.9425911357363000E-09,
    6.1154790411264000E-14,
    5.3641121587003300E-08,
    5.6147516417668600E-09,
    5.6579259880338500E-14,
    5.0444420814224800E-08,
    5.3065925234591300E-09,
    5.2346065240005300E-14,
    4.7450481087232700E-08,
    5.0165669290146600E-09,
    4.8429593315749500E-14,
    4.4641642988853800E-08,
    4.7430773896300200E-09,
    4.4806147281083600E-14,
    4.2001433204776300E-08,
    4.4846553301911600E-09,
    4.1453803278609700E-14,
    3.9515252370601800E-08,
    4.2399984790317900E-09,
    3.8352277768527600E-14,
    3.7170159952417200E-08,
    4.0079530069062000E-09,
    3.5482804801973000E-14,
    3.4956025700306300E-08,
    3.7876440681695100E-09,
    3.2828022476623100E-14,
    3.2868736514719800E-08,
    3.5788367703481700E-09,
    3.0371867887562000E-14,
    3.0905038572969400E-08,
    3.3813814887807400E-09,
    2.8099479937800000E-14,
    2.9061156282743200E-08,
    3.1950625206234300E-09,
    2.5997109420398100E-14,
    2.7332906137775500E-08,
    3.0196099016462200E-09,
    2.4052035828143000E-14,
    2.5715320411503900E-08,
    2.8546483730968400E-09,
    2.2252490387426200E-14,
    2.4201530969805300E-08,
    2.6995497830030300E-09,
    2.0587584850638600E-14,
    2.2784629974632700E-08,
    2.5536671557859100E-09,
    1.9047245616236300E-14,
    2.1458177824324600E-08,
    2.4163996313629200E-09,
    1.7622152777866100E-14,
    2.0216169488853300E-08,
    2.2871889322877000E-09,
    1.6303683733764700E-14,
    1.9052708808226400E-08,
    2.1654833596439000E-09,
    1.5083861015238000E-14,
    1.7961214523245800E-08,
    2.0506505120565000E-09,
    1.3955304018552700E-14,
    1.6935508843828000E-08,
    1.9420944663085900E-09,
    1.2911184348191200E-14,
    1.5970070744078800E-08,
    1.8392842027922200E-09,
    1.1945184501273600E-14,
    1.5059964217330700E-08,
    1.7417468380886900E-09,
    1.1051459643162600E-14,
    1.4201071367470000E-08,
    1.6490977942791000E-09,
    1.0224602242973200E-14,
    1.3390794645685300E-08,
    1.5611295370988200E-09,
    9.4596093550133500E-15,
    1.2626900550496400E-08,
    1.4776723631444300E-09,
    8.7518523481882500E-15,
    1.1907193738845400E-08,
    1.3985551950291700E-09,
    8.0970489002164500E-15,
    1.1229526386933600E-08,
    1.3236064377222600E-09,
    7.4912370872057500E-15,
    1.0591803618840600E-08,
    1.2526549843187900E-09,
    6.9307514118170500E-15,
    9.9919888324420300E-09,
    1.1855311805449400E-09,
    6.4122006249733200E-15,
    9.4281095458002800E-09,
    1.1220673639110400E-09,
    5.9324472069225000E-15,
    8.8982589477618900E-09,
    1.0620987331040700E-09,
    5.4885883835035200E-15,
    8.4005998837123000E-09,
    1.0054637111368600E-09,
    5.0779385627532800E-15,
];
