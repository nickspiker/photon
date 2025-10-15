use super::renderer::Renderer;
use super::text::TextRenderer;
use winit::{
    dpi::PhysicalSize,
    event::{ElementState, KeyEvent, MouseButton},
    keyboard::{Key, NamedKey},
    window::{Window, CursorIcon},
};

pub struct TMessageApp {
    renderer: Renderer,
    text_renderer: TextRenderer,
    window_width: u32,
    window_height: u32,
    screen_width: u32,
    screen_height: u32,

    // Launch screen state
    username_input: String,
    cursor_blink: f32,
    username_available: Option<bool>, // None = checking, Some(true) = available, Some(false) = taken

    // Input state
    mouse_x: f32,
    mouse_y: f32,
    is_dragging_resize: bool,
    resize_edge: ResizeEdge,
    drag_start_cursor_screen_pos: (f64, f64), // Global screen position when drag starts
    drag_start_size: (u32, u32),
    drag_start_window_pos: (i32, i32),
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

impl TMessageApp {
    pub async fn new(window: &Window, screen_width: u32, screen_height: u32) -> Self {
        let size = window.inner_size();
        let renderer = Renderer::new(window, size.width, size.height).await;
        let text_renderer = TextRenderer::new();

        Self {
            renderer,
            text_renderer,
            window_width: size.width,
            window_height: size.height,
            screen_width,
            screen_height,
            username_input: String::new(),
            cursor_blink: 0.0,
            username_available: None,
            mouse_x: 0.0,
            mouse_y: 0.0,
            is_dragging_resize: false,
            resize_edge: ResizeEdge::None,
            drag_start_cursor_screen_pos: (0.0, 0.0),
            drag_start_size: (0, 0),
            drag_start_window_pos: (0, 0),
        }
    }

    pub fn resize(&mut self, size: PhysicalSize<u32>) {
        self.window_width = size.width;
        self.window_height = size.height;
        self.renderer.resize(size.width, size.height);
    }

    pub fn handle_keyboard(&mut self, event: KeyEvent) {
        if event.state == ElementState::Pressed {
            match event.logical_key {
                Key::Named(NamedKey::Backspace) => {
                    self.username_input.pop();
                    self.username_available = None; // Reset availability check
                }
                Key::Named(NamedKey::Enter) => {
                    if !self.username_input.is_empty() {
                        self.submit_username();
                    }
                }
                Key::Character(ref c) => {
                    // Only allow alphanumeric and basic chars for username
                    if c.chars()
                        .all(|ch| ch.is_alphanumeric() || ch == '_' || ch == '-')
                    {
                        self.username_input.push_str(c);
                        self.username_available = None; // Reset availability check
                    }
                }
                _ => {}
            }
        }
    }

