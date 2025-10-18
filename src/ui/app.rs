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

        // Clear hover state on resize since button positions/sizes change
        self.hovered_button = HoveredButton::None;

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

            Self::draw_logo(pixels, self.window_width, self.window_height);

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

    fn draw_logo(pixels: &mut [u8], window_width: u32, window_height: u32) {
        let window_width = window_width as usize;
        let window_height = window_height as usize;
        let smaller_dim = window_width.min(window_height) as f32;

        // Size the logo relative to window dimensions
        let logo_width = (smaller_dim / 2.) as usize;
        let logo_height = (smaller_dim / 5.) as usize;

        // Position at top center
        let x_start = (window_width - logo_width) / 2;

        // Draw horizontal spectrum rainbow
        for y in 0..logo_height * 2 {
            for x in 0..logo_width {
                let x_norm = x as f32 / logo_width as f32;
                let amplitude = logo_height as f32 / (1. + 12. * x_norm);

                let wave_phase = (logo_width as f32 / (x + logo_width / 2) as f32) * 55.;
                let wave_offset = wave_phase.sin() * amplitude;

                let mut scale = (y as f32 + wave_offset - logo_height as f32) / logo_height as f32;
                scale = ((logo_height * 2 - y) as f32 / logo_height as f32) * 12000.
                    / (scale.abs() + amplitude / smaller_dim * 0.25);
                let px = x_start + x;

                // Map x position to wavelength index (0-480)
                let wavelength_idx = (x * 480) / logo_width;
                let lms_idx = wavelength_idx * 3;

                // Extract L, M, S from LMS2006SO array
                let l = LMS2006SO[lms_idx];
                let m = LMS2006SO[lms_idx + 1];
                let s = LMS2006SO[lms_idx + 2];

                // Convert LMS to REC2020 RGB using matrix multiplication
                // LMS2REC2020 matrix (row-major):
                // [ 3.168241,  -2.156883,   0.096457]
                // [-0.266363,   1.404946,  -0.175555]
                // [ 0.003892,  -0.020568,   0.945833]
                let r =
                    3.168241098811690000 * l + -2.156882856491830000 * m + 0.096456879211209600 * s;
                let g = -0.266362510245695000 * l
                    + 1.404945732577530000 * m
                    + -0.175554801656117000 * s;
                let b =
                    0.003891529873740330 * l + -0.020567680031394800 * m + 0.945832607950864000 * s;

                // Write pixel
                let idx = (y * window_width + px) * 4;
                let r_b = pixels[idx] as f32 * pixels[idx] as f32;
                let g_b = pixels[idx + 1] as f32 * pixels[idx + 1] as f32;
                let b_b = pixels[idx + 2] as f32 * pixels[idx + 2] as f32;
                pixels[idx] = (r * scale + r_b).sqrt() as u8;
                pixels[idx + 1] = (g * scale + g_b).sqrt() as u8;
                pixels[idx + 2] = (b * scale + b_b).sqrt() as u8;
            }
        }
    }
}

// Color conversion matrices (commented out - using inline calculations instead)
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
