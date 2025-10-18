use super::renderer::Renderer;
use super::text::TextRenderer;
use winit::{
    dpi::PhysicalSize,
    event::{ElementState, KeyEvent, MouseButton},
    keyboard::{Key, NamedKey},
    window::{CursorIcon, Window},
};

pub struct PhotonApp {
    renderer: Renderer,
    #[allow(dead_code)] // Will be used for drawing text/logos soon
    text_renderer: TextRenderer,
    window_width: u32,
    window_height: u32,
    screen_width: u32,
    screen_height: u32,
    needs_redraw: bool, // True when dimensions change or content updates

    // Launch screen state
    username_input: String,
    cursor_blink: f32,
    username_available: Option<bool>, // None = checking, Some(true) = available, Some(false) = taken

    // Input state
    mouse_x: f32,
    mouse_y: f32,
    is_dragging_resize: bool,
    is_dragging_move: bool,
    resize_edge: ResizeEdge,
    drag_start_cursor_screen_pos: (f64, f64), // Global screen position when drag starts
    drag_start_size: (u32, u32),
    drag_start_window_pos: (i32, i32),

    // Window control buttons
    close_button_bounds: (f32, f32, f32, f32), // (x, y, width, height)
    maximize_button_bounds: (f32, f32, f32, f32),
    minimize_button_bounds: (f32, f32, f32, f32),
    hovered_button: HoveredButton,

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
}

// Hit test element IDs
const HIT_NONE: u8 = 0;
const HIT_MINIMIZE_BUTTON: u8 = 1;
const HIT_MAXIMIZE_BUTTON: u8 = 2;
const HIT_CLOSE_BUTTON: u8 = 3;
const HIT_DEBUG_INIT: u8 = 255; // For visualizing what areas get reset