    pub fn handle_mouse_click(&mut self, window: &Window, state: ElementState, _button: MouseButton) {
        match state {
            ElementState::Pressed => {
                let edge = self.get_resize_edge(self.mouse_x, self.mouse_y);
                if edge != ResizeEdge::None {
                    self.is_dragging_resize = true;
                    self.resize_edge = edge;
                    self.drag_start_size = (self.window_width, self.window_height);

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
                self.resize_edge = ResizeEdge::None;
            }
        }
    }

    pub fn handle_mouse_move(
        &mut self,
        window: &Window,
        position: winit::dpi::PhysicalPosition<f64>,
    ) {
        self.mouse_x = position.x as f32;
        self.mouse_y = position.y as f32;

        // Handle resize dragging
        if self.is_dragging_resize {
            // Get current global cursor position
            if let Some(window_pos) = window.outer_position().ok() {
                let current_cursor_screen_x = window_pos.x as f64 + self.mouse_x as f64;
                let current_cursor_screen_y = window_pos.y as f64 + self.mouse_y as f64;

                // Calculate delta in global screen space
                let dx = (current_cursor_screen_x - self.drag_start_cursor_screen_pos.0) as f32;
                let dy = (current_cursor_screen_y - self.drag_start_cursor_screen_pos.1) as f32;

            // Minimum window dimension: 1/32 of smallest screen dimension
            let min_screen_dim = self.screen_width.min(self.screen_height) as f32;
            let min_size = (min_screen_dim / 32.0).ceil();

            let (new_width, new_height, should_move, new_x, new_y) = match self.resize_edge {
                ResizeEdge::Right => {
                    let w = ((self.drag_start_size.0 as f32 + dx).max(min_size) as u32).max(min_size as u32);
                    let h = self.drag_start_size.1.max(min_size as u32);
                    (w, h, false, 0, 0)
                }
                ResizeEdge::Left => {
                    let w = ((self.drag_start_size.0 as f32 - dx).max(min_size) as u32).max(min_size as u32);
                    let h = self.drag_start_size.1.max(min_size as u32);
                    let width_change = self.drag_start_size.0 as i32 - w as i32;
                    let new_x = self.drag_start_window_pos.0 + width_change;
                    (w, h, true, new_x, self.drag_start_window_pos.1)
                }
                ResizeEdge::Bottom => {
                    let w = self.drag_start_size.0.max(min_size as u32);
                    let h = ((self.drag_start_size.1 as f32 + dy).max(min_size) as u32).max(min_size as u32);
                    (w, h, false, 0, 0)
                }
                ResizeEdge::Top => {
                    let w = self.drag_start_size.0.max(min_size as u32);
                    let h = ((self.drag_start_size.1 as f32 - dy).max(min_size) as u32).max(min_size as u32);
                    let height_change = self.drag_start_size.1 as i32 - h as i32;
                    let new_y = self.drag_start_window_pos.1 + height_change;
                    (w, h, true, self.drag_start_window_pos.0, new_y)
                }
                ResizeEdge::TopRight => {
                    let w = ((self.drag_start_size.0 as f32 + dx).max(min_size) as u32).max(min_size as u32);
                    let h = ((self.drag_start_size.1 as f32 - dy).max(min_size) as u32).max(min_size as u32);
                    let height_change = self.drag_start_size.1 as i32 - h as i32;
                    let new_y = self.drag_start_window_pos.1 + height_change;
                    (w, h, true, self.drag_start_window_pos.0, new_y)
                }
                ResizeEdge::TopLeft => {
                    let w = ((self.drag_start_size.0 as f32 - dx).max(min_size) as u32).max(min_size as u32);
                    let h = ((self.drag_start_size.1 as f32 - dy).max(min_size) as u32).max(min_size as u32);
                    let width_change = self.drag_start_size.0 as i32 - w as i32;
                    let height_change = self.drag_start_size.1 as i32 - h as i32;
                    let new_x = self.drag_start_window_pos.0 + width_change;
                    let new_y = self.drag_start_window_pos.1 + height_change;
                    (w, h, true, new_x, new_y)
                }
                ResizeEdge::BottomRight => {
                    let w = ((self.drag_start_size.0 as f32 + dx).max(min_size) as u32).max(min_size as u32);
                    let h = ((self.drag_start_size.1 as f32 + dy).max(min_size) as u32).max(min_size as u32);
                    (w, h, false, 0, 0)
                }
                ResizeEdge::BottomLeft => {
                    let w = ((self.drag_start_size.0 as f32 - dx).max(min_size) as u32).max(min_size as u32);
                    let h = ((self.drag_start_size.1 as f32 + dy).max(min_size) as u32).max(min_size as u32);
                    let width_change = self.drag_start_size.0 as i32 - w as i32;
                    let new_x = self.drag_start_window_pos.0 + width_change;
                    (w, h, true, new_x, self.drag_start_window_pos.1)
                }
                _ => (self.drag_start_size.0, self.drag_start_size.1, false, 0, 0),
            };

            // Move window if resizing from left/top
            if should_move {
                let _ = window.set_outer_position(winit::dpi::PhysicalPosition::new(new_x, new_y));
            }

            let _ = window
                .request_inner_size(winit::dpi::PhysicalSize::new(new_width, new_height));
            }
        } else {
            // Update cursor icon based on hover position
            let edge = self.get_resize_edge(self.mouse_x, self.mouse_y);
            let cursor = match edge {
                ResizeEdge::None => CursorIcon::Default,
                ResizeEdge::Top | ResizeEdge::Bottom => CursorIcon::NsResize,
                ResizeEdge::Left | ResizeEdge::Right => CursorIcon::EwResize,
                ResizeEdge::TopLeft | ResizeEdge::BottomRight => CursorIcon::NwseResize,
                ResizeEdge::TopRight | ResizeEdge::BottomLeft => CursorIcon::NeswResize,
            };
            window.set_cursor(cursor);
        }
    }

    fn get_resize_edge(&self, x: f32, y: f32) -> ResizeEdge {
        let resize_border = ((self.window_width.min(self.window_height) as f32) / 32.0).ceil();

        let at_left = x < resize_border;
        let at_right = x > (self.window_width as f32 - resize_border);
        let at_top = y < resize_border;
        let at_bottom = y > (self.window_height as f32 - resize_border);

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
        // TODO: Query DHT for username availability
        log::info!("Submitting username: {}", self.username_input);

        // Placeholder - simulate DHT query
        self.username_available = Some(true); // For now, always available
    }

    pub fn render(&mut self) {
        self.cursor_blink += 0.1;

        // Get dimensions
        let width = self.window_width;
        let height = self.window_height;

        // Get pixel buffer from renderer
        let pixels = self.renderer.get_pixel_buffer_mut();

        // Clear to white (testing overflow)
        for pixel in pixels.chunks_exact_mut(4) {
            pixel[0] = 255; // White RGB
            pixel[1] = 255;
            pixel[2] = 255;
            pixel[3] = 0; // Fully transparent
        }

        // Draw launch screen (extract data before borrowing pixels)
        let username_input = self.username_input.clone();
        let username_available = self.username_available;
        let cursor_blink = self.cursor_blink;

        Self::draw_launch_screen_static(
            pixels,
            width,
            height,
            &username_input,
            username_available,
            cursor_blink,
        );

        // Present to screen
        self.renderer.present();
    }

    fn draw_launch_screen_static(
        pixels: &mut [u8],
        width: u32,
        height: u32,
        username_input: &str,
        username_available: Option<bool>,
        cursor_blink: f32,
    ) {
        let width = width as usize;
        let height = height as usize;
        // Draw rounded corners - scale with smaller dimension to keep proportional
        let smaller_dim = width.min(height) as f32;
        let corner_radius = (smaller_dim / 2.0).min(width as f32 / 2.0);
        let bg_r = 18;
        let bg_g = 18;
        let bg_b = 24;

        for y in 0..height {
            for x in 0..width {
                let left_dist = x as f32;
                let right_dist = (width - x - 1) as f32;
                let top_dist = y as f32;
                let bottom_dist = (height - y - 1) as f32;

                let min_x_dist = left_dist.min(right_dist);
                let min_y_dist = top_dist.min(bottom_dist);

                // Check if in corner region
                let in_corner = min_x_dist < corner_radius && min_y_dist < corner_radius;
                let in_pill = if in_corner {
                    let cx = if left_dist < corner_radius {
                        corner_radius - left_dist
                    } else {
                        corner_radius - right_dist
                    };
                    let cy = if top_dist < corner_radius {
                        corner_radius - top_dist
                    } else {
                        corner_radius - bottom_dist
                    };
                    let corner_dist = (cx * cx + cy * cy).sqrt();
                    corner_dist < corner_radius
                } else {
                    true // Not in corner, so inside pill
                };

                let idx = (y * width + x) * 4;
                if in_pill {
                    pixels[idx] = bg_r;
                    pixels[idx + 1] = bg_g;
                    pixels[idx + 2] = bg_b;
                    pixels[idx + 3] = 255;
                } // else stays transparent
            }
        }

        // Draw "tmessage" title at top (commented out - no text rendering for now)
        // let title_y = height / 8;
        // self.text_renderer.draw_text_center(...);

        // Draw username input box
        let input_y = height / 2;
        let input_width = (width as f32 * 0.6) as usize;
        let input_x = (width - input_width) / 2;
        let input_height = ((height as f32) / 8.0).max(40.0) as usize; // Scale with window, min 40px

        Self::draw_input_box_static(
            pixels,
            width,
            height,
            input_x,
            input_y,
            input_width,
            input_height,
        );

        // Draw username text inside box (commented out - no text rendering for now)
        // if !username_input.is_empty() {
        //     self.text_renderer.draw_text_left(...);
        // } else {
        //     self.text_renderer.draw_text_left(...);
        // }

        // Draw blinking cursor
        if !username_input.is_empty() && (cursor_blink % 1.0) < 0.5 {
            let char_width = (width as f32 / 30.0).max(10.0) as usize; // Scale character width
            let cursor_padding = (input_height as f32 / 4.0).max(5.0) as usize; // Scale padding
            let cursor_x = input_x + cursor_padding + username_input.len() * char_width;
            let cursor_y_start = input_y + cursor_padding;
            let cursor_y_end = input_y + input_height - cursor_padding;

            for y in cursor_y_start..cursor_y_end {
                let idx = (y * width + cursor_x) * 4;
                if idx + 3 < pixels.len() {
                    pixels[idx] = 255;
                    pixels[idx + 1] = 255;
                    pixels[idx + 2] = 255;
                    pixels[idx + 3] = 255;
                }
            }
        }

        // Draw availability indicator (commented out - no text rendering for now)
        // if let Some(available) = username_available {
        //     self.text_renderer.draw_text_center(...);
        // } else if !username_input.is_empty() {
        //     self.text_renderer.draw_text_center(...);
        // }
    }

    fn draw_input_box_static(
        pixels: &mut [u8],
        width: usize,
        height: usize,
        x: usize,
        y: usize,
        box_width: usize,
        box_height: usize,
    ) {
        // Draw pill shape - fully rounded ends (radius = height/2)
        let border_thickness = ((box_height as f32) / 30.0).max(1.0) as usize; // Scale border with box
        let corner_radius = (box_height as f32) / 2.0; // Pill: radius = half the height

        for dy in 0..box_height {
            for dx in 0..box_width {
                let px = x + dx;
                let py = y + dy;

                if px >= width || py >= height {
                    continue;
                }

                // Calculate distance from edges for rounded corners
                let left_dist = dx as f32;
                let right_dist = (box_width - dx - 1) as f32;
                let top_dist = dy as f32;
                let bottom_dist = (box_height - dy - 1) as f32;

                let min_x_dist = left_dist.min(right_dist);
                let min_y_dist = top_dist.min(bottom_dist);

                // Rounded corner check
                let in_corner = min_x_dist < corner_radius && min_y_dist < corner_radius;
                let corner_dist = if in_corner {
                    let cx = if left_dist < corner_radius {
                        corner_radius - left_dist
                    } else {
                        corner_radius - right_dist
                    };
                    let cy = if top_dist < corner_radius {
                        corner_radius - top_dist
                    } else {
                        corner_radius - bottom_dist
                    };
                    (cx * cx + cy * cy).sqrt()
                } else {
                    0.0
                };

                let is_border =
                    min_x_dist < border_thickness as f32 || min_y_dist < border_thickness as f32;
                let in_corner_radius = !in_corner || corner_dist < corner_radius;

                if !in_corner_radius {
                    continue; // Outside rounded corner
                }

                let idx = (py * width + px) * 4;
                if idx + 3 >= pixels.len() {
                    continue;
                }

                if is_border {
                    // Border - gradient blue
                    let gradient = (dx as f32 / box_width as f32) * 0.5 + 0.5;
                    pixels[idx] = (60.0 * gradient) as u8;
                    pixels[idx + 1] = (100.0 * gradient) as u8;
                    pixels[idx + 2] = (200.0 * gradient) as u8;
                    pixels[idx + 3] = 255;
                } else {
                    // Interior - dark
                    pixels[idx] = 30;
                    pixels[idx + 1] = 30;
                    pixels[idx + 2] = 40;
                    pixels[idx + 3] = 255;
                }
            }
        }
    }
}
