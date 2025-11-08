use super::renderer::Renderer;
use super::text_rasterizing::TextRenderer;
use crate::network::{HandleQuery, QueryResult};
use winit::{dpi::PhysicalSize, keyboard::ModifiersState, window::Window};

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

/// Handle attestation status for launch screen flow
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HandleStatus {
    Empty,              // No query sent yet, show "Query" button (when textbox non-empty)
    Checking,           // Query in flight, show "Querying..." button (disabled)
    Unattested,         // Handle is available, show "Attest" button
    AlreadyAttested,    // Handle is taken, show "Recover / Challenge" button
    RecoverOrChallenge, // User clicked "Recover / Challenge", show both buttons + explanation
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
    pub min_dim: usize,     // min(width, height)
    pub perimeter: usize,   // width + height
    pub diagonal_sq: usize, // width² + height²

    // Launch screen state
    pub blinkey_blink_rate_ms: u64, // System blinkey blink rate in milliseconds (max for random range)
    pub blinkey_wave_top_bright: bool, // True=top is bright, False=top is dark
    pub blinkey_visible: bool,      // Whether blinkey is currently visible (for blinking)
    pub is_mouse_selecting: bool,   // True when actively dragging mouse to select text
    pub blinkey_pixel_x: usize,     // Cursor x position in pixels
    pub blinkey_pixel_y: usize,     // Cursor y position in pixels
    pub next_blinkey_blink_time: std::time::Instant, // When next blinkey blink should happen
    pub handle_status: HandleStatus, // Handle attestation status for button flow
    pub query_start_time: Option<std::time::Instant>, // When handle query started (for 1s simulation)
    pub handle_query: HandleQuery,                    // Network query system for handle attestation
    pub spectrum_phase: f32,                          // Rainbow sine wave phase (radians), animates during query
    pub speckle_counter: f32,                         // Background speckle animation counter, animates during query
    pub last_frame_time: std::time::Instant,          // Last frame timestamp for delta time calculation
    pub fps: f32,                                     // Current frames per second
    pub frame_times: Vec<f32>,                        // Recent frame delta times for FPS averaging
    pub target_frame_duration_ms: u64,                // Target frame duration based on monitor refresh rate
    pub next_animation_frame: std::time::Instant,     // When next animation frame should be drawn

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
}

// Hit test element IDs
pub const HIT_NONE: u8 = 0;
pub const HIT_MINIMIZE_BUTTON: u8 = 1;
pub const HIT_MAXIMIZE_BUTTON: u8 = 2;
pub const HIT_CLOSE_BUTTON: u8 = 3;
pub const HIT_HANDLE_TEXTBOX: u8 = 4;
pub const HIT_PRIMARY_BUTTON: u8 = 5; // "Attest" or "Recover / Challenge"
pub const HIT_RECOVER_BUTTON: u8 = 6; // "Recover" (left button in dual mode)
pub const HIT_CHALLENGE_BUTTON: u8 = 7; // "Challenge" (right button in dual mode)