// Button hover color deltas (applied on hover, negated on unhover)
const CLOSE_HOVER: (i8, i8, i8) = (33, -3, -7); // Red
const MAXIMIZE_HOVER: (i8, i8, i8) = (-6, 16, -6); // Green
const MINIMIZE_HOVER: (i8, i8, i8) = (-9, -6, 37); // Blue

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
    pub async fn new(window: &Window, screen_width: u32, screen_height: u32) -> Self {
        let size = window.inner_size();
        let renderer = Renderer::new(window, size.width, size.height).await;
        let text_renderer = TextRenderer::new();

        let mut app = Self {
            renderer,
            text_renderer,
            window_width: size.width,
            window_height: size.height,
            screen_width,
            screen_height,
            needs_redraw: true,
            username_input: String::new(),
            cursor_blink: 0.0,
            username_available: None,
            mouse_x: 0.0,
            mouse_y: 0.0,
            is_dragging_resize: false,
            is_dragging_move: false,
            resize_edge: ResizeEdge::None,
            drag_start_cursor_screen_pos: (0.0, 0.0),
            drag_start_size: (0, 0),
            drag_start_window_pos: (0, 0),
            close_button_bounds: (0.0, 0.0, 0.0, 0.0),
            maximize_button_bounds: (0.0, 0.0, 0.0, 0.0),
            minimize_button_bounds: (0.0, 0.0, 0.0, 0.0),
            hovered_button: HoveredButton::None,
            button_x_start: 0,
            button_height: 0,
            button_curve_start: 0,
            button_crossings: Vec::new(),
            minimize_pixels: Vec::new(),
            maximize_pixels: Vec::new(),
            close_pixels: Vec::new(),
            hit_test_map: vec![0; (size.width * size.height) as usize],
            debug_hit_test: false,
        };
        app.update_button_bounds();
        app
    }

    #[cfg(target_os = "windows")]
    pub fn new(window: &Window, screen_width: u32, screen_height: u32) -> Self {
        let size = window.inner_size();
        let renderer = Renderer::new(window, size.width, size.height);
        let text_renderer = TextRenderer::new();

        let mut app = Self {
            renderer,
            text_renderer,
            window_width: size.width,
            window_height: size.height,
            screen_width,
            screen_height,
            needs_redraw: true, // Initial draw needed
            username_input: String::new(),
            cursor_blink: 0.0,
            username_available: None,
            mouse_x: 0.0,
            mouse_y: 0.0,
            is_dragging_resize: false,
            is_dragging_move: false,
            resize_edge: ResizeEdge::None,
            drag_start_cursor_screen_pos: (0.0, 0.0),
            drag_start_size: (0, 0),
            drag_start_window_pos: (0, 0),
            close_button_bounds: (0.0, 0.0, 0.0, 0.0),
            maximize_button_bounds: (0.0, 0.0, 0.0, 0.0),
            minimize_button_bounds: (0.0, 0.0, 0.0, 0.0),
            hovered_button: HoveredButton::None,
            button_x_start: 0,
            button_height: 0,
            button_curve_start: 0,
            button_crossings: Vec::new(),
            minimize_pixels: Vec::new(),
            maximize_pixels: Vec::new(),
            close_pixels: Vec::new(),
            hit_test_map: vec![0; (size.width * size.height) as usize],
            debug_hit_test: false,
        };
        app.update_button_bounds();
        app
    }

    pub fn resize(&mut self, size: PhysicalSize<u32>) {
        self.window_width = size.width;
        self.window_height = size.height;
        self.renderer.resize(size.width, size.height);
        self.update_button_bounds();
        self.hit_test_map
            .resize((size.width * size.height) as usize, 0);
        self.needs_redraw = true; // Dimensions changed, need to redraw
    }

    fn update_button_bounds(&mut self) {
        // Button area: 3x wider than tall, extends to top-right corner of window
        // Height = 1/32 of smaller dimension (same as resize border)
        let smaller_dim = self.window_width.min(self.window_height) as f32;
        let button_height = (smaller_dim / 32.0).ceil();
        let button_width = button_height; // Each button is square
        let total_width = button_width * 3.0;

        // Buttons extend to the very top-right corner (0,0 is top-left)
        // Bottom-left corner at (window_width - total_width, button_height)
        let x_start = self.window_width as f32 - total_width;
        let y_start = 0.0;

        // Three buttons: minimize, maximize, close (left to right)
        self.minimize_button_bounds = (x_start, y_start, button_width, button_height);
        self.maximize_button_bounds =
            (x_start + button_width, y_start, button_width, button_height);
        self.close_button_bounds = (
            x_start + button_width * 2.0,
            y_start,
            button_width,
            button_height,
        );
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
                    // Toggle hit test debug visualization with 'h' key
                    if c == "h" || c == "H" {
                        self.debug_hit_test = !self.debug_hit_test;
                        self.needs_redraw = true;
                    } else if c
                        .chars()
                        .all(|ch| ch.is_alphanumeric() || ch == '_' || ch == '-')
                    {
                        // Only allow alphanumeric and basic chars for username
                        self.username_input.push_str(c);
                        self.username_available = None; // Reset availability check
                    }
                }
                _ => {}
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

                if mouse_x < self.window_width as usize && mouse_y < self.window_height as usize {
                    let hit_idx = mouse_y * self.window_width as usize + mouse_x;
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
                        _ => {}
                    }
                }

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

    pub fn handle_mouse_move(
        &mut self,
        window: &Window,
        position: winit::dpi::PhysicalPosition<f64>,
    ) {
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
        } else {
            // Check button hover state using hitmap
            let old_hovered = self.hovered_button;

            // Get hit test value at mouse position
            let mouse_x = self.mouse_x as usize;
            let mouse_y = self.mouse_y as usize;
            if mouse_x < self.window_width as usize && mouse_y < self.window_height as usize {
                let hit_idx = mouse_y * self.window_width as usize + mouse_x;
                let element_id = self.hit_test_map[hit_idx];

                self.hovered_button = match element_id {
                    HIT_CLOSE_BUTTON => HoveredButton::Close,
                    HIT_MAXIMIZE_BUTTON => HoveredButton::Maximize,
                    HIT_MINIMIZE_BUTTON => HoveredButton::Minimize,
                    _ => HoveredButton::None,
                };
            } else {
                self.hovered_button = HoveredButton::None;
            }

            // Apply or remove hover effect when state changes
            if old_hovered != self.hovered_button {
                let pixels = self.renderer.get_pixel_buffer_mut();

                // Unhover old button
                match old_hovered {
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
            }

            // Update cursor icon based on hover position
            // If hovering over a button, show pointer cursor, otherwise check for resize edges
            let cursor = if self.hovered_button != HoveredButton::None {
                CursorIcon::Pointer
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
        }
    }

    fn is_point_in_button(&self, x: f32, y: f32, bounds: (f32, f32, f32, f32)) -> bool {
        let (bx, by, bw, bh) = bounds;
        x >= bx && x < bx + bw && y >= by && y < by + bh
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

        // Only redraw content if dimensions changed or content is dirty
        if self.needs_redraw {
            // Initialize hit test map with debug value to visualize what gets reset
            self.hit_test_map.fill(HIT_DEBUG_INIT);

            let pixels = self.renderer.get_pixel_buffer_mut();

            Self::draw_background_texture(pixels, self.window_width, self.window_height);

            let (start, edges, button_x_start, button_height) = Self::draw_window_controls(
                pixels,
                &mut self.hit_test_map,
                self.window_width,
                self.window_height,
            );

            // Cache button rendering data for hover effects
            self.button_x_start = button_x_start;
            self.button_height = button_height;
            self.button_curve_start = start;
            self.button_crossings = edges.clone();

            Self::draw_window_edges_and_mask(
                pixels,
                &mut self.hit_test_map,
                self.window_width,
                self.window_height,
                start,
                &edges,
            );

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
                self.window_width,
                self.window_height,
                button_x_start,
                button_height,
                start,
                &edges,
            );

            // 2. TODO: Draw input boxes

            // 3. TODO: Draw text

            self.needs_redraw = false;
        }

        // Debug: visualize hit test map on R/B channels
        if self.debug_hit_test {
            let pixels = self.renderer.get_pixel_buffer_mut();
            for y in 0..self.window_height as usize {
                for x in 0..self.window_width as usize {
                    let hit_idx = y * self.window_width as usize + x;
                    let pixel_idx = hit_idx * 4;
                    let element_id = self.hit_test_map[hit_idx];

                    if element_id != HIT_NONE {
                        // Map element ID to color (R and B channels)
                        pixels[pixel_idx] = (element_id * 80) % 255; // Red
                        pixels[pixel_idx + 2] = (element_id * 120) % 255; // Blue
                                                                          // Keep green channel as-is so we can still see the UI
                    }
                }
            }
        }

        self.renderer.present();
    }

    fn draw_window_controls(
        pixels: &mut [u8],
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

        let edge_colour = (50, 50, 50);
        let bg_r = 30u8;
        let bg_g = 30u8;
        let bg_b = 30u8;

        // Left edge (vertical) - draw light hairline following squircle curve
        let mut y_offset = start;
        for (inset, l, h) in &crossings {
            if y_offset >= button_height {
                break;
            }
            let py = y_start + button_height - 1 - y_offset;

            // Fill grey to the right of the curve and populate hit test map
            let col_end = total_width.min(window_width - x_start);
            for col in (*inset as usize + 2)..col_end {
                let px = x_start + col;
                let idx = (py * window_width + px) * 4;
                let hit_idx = py * window_width + px;

                pixels[idx] = bg_r;
                pixels[idx + 1] = bg_g;
                pixels[idx + 2] = bg_b;

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
                hit_test_map[hit_idx] = button_id;
            }

            // Outer edge pixel (blend hairline with background texture behind)
            let px = x_start + *inset as usize;
            if px < window_width {
                let idx = (py * window_width + px) * 4;

                let mut existing = pixels[idx] as u64
                    | (pixels[idx + 1] as u64) << 16
                    | (pixels[idx + 2] as u64) << 32;
                let mut light = edge_colour.0 as u64
                    | (edge_colour.1 as u64) << 16
                    | (edge_colour.2 as u64) << 32;
                existing *= *l as u64;
                light *= *h as u64;
                let combined = existing + light;
                pixels[idx] = (combined >> 8) as u8;
                pixels[idx + 1] = (combined >> 24) as u8;
                pixels[idx + 2] = (combined >> 40) as u8;
            }

            // Inner edge pixel (blend hairline with grey button background)
            let px = x_start + *inset as usize + 1;
            if px < window_width {
                let idx = (py * window_width + px) * 4;
                let hit_idx = py * window_width + px;

                let bg = bg_r as u64 | (bg_g as u64) << 16 | (bg_b as u64) << 32;
                let light = edge_colour.0 as u64
                    | (edge_colour.1 as u64) << 16
                    | (edge_colour.2 as u64) << 32;
                let combined = bg * *h as u64 + light * *l as u64;
                pixels[idx] = (combined >> 8) as u8;
                pixels[idx + 1] = (combined >> 24) as u8;
                pixels[idx + 2] = (combined >> 40) as u8;

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
                hit_test_map[hit_idx] = button_id;
            }

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
            let idx = (py * window_width + px) * 4;
            let mut existing = pixels[idx] as u64
                | (pixels[idx + 1] as u64) << 16
                | (pixels[idx + 2] as u64) << 32;
            let mut light =
                edge_colour.0 as u64 | (edge_colour.1 as u64) << 16 | (edge_colour.2 as u64) << 32;
            existing *= l as u64;
            light *= h as u64;
            let combined = existing + light;
            pixels[idx] = (combined >> 8) as u8;
            pixels[idx + 1] = (combined >> 24) as u8;
            pixels[idx + 2] = (combined >> 40) as u8;

            // Fill grey above the curve (towards center of buttons) and populate hit test
            for row in (i + 2)..start {
                let py = y_start + button_height - 1 - row;
                let idx = (py * window_width + px) * 4;
                let hit_idx = py * window_width + px;

                pixels[idx] = bg_r;
                pixels[idx + 1] = bg_g;
                pixels[idx + 2] = bg_b;

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
                hit_test_map[hit_idx] = button_id;
            }

            let py = y_start + button_height - 1 - (i + 1);
            let idx = (py * window_width + px) * 4;
            let hit_idx = py * window_width + px;
            let bg = bg_r as u64 | (bg_g as u64) << 16 | (bg_b as u64) << 32;
            let light =
                edge_colour.0 as u64 | (edge_colour.1 as u64) << 16 | (edge_colour.2 as u64) << 32;
            let combined = bg * h as u64 + light * l as u64;
            pixels[idx] = (combined >> 8) as u8;
            pixels[idx + 1] = (combined >> 24) as u8;
            pixels[idx + 2] = (combined >> 40) as u8;

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
            hit_test_map[hit_idx] = button_id;

            x_offset += 1;
        }

        x_start += button_width / 4;

        // Draw button symbols
        Self::draw_minimize_symbol(
            pixels,
            window_width,
            x_start + button_width / 2,
            y_start + button_width / 2,
            button_width / 4,
            (200, 200, 165), // stroke_color (warm white, clearance for blue +90)
        );
        Self::draw_maximize_symbol(
            pixels,
            window_width,
            x_start + button_width + button_width / 2,
            y_start + button_width / 2,
            button_width / 4,
            (200, 200, 165), // stroke_color (warm white, clearance for blue +90)
            (60, 60, 60),    // fill_color (dark grey)
        );
        Self::draw_close_symbol(
            pixels,
            window_width,
            window_height,
            x_start + button_width * 2,
            y_start,
            button_width,
            button_height,
        );
        (start, crossings, x_start, button_height)
    }

    fn draw_minimize_symbol(
        pixels: &mut [u8],
        width: usize,
        x: usize,
        y: usize,
        r: usize,
        stroke_color: (u8, u8, u8),
    ) {
        let r_render = r / 4 + 1;
        let r_2 = r_render * r_render;
        let r_4 = r_2 * r_2;
        let r_3 = r_render * r_render * r_render;

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
                    let idx = (py * width + px) * 4;
                    let gradient = ((r_4 - dist_4) << 8) / (r_3 << 2);
                    if gradient > 255 {
                        pixels[idx] = stroke_color.0;
                        pixels[idx + 1] = stroke_color.1;
                        pixels[idx + 2] = stroke_color.2;
                    } else {
                        // Blend background towards stroke_color
                        let bg_r = pixels[idx] as u64;
                        let bg_g = pixels[idx + 1] as u64;
                        let bg_b = pixels[idx + 2] as u64;
                        let stroke_r = stroke_color.0 as u64;
                        let stroke_g = stroke_color.1 as u64;
                        let stroke_b = stroke_color.2 as u64;
                        let alpha = gradient as u64;
                        let inv_alpha = 256 - alpha;

                        pixels[idx] = ((bg_r * inv_alpha + stroke_r * alpha) >> 8) as u8;
                        pixels[idx + 1] = ((bg_g * inv_alpha + stroke_g * alpha) >> 8) as u8;
                        pixels[idx + 2] = ((bg_b * inv_alpha + stroke_b * alpha) >> 8) as u8;
                    }
                }
            }
        }
    }

    fn draw_maximize_symbol(
        pixels: &mut [u8],
        width: usize,
        x: usize,
        y: usize,
        r: usize,
        stroke_color: (u8, u8, u8),
        fill_color: (u8, u8, u8),
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
                    let idx = (py * width + px) * 4;

                    // Determine which zone we're in
                    let dist_from_outer = r_4 - dist_4;

                    if dist_4 <= r_inner_4 {
                        let dist_from_inner = r_inner_4 - dist_4;

                        // Inside inner squircle
                        if dist_from_inner <= inner_edge_threshold {
                            // Inner edge: blend from stroke to fill
                            let gradient = ((dist_from_inner) << 8) / inner_edge_threshold;
                            let alpha = gradient as u64;
                            let inv_alpha = 256 - alpha;

                            let stroke_r = stroke_color.0 as u64;
                            let stroke_g = stroke_color.1 as u64;
                            let stroke_b = stroke_color.2 as u64;
                            let fill_r = fill_color.0 as u64;
                            let fill_g = fill_color.1 as u64;
                            let fill_b = fill_color.2 as u64;

                            pixels[idx] = ((stroke_r * inv_alpha + fill_r * alpha) >> 8) as u8;
                            pixels[idx + 1] = ((stroke_g * inv_alpha + fill_g * alpha) >> 8) as u8;
                            pixels[idx + 2] = ((stroke_b * inv_alpha + fill_b * alpha) >> 8) as u8;
                        } else {
                            // Solid fill center
                            pixels[idx] = fill_color.0;
                            pixels[idx + 1] = fill_color.1;
                            pixels[idx + 2] = fill_color.2;
                        }
                    } else {
                        // Between inner and outer: stroke ring
                        if dist_from_outer <= outer_edge_threshold {
                            // Outer edge: blend from background to stroke
                            let gradient = ((dist_from_outer) << 8) / outer_edge_threshold;
                            let bg_r = pixels[idx] as u64;
                            let bg_g = pixels[idx + 1] as u64;
                            let bg_b = pixels[idx + 2] as u64;
                            let stroke_r = stroke_color.0 as u64;
                            let stroke_g = stroke_color.1 as u64;
                            let stroke_b = stroke_color.2 as u64;
                            let alpha = gradient as u64;
                            let inv_alpha = 256 - alpha;

                            pixels[idx] = ((bg_r * inv_alpha + stroke_r * alpha) >> 8) as u8;
                            pixels[idx + 1] = ((bg_g * inv_alpha + stroke_g * alpha) >> 8) as u8;
                            pixels[idx + 2] = ((bg_b * inv_alpha + stroke_b * alpha) >> 8) as u8;
                        } else {
                            // Solid stroke ring
                            pixels[idx] = stroke_color.0;
                            pixels[idx + 1] = stroke_color.1;
                            pixels[idx + 2] = stroke_color.2;
                        }
                    }
                }
            }
        }
    }

    fn draw_close_symbol(
        pixels: &mut [u8],
        width: usize,
        height: usize,
        x: usize,
        y: usize,
        w: usize,
        h: usize,
    ) {
        // Draw X with antialiased rounded-end diagonals (capsule/pill shaped)
        let thickness = (h / 12).max(1) as f32;
        let radius = thickness / 2.;
        let size = (w / 2) as f32;
        let cxi = (x + w / 2) as i32;
        let cyi = (y + h / 2) as i32;
        let cxf = cxi as f32;
        let cyf = cyi as f32;

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

        let color = (220u8, 100u8, 100u8);

        // Scan the bounding box and render both capsules
        let min_x = (x as i32).max(0);
        let max_x = ((x + w) as i32).min(width as i32);
        let min_y = (y as i32).max(0);
        let max_y = ((y + h) as i32).min(height as i32);

        for py in min_y..cyi {
            for px in min_x..cxi {
                let px_f = px as f32 + 0.5;
                let py_f = py as f32 + 0.5;

                let dist = Self::distance_to_capsule(
                    px_f, py_f, x1_start, y1_start, x1_end, y1_end, radius,
                );

                let alpha = if dist < -0.5 {
                    1.
                } else if dist < 0.5 {
                    0.5 - dist
                } else {
                    0.
                };

                if alpha > 0. {
                    let idx = (py as usize * width + px as usize) * 4;
                    if idx + 3 < pixels.len() {
                        let existing_r = pixels[idx] as f32;
                        let existing_g = pixels[idx + 1] as f32;
                        let existing_b = pixels[idx + 2] as f32;

                        pixels[idx] = (existing_r * (1.0 - alpha) + color.0 as f32 * alpha) as u8;
                        pixels[idx + 1] =
                            (existing_g * (1. - alpha) + color.1 as f32 * alpha) as u8;
                        pixels[idx + 2] =
                            (existing_b * (1. - alpha) + color.2 as f32 * alpha) as u8;
                        pixels[idx + 3] = 255;
                    }
                }
            }
        }

        for py in min_y..cyi {
            for px in cxi..max_x {
                let px_f = px as f32 + 0.5;
                let py_f = py as f32 + 0.5;

                let dist = Self::distance_to_capsule(
                    px_f, py_f, x2_start, y2_start, x2_end, y2_end, radius,
                );

                let alpha = if dist < -0.5 {
                    1.
                } else if dist < 0.5 {
                    0.5 - dist
                } else {
                    0.
                };

                if alpha > 0. {
                    let idx = (py as usize * width + px as usize) * 4;
                    if idx + 3 < pixels.len() {
                        let existing_r = pixels[idx] as f32;
                        let existing_g = pixels[idx + 1] as f32;
                        let existing_b = pixels[idx + 2] as f32;

                        pixels[idx] = (existing_r * (1.0 - alpha) + color.0 as f32 * alpha) as u8;
                        pixels[idx + 1] =
                            (existing_g * (1. - alpha) + color.1 as f32 * alpha) as u8;
                        pixels[idx + 2] =
                            (existing_b * (1. - alpha) + color.2 as f32 * alpha) as u8;
                        pixels[idx + 3] = 255;
                    }
                }
            }
        }

        for py in cyi..max_y {
            for px in min_x..cxi {
                let px_f = px as f32 + 0.5;
                let py_f = py as f32 + 0.5;

                let dist = Self::distance_to_capsule(
                    px_f, py_f, x2_start, y2_start, x2_end, y2_end, radius,
                );

                let alpha = if dist < -0.5 {
                    1.
                } else if dist < 0.5 {
                    0.5 - dist
                } else {
                    0.
                };

                if alpha > 0. {
                    let idx = (py as usize * width + px as usize) * 4;
                    if idx + 3 < pixels.len() {
                        let existing_r = pixels[idx] as f32;
                        let existing_g = pixels[idx + 1] as f32;
                        let existing_b = pixels[idx + 2] as f32;

                        pixels[idx] = (existing_r * (1.0 - alpha) + color.0 as f32 * alpha) as u8;
                        pixels[idx + 1] =
                            (existing_g * (1. - alpha) + color.1 as f32 * alpha) as u8;
                        pixels[idx + 2] =
                            (existing_b * (1. - alpha) + color.2 as f32 * alpha) as u8;
                        pixels[idx + 3] = 255;
                    }
                }
            }
        }

        for py in cyi..max_y {
            for px in cxi..max_x {
                let px_f = px as f32 + 0.5;
                let py_f = py as f32 + 0.5;

                let dist = Self::distance_to_capsule(
                    px_f, py_f, x1_start, y1_start, x1_end, y1_end, radius,
                );

                let alpha = if dist < -0.5 {
                    1.
                } else if dist < 0.5 {
                    0.5 - dist
                } else {
                    0.
                };

                if alpha > 0. {
                    let idx = (py as usize * width + px as usize) * 4;
                    if idx + 3 < pixels.len() {
                        let existing_r = pixels[idx] as f32;
                        let existing_g = pixels[idx + 1] as f32;
                        let existing_b = pixels[idx + 2] as f32;

                        pixels[idx] = (existing_r * (1.0 - alpha) + color.0 as f32 * alpha) as u8;
                        pixels[idx + 1] =
                            (existing_g * (1. - alpha) + color.1 as f32 * alpha) as u8;
                        pixels[idx + 2] =
                            (existing_b * (1. - alpha) + color.2 as f32 * alpha) as u8;
                        pixels[idx + 3] = 255;
                    }
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

    fn draw_launch_screen_static(
        pixels: &mut [u8],
        width: u32,
        height: u32,
        username_input: &str,
        _username_available: Option<bool>,
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
                let in_window = if in_corner {
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
                    true // Not in corner, so inside window
                };

                let idx = (y * width + x) * 4;
                if in_window {
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
        // Draw squirtle shape - fully roundamicated
        let border_thickness = ((box_height as f32) / 30.0).max(1.0) as usize; // Scale border with box
        let corner_radius = (box_height as f32) / 2.0; // Squirtle: radius = half the height

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

    /// Draw just the background texture (noisy gradient)
    fn draw_background_texture(pixels: &mut [u8], width: u32, height: u32) {
        use rayon::prelude::*;

        let bg_colour = [6u8, 8u8, 9u8];
        let width_bytes = (width * 4) as usize;

        // Skip first and last rows, process middle rows in parallel
        let middle_rows = &mut pixels[width_bytes..(height as usize - 1) * width_bytes];

        middle_rows
            .par_chunks_mut(width_bytes)
            .enumerate()
            .for_each(|(row_idx, row_pixels)| {
                let y = (row_idx + 1) as u32;
                // Reset RNG state at start of each row (consistent scanlines)
                let rng_seed: u32 =
                    0xDEADBEEF ^ ((y.wrapping_sub(height / 2)).wrapping_mul(0x9E3779B9));
                let mut rng_state = rng_seed;

                let start_colour = [
                    (rng_state % bg_colour[0] as u32) as u8,
                    ((rng_state >> 8) % bg_colour[1] as u32) as u8,
                    ((rng_state >> 16) % bg_colour[2] as u32) as u8,
                ];
                let mut fill_colour = start_colour;

                // Right half: left-to-right
                for x in width / 2..width - 1 {
                    rng_state ^= rng_state.rotate_left(5).wrapping_add(7);

                    // Extract 2 bits per channel from different parts of rng_state
                    // Values: 0, 1, 2, 3  →  we'll map to: -1, 0, +1, 0
                    let r0 = (rng_state & 0x03) as u8;
                    let r1 = ((rng_state >> 5) & 0x03) as u8;
                    let r2 = ((rng_state >> 19) & 0x03) as u8;

                    // Apply deltas: 0→-1, 2→+1, 1,3→0
                    if r0 == 0 {
                        fill_colour[0] = fill_colour[0].wrapping_sub(1);
                    } else if r0 == 2 {
                        fill_colour[0] = fill_colour[0].wrapping_add(1);
                    }

                    if r1 == 0 {
                        fill_colour[1] = fill_colour[1].wrapping_sub(1);
                    } else if r1 == 2 {
                        fill_colour[1] = fill_colour[1].wrapping_add(1);
                    }

                    if r2 == 0 {
                        fill_colour[2] = fill_colour[2].wrapping_sub(1);
                    } else if r2 == 2 {
                        fill_colour[2] = fill_colour[2].wrapping_add(1);
                    }

                    // Clamp to valid ranges
                    if (fill_colour[0] as i8).is_negative() {
                        fill_colour[0] = bg_colour[0];
                    } else if fill_colour[0] > bg_colour[0] {
                        fill_colour[0] = 0;
                    }
                    if (fill_colour[1] as i8).is_negative() {
                        fill_colour[1] = 0;
                    } else if fill_colour[1] > bg_colour[1] {
                        fill_colour[1] = bg_colour[1];
                    }
                    if (fill_colour[2] as i8).is_negative() {
                        fill_colour[2] = 0;
                    } else if fill_colour[2] > bg_colour[2] {
                        fill_colour[2] = bg_colour[2];
                    }

                    let pixel_idx = (x * 4) as usize;
                    row_pixels[pixel_idx] = fill_colour[0] + 16;
                    row_pixels[pixel_idx + 1] = fill_colour[1] + 17;
                    row_pixels[pixel_idx + 2] = fill_colour[2] + 20;
                    row_pixels[pixel_idx + 3] = 255;
                }

                // Left half: right-to-left (mirror)
                rng_state = rng_seed;
                fill_colour = start_colour;

                for x in (1..width / 2).rev() {
                    rng_state ^= rng_state.rotate_left(5).wrapping_sub(7);

                    let r0 = (rng_state & 0x03) as u8;
                    let r1 = ((rng_state >> 5) & 0x03) as u8;
                    let r2 = ((rng_state >> 19) & 0x03) as u8;

                    if r0 == 0 {
                        fill_colour[0] = fill_colour[0].wrapping_sub(1);
                    } else if r0 == 2 {
                        fill_colour[0] = fill_colour[0].wrapping_add(1);
                    }

                    if r1 == 0 {
                        fill_colour[1] = fill_colour[1].wrapping_sub(1);
                    } else if r1 == 2 {
                        fill_colour[1] = fill_colour[1].wrapping_add(1);
                    }

                    if r2 == 0 {
                        fill_colour[2] = fill_colour[2].wrapping_sub(1);
                    } else if r2 == 2 {
                        fill_colour[2] = fill_colour[2].wrapping_add(1);
                    }

                    if (fill_colour[0] as i8).is_negative() {
                        fill_colour[0] = bg_colour[0];
                    } else if fill_colour[0] > bg_colour[0] {
                        fill_colour[0] = 0;
                    }
                    if (fill_colour[1] as i8).is_negative() {
                        fill_colour[1] = 0;
                    } else if fill_colour[1] > bg_colour[1] {
                        fill_colour[1] = bg_colour[1];
                    }
                    if (fill_colour[2] as i8).is_negative() {
                        fill_colour[2] = 0;
                    } else if fill_colour[2] > bg_colour[2] {
                        fill_colour[2] = bg_colour[2];
                    }

                    let pixel_idx = (x * 4) as usize;
                    row_pixels[pixel_idx] = fill_colour[0] + 16;
                    row_pixels[pixel_idx + 1] = fill_colour[1] + 17;
                    row_pixels[pixel_idx + 2] = fill_colour[2] + 20;
                    row_pixels[pixel_idx + 3] = 255;
                }
            });
    }

    /// Draw window edge hairlines and apply squircle alpha mask
    fn draw_window_edges_and_mask(
        pixels: &mut [u8],
        hit_test_map: &mut [u8],
        width: u32,
        height: u32,
        start: usize,
        crossings: &[(u16, u8, u8)],
    ) {
        let light_colour = (75, 70, 65);
        let shadow_colour = (52, 60, 68);

        // Fill all four edges with white before squircle clipping
        // Top edge
        for x in 0..width {
            let idx = (0 * width + x) * 4;
            pixels[idx as usize] = light_colour.0;
            pixels[idx as usize + 1] = light_colour.1;
            pixels[idx as usize + 2] = light_colour.2;
            pixels[idx as usize + 3] = 255;
        }

        // Bottom edge
        for x in 0..width {
            let idx = ((height - 1) * width + x) * 4;
            pixels[idx as usize] = shadow_colour.0;
            pixels[idx as usize + 1] = shadow_colour.1;
            pixels[idx as usize + 2] = shadow_colour.2;
            pixels[idx as usize + 3] = 255;
        }

        // Left edge
        for y in 0..height {
            let idx = (y * width + 0) * 4;
            pixels[idx as usize] = light_colour.0;
            pixels[idx as usize + 1] = light_colour.1;
            pixels[idx as usize + 2] = light_colour.2;
            pixels[idx as usize + 3] = 255;
        }

        // Right edge
        for y in 0..height {
            let idx = (y * width + (width - 1)) * 4;
            pixels[idx as usize] = shadow_colour.0;
            pixels[idx as usize + 1] = shadow_colour.1;
            pixels[idx as usize + 2] = shadow_colour.2;
            pixels[idx as usize + 3] = 255;
        }

        // Fill four corner squares and clear hitmap
        for row in 0..start {
            for col in 0..start {
                let idx = (row * width as usize + col) * 4;
                let hit_idx = row * width as usize + col;
                pixels[idx] = 0;
                pixels[idx + 1] = 0;
                pixels[idx + 2] = 0;
                pixels[idx + 3] = 0;
                hit_test_map[hit_idx] = HIT_NONE;
            }
        }
        for row in 0..start {
            for col in (width as usize - start)..width as usize {
                let idx = (row * width as usize + col) * 4;
                let hit_idx = row * width as usize + col;
                pixels[idx] = 0;
                pixels[idx + 1] = 0;
                pixels[idx + 2] = 0;
                pixels[idx + 3] = 0;
                hit_test_map[hit_idx] = HIT_NONE;
            }
        }
        for row in (height as usize - start)..height as usize {
            for col in 0..start {
                let idx = (row * width as usize + col) * 4;
                let hit_idx = row * width as usize + col;
                pixels[idx] = 0;
                pixels[idx + 1] = 0;
                pixels[idx + 2] = 0;
                pixels[idx + 3] = 0;
                hit_test_map[hit_idx] = HIT_NONE;
            }
        }
        for row in (height as usize - start)..height as usize {
            for col in (width as usize - start)..width as usize {
                let idx = (row * width as usize + col) * 4;
                let hit_idx = row * width as usize + col;
                pixels[idx] = 0;
                pixels[idx + 1] = 0;
                pixels[idx + 2] = 0;
                pixels[idx + 3] = 0;
                hit_test_map[hit_idx] = HIT_NONE;
            }
        }

        // Top left/right edges
        let mut y_top = start;
        for crossing in 0..crossings.len() {
            let (inset, l, h) = crossings[crossing];
            // Left edge fill
            for idx in y_top * width as usize..y_top * width as usize + inset as usize {
                pixels[idx * 4] = 0;
                pixels[idx * 4 + 1] = 0;
                pixels[idx * 4 + 2] = 0;
                pixels[idx * 4 + 3] = 0;
                hit_test_map[idx] = HIT_NONE;
            }

            // Left edge outer pixel
            let idx = (y_top * width as usize + inset as usize) * 4;
            let hit_idx = y_top * width as usize + inset as usize;
            let light = light_colour.0 as u64
                | (light_colour.1 as u64) << 16
                | (light_colour.2 as u64) << 32;
            let result = light * h as u64;
            pixels[idx] = (result >> 8) as u8;
            pixels[idx + 1] = (result >> 24) as u8;
            pixels[idx + 2] = (result >> 40) as u8;
            pixels[idx + 3] = h;
            if h < 255 {
                hit_test_map[hit_idx] = HIT_NONE;
            }

            // Left edge inner pixel
            let idx = idx + 4;
            let existing = pixels[idx] as u64
                | (pixels[idx + 1] as u64) << 16
                | (pixels[idx + 2] as u64) << 32;
            let light = light_colour.0 as u64
                | (light_colour.1 as u64) << 16
                | (light_colour.2 as u64) << 32;
            let combined = existing * h as u64 + light * l as u64;
            pixels[idx] = (combined >> 8) as u8;
            pixels[idx + 1] = (combined >> 24) as u8;
            pixels[idx + 2] = (combined >> 40) as u8;

            // Right edge inner pixel
            let idx = (y_top * width as usize + width as usize - 2 - inset as usize) * 4;
            let existing = pixels[idx] as u64
                | (pixels[idx + 1] as u64) << 16
                | (pixels[idx + 2] as u64) << 32;
            let shadow = shadow_colour.0 as u64
                | (shadow_colour.1 as u64) << 16
                | (shadow_colour.2 as u64) << 32;
            let combined = existing * h as u64 + shadow * l as u64;
            pixels[idx] = (combined >> 8) as u8;
            pixels[idx + 1] = (combined >> 24) as u8;
            pixels[idx + 2] = (combined >> 40) as u8;

            // Right edge outer pixel
            let idx = idx + 4;
            let hit_idx = y_top * width as usize + width as usize - 1 - inset as usize;
            let light = shadow_colour.0 as u64
                | (shadow_colour.1 as u64) << 16
                | (shadow_colour.2 as u64) << 32;
            let result = light * h as u64;
            pixels[idx] = (result >> 8) as u8;
            pixels[idx + 1] = (result >> 24) as u8;
            pixels[idx + 2] = (result >> 40) as u8;
            pixels[idx + 3] = h;
            if h < 255 {
                hit_test_map[hit_idx] = HIT_NONE;
            }

            // Right edge fill
            for idx in (y_top * width as usize + width as usize - inset as usize)
                ..((y_top + 1) * width as usize)
            {
                pixels[idx * 4] = 0;
                pixels[idx * 4 + 1] = 0;
                pixels[idx * 4 + 2] = 0;
                pixels[idx * 4 + 3] = 0;
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
                pixels[idx * 4] = 0;
                pixels[idx * 4 + 1] = 0;
                pixels[idx * 4 + 2] = 0;
                pixels[idx * 4 + 3] = 0;
                hit_test_map[idx] = HIT_NONE;
            }

            // Left outer edge pixel
            let idx = (y_bottom * width as usize + inset as usize) * 4;
            let hit_idx = y_bottom * width as usize + inset as usize;
            let light = light_colour.0 as u64
                | (light_colour.1 as u64) << 16
                | (light_colour.2 as u64) << 32;
            let result = light * h as u64;
            pixels[idx] = (result >> 8) as u8;
            pixels[idx + 1] = (result >> 24) as u8;
            pixels[idx + 2] = (result >> 40) as u8;
            pixels[idx + 3] = h;
            if h < 255 {
                hit_test_map[hit_idx] = HIT_NONE;
            }

            // Left inner edge pixel
            let idx = idx + 4;
            let existing = pixels[idx] as u64
                | (pixels[idx + 1] as u64) << 16
                | (pixels[idx + 2] as u64) << 32;
            let light = light_colour.0 as u64
                | (light_colour.1 as u64) << 16
                | (light_colour.2 as u64) << 32;
            let combined = existing * h as u64 + light * l as u64;
            pixels[idx] = (combined >> 8) as u8;
            pixels[idx + 1] = (combined >> 24) as u8;
            pixels[idx + 2] = (combined >> 40) as u8;

            // Right edge inner pixel
            let idx = (y_bottom * width as usize + width as usize - 2 - inset as usize) * 4;
            let existing = pixels[idx] as u64
                | (pixels[idx + 1] as u64) << 16
                | (pixels[idx + 2] as u64) << 32;
            let shadow = shadow_colour.0 as u64
                | (shadow_colour.1 as u64) << 16
                | (shadow_colour.2 as u64) << 32;
            let combined = existing * h as u64 + shadow * l as u64;
            pixels[idx] = (combined >> 8) as u8;
            pixels[idx + 1] = (combined >> 24) as u8;
            pixels[idx + 2] = (combined >> 40) as u8;

            // Right edge outer pixel
            let idx = idx + 4;
            let hit_idx = y_bottom * width as usize + width as usize - 1 - inset as usize;
            let light = shadow_colour.0 as u64
                | (shadow_colour.1 as u64) << 16
                | (shadow_colour.2 as u64) << 32;
            let result = light * h as u64;
            pixels[idx] = (result >> 8) as u8;
            pixels[idx + 1] = (result >> 24) as u8;
            pixels[idx + 2] = (result >> 40) as u8;
            pixels[idx + 3] = h;
            if h < 255 {
                hit_test_map[hit_idx] = HIT_NONE;
            }

            // Right edge fill
            for idx in (y_bottom * width as usize + width as usize - inset as usize)
                ..((y_bottom + 1) * width as usize)
            {
                pixels[idx * 4] = 0;
                pixels[idx * 4 + 1] = 0;
                pixels[idx * 4 + 2] = 0;
                pixels[idx * 4 + 3] = 0;
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
                let idx = (row * width as usize + x_left) * 4;
                let hit_idx = row * width as usize + x_left;
                pixels[idx] = 0;
                pixels[idx + 1] = 0;
                pixels[idx + 2] = 0;
                pixels[idx + 3] = 0;
                hit_test_map[hit_idx] = HIT_NONE;
            }

            // Top outer edge pixel
            let idx = (inset as usize * width as usize + x_left) * 4;
            let hit_idx = inset as usize * width as usize + x_left;
            let light = light_colour.0 as u64
                | (light_colour.1 as u64) << 16
                | (light_colour.2 as u64) << 32;
            let result = light * h as u64;
            pixels[idx] = (result >> 8) as u8;
            pixels[idx + 1] = (result >> 24) as u8;
            pixels[idx + 2] = (result >> 40) as u8;
            pixels[idx + 3] = h;
            if h < 255 {
                hit_test_map[hit_idx] = HIT_NONE;
            }

            // Top inner edge pixel
            let idx = ((inset as usize + 1) * width as usize + x_left) * 4;
            let existing = pixels[idx] as u64
                | (pixels[idx + 1] as u64) << 16
                | (pixels[idx + 2] as u64) << 32;
            let light = light_colour.0 as u64
                | (light_colour.1 as u64) << 16
                | (light_colour.2 as u64) << 32;
            let combined = existing * h as u64 + light * l as u64;
            pixels[idx] = (combined >> 8) as u8;
            pixels[idx + 1] = (combined >> 24) as u8;
            pixels[idx + 2] = (combined >> 40) as u8;

            // Bottom outer edge pixel
            let idx = ((height as usize - 1 - inset as usize) * width as usize + x_left) * 4;
            let hit_idx = (height as usize - 1 - inset as usize) * width as usize + x_left;
            let shadow = shadow_colour.0 as u64
                | (shadow_colour.1 as u64) << 16
                | (shadow_colour.2 as u64) << 32;
            let result = shadow * h as u64;
            pixels[idx] = (result >> 8) as u8;
            pixels[idx + 1] = (result >> 24) as u8;
            pixels[idx + 2] = (result >> 40) as u8;
            pixels[idx + 3] = h;
            if h < 255 {
                hit_test_map[hit_idx] = HIT_NONE;
            }

            // Bottom inner edge pixel
            let idx = ((height as usize - 2 - inset as usize) * width as usize + x_left) * 4;
            let existing = pixels[idx] as u64
                | (pixels[idx + 1] as u64) << 16
                | (pixels[idx + 2] as u64) << 32;
            let shadow = shadow_colour.0 as u64
                | (shadow_colour.1 as u64) << 16
                | (shadow_colour.2 as u64) << 32;
            let combined = existing * h as u64 + shadow * l as u64;
            pixels[idx] = (combined >> 8) as u8;
            pixels[idx + 1] = (combined >> 24) as u8;
            pixels[idx + 2] = (combined >> 40) as u8;

            // Bottom edge fill
            for row in (height as usize - inset as usize)..height as usize {
                let idx = (row * width as usize + x_left) * 4;
                let hit_idx = row * width as usize + x_left;
                pixels[idx] = 0;
                pixels[idx + 1] = 0;
                pixels[idx + 2] = 0;
                pixels[idx + 3] = 0;
                hit_test_map[hit_idx] = HIT_NONE;
            }

            x_left += 1;
        }

        // Right side top/bottom edges
        let mut x_right = width as usize - start - 1;
        for crossing in 0..crossings.len() {
            let (inset, l, h) = crossings[crossing];

            // Top edge fill
            for row in 0..inset as usize {
                let idx = (row * width as usize + x_right) * 4;
                let hit_idx = row * width as usize + x_right;
                pixels[idx] = 0;
                pixels[idx + 1] = 0;
                pixels[idx + 2] = 0;
                pixels[idx + 3] = 0;
                hit_test_map[hit_idx] = HIT_NONE;
            }

            // Top outer edge pixel
            let idx = (inset as usize * width as usize + x_right) * 4;
            let hit_idx = inset as usize * width as usize + x_right;
            let light = light_colour.0 as u64
                | (light_colour.1 as u64) << 16
                | (light_colour.2 as u64) << 32;
            let result = light * h as u64;
            pixels[idx] = (result >> 8) as u8;
            pixels[idx + 1] = (result >> 24) as u8;
            pixels[idx + 2] = (result >> 40) as u8;
            pixels[idx + 3] = h;
            if h < 255 {
                hit_test_map[hit_idx] = HIT_NONE;
            }

            // Top inner edge pixel
            let idx = ((inset as usize + 1) * width as usize + x_right) * 4;
            let existing = pixels[idx] as u64
                | (pixels[idx + 1] as u64) << 16
                | (pixels[idx + 2] as u64) << 32;
            let light = light_colour.0 as u64
                | (light_colour.1 as u64) << 16
                | (light_colour.2 as u64) << 32;
            let combined = existing * h as u64 + light * l as u64;
            pixels[idx] = (combined >> 8) as u8;
            pixels[idx + 1] = (combined >> 24) as u8;
            pixels[idx + 2] = (combined >> 40) as u8;

            // Bottom outer edge pixel
            let idx = ((height as usize - 1 - inset as usize) * width as usize + x_right) * 4;
            let hit_idx = (height as usize - 1 - inset as usize) * width as usize + x_right;
            let shadow = shadow_colour.0 as u64
                | (shadow_colour.1 as u64) << 16
                | (shadow_colour.2 as u64) << 32;
            let result = shadow * h as u64;
            pixels[idx] = (result >> 8) as u8;
            pixels[idx + 1] = (result >> 24) as u8;
            pixels[idx + 2] = (result >> 40) as u8;
            pixels[idx + 3] = h;
            if h < 255 {
                hit_test_map[hit_idx] = HIT_NONE;
            }

            // Bottom inner edge pixel
            let idx = ((height as usize - 2 - inset as usize) * width as usize + x_right) * 4;
            let existing = pixels[idx] as u64
                | (pixels[idx + 1] as u64) << 16
                | (pixels[idx + 2] as u64) << 32;
            let shadow = shadow_colour.0 as u64
                | (shadow_colour.1 as u64) << 16
                | (shadow_colour.2 as u64) << 32;
            let combined = existing * h as u64 + shadow * l as u64;
            pixels[idx] = (combined >> 8) as u8;
            pixels[idx + 1] = (combined >> 24) as u8;
            pixels[idx + 2] = (combined >> 40) as u8;

            // Bottom edge fill
            for row in (height as usize - inset as usize)..height as usize {
                let idx = (row * width as usize + x_right) * 4;
                let hit_idx = row * width as usize + x_right;
                pixels[idx] = 0;
                pixels[idx + 1] = 0;
                pixels[idx + 2] = 0;
                pixels[idx + 3] = 0;
                hit_test_map[hit_idx] = HIT_NONE;
            }

            x_right -= 1;
        }
    }

    /// Apply hover effect to button using cached pixel list
    fn draw_button_hover_by_pixels(
        pixels: &mut [u8],
        pixel_list: &[usize],
        hover: bool,
        button_type: HoveredButton,
    ) {
        // Get the hover deltas for this button type
        let (r, g, b) = match button_type {
            HoveredButton::Close => CLOSE_HOVER,
            HoveredButton::Maximize => MAXIMIZE_HOVER,
            HoveredButton::Minimize => MINIMIZE_HOVER,
            HoveredButton::None => (0, 0, 0),
        };

        // Apply deltas (positive for hover, negative for unhover)
        let sign = if hover { 1 } else { -1 };
        let r_delta = r * sign;
        let g_delta = g * sign;
        let b_delta = b * sign;

        // Iterate only over the cached pixels for this button
        for &hit_idx in pixel_list {
            let pixel_idx = hit_idx * 4;
            if pixel_idx + 3 < pixels.len() {
                pixels[pixel_idx] += r_delta as u8;
                pixels[pixel_idx + 1] += g_delta as u8;
                pixels[pixel_idx + 2] += b_delta as u8;
            }
        }
    }

    /// Draw vertical hairlines between buttons
    fn draw_button_hairlines(
        pixels: &mut [u8],
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

        // Edge/hairline color
        let edge_r = 50u8;
        let edge_g = 50u8;
        let edge_b = 50u8;

        // Draw left hairline
        // Draw upward from center (inclusive) until we hit transparency
        for py in (y_start..=center_y).rev() {
            let idx = (py * width + left_px) * 4;
            let hit_idx = py * width + left_px;
            // Check if pixel has any transparency (alpha < 255)
            if pixels[idx + 3] < 255 {
                break;
            }
            pixels[idx] = edge_r;
            pixels[idx + 1] = edge_g;
            pixels[idx + 2] = edge_b;
            hit_test_map[hit_idx] = HIT_NONE;
        }

        // Draw downward from center until we hit a pixel that differs from button grey
        for py in (center_y + 1)..(y_start + button_height) {
            let idx = (py * width + left_px) * 4;
            let hit_idx = py * width + left_px;
            let r = pixels[idx];

            // Always draw the hairline pixel
            pixels[idx] = edge_r;
            pixels[idx + 1] = edge_g;
            pixels[idx + 2] = edge_b;
            hit_test_map[hit_idx] = HIT_NONE;

            // Check if pixel differed from button grey (30) - if so, stop after drawing it
            if r != 30 {
                break;
            }
        }

        // Draw right hairline
        // Draw upward from center (inclusive) until we hit transparency
        for py in (y_start..=center_y).rev() {
            let idx = (py * width + right_px) * 4;
            let hit_idx = py * width + right_px;
            // Check if pixel has any transparency (alpha < 255)
            if pixels[idx + 3] < 255 {
                break;
            }
            pixels[idx] = edge_r;
            pixels[idx + 1] = edge_g;
            pixels[idx + 2] = edge_b;
            hit_test_map[hit_idx] = HIT_NONE;
        }

        // Draw downward from center until we hit a pixel that differs from button grey
        for py in (center_y + 1)..(y_start + button_height) {
            let idx = (py * width + right_px) * 4;
            let hit_idx = py * width + right_px;
            let r = pixels[idx];

            // Always draw the hairline pixel
            pixels[idx] = edge_r;
            pixels[idx + 1] = edge_g;
            pixels[idx + 2] = edge_b;
            hit_test_map[hit_idx] = HIT_NONE;

            // Check if pixel differed from button grey (30) - if so, stop after drawing it
            if r != 30 {
                break;
            }
        }
    }
}
