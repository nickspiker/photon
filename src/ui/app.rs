use super::renderer::Renderer;
use super::text_rasterizing::TextRenderer;
use super::PhotonEvent;
use crate::network::{HandleQuery, QueryResult};
use winit::{dpi::PhysicalSize, event_loop::EventLoopProxy, keyboard::ModifiersState, window::Window};

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

/// Application lifecycle state
///
/// State machine:
/// ```text
///                    ┌─────────────────────────────────────────────────────────┐
///                    │                      FRESH INSTALL                      │
///                    └─────────────────────────────────────────────────────────┘
///                                            │
///                                            ▼
///     ┌──────────────────────────────────────────────────────────────────────┐
///     │  Launch::Fresh                                                        │
///     │  - No device key on disk                                              │
///     │  - Show "Enter your handle" prompt                                    │
///     │  - User types handle, clicks Query                                    │
///     └──────────────────────────────────────────────────────────────────────┘
///                           │                              │
///                    (handle available)            (handle claimed)
///                           ▼                              ▼
///     ┌─────────────────────────────┐    ┌─────────────────────────────────────┐
///     │  Launch::Available          │    │  Launch::Claimed                    │
///     │  - Show "Attest" button     │    │  - Show "Add Device" / "Challenge"  │
///     │  - Create new identity      │    │  - Need auth code from other device │
///     └─────────────────────────────┘    └─────────────────────────────────────┘
///                           │                              │
///                    (attest success)              (auth verified)
///                           ▼                              ▼
///     ┌──────────────────────────────────────────────────────────────────────┐
///     │  Ready                                                                │
///     │  - Device key encrypted on disk                                       │
///     │  - Registered with FGTW                                               │
///     │  - Can search for peers and start conversations                       │
///     └──────────────────────────────────────────────────────────────────────┘
///                                            │
///                                     (start P2P session)
///                                            ▼
///     ┌──────────────────────────────────────────────────────────────────────┐
///     │  Connected { peer_handle: String }                                    │
///     │  - Active encrypted P2P session                                       │
///     │  - Can send/receive messages                                          │
///     └──────────────────────────────────────────────────────────────────────┘
///
///                    ┌─────────────────────────────────────────────────────────┐
///                    │                    RETURNING USER                       │
///                    └─────────────────────────────────────────────────────────┘
///                                            │
///                                            ▼
///     ┌──────────────────────────────────────────────────────────────────────┐
///     │  Launch::Locked                                                       │
///     │  - Device key exists on disk (encrypted)                              │
///     │  - Show "Enter your handle" prompt                                    │
///     │  - User types handle to unlock                                        │
///     └──────────────────────────────────────────────────────────────────────┘
///                           │                              │
///                    (handle correct)              (handle wrong)
///                           ▼                              ▼
///                        Ready                    "Wrong handle" error
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AppState {
    /// Launch screen states (before main messenger UI)
    Launch(LaunchState),

    /// Main messenger - ready to search peers and chat
    Ready,

    /// Active P2P conversation
    Connected { peer_handle: String },
}

/// Sub-states for the launch screen
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LaunchState {
    /// Ready to attest - show handle input + "Attest" button
    Fresh,

    /// Computing handle_proof + announcing to FGTW
    /// Show loading spinner, no button
    Attesting,

    /// Attestation failed - show error message, no button
    /// User can edit textbox to return to Fresh
    Error(String),
}

impl LaunchState {
    /// Check if we're in a state where the user can type in the handle textbox
    pub fn can_edit_handle(&self) -> bool {
        !matches!(self, LaunchState::Attesting)
    }

    /// Check if we're waiting for a network response
    pub fn is_loading(&self) -> bool {
        matches!(self, LaunchState::Attesting)
    }
}

/// Result of searching for a handle
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SearchResult {
    Found(String),  // IP:port of found peer
    NotFound,
}