// Button hover colour deltas are now in theme module

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum HoveredButton {
    None,
    Close,
    Maximize,
    Minimize,
    Textbox,
    QueryButton,
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
    #[cfg(target_os = "linux")]
    pub async fn new(window: &Window, blinkey_blink_rate_ms: u64, target_frame_duration_ms: u64) -> Self {
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
            window_dirty: true,
            selection_dirty: false,
            text_dirty: false,
            controls_dirty: false,
            min_dim: w.min(h),
            perimeter: w + h,
            diagonal_sq: w * w + h * h,
            blinkey_blink_rate_ms,
            blinkey_visible: false,
            is_mouse_selecting: false,
            blinkey_wave_top_bright: false,
            blinkey_pixel_x: 0,
            blinkey_pixel_y: 0,
            next_blinkey_blink_time: std::time::Instant::now(),
            handle_status: HandleStatus::Empty,
            query_start_time: None,
            handle_query: HandleQuery::new(),
            spectrum_phase: 0.0,
            speckle_counter: 0.0,
            last_frame_time: std::time::Instant::now(),
            fps: 0.0,
            frame_times: Vec::with_capacity(60),
            target_frame_duration_ms,
            next_animation_frame: std::time::Instant::now(),
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
        };
        app
    }

    #[cfg(target_os = "windows")]
    pub fn new(window: &Window, blinkey_blink_rate_ms: u64, target_frame_duration_ms: u64) -> Self {
        let size = window.inner_size();
        let renderer = Renderer::new(window, size.width, size.height);
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
            window_dirty: true,
            selection_dirty: false,
            text_dirty: false,
            controls_dirty: false,
            min_dim: w.min(h),
            perimeter: w + h,
            diagonal_sq: w * w + h * h,
            blinkey_blink_rate_ms,
            blinkey_visible: false,
            is_mouse_selecting: false,
            blinkey_wave_top_bright: false,
            blinkey_pixel_x: 0,
            blinkey_pixel_y: 0,
            next_blinkey_blink_time: std::time::Instant::now(),
            handle_status: HandleStatus::Empty,
            query_start_time: None,
            handle_query: HandleQuery::new(),
            spectrum_phase: 0.0,
            speckle_counter: 0.0,
            last_frame_time: std::time::Instant::now(),
            fps: 0.0,
            frame_times: Vec::with_capacity(60),
            target_frame_duration_ms,
            next_animation_frame: std::time::Instant::now(),
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
        };
        app
    }

    /// Update the fullscreen/maximized state
    /// When true, window edges are not drawn
    pub fn set_fullscreen(&mut self, is_fullscreen: bool) {
        if self.is_fullscreen != is_fullscreen {
            self.is_fullscreen = is_fullscreen;
        }
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
        self.textbox_mask
            .resize((size.width * size.height) as usize, 0);

        // Clear hover state on resize since button positions/sizes change
        self.hovered_button = HoveredButton::None;

        // Recalculate character widths for new font size
        self.recalculate_char_widths();

        // Recalculate scroll to keep blinkey in view with new dimensions
        if !self.current_text_state.chars.is_empty() {
            let margin = self.min_dim / 8;
            let box_width = self.width as usize - margin * 2;
            self.update_text_scroll(box_width);
        } else {
            // No text - center it
            self.current_text_state.scroll_offset = 0.0;
        }

        // Clear textbox focus on resize - user must click to refocus
        self.current_text_state.textbox_focused = false;
        self.blinkey_visible = false;

        // Trigger full redraw - differential rendering will be skipped automatically
        self.window_dirty = true;
    }

    pub fn update_modifiers(&mut self, modifiers: ModifiersState) {
        self.modifiers = modifiers;
    }

    pub fn get_resize_edge(&self, x: f32, y: f32) -> ResizeEdge {
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

    pub fn submit_username(&mut self) {
        // TODO: Implement semantic embedding for discovery
        // Username -> Hash (identity/keys) + Embedding vector (semantic search, opt-in)
        // Embedding platform: SentenceTransformers (all-MiniLM-L6-v2, ~80MB, 384-dim)
        // Store on DHT: hash=identity, embedding=public discovery vector
        log::warn!(
            "⚠️  SEMANTIC EMBEDDING NOT IMPLEMENTED - Username discovery will be hash-only!"
        );
        log::warn!("⚠️  TODO: Add semantic embedding for fuzzy username search");

        // TODO: Query DHT for username availability
        let username: String = self.current_text_state.chars.iter().collect();
        log::info!("Querying handle availability: {}", username);

        // Start simulated 1-second DHT query
        self.handle_status = HandleStatus::Empty;
        self.query_start_time = Some(std::time::Instant::now());
        self.window_dirty = true;
    }

    /// Start a network query for handle attestation status
    pub fn query_handle(&mut self) {
        let handle: String = self.current_text_state.chars.iter().collect();

        // Set status to Checking and trigger query
        self.handle_status = HandleStatus::Checking;
        self.handle_query.query(handle);
        let now = std::time::Instant::now();
        self.query_start_time = Some(now);
        self.last_frame_time = now; // Reset to prevent animation jerk on first frame
        // Initialize animation frame timing
        self.next_animation_frame = now + std::time::Duration::from_millis(self.target_frame_duration_ms);
    }

    /// Check if query response is ready and update handle_status
    pub fn check_query_response(&mut self) -> bool {
        if let Some(result) = self.handle_query.try_recv() {
            let new_status = match result {
                QueryResult::Unattested => HandleStatus::Unattested,
                QueryResult::AlreadyAttested => HandleStatus::AlreadyAttested,
            };
            debug_println!("Query completed: {:?} -> {:?}", self.handle_status, new_status);
            self.handle_status = new_status;
            self.query_start_time = None;
            self.window_dirty = true; // Trigger redraw to update button
            return true;
        }
        false
    }

    /// Check if we should continuously animate (request redraws every frame)
    pub fn should_animate(&self) -> bool {
        self.handle_status == HandleStatus::Checking
    }
}
