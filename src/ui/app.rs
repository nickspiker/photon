use super::renderer::Renderer;
use super::text_rasterizing::TextRenderer;
use super::theme;
#[cfg(not(target_os = "android"))]
use super::PhotonEvent;
use crate::network::StatusChecker;
use crate::network::{HandleQuery, QueryResult};
use crate::types::{Contact, HandleText};
#[cfg(not(target_os = "android"))]
use winit::{
    dpi::PhysicalSize, event_loop::EventLoopProxy, keyboard::ModifiersState, window::Window,
};

/// Cross-platform keyboard modifier state
#[cfg(target_os = "android")]
#[derive(Clone, Copy, Default)]
pub struct ModifiersState {
    pub shift: bool,
    pub ctrl: bool,
    pub alt: bool,
}

#[cfg(target_os = "android")]
impl ModifiersState {
    pub fn empty() -> Self {
        Self::default()
    }
    pub fn shift_key(&self) -> bool {
        self.shift
    }
    pub fn control_key(&self) -> bool {
        self.ctrl
    }
    pub fn alt_key(&self) -> bool {
        self.alt
    }
}

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
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AppState {
    /// Launch screen states (before main messenger UI)
    Launch(LaunchState),

    /// Main messenger - ready to search peers and chat
    Ready,

    /// Searching for a peer handle (computing handle_proof in background)
    Searching,

    /// Viewing conversation with a contact (contact index stored separately)
    Conversation,

    /// Active P2P conversation (legacy - may remove)
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
#[derive(Debug, Clone)]
pub struct FoundPeer {
    pub handle: HandleText,
    pub device_pubkey: crate::types::PublicIdentity,
    pub ip: std::net::SocketAddr,
}

#[derive(Debug, Clone)]
pub enum SearchResult {
    Found(FoundPeer),
    NotFound,
    Error(String),
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
    pub app_state: AppState,        // Application lifecycle state
    pub query_start_time: Option<std::time::Instant>, // When handle query started (for 1s simulation)
    pub handle_query: Option<HandleQuery>,            // Network query system for handle attestation
    pub fgtw_online: bool,                            // True if FGTW server is reachable
    pub prev_fgtw_online: bool,                       // Previous state for differential rendering
    pub hint_was_shown: bool, // Track if network hint was shown (for cleanup)
    pub search_result: Option<SearchResult>, // Result of handle search
    pub search_receiver: Option<std::sync::mpsc::Receiver<SearchResult>>, // Async search result
    pub searching_handle: Option<String>, // Handle being searched (for display)
    pub glow_colour: u32,     // Current textbox glow colour (0x00RRGGBB)
    pub spectrum_phase: f32,  // Rainbow sine wave phase (radians), animates during query
    pub speckle_counter: f32, // Background speckle animation counter, animates during query
    pub last_frame_time: std::time::Instant, // Last frame timestamp for delta time calculation
    pub fps: f32,             // Current frames per second
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

    // Contacts list (handles we've searched and found)
    pub contacts: Vec<Contact>,
    // Shared pubkey list for StatusChecker (synced with contacts)
    pub contact_pubkeys: crate::network::status::ContactPubkeys,
    // Currently hovered contact index (None if not hovering any)
    pub hovered_contact: Option<usize>,
    pub prev_hovered_contact: Option<usize>,
    // Selected contact for conversation view (None = main view)
    pub selected_contact: Option<usize>,

    // P2P status checker for contact online status
    pub status_checker: Option<StatusChecker>,
    pub next_status_ping: std::time::Instant, // When to ping contacts next

    // Periodic FGTW refresh
    pub next_fgtw_refresh: std::time::Instant, // When to re-announce to FGTW
    pub attesting_handle: Option<String>,      // Handle being attested (for storing handle_proof)

    // User avatar and identity
    pub avatar_pixels: Option<Vec<u8>>, // Decoded VSF RGB pixels (AVATAR_SIZE x AVATAR_SIZE x 3)
    pub avatar_scaled: Option<Vec<u8>>, // Mitchell-resampled avatar at current display size
    pub avatar_scaled_diameter: usize,  // Diameter the scaled avatar was rendered at
    pub user_handle: Option<String>,    // User's attested handle
    pub show_avatar_hint: bool,         // Show "drag and drop" hint after clicking avatar
    pub file_hovering_avatar: bool,     // Track if file is being dragged over avatar

    // Contact avatar fetching (background thread)
    pub contact_avatar_rx: std::sync::mpsc::Receiver<crate::avatar::AvatarDownloadResult>,
    pub contact_avatar_tx: std::sync::mpsc::Sender<crate::avatar::AvatarDownloadResult>,

    // Device keypair for signing (needed by StatusChecker)
    pub device_keypair: crate::network::fgtw::Keypair,
}

// Hit test element IDs
pub const HIT_NONE: u8 = 0;
pub const HIT_MINIMIZE_BUTTON: u8 = 1;
pub const HIT_MAXIMIZE_BUTTON: u8 = 2;
pub const HIT_CLOSE_BUTTON: u8 = 3;
pub const HIT_HANDLE_TEXTBOX: u8 = 4;
pub const HIT_PRIMARY_BUTTON: u8 = 5; // "Attest" button
pub const HIT_BACK_HEADER: u8 = 6; // Conversation header back button
pub const HIT_AVATAR: u8 = 7; // User's avatar circle (Ready screen)
pub const HIT_CONTACT_BASE: u8 = 64; // Contact 0 = 64, Contact 1 = 65, etc. (up to 192 contacts)

// Button hover colour deltas are now in theme module

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum HoveredButton {
    None,
    Close,
    Maximize,
    Minimize,
    Textbox,
    QueryButton,
    BackHeader,
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
    /// Desktop constructor (Linux + Windows)
    #[cfg(not(target_os = "android"))]
    pub fn new(
        window: &Window,
        blinkey_blink_rate_ms: u64,
        target_frame_duration_ms: u64,
        event_proxy: EventLoopProxy<PhotonEvent>,
    ) -> Self {
        let size = window.inner_size();
        let renderer = Renderer::new(window, size.width, size.height);
        let text_renderer = TextRenderer::new();

        // Check initial fullscreen/maximized state
        let is_fullscreen = window.fullscreen().is_some() || window.is_maximized();

        let w = size.width as usize;
        let h = size.height as usize;

        // Avatar is loaded after attestation when we have a handle
        // (the storage key is derived from handle)
        let avatar_pixels: Option<Vec<u8>> = None;

        // Create channel for background avatar downloads
        let (contact_avatar_tx, contact_avatar_rx) = std::sync::mpsc::channel();

        let mut app = Self {
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
            app_state: AppState::Launch(LaunchState::Fresh),
            query_start_time: None,
            handle_query: None, // Initialized below after device_keypair
            device_keypair: {
                // Derive deterministically from machine-id - NEVER stored to disk
                use crate::network::fgtw::{derive_device_keypair, get_machine_fingerprint};
                let fingerprint = get_machine_fingerprint()
                    .expect("Failed to get machine fingerprint for key derivation");
                let keypair = derive_device_keypair(&fingerprint);
                crate::log_info(&format!(
                    "Device pubkey: {}",
                    hex::encode(keypair.public.as_bytes())
                ));
                keypair
            },
            fgtw_online: false, // Updated by connectivity check
            prev_fgtw_online: false,
            hint_was_shown: false,
            search_result: None,
            search_receiver: None,
            searching_handle: None,
            glow_colour: theme::GLOW_DEFAULT, // White glow by default
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
            contacts: Vec::new(),
            contact_pubkeys: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
            hovered_contact: None,
            prev_hovered_contact: None,
            selected_contact: None,
            status_checker: None, // Initialized AFTER attestation succeeds
            next_status_ping: std::time::Instant::now(),
            next_fgtw_refresh: std::time::Instant::now() + std::time::Duration::from_secs(60),
            attesting_handle: None,
            avatar_pixels,
            avatar_scaled: None,
            avatar_scaled_diameter: 0,
            user_handle: None,
            show_avatar_hint: false,
            file_hovering_avatar: false,
            contact_avatar_rx,
            contact_avatar_tx,
        };

        // Initialize handle_query with the derived keypair
        {
            use crate::network::fgtw::FgtwTransport;
            use crate::network::HandleQuery;
            use crate::types::PublicIdentity;

            let our_identity = PublicIdentity::from_bytes(*app.device_keypair.public.as_bytes());
            let handle_query = HandleQuery::new(app.device_keypair.clone(), event_proxy.clone());
            let transport = std::sync::Arc::new(FgtwTransport::new(our_identity, 41641));
            handle_query.set_transport(transport);
            app.handle_query = Some(handle_query);
        }

        app
    }

    /// Android constructor - takes device keypair derived from JNI fingerprint
    #[cfg(target_os = "android")]
    pub fn new(width: u32, height: u32, device_keypair: crate::network::fgtw::Keypair) -> Self {
        let renderer = Renderer::new(width, height);
        let text_renderer = TextRenderer::new();

        let w = width as usize;
        let h = height as usize;

        // Create channel for background avatar downloads
        let (contact_avatar_tx, contact_avatar_rx) = std::sync::mpsc::channel();

        Self {
            renderer,
            text_renderer,
            width,
            height,
            window_dirty: true,
            selection_dirty: false,
            text_dirty: false,
            controls_dirty: false,
            min_dim: w.min(h),
            perimeter: w + h,
            diagonal_sq: w * w + h * h,
            blinkey_blink_rate_ms: 500,
            blinkey_visible: false,
            is_mouse_selecting: false,
            blinkey_wave_top_bright: false,
            blinkey_pixel_x: 0,
            blinkey_pixel_y: 0,
            next_blinkey_blink_time: std::time::Instant::now(),
            app_state: AppState::Launch(LaunchState::Fresh),
            query_start_time: None,
            handle_query: None, // Initialized via set_handle_query() after this
            device_keypair,
            fgtw_online: false,
            prev_fgtw_online: false,
            hint_was_shown: false,
            search_result: None,
            search_receiver: None,
            searching_handle: None,
            glow_colour: theme::GLOW_DEFAULT,
            spectrum_phase: 0.0,
            speckle_counter: 0.0,
            last_frame_time: std::time::Instant::now(),
            fps: 0.0,
            frame_times: Vec::with_capacity(60),
            target_frame_duration_ms: 16, // ~60fps
            next_animation_frame: std::time::Instant::now(),
            current_text_state: TextState::new(),
            previous_text_state: TextState::new(),
            textbox_mask: vec![0; (width * height) as usize],
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
            hit_test_map: vec![0; (width * height) as usize],
            debug_hit_test: false,
            debug_hit_colours: Vec::new(),
            debug: false,
            is_fullscreen: true, // Android is always fullscreen
            contacts: Vec::new(),
            contact_pubkeys: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
            hovered_contact: None,
            prev_hovered_contact: None,
            selected_contact: None,
            status_checker: None,
            next_status_ping: std::time::Instant::now(),
            next_fgtw_refresh: std::time::Instant::now() + std::time::Duration::from_secs(60),
            attesting_handle: None,
            avatar_pixels: None,
            avatar_scaled: None,
            avatar_scaled_diameter: 0,
            user_handle: None,
            show_avatar_hint: false,
            file_hovering_avatar: false,
            contact_avatar_rx,
            contact_avatar_tx,
        }
    }

    /// Update the fullscreen/maximized state
    /// When true, window edges are not drawn
    pub fn set_fullscreen(&mut self, is_fullscreen: bool) {
        if self.is_fullscreen != is_fullscreen {
            self.is_fullscreen = is_fullscreen;
        }
    }

    /// Handle touch events on Android
    /// action: 0=DOWN, 1=UP, 2=MOVE, 3=CANCEL
    /// Returns: 1=show keyboard, -1=hide keyboard, 0=no change
    #[cfg(target_os = "android")]
    pub fn handle_touch(&mut self, action: i32, x: f32, y: f32) -> i32 {
        self.mouse_x = x;
        self.mouse_y = y;

        let keyboard_action = match action {
            0 => {
                // DOWN
                self.mouse_button_pressed = true;
                self.handle_touch_down();
                0
            }
            1 | 3 => {
                // UP or CANCEL
                self.mouse_button_pressed = false;
                self.handle_touch_up()
            }
            2 => {
                // MOVE
                if self.mouse_button_pressed {
                    self.handle_touch_move();
                }
                0
            }
            _ => 0,
        };
        self.window_dirty = true;
        keyboard_action
    }

    #[cfg(target_os = "android")]
    fn handle_touch_down(&mut self) {
        // Use hit_test_map to determine what was touched
        let hit_idx = (self.mouse_y as usize) * (self.width as usize) + (self.mouse_x as usize);
        if hit_idx >= self.hit_test_map.len() {
            return;
        }

        let element = self.hit_test_map[hit_idx];

        // Set hover state based on touched element (brightens button)
        self.prev_hovered_button = self.hovered_button;
        self.hovered_button = match element {
            HIT_PRIMARY_BUTTON => HoveredButton::QueryButton,
            HIT_HANDLE_TEXTBOX => {
                if !self.current_text_state.textbox_focused {
                    HoveredButton::Textbox
                } else {
                    HoveredButton::None
                }
            }
            HIT_BACK_HEADER => HoveredButton::BackHeader,
            _ => HoveredButton::None,
        };

        // Track hovered contact
        if element >= HIT_CONTACT_BASE {
            self.prev_hovered_contact = self.hovered_contact;
            self.hovered_contact = Some((element - HIT_CONTACT_BASE) as usize);
        } else {
            self.prev_hovered_contact = self.hovered_contact;
            self.hovered_contact = None;
        }

        if self.hovered_button != self.prev_hovered_button
            || self.hovered_contact != self.prev_hovered_contact
        {
            self.controls_dirty = true;
        }
    }

    #[cfg(target_os = "android")]
    fn handle_touch_move(&mut self) {
        // Already cancelled - stay cancelled until touch up
        if self.hovered_button == HoveredButton::None && self.hovered_contact.is_none() {
            return;
        }

        let hit_idx = (self.mouse_y as usize) * (self.width as usize) + (self.mouse_x as usize);
        if hit_idx >= self.hit_test_map.len() {
            // Off screen - cancel
            self.prev_hovered_button = self.hovered_button;
            self.hovered_button = HoveredButton::None;
            self.prev_hovered_contact = self.hovered_contact;
            self.hovered_contact = None;
            self.controls_dirty = true;
            return;
        }

        let element = self.hit_test_map[hit_idx];

        // Check if still on the SAME element we started on
        let still_on_button = match self.hovered_button {
            HoveredButton::QueryButton => element == HIT_PRIMARY_BUTTON,
            HoveredButton::Textbox => element == HIT_HANDLE_TEXTBOX,
            HoveredButton::BackHeader => element == HIT_BACK_HEADER,
            _ => true, // None stays None
        };

        let still_on_contact = match self.hovered_contact {
            Some(idx) => element == HIT_CONTACT_BASE + idx as u8,
            None => true, // None stays None
        };

        // Dragged off - cancel permanently
        if !still_on_button || !still_on_contact {
            self.prev_hovered_button = self.hovered_button;
            self.hovered_button = HoveredButton::None;
            self.prev_hovered_contact = self.hovered_contact;
            self.hovered_contact = None;
            self.controls_dirty = true;
        }
    }

    /// Returns: 1=show keyboard, -1=hide keyboard, 0=no change
    #[cfg(target_os = "android")]
    fn handle_touch_up(&mut self) -> i32 {
        let mut keyboard_action = 0;

        // Check what element we're over
        let hit_idx = (self.mouse_y as usize) * (self.width as usize) + (self.mouse_x as usize);
        let element = if hit_idx < self.hit_test_map.len() {
            self.hit_test_map[hit_idx]
        } else {
            HIT_NONE
        };

        // Special case: tap on textbox when already focused = position cursor
        if element == HIT_HANDLE_TEXTBOX && self.current_text_state.textbox_focused {
            self.current_text_state.blinkey_index = self.blinkey_index_from_x(self.mouse_x);
            self.current_text_state.selection_anchor = None;
            self.text_dirty = true;

            // Clear hover state
            self.prev_hovered_button = self.hovered_button;
            self.hovered_button = HoveredButton::None;
            self.controls_dirty = true;

            // On Android, always request keyboard - user may have dismissed it
            #[cfg(target_os = "android")]
            return 1;
            #[cfg(not(target_os = "android"))]
            return 0;
        }

        // Only execute action if we're still in hover state (didn't drag off)
        match self.hovered_button {
            HoveredButton::Textbox => {
                // Focus textbox and show keyboard
                // Always return 1 even if already focused - keyboard may have been dismissed
                self.current_text_state.textbox_focused = true;
                self.blinkey_visible = true;
                self.text_dirty = true;
                #[cfg(target_os = "android")]
                {
                    keyboard_action = 1; // Always request keyboard on Android
                }
            }
            HoveredButton::QueryButton => {
                // Execute primary button action
                match &self.app_state {
                    AppState::Launch(LaunchState::Fresh) => {
                        self.start_attestation();
                    }
                    AppState::Ready => {
                        let handle: String = self.current_text_state.chars.iter().collect();
                        if !handle.is_empty() {
                            self.start_handle_search(&handle);
                        }
                    }
                    _ => {}
                }
            }
            HoveredButton::BackHeader => {
                // Go back to contacts list
                self.app_state = AppState::Ready;
                self.selected_contact = None;
            }
            HoveredButton::None => {
                // Check if we're on avatar (doesn't use hover state)
                if element == HIT_AVATAR {
                    if matches!(self.app_state, AppState::Ready | AppState::Searching) {
                        #[cfg(target_os = "android")]
                        {
                            // Return 2 to signal "open image picker" to Android
                            keyboard_action = 2;
                        }
                        #[cfg(not(target_os = "android"))]
                        {
                            self.show_avatar_hint = true;
                        }
                    }
                } else if self.hovered_contact.is_none() {
                    // Tapped outside interactive elements - unfocus textbox and hide keyboard
                    if self.current_text_state.textbox_focused {
                        self.current_text_state.textbox_focused = false;
                        self.blinkey_visible = false;
                        self.text_dirty = true;
                        keyboard_action = -1;
                    }
                    // Hide avatar hint
                    if self.show_avatar_hint {
                        self.show_avatar_hint = false;
                    }
                }
            }
            // Window controls - not used on Android
            HoveredButton::Close | HoveredButton::Maximize | HoveredButton::Minimize => {}
        }

        // Handle contact tap if we're still hovering on one
        if let Some(contact_idx) = self.hovered_contact {
            if contact_idx < self.contacts.len() {
                self.selected_contact = Some(contact_idx);
                self.app_state = AppState::Conversation;
            }
        }

        // Clear hover state on touch up
        self.prev_hovered_button = self.hovered_button;
        self.hovered_button = HoveredButton::None;
        self.prev_hovered_contact = self.hovered_contact;
        self.hovered_contact = None;
        self.controls_dirty = true;

        keyboard_action
    }

    /// Handle text input from Android soft keyboard
    #[cfg(target_os = "android")]
    pub fn handle_text_input(&mut self, text: &str) {
        if !self.current_text_state.textbox_focused {
            return;
        }

        // Delete selection first if it exists
        if self.current_text_state.selection_anchor.is_some() {
            self.delete_selection();
        }

        let font_size = self.font_size();
        for ch in text.chars() {
            // Measure character width
            let width = self.text_renderer.measure_text_width(
                &ch.to_string(),
                font_size,
                theme::FONT_WEIGHT_USER_CONTENT,
                theme::FONT_USER_CONTENT,
            ) as usize;

            // Insert character with its width
            let blinkey_idx = self.current_text_state.blinkey_index;
            self.current_text_state.insert(blinkey_idx, ch, width);
            self.current_text_state.blinkey_index += 1;
        }

        // Update state
        if matches!(self.app_state, AppState::Launch(_)) {
            self.set_launch_state(LaunchState::Fresh);
        }
        self.text_dirty = true;
        self.glow_colour = theme::GLOW_DEFAULT;
        self.search_result = None;
        self.controls_dirty = true;
    }

    /// Handle backspace key from Android
    #[cfg(target_os = "android")]
    pub fn handle_backspace(&mut self) -> bool {
        if !self.current_text_state.textbox_focused {
            return false;
        }

        if self.current_text_state.selection_anchor.is_some() {
            self.delete_selection();
        } else if self.current_text_state.blinkey_index > 0 {
            let idx = self.current_text_state.blinkey_index - 1;
            self.current_text_state.remove(idx);
            self.current_text_state.blinkey_index -= 1;
        } else {
            return false;
        }

        if matches!(self.app_state, AppState::Launch(_)) {
            self.set_launch_state(LaunchState::Fresh);
        }
        self.text_dirty = true;
        self.glow_colour = theme::GLOW_DEFAULT;
        self.search_result = None;
        self.selection_dirty = true;
        self.controls_dirty = true;
        true
    }

    /// Handle enter key from Android
    #[cfg(target_os = "android")]
    pub fn handle_enter(&mut self) -> bool {
        if !self.current_text_state.textbox_focused || self.current_text_state.chars.is_empty() {
            return false;
        }

        match &self.app_state {
            AppState::Launch(LaunchState::Fresh) => {
                self.start_attestation();
            }
            AppState::Ready => {
                let handle: String = self.current_text_state.chars.iter().collect();
                if !handle.is_empty() {
                    self.start_handle_search(&handle);
                }
            }
            _ => return false,
        }
        true
    }

    /// Handle left arrow key from Android
    #[cfg(target_os = "android")]
    pub fn handle_arrow_left(&mut self) -> bool {
        if !self.current_text_state.textbox_focused {
            return false;
        }

        // Clear selection and move cursor left
        if self.current_text_state.selection_anchor.is_some() {
            let anchor = self.current_text_state.selection_anchor.unwrap();
            let left = anchor.min(self.current_text_state.blinkey_index);
            self.current_text_state.blinkey_index = left;
            self.current_text_state.selection_anchor = None;
            self.selection_dirty = true;
            self.controls_dirty = true;
            return true;
        }

        if self.current_text_state.blinkey_index > 0 {
            self.current_text_state.blinkey_index -= 1;
            self.selection_dirty = true;
            self.controls_dirty = true;
            return true;
        }
        false
    }

    /// Handle right arrow key from Android
    #[cfg(target_os = "android")]
    pub fn handle_arrow_right(&mut self) -> bool {
        if !self.current_text_state.textbox_focused {
            return false;
        }

        // Clear selection and move cursor right
        if self.current_text_state.selection_anchor.is_some() {
            let anchor = self.current_text_state.selection_anchor.unwrap();
            let right = anchor.max(self.current_text_state.blinkey_index);
            self.current_text_state.blinkey_index = right;
            self.current_text_state.selection_anchor = None;
            self.selection_dirty = true;
            self.controls_dirty = true;
            return true;
        }

        if self.current_text_state.blinkey_index < self.current_text_state.chars.len() {
            self.current_text_state.blinkey_index += 1;
            self.selection_dirty = true;
            self.controls_dirty = true;
            return true;
        }
        false
    }

    // ============ Network Methods ============

    /// Set the handle query system (called from JNI after keypair is available on Android,
    /// or can be used to reinitialize on any platform)
    pub fn set_handle_query(&mut self, handle_query: HandleQuery) {
        self.handle_query = Some(handle_query);
    }

    /// Set avatar from raw image file bytes (Android image picker)
    ///
    /// This receives the raw file bytes (JPEG/PNG/WebP) from Android's ContentResolver
    /// and passes them to encode_avatar_from_image() which properly handles ICC profiles
    /// for accurate color conversion to VSF RGB.
    #[cfg(target_os = "android")]
    pub fn set_avatar_from_file(&mut self, image_bytes: Vec<u8>) {
        use log::info;

        // Need handle to save avatar (storage key derived from handle)
        let handle = match &self.user_handle {
            Some(h) => h.clone(),
            None => {
                info!("Cannot save avatar: no handle (need to attest first)");
                return;
            }
        };

        info!("Processing avatar from picker: {} bytes", image_bytes.len());

        // Encode avatar using full ICC profile color management
        let av1_data = match crate::avatar::encode_avatar_from_image(&image_bytes) {
            Ok(data) => data,
            Err(e) => {
                info!("Avatar encoding failed: {}", e);
                return;
            }
        };

        info!("AV1 data size: {} bytes", av1_data.len());

        // Save avatar to local cache by handle's storage key
        if let Err(e) = crate::avatar::save_avatar(&av1_data, &handle) {
            info!("Failed to save avatar: {}", e);
            return;
        }

        // Read back from disk to verify end-to-end, convert to display colorspace
        let (_, pixels) = match crate::avatar::load_avatar(&handle) {
            Some(result) => result,
            None => {
                info!("Failed to load saved avatar");
                return;
            }
        };

        self.avatar_pixels =
            Some(crate::display_profile::DisplayConverter::new().convert_avatar(&pixels));
        self.avatar_scaled = None; // Invalidate cache to force re-scale
        self.window_dirty = true;

        info!("Avatar saved successfully");

        // Upload to FGTW
        if let Err(e) = crate::avatar::upload_avatar(&self.device_keypair.secret, &handle) {
            info!("Failed to upload avatar to FGTW: {}", e);
        } else {
            info!("Avatar uploaded to FGTW");
        }
    }

    /// Handle file hover during drag operation
    pub fn handle_file_hover(&mut self, _path: &std::path::Path) {
        // Only on Ready screen
        if !matches!(self.app_state, AppState::Ready | AppState::Searching) {
            eprintln!("File hover ignored - not on Ready screen");
            return;
        }

        // Check if mouse is over avatar circle
        let mx = self.mouse_x as usize;
        let my = self.mouse_y as usize;

        eprintln!("File hover at ({}, {})", mx, my);

        if mx < self.width as usize && my < self.height as usize {
            let idx = my * self.width as usize + mx;
            let hit = self.hit_test_map[idx];
            eprintln!("Hit test value: {}", hit);
            if hit == HIT_AVATAR {
                // Mouse is over avatar - set hover state
                if !self.file_hovering_avatar {
                    eprintln!("Setting file_hovering_avatar = true");
                    self.file_hovering_avatar = true;
                    self.window_dirty = true;
                }
                return;
            }
        }

        // Mouse not over avatar - clear hover state
        if self.file_hovering_avatar {
            eprintln!("Clearing file_hovering_avatar");
            self.file_hovering_avatar = false;
            self.window_dirty = true;
        }
    }

    /// Handle file hover cancelled
    pub fn handle_file_hover_cancelled(&mut self) {
        if self.file_hovering_avatar {
            self.file_hovering_avatar = false;
            self.window_dirty = true;
        }
    }

    /// Handle dropped file for avatar upload
    pub fn handle_dropped_file(&mut self, path: &std::path::Path) -> Result<(), String> {
        // Only accept file drops on Ready screen (avatar visible)
        if !matches!(self.app_state, AppState::Ready | AppState::Searching) {
            return Ok(()); // Silently ignore on other screens
        }

        eprintln!("Processing dropped file: {:?}", path);

        // Clear hover state
        self.file_hovering_avatar = false;

        // Read file
        let image_data = std::fs::read(path).map_err(|e| format!("Failed to read file: {}", e))?;

        // Need handle to save avatar (storage key derived from handle)
        let handle = self
            .user_handle
            .as_ref()
            .ok_or("Cannot save avatar: no handle (need to attest first)")?;

        // Encode avatar at fixed size, save to .vsf
        let av1_data = crate::avatar::encode_avatar_from_image(&image_data)?;
        eprintln!("AV1 data size: {} bytes", av1_data.len());

        // Save avatar to local cache by handle's storage key
        crate::avatar::save_avatar(&av1_data, handle)
            .map_err(|e| format!("Failed to save avatar: {}", e))?;

        // Read back from disk to verify end-to-end, convert to display colorspace
        let (_, pixels) =
            crate::avatar::load_avatar(handle).ok_or("Failed to load saved avatar")?;

        self.avatar_pixels =
            Some(crate::display_profile::DisplayConverter::new().convert_avatar(&pixels));
        self.avatar_scaled = None; // Invalidate cache to force re-scale
        self.show_avatar_hint = false; // Hide hint after successful upload
        self.window_dirty = true;

        // Upload to FGTW
        if let Err(e) = crate::avatar::upload_avatar(&self.device_keypair.secret, handle) {
            eprintln!("Avatar: Failed to upload to FGTW: {}", e);
        }

        eprintln!("Avatar saved successfully");

        Ok(())
    }

    #[cfg(not(target_os = "android"))]
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

    /// Calculate blinkey_index from click/tap X coordinate
    /// Returns the cursor position (0 = before first char, len = after last char)
    pub fn blinkey_index_from_x(&self, click_x: f32) -> usize {
        if self.current_text_state.chars.is_empty() {
            return 0;
        }

        let center_x = self.width as usize / 2;
        let total_text_width: usize = self.current_text_state.width;
        let text_half = total_text_width / 2;
        let text_start_x =
            center_x as f32 - text_half as f32 + self.current_text_state.scroll_offset;

        let mut x_offset = text_start_x;

        for (i, &char_width) in self.current_text_state.widths.iter().enumerate() {
            let char_center = x_offset + char_width as f32 / 2.0;
            if click_x < char_center {
                return i;
            }
            x_offset += char_width as f32;
        }

        self.current_text_state.chars.len()
    }

    /// Start attestation - compute handle_proof and announce to FGTW
    pub fn start_attestation(&mut self) {
        let handle: String = self.current_text_state.chars.iter().collect();

        // Store handle for computing handle_proof after success
        self.attesting_handle = Some(handle.clone());

        // Set status to Attesting and trigger attestation
        self.app_state = AppState::Launch(LaunchState::Attesting);
        self.glow_colour = theme::GLOW_ATTESTING; // Yellow for attesting
        if let Some(hq) = &self.handle_query {
            hq.query(handle);
        }
        let now = std::time::Instant::now();
        self.query_start_time = Some(now);
        self.last_frame_time = now; // Reset to prevent animation jerk on first frame
                                    // Initialize animation frame timing
        self.next_animation_frame =
            now + std::time::Duration::from_millis(self.target_frame_duration_ms);
    }

    /// Check if FGTW connectivity status is available and update fgtw_online
    pub fn check_fgtw_online(&mut self) {
        let online_opt = self
            .handle_query
            .as_ref()
            .and_then(|hq| hq.try_recv_online());
        if let Some(online) = online_opt {
            if online != self.fgtw_online {
                self.fgtw_online = online;
                self.controls_dirty = true; // Trigger indicator redraw
            }
        }
    }

    /// Search for a handle in our peer list (async - spawns background thread)
    pub fn start_handle_search(&mut self, handle: &str) {
        use crate::types::Handle;
        use std::sync::mpsc;

        // Set state to Searching and start animation
        self.app_state = AppState::Searching;
        self.searching_handle = Some(handle.to_string());
        self.glow_colour = theme::GLOW_ATTESTING; // Yellow for searching
        let now = std::time::Instant::now();
        self.query_start_time = Some(now);
        self.last_frame_time = now;
        self.next_animation_frame =
            now + std::time::Duration::from_millis(self.target_frame_duration_ms);

        // Create channel for result
        let (tx, rx) = mpsc::channel();
        self.search_receiver = Some(rx);

        // Get transport for peer lookup (need to clone Arc)
        let transport = self.handle_query.as_ref().and_then(|hq| hq.get_transport());
        let handle_owned = handle.to_string();

        // Spawn background thread to compute handle_proof and lookup
        std::thread::spawn(move || {
            // Compute handle_proof (~1 second)
            let handle_proof = Handle::username_to_handle_proof(&handle_owned);

            // Check peer store
            let result = if let Some(transport) = transport {
                let peer_store = transport.peer_store();
                let store = peer_store.lock().unwrap();

                let peers = store.get_devices_for_handle(&handle_proof);
                if let Some(peer) = peers.first() {
                    SearchResult::Found(FoundPeer {
                        handle: HandleText::new(&handle_owned),
                        device_pubkey: peer.device_pubkey.clone(),
                        ip: peer.ip,
                    })
                } else {
                    SearchResult::NotFound
                }
            } else {
                SearchResult::NotFound
            };

            let _ = tx.send(result);
        });

        self.window_dirty = true;
    }

    /// Check if search result is ready (non-blocking)
    pub fn check_search_result(&mut self) -> bool {
        if let Some(ref receiver) = self.search_receiver {
            if let Ok(result) = receiver.try_recv() {
                match &result {
                    SearchResult::Found(found_peer) => {
                        // Green glow
                        self.glow_colour = theme::GLOW_SUCCESS;

                        // Add to contacts if not already present
                        let already_exists =
                            self.contacts.iter().any(|c| c.handle == found_peer.handle);
                        if !already_exists {
                            let contact = Contact::new(
                                found_peer.handle.clone(),
                                found_peer.device_pubkey.clone(),
                            )
                            .with_ip(found_peer.ip);
                            self.contacts.push(contact);

                            // Update shared pubkey list for StatusChecker
                            {
                                let mut pubkeys = self.contact_pubkeys.lock().unwrap();
                                pubkeys.push(found_peer.device_pubkey.clone());
                                crate::log_info(&format!(
                                    "Contact added: {} ({})",
                                    found_peer.handle,
                                    hex::encode(&found_peer.device_pubkey.as_bytes()[..8])
                                ));
                            }

                            // Try to load avatar from local cache immediately
                            if let Some((_, pixels)) =
                                crate::avatar::load_avatar(found_peer.handle.as_str())
                            {
                                if let Some(contact) = self.contacts.last_mut() {
                                    contact.avatar_pixels = Some(
                                        crate::display_profile::DisplayConverter::new()
                                            .convert_avatar(&pixels),
                                    );
                                    crate::log_info(&format!(
                                        "Avatar: Loaded {} from local cache on add",
                                        found_peer.handle
                                    ));
                                }
                            } else {
                                // Not in cache - fetch from FGTW
                                crate::log_info(&format!(
                                    "Avatar: {} not in cache, fetching from FGTW",
                                    found_peer.handle
                                ));
                                crate::avatar::download_avatar_background(
                                    found_peer.handle.as_str().to_string(),
                                    self.contact_avatar_tx.clone(),
                                );
                            }
                        }

                        // Clear textbox
                        self.current_text_state.chars.clear();
                        self.current_text_state.widths.clear();
                        self.current_text_state.width = 0;
                        self.current_text_state.blinkey_index = 0;
                        self.current_text_state.selection_anchor = None;
                        self.current_text_state.scroll_offset = 0.0;
                        self.current_text_state.is_empty = true;
                        self.text_dirty = true;
                        self.controls_dirty = true;
                        self.selection_dirty = true;
                    }
                    SearchResult::NotFound | SearchResult::Error(_) => {
                        // Red glow, keep text in box
                        self.glow_colour = theme::GLOW_ERROR;
                    }
                }

                self.search_result = Some(result);
                self.search_receiver = None;
                self.searching_handle = None;
                self.query_start_time = None;
                self.app_state = AppState::Ready;
                self.window_dirty = true;
                return true;
            }
        }
        false
    }

    /// Check if attestation response is ready and update app_state
    pub fn check_attestation_response(&mut self) -> bool {
        let result = self.handle_query.as_ref().and_then(|hq| hq.try_recv());
        let Some(result) = result else { return false };

        crate::log_info(&format!("UI: Received attestation result: {:?}", result));

        let new_state = match result {
            QueryResult::Success => {
                crate::log_info("UI: Attestation SUCCESS - transitioning to Ready state");
                AppState::Ready
            }
            QueryResult::AlreadyAttested(_peers) => {
                crate::log_info("UI: Handle already attested - showing error");
                AppState::Launch(LaunchState::Error("Handle already attested".to_string()))
            }
            QueryResult::Error(msg) => {
                crate::log_error(&format!("UI: Attestation error - {}", msg));
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
            self.current_text_state.widths.clear();
            self.current_text_state.width = 0;
            self.current_text_state.blinkey_index = 0;
            self.current_text_state.selection_anchor = None;
            self.current_text_state.scroll_offset = 0.0;
            self.current_text_state.is_empty = true;
            self.text_dirty = true;
            self.controls_dirty = true;
            self.selection_dirty = true;

            // Store handle_proof for periodic refresh and save user handle for display
            if let Some(ref handle) = self.attesting_handle {
                use crate::types::Handle;
                let handle_proof = Handle::username_to_handle_proof(handle);
                if let Some(hq) = &self.handle_query {
                    hq.set_handle_proof(handle_proof, handle);
                }
                self.user_handle = Some(handle.clone());
                crate::log_info("UI: Stored handle_proof for periodic refresh");

                // Load avatar from local cache now that we have a handle
                if let Some((_, pixels)) = crate::avatar::load_avatar(handle) {
                    self.avatar_pixels = Some(
                        crate::display_profile::DisplayConverter::new().convert_avatar(&pixels),
                    );
                    self.avatar_scaled = None; // Force re-scale
                    crate::log_info("UI: Loaded avatar from local cache");
                } else {
                    // Not in local cache - try to fetch from FGTW
                    crate::log_info("UI: Avatar not in local cache, fetching from FGTW");
                    crate::avatar::download_avatar_background(
                        handle.clone(),
                        self.event_proxy.clone(),
                    );
                }
            }
            self.attesting_handle = None;

            // Initialize status checker for P2P contact pinging
            if self.user_handle.is_some() {
                if let Some(hq) = &self.handle_query {
                    self.status_checker = StatusChecker::new(
                        hq.socket().clone(),
                        self.device_keypair.clone(),
                        self.contact_pubkeys.clone(),
                    )
                    .ok();
                    crate::log_info("UI: Status checker initialized after attestation");
                }
            }

            // Schedule first FGTW refresh in 60-120 seconds
            {
                use rand::Rng;
                let delay = rand::thread_rng().gen_range(60..=120);
                self.next_fgtw_refresh =
                    std::time::Instant::now() + std::time::Duration::from_secs(delay);
            }
        }

        // Set glow colour based on new state
        if matches!(new_state, AppState::Launch(LaunchState::Error(_))) {
            self.glow_colour = theme::GLOW_ERROR;
        } else {
            self.glow_colour = theme::GLOW_DEFAULT;
        }
        self.app_state = new_state;
        self.query_start_time = None;
        self.window_dirty = true;
        crate::log_info("UI: Attestation complete, window marked dirty for redraw");
        true
    }

    /// Check if we should continuously animate (request redraws every frame)
    pub fn should_animate(&self) -> bool {
        matches!(
            self.app_state,
            AppState::Launch(LaunchState::Attesting) | AppState::Searching
        )
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

    /// Check for status updates from P2P checker (non-blocking)
    /// Returns true if any contact status changed
    pub fn check_status_updates(&mut self) -> bool {
        let checker = match &self.status_checker {
            Some(c) => c,
            None => return false,
        };

        let mut changed = false;
        while let Some(update) = checker.try_recv() {
            // Find matching contact and update status
            for contact in &mut self.contacts {
                if contact.public_identity == update.peer_pubkey {
                    if contact.is_online != update.is_online {
                        contact.is_online = update.is_online;
                        changed = true;
                        crate::log_info(&format!(
                            "Status: {} is now {}",
                            contact.handle,
                            if update.is_online {
                                "ONLINE"
                            } else {
                                "offline"
                            }
                        ));
                    }

                    // Fetch avatar by handle if we don't have it yet and contact is online
                    // Storage key is deterministic: BLAKE3(BLAKE3(handle) || "avatar")
                    if update.is_online && contact.avatar_pixels.is_none() {
                        crate::log_info(&format!(
                            "Avatar: {} is online, fetching avatar by handle from FGTW",
                            contact.handle
                        ));
                        // Spawn background download using handle-based storage key
                        crate::avatar::download_avatar_background(
                            contact.handle.as_str().to_string(),
                            self.contact_avatar_tx.clone(),
                        );
                    }
                    break;
                }
            }
        }
        changed
    }

    /// Check for completed avatar downloads and update contacts
    /// Returns true if any avatars were updated
    pub fn check_avatar_downloads(&mut self) -> bool {
        let mut changed = false;
        while let Ok(result) = self.contact_avatar_rx.try_recv() {
            // Find matching contact by handle
            for contact in &mut self.contacts {
                if contact.handle.as_str() == result.handle {
                    if let Some(pixels) = result.pixels {
                        // Convert to display colorspace and store
                        let display_pixels =
                            crate::display_profile::DisplayConverter::new().convert_avatar(&pixels);
                        contact.avatar_pixels = Some(display_pixels);
                        contact.avatar_scaled = None; // Invalidate scaled cache
                        contact.avatar_scaled_diameter = 0;
                        changed = true;
                        crate::log_info(&format!(
                            "Avatar: {} loaded ({} bytes)",
                            contact.handle,
                            pixels.len()
                        ));
                    } else {
                        crate::log_info(&format!("Avatar: {} download failed", contact.handle));
                    }
                    break;
                }
            }
        }
        if changed {
            self.window_dirty = true;
        }
        changed
    }

    /// Ping all contacts that have IP addresses (call periodically)
    pub fn ping_contacts(&mut self) {
        let checker = match &self.status_checker {
            Some(c) => c,
            None => {
                crate::log_info("Status: No checker available!");
                return;
            }
        };

        let mut pinged = 0;
        for contact in &self.contacts {
            if let Some(ip) = contact.ip {
                checker.ping(ip, contact.public_identity.clone());
                pinged += 1;
            }
        }
        if pinged > 0 {
            crate::log_info(&format!("Status: Pinged {} contact(s)", pinged));
        }
    }

    /// Check if it's time to ping contacts and do so
    /// Returns true if pings were sent
    pub fn maybe_ping_contacts(&mut self) -> bool {
        let now = std::time::Instant::now();
        if now >= self.next_status_ping && !self.contacts.is_empty() {
            self.ping_contacts();
            // Ping every 5-15 seconds (randomized to avoid synchronized traffic)
            use rand::Rng;
            let delay = rand::thread_rng().gen_range(5..=15);
            self.next_status_ping = now + std::time::Duration::from_secs(delay);
            true
        } else {
            false
        }
    }

    /// Check if it's time to refresh FGTW and do so
    /// Returns true if refresh was triggered
    pub fn maybe_refresh_fgtw(&mut self) -> bool {
        let now = std::time::Instant::now();
        if now >= self.next_fgtw_refresh && matches!(self.app_state, AppState::Ready) {
            if let Some(hq) = &self.handle_query {
                if hq.refresh() {
                    crate::log_info("Network: Triggering FGTW refresh");
                    // Refresh every 60-120 seconds (randomized to avoid synchronized traffic)
                    use rand::Rng;
                    let delay = rand::thread_rng().gen_range(60..=120);
                    self.next_fgtw_refresh = now + std::time::Duration::from_secs(delay);
                    return true;
                }
            }
        }
        false
    }

    /// Check for FGTW refresh results and update contact IPs
    /// Returns true if any contacts were updated
    pub fn check_refresh_result(&mut self) -> bool {
        let result = self
            .handle_query
            .as_ref()
            .and_then(|hq| hq.try_recv_refresh());
        let Some(result) = result else { return false };

        if let Some(ref error) = result.error {
            crate::log_error(&format!("Network: Refresh error: {}", error));
        }

        if result.peers.is_empty() {
            return false;
        }

        crate::log_info(&format!(
            "Network: Refresh got {} peer(s)",
            result.peers.len()
        ));

        // Update contact IPs from fresh peer data
        let mut updated = 0;
        for peer in &result.peers {
            for contact in &mut self.contacts {
                if contact.public_identity == peer.device_pubkey {
                    if contact.ip != Some(peer.ip) {
                        crate::log_info(&format!(
                            "Network: Updated {} IP: {:?} -> {}",
                            contact.handle, contact.ip, peer.ip
                        ));
                        contact.ip = Some(peer.ip);
                        updated += 1;
                    }
                    break;
                }
            }
        }

        if updated > 0 {
            crate::log_info(&format!("Network: Updated {} contact IP(s)", updated));
        }

        updated > 0
    }
}