impl Default for AppState {
    fn default() -> Self {
        AppState::Launch(LaunchState::Fresh)
    }
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
    pub app_state: AppState,         // Application lifecycle state
    pub query_start_time: Option<std::time::Instant>, // When handle query started (for 1s simulation)
    pub handle_query: HandleQuery,                    // Network query system for handle attestation
    pub fgtw_online: bool,                            // True if FGTW server is reachable
    pub prev_fgtw_online: bool,                       // Previous state for differential rendering
    pub search_result: Option<SearchResult>,          // Result of handle search
    pub spectrum_phase: f32, // Rainbow sine wave phase (radians), animates during query
    pub speckle_counter: f32, // Background speckle animation counter, animates during query
    pub last_frame_time: std::time::Instant, // Last frame timestamp for delta time calculation
    pub fps: f32,            // Current frames per second
    pub frame_times: Vec<f32>, // Recent frame delta times for FPS averaging
    pub target_frame_duration_ms: u64, // Target frame duration based on monitor refresh rate
    pub next_animation_frame: std::time::Instant, // When next animation frame should be drawn

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
pub const HIT_PRIMARY_BUTTON: u8 = 5; // "Attest" button

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
    pub async fn new(
        window: &Window,
        blinkey_blink_rate_ms: u64,
        target_frame_duration_ms: u64,
        event_proxy: EventLoopProxy<PhotonEvent>,
    ) -> Self {
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
            app_state: {
                // Determine initial state based on whether device key exists
                // Device key = random Ed25519, stored unencrypted
                // Handle hash = BLAKE3(Argon2(handle)), computed on-the-fly, never stored
                use crate::network::fgtw::FgtwPaths;
                let paths = FgtwPaths::new().expect("Failed to get FGTW paths");
                if paths.device_key.exists() {
                    // Returning user - device key exists, need handle to verify with FGTW
                    AppState::Launch(LaunchState::Fresh)
                } else {
                    // Fresh install - no device key yet
                    AppState::Launch(LaunchState::Fresh)
                }
            },
            query_start_time: None,
            handle_query: {
                use crate::network::fgtw::{load_or_generate_device_key, FgtwPaths, FgtwTransport};
                let paths = FgtwPaths::new().expect("Failed to get FGTW paths");
                let device_keypair = load_or_generate_device_key(&paths.device_key)
                    .expect("Failed to load/generate device key");

                let our_identity = crate::types::PublicIdentity::from_bytes(
                    *device_keypair.public.as_bytes(),
                );

                let handle_query = HandleQuery::new(our_identity.clone(), event_proxy.clone());
                let transport = std::sync::Arc::new(FgtwTransport::new(our_identity, 41641));
                handle_query.set_transport(transport);

                handle_query
            },
            fgtw_online: false, // Updated by connectivity check
            prev_fgtw_online: false,
            search_result: None,
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
    pub fn new(window: &Window, blinkey_blink_rate_ms: u64, target_frame_duration_ms: u64, event_proxy: EventLoopProxy<PhotonEvent>) -> Self {
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
            app_state: {
                // Determine initial state based on whether device key exists
                // Device key = random Ed25519, stored unencrypted
                // Handle hash = BLAKE3(Argon2(handle)), computed on-the-fly, never stored
                use crate::network::fgtw::FgtwPaths;
                let paths = FgtwPaths::new().expect("Failed to get FGTW paths");
                if paths.device_key.exists() {
                    // Returning user - device key exists, need handle to verify with FGTW
                    AppState::Launch(LaunchState::Fresh)
                } else {
                    // Fresh install - no device key yet
                    AppState::Launch(LaunchState::Fresh)
                }
            },
            query_start_time: None,
            handle_query: {
                use crate::network::fgtw::{load_or_generate_device_key, FgtwPaths, FgtwTransport};
                let paths = FgtwPaths::new().expect("Failed to get FGTW paths");
                let device_keypair = load_or_generate_device_key(&paths.device_key)
                    .expect("Failed to load/generate device key");

                let our_identity = crate::types::PublicIdentity::from_bytes(
                    *device_keypair.public.as_bytes(),
                );

                let handle_query = HandleQuery::new(our_identity.clone(), event_proxy.clone());
                let transport = std::sync::Arc::new(FgtwTransport::new(our_identity, 41641));
                handle_query.set_transport(transport);

                handle_query
            },
            fgtw_online: false, // Updated by connectivity check
            prev_fgtw_online: false,
            search_result: None,
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

    /// Start attestation - compute handle_proof and announce to FGTW
    pub fn start_attestation(&mut self) {
        let handle: String = self.current_text_state.chars.iter().collect();

        // Set status to Attesting and trigger attestation
        self.app_state = AppState::Launch(LaunchState::Attesting);
        self.handle_query.query(handle);
        let now = std::time::Instant::now();
        self.query_start_time = Some(now);
        self.last_frame_time = now; // Reset to prevent animation jerk on first frame
                                    // Initialize animation frame timing
        self.next_animation_frame =
            now + std::time::Duration::from_millis(self.target_frame_duration_ms);
    }

    /// Check if FGTW connectivity status is available and update fgtw_online
    pub fn check_fgtw_online(&mut self) {
        if let Some(online) = self.handle_query.try_recv_online() {
            if online != self.fgtw_online {
                self.fgtw_online = online;
                self.controls_dirty = true; // Trigger indicator redraw
            }
        }
    }

    /// Search for a handle in our peer list
    pub fn start_handle_search(&mut self, handle: &str) {
        use crate::types::Handle;

        // Compute handle_proof for the search term
        let handle_proof = Handle::username_to_handle_proof(handle);

        // Check if this handle_proof exists in our peer store
        if let Some(transport) = self.handle_query.get_transport() {
            let peer_store = transport.peer_store();
            let store = peer_store.lock().unwrap();

            let peers = store.get_devices_for_handle(&handle_proof);
            if let Some(peer) = peers.first() {
                eprintln!("Found peer: {} at {}", handle, peer.ip);
                self.search_result = Some(SearchResult::Found(peer.ip.to_string()));
            } else {
                eprintln!("Handle '{}' not found in peer list", handle);
                self.search_result = Some(SearchResult::NotFound);
            }
        } else {
            eprintln!("No transport available for search");
            self.search_result = Some(SearchResult::NotFound);
        }
    }

    /// Check if attestation response is ready and update app_state
    pub fn check_attestation_response(&mut self) -> bool {
        if let Some(result) = self.handle_query.try_recv() {
            eprintln!("UI: Received attestation result: {:?}", result);
            let new_state = match result {
                QueryResult::Success => {
                    // Success - we're now registered
                    eprintln!("UI: Attestation SUCCESS - transitioning to Ready state");
                    AppState::Ready
                }
                QueryResult::AlreadyAttested(_peers) => {
                    // Handle already bound to different device
                    eprintln!("UI: Handle already attested - showing error");
                    AppState::Launch(LaunchState::Error(
                        "Handle already attested".to_string(),
                    ))
                }
                QueryResult::Error(msg) => {
                    // Error during attestation
                    eprintln!("UI: Attestation error - {}", msg);
                    AppState::Launch(LaunchState::Error(msg))
                }
            };
            debug_println!(
                "Attestation completed: {:?} -> {:?}",
                self.app_state,
                new_state
            );
            // Clear textbox when transitioning to Ready
            if matches!(new_state, AppState::Ready) {
                self.current_text_state.chars.clear();
                self.current_text_state.blinkey_index = 0;
                self.current_text_state.selection_anchor = None;
                self.current_text_state.scroll_offset = 0.0;
            }
            self.app_state = new_state;
            self.query_start_time = None;
            self.window_dirty = true;
            eprintln!("UI: Attestation complete, window marked dirty for redraw");
            return true;
        }
        false
    }

    /// Check if we should continuously animate (request redraws every frame)
    pub fn should_animate(&self) -> bool {
        matches!(self.app_state, AppState::Launch(LaunchState::Attesting))
    }

    /// Get the current launch state (if in Launch mode)
    pub fn launch_state(&self) -> Option<&LaunchState> {
        match &self.app_state {
            AppState::Launch(state) => Some(state),
            _ => None,
        }
    }

    /// Set the launch state (only if currently in Launch mode)
    pub fn set_launch_state(&mut self, state: LaunchState) {
        self.app_state = AppState::Launch(state);
    }
}
