use super::renderer::Renderer;
use super::text_rasterizing::TextRenderer;
use winit::{dpi::PhysicalSize, keyboard::ModifiersState, window::Window};

impl TextState {
    pub fn new() -> Self {
        Self {
            chars: Vec::new(),
            widths: Vec::new(),
            width: 0,
            cursor_index: 0,
            scroll_offset: 0.0,
            selection_anchor: None,
            textbox_focused: false,
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
    Checking,           // Still checking if handle is attested (initial state)
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
    pub cursor_index: usize,
    pub selection_anchor: Option<usize>,
    pub textbox_focused: bool,
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
    pub cursor_blink_rate_ms: u64, // System cursor blink rate in milliseconds (max for random range)
    pub cursor_wave_top_bright: bool, // True=top is bright, False=top is dark
    pub cursor_visible: bool,      // Whether cursor is currently visible (for blinking)
    pub is_mouse_selecting: bool,  // True when actively dragging mouse to select text
    pub cursor_pixel_x: usize,     // Cursor x position in pixels
    pub cursor_pixel_y: usize,     // Cursor y position in pixels
    pub next_cursor_blink_time: std::time::Instant, // When next cursor blink should happen
    pub handle_status: HandleStatus, // Handle attestation status for button flow
    pub query_start_time: Option<std::time::Instant>, // When handle query started (for 1s simulation)

    // Text state for differential rendering
    pub current_text_state: TextState,
    pub old_text_state: TextState,

    pub textbox_mask: Vec<u8>, // Single-channel alpha mask for textbox (0=outside, 255=inside, faded at edges)
    pub show_textbox_mask: bool, // Debug: show textbox mask visualization (Ctrl+T)
    pub frame_counter: usize,  // Every render() call (from RedrawRequested)
    pub update_counter: usize, // Any actual drawing (partial or full)
    pub full_redraw_counter: usize, // Complete scene redraws only

    // Input state
    pub mouse_x: f32,
    pub mouse_y: f32,
    pub mouse_button_pressed: bool, // True when left mouse button is held down
    pub is_dragging_resize: bool,
    pub is_dragging_move: bool,
    pub resize_edge: ResizeEdge,
    pub drag_start_cursor_screen_pos: (f64, f64), // Global screen position when drag starts
    pub drag_start_size: (u32, u32),
    pub drag_start_window_pos: (i32, i32),
    pub modifiers: ModifiersState,
    pub is_fullscreen: bool, // True when window is fullscreen

    // Window control buttons
    pub hovered_button: HoveredButton,
    pub prev_hovered_button: HoveredButton, // Previous hover state to detect changes

    // Mouse selection state
    pub selection_last_update_time: Option<std::time::Instant>, // Last time selection scroll was updated

    // Cached button pixel coordinates for fast hover effects
    pub minimize_pixels: Vec<usize>,
    pub maximize_pixels: Vec<usize>,
    pub close_pixels: Vec<usize>,

    // Hit test bitmap (one byte per pixel, element ID)
    pub hit_test_map: Vec<u8>,
    pub debug_hit_test: bool,
    pub debug_hit_colours: Vec<(u8, u8, u8)>, // Random colours for each hit area ID
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
            window_dirty: true,
            selection_dirty: false,
            text_dirty: false,
            controls_dirty: false,
            min_dim: w.min(h),
            perimeter: w + h,
            diagonal_sq: w * w + h * h,
            cursor_blink_rate_ms,
            cursor_visible: false,
            is_mouse_selecting: false,
            cursor_wave_top_bright: false,
            cursor_pixel_x: 0,
            cursor_pixel_y: 0,
            next_cursor_blink_time: std::time::Instant::now(),
            handle_status: HandleStatus::Checking,
            query_start_time: None,
            current_text_state: TextState::new(),
            old_text_state: TextState::new(),
            textbox_mask: vec![0; (size.width * size.height) as usize],
            show_textbox_mask: false,
            frame_counter: 0,
            update_counter: 0,
            full_redraw_counter: 0,
            mouse_x: 0.,
            mouse_y: 0.,
            mouse_button_pressed: false,
            is_dragging_resize: false,
            is_dragging_move: false,
            resize_edge: ResizeEdge::None,
            drag_start_cursor_screen_pos: (0., 0.),
            drag_start_size: (0, 0),
            drag_start_window_pos: (0, 0),
            modifiers: ModifiersState::empty(),
            hovered_button: HoveredButton::None,
            prev_hovered_button: HoveredButton::None,
            selection_last_update_time: None,
            minimize_pixels: Vec::new(), // Needs nuked in favor of centerpoint fill
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
            cursor_blink_rate_ms,
            cursor_wave_top_bright: false,
            cursor_visible: false,
            cursor_pixel_x: 0,
            cursor_pixel_y: 0,
            cursor_height: 0.0,
            username_available: None,
            textbox_focused: false,
            current_text_state: TextState {
                text: EditableText::new(),
                scroll_offset: 0.0,
                cursor_index: 0,
                selection_anchor: None,
                textbox_focused: false,
            },
            old_text_state: TextState {
                text: EditableText::new(),
                scroll_offset: 0.0,
                cursor_index: 0,
                selection_anchor: None,
                textbox_focused: false,
            },
            textbox_mask: Vec::new(),
            textbox_bounds: (0, 0, 0, 0),
            show_textbox_mask: false,
            redraw_counter: 0,
            render_counter: 0,
            text_redraw_counter: 0,
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

        // Clear textbox focus on resize - user must click to refocus
        self.current_text_state.textbox_focused = false;
        self.recalculate_char_widths();
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
        self.handle_status = HandleStatus::Checking;
        self.query_start_time = Some(std::time::Instant::now());
        self.window_dirty = true;
    }
}
