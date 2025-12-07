use super::renderer::Renderer;
use super::text_rasterizing::TextRenderer;
use super::theme;
#[cfg(not(target_os = "android"))]
use super::PhotonEvent;
use crate::crypto::chain::MessageChain;
use crate::crypto::clutch::ClutchAllKeypairs;
use crate::network::StatusChecker;
use crate::network::{HandleQuery, QueryResult};
use crate::types::{ChatMessage, Contact, ContactId, EncryptedMessage, HandleText};
use std::collections::HashMap;

/// Result from background CLUTCH keypair generation
pub struct ClutchKeygenResult {
    pub contact_id: ContactId,
    pub keypairs: ClutchAllKeypairs,
}
#[cfg(not(target_os = "android"))]
use winit::{
    dpi::PhysicalSize, event_loop::EventLoopProxy, keyboard::ModifiersState, window::Window,
};
use zeroize::Zeroize;

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
    pub handle_proof: [u8; 32], // Cached handle_proof (expensive - ~1 second to compute)
    pub device_pubkey: crate::types::DevicePubkey,
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
    pub hourglass_angle: f32, // Hourglass rotation (degrees), stochastic wobble during search
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
    pub user_handle_proof: Option<[u8; 32]>, // Our handle_proof (for CLUTCH initiator check)
    pub user_identity_seed: Option<[u8; 32]>, // BLAKE3(handle) for storage encryption key derivation
    pub show_avatar_hint: bool,               // Show "drag and drop" hint after clicking avatar
    pub file_hovering_avatar: bool,           // Track if file is being dragged over avatar

    // Contact avatar fetching (background thread)
    pub contact_avatar_rx: std::sync::mpsc::Receiver<crate::avatar::AvatarDownloadResult>,
    pub contact_avatar_tx: std::sync::mpsc::Sender<crate::avatar::AvatarDownloadResult>,

    // Background CLUTCH keypair generation (McEliece is slow, do it off main thread)
    pub clutch_keygen_rx: std::sync::mpsc::Receiver<ClutchKeygenResult>,
    pub clutch_keygen_tx: std::sync::mpsc::Sender<ClutchKeygenResult>,

    // Device keypair for signing (needed by StatusChecker)
    pub device_keypair: crate::network::fgtw::Keypair,

    // Message encryption chains (keyed by contact ID, runtime-only, not serialized)
    pub message_chains: HashMap<ContactId, MessageChain>,

    // Event loop proxy for waking event loop on network updates (desktop only)
    #[cfg(not(target_os = "android"))]
    pub event_proxy: EventLoopProxy<PhotonEvent>,

    // WebSocket client for real-time peer IP updates (desktop only)
    #[cfg(not(target_os = "android"))]
    pub peer_update_client: Option<crate::network::PeerUpdateClient>,
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

        // Create channel for background CLUTCH keypair generation
        let (clutch_keygen_tx, clutch_keygen_rx) = std::sync::mpsc::channel();

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
            hourglass_angle: 0.0,
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
            user_handle_proof: None,
            user_identity_seed: None,
            show_avatar_hint: false,
            file_hovering_avatar: false,
            contact_avatar_rx,
            contact_avatar_tx,
            clutch_keygen_rx,
            clutch_keygen_tx,
            message_chains: HashMap::new(),
            event_proxy: event_proxy.clone(),
            peer_update_client: None, // Started after attestation
        };

        // Initialize handle_query with the derived keypair
        {
            use crate::network::fgtw::PeerStore;
            use crate::network::HandleQuery;

            let handle_query = HandleQuery::new(app.device_keypair.clone(), event_proxy.clone());
            let peer_store = std::sync::Arc::new(std::sync::Mutex::new(PeerStore::new()));
            handle_query.set_transport(peer_store);
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

        // Create channel for background CLUTCH keypair generation
        let (clutch_keygen_tx, clutch_keygen_rx) = std::sync::mpsc::channel();

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
            hourglass_angle: 0.0,
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
            user_handle_proof: None,
            user_identity_seed: None,
            show_avatar_hint: false,
            file_hovering_avatar: false,
            contact_avatar_rx,
            contact_avatar_tx,
            clutch_keygen_rx,
            clutch_keygen_tx,
            message_chains: HashMap::new(),
        }
    }

    /// Update the fullscreen/maximized state
    /// When true, window edges are not drawn
    pub fn set_fullscreen(&mut self, is_fullscreen: bool) {
        if self.is_fullscreen != is_fullscreen {
            self.is_fullscreen = is_fullscreen;
        }
    }

    /// Reset textbox state when changing screens
    /// Clears text content, hides blinkey, unfocuses textbox
    pub fn reset_textbox(&mut self) {
        self.current_text_state.chars.clear();
        self.current_text_state.widths.clear();
        self.current_text_state.width = 0;
        self.current_text_state.blinkey_index = 0;
        self.current_text_state.selection_anchor = None;
        self.current_text_state.scroll_offset = 0.0;
        self.current_text_state.is_empty = true;
        self.current_text_state.textbox_focused = false;
        self.blinkey_visible = false;
        self.text_dirty = true;
        self.selection_dirty = true;
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
                self.reset_textbox();
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
                self.reset_textbox();
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

    /// Handle Android back button
    /// Returns true if handled (stay in app), false to allow default back behavior (exit)
    #[cfg(target_os = "android")]
    pub fn handle_back(&mut self) -> bool {
        // If in a chat, go back to contacts list (same as tapping back header button)
        if self.selected_contact.is_some() {
            self.app_state = AppState::Ready;
            self.selected_contact = None;
            self.reset_textbox();
            self.window_dirty = true;
            return true; // Handled - don't exit
        }

        // On contacts screen - allow default back (exit app)
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

        // Upload to FGTW (only if we have handle_proof)
        if let Some(ref handle_proof) = self.user_handle_proof {
            if let Err(e) =
                crate::avatar::upload_avatar(&self.device_keypair.secret, &handle, handle_proof)
            {
                info!("Failed to upload avatar to FGTW: {}", e);
            } else {
                info!("Avatar uploaded to FGTW");
            }
        } else {
            info!("Skipping avatar upload - no handle_proof yet");
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

        // Upload to FGTW (only if we have handle_proof)
        if let Some(ref handle_proof) = self.user_handle_proof {
            if let Err(e) =
                crate::avatar::upload_avatar(&self.device_keypair.secret, handle, handle_proof)
            {
                eprintln!("Avatar: Failed to upload to FGTW: {}", e);
            }
        } else {
            eprintln!("Avatar: Skipping FGTW upload - no handle_proof yet");
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

        // Disable textbox and hide blinkey during attestation
        self.current_text_state.textbox_focused = false;
        self.blinkey_visible = false;

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

        // Disable textbox and hide blinkey during search
        self.current_text_state.textbox_focused = false;
        self.blinkey_visible = false;

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
            let result = if let Some(peer_store) = transport {
                let store = peer_store.lock().unwrap();

                let peers = store.get_devices_for_handle(&handle_proof);
                if let Some(peer) = peers.first() {
                    SearchResult::Found(FoundPeer {
                        handle: HandleText::new(&handle_owned),
                        handle_proof,
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
                                found_peer.handle_proof,
                                found_peer.device_pubkey.clone(),
                            )
                            .with_ip(found_peer.ip);
                            let contact_id = contact.id.clone();
                            self.contacts.push(contact);

                            // Start background CLUTCH keypair generation immediately
                            // McEliece is slow, so we do this off the main thread
                            self.spawn_clutch_keygen(contact_id);

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

                            // Fetch avatar immediately for new contact
                            let handle = found_peer.handle.as_str().to_string();
                            #[cfg(not(target_os = "android"))]
                            crate::avatar::download_avatar_background(
                                handle.clone(),
                                self.contact_avatar_tx.clone(),
                                Some(self.event_proxy.clone()),
                            );
                            #[cfg(target_os = "android")]
                            crate::avatar::download_avatar_background(
                                handle,
                                self.contact_avatar_tx.clone(),
                                None,
                            );

                            // Save contact (updates both state file and contact list)
                            if let Some(ref identity_seed) = self.user_identity_seed {
                                let device_secret = self.device_keypair.secret.as_bytes();
                                if let Some(contact) = self.contacts.last() {
                                    if let Err(e) = crate::storage::contacts::save_contact(
                                        contact,
                                        identity_seed,
                                        device_secret,
                                    ) {
                                        crate::log_error(&format!("Failed to save contact: {}", e));
                                    }
                                }

                                // Sync contacts to cloud
                                if let Some(ref handle_proof) = self.user_handle_proof {
                                    if let Err(e) = crate::storage::cloud::sync_contacts_to_cloud(
                                        &self.contacts,
                                        identity_seed,
                                        &self.device_keypair,
                                        handle_proof,
                                    ) {
                                        crate::log_error(&format!(
                                            "Failed to sync contacts to cloud: {}",
                                            e
                                        ));
                                    }
                                }
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
                                #[cfg(not(target_os = "android"))]
                                crate::avatar::download_avatar_background(
                                    found_peer.handle.as_str().to_string(),
                                    self.contact_avatar_tx.clone(),
                                    Some(self.event_proxy.clone()),
                                );
                                #[cfg(target_os = "android")]
                                crate::avatar::download_avatar_background(
                                    found_peer.handle.as_str().to_string(),
                                    self.contact_avatar_tx.clone(),
                                    None,
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

                        // Immediately ping to check if online
                        // CLUTCH starts when PONG confirms they're online
                        if let Some(checker) = &self.status_checker {
                            if let Some(contact) = self.contacts.last() {
                                if let Some(ip) = contact.ip {
                                    crate::log_info(&format!(
                                        "Status: Immediately pinging {} (on add)",
                                        contact.handle
                                    ));
                                    checker.ping(ip, contact.public_identity.clone());
                                }
                            }
                        }
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

        let (new_state, initial_peers) = match result {
            QueryResult::Success(peers) => {
                crate::log_info("UI: Attestation SUCCESS - transitioning to Ready state");
                (AppState::Ready, peers)
            }
            QueryResult::AlreadyAttested(_peers) => {
                crate::log_info("UI: Handle already attested - showing error");
                (
                    AppState::Launch(LaunchState::Error("Handle already attested".to_string())),
                    vec![],
                )
            }
            QueryResult::Error(msg) => {
                crate::log_error(&format!("UI: Attestation error - {}", msg));
                (AppState::Launch(LaunchState::Error(msg)), vec![])
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

            // Store handle_proof for periodic refresh, CLUTCH, and save user handle for display
            if let Some(ref handle) = self.attesting_handle {
                use crate::types::Handle;
                let handle_proof = Handle::username_to_handle_proof(handle);
                // Derive identity_seed using VSF normalization for consistent key derivation
                let identity_seed = crate::storage::contacts::derive_identity_seed(handle);
                if let Some(hq) = &self.handle_query {
                    hq.set_handle_proof(handle_proof, handle);
                }
                self.user_handle = Some(handle.clone());
                self.user_handle_proof = Some(handle_proof);
                self.user_identity_seed = Some(identity_seed);
                crate::log_info(
                    "UI: Stored handle_proof and identity_seed for refresh/CLUTCH/storage",
                );

                // Load contacts from encrypted storage
                let device_secret = self.device_keypair.secret.as_bytes();
                let loaded_contacts =
                    crate::storage::contacts::load_all_contacts(&identity_seed, device_secret);
                if !loaded_contacts.is_empty() {
                    crate::log_info(&format!(
                        "UI: Loaded {} contacts from storage",
                        loaded_contacts.len()
                    ));
                    for contact in loaded_contacts {
                        // Update shared pubkey list for StatusChecker
                        {
                            let mut pubkeys = self.contact_pubkeys.lock().unwrap();
                            pubkeys.push(contact.public_identity.clone());
                        }
                        self.contacts.push(contact);
                    }

                    // Proactive avatar loading: fetch all contact avatars immediately
                    // This makes the contact list snappy - avatars ready before user clicks
                    crate::log_info(&format!(
                        "Avatar: Proactively fetching avatars for {} contact(s)",
                        self.contacts.len()
                    ));
                    for contact in &self.contacts {
                        if contact.avatar_pixels.is_none() {
                            let handle = contact.handle.as_str().to_string();
                            #[cfg(not(target_os = "android"))]
                            crate::avatar::download_avatar_background(
                                handle,
                                self.contact_avatar_tx.clone(),
                                Some(self.event_proxy.clone()),
                            );
                            #[cfg(target_os = "android")]
                            crate::avatar::download_avatar_background(
                                handle,
                                self.contact_avatar_tx.clone(),
                                None,
                            );
                        }
                    }
                }

                // Load local avatar for immediate display (if exists)
                if let Some((_, pixels)) = crate::avatar::load_avatar(handle) {
                    self.avatar_pixels = Some(
                        crate::display_profile::DisplayConverter::new().convert_avatar(&pixels),
                    );
                    self.avatar_scaled = None; // Force re-scale
                    crate::log_info("UI: Loaded avatar from local cache");
                }

                // Start bidirectional sync in background (newest wins)
                // This will upload if local is newer, or download if server is newer
                crate::log_info("UI: Starting bidirectional avatar sync with FGTW");
                #[cfg(not(target_os = "android"))]
                crate::avatar::sync_avatar_background(
                    *self.device_keypair.secret.as_bytes(),
                    handle.clone(),
                    self.user_handle_proof,
                    self.contact_avatar_tx.clone(),
                    Some(self.event_proxy.clone()),
                );
                #[cfg(target_os = "android")]
                crate::avatar::sync_avatar_background(
                    *self.device_keypair.secret.as_bytes(),
                    handle.clone(),
                    self.user_handle_proof,
                    self.contact_avatar_tx.clone(),
                    None,
                );

                // Sync contacts with cloud (check if cloud has more contacts)
                crate::log_info("UI: Checking cloud contacts sync");
                match crate::storage::cloud::load_contacts_from_cloud(
                    &identity_seed,
                    &self.device_keypair,
                ) {
                    Ok(Some(cloud_contacts)) => {
                        let local_count = self.contacts.len();
                        let cloud_count = cloud_contacts.len();
                        crate::log_info(&format!(
                            "Cloud: {} contacts (local: {})",
                            cloud_count, local_count
                        ));

                        // Simple merge: add any cloud contacts we don't have locally
                        let mut added = 0;
                        for cc in cloud_contacts {
                            let exists = self
                                .contacts
                                .iter()
                                .any(|c| c.handle_proof == cc.handle_proof);
                            if !exists {
                                let contact = cc.to_contact();
                                // Update shared pubkey list
                                {
                                    let mut pubkeys = self.contact_pubkeys.lock().unwrap();
                                    pubkeys.push(contact.public_identity.clone());
                                }
                                // Save to local storage
                                let device_secret = self.device_keypair.secret.as_bytes();
                                if let Err(e) = crate::storage::contacts::save_contact(
                                    &contact,
                                    &identity_seed,
                                    device_secret,
                                ) {
                                    crate::log_error(&format!(
                                        "Failed to save cloud contact locally: {}",
                                        e
                                    ));
                                }

                                // Fetch avatar for cloud-synced contact
                                let handle = contact.handle.as_str().to_string();
                                #[cfg(not(target_os = "android"))]
                                crate::avatar::download_avatar_background(
                                    handle,
                                    self.contact_avatar_tx.clone(),
                                    Some(self.event_proxy.clone()),
                                );
                                #[cfg(target_os = "android")]
                                crate::avatar::download_avatar_background(
                                    handle,
                                    self.contact_avatar_tx.clone(),
                                    None,
                                );

                                self.contacts.push(contact);
                                added += 1;
                            }
                        }
                        if added > 0 {
                            crate::log_info(&format!("Cloud: Added {} contacts from cloud", added));
                        }

                        // If we have more contacts locally, upload to cloud
                        if self.contacts.len() > cloud_count {
                            crate::log_info("Cloud: Uploading local contacts to cloud");
                            if let Err(e) = crate::storage::cloud::sync_contacts_to_cloud(
                                &self.contacts,
                                &identity_seed,
                                &self.device_keypair,
                                &handle_proof,
                            ) {
                                crate::log_error(&format!(
                                    "Failed to sync contacts to cloud: {}",
                                    e
                                ));
                            }
                        }
                    }
                    Ok(None) => {
                        // No cloud contacts yet - upload if we have any
                        if !self.contacts.is_empty() {
                            crate::log_info(&format!(
                                "Cloud: No cloud contacts, uploading {} local contacts",
                                self.contacts.len()
                            ));
                            if let Err(e) = crate::storage::cloud::sync_contacts_to_cloud(
                                &self.contacts,
                                &identity_seed,
                                &self.device_keypair,
                                &handle_proof,
                            ) {
                                crate::log_error(&format!(
                                    "Failed to upload contacts to cloud: {}",
                                    e
                                ));
                            }
                        }
                    }
                    Err(e) => {
                        crate::log_error(&format!("Failed to load cloud contacts: {}", e));
                    }
                }
            }
            self.attesting_handle = None;

            // Initialize status checker for P2P contact pinging
            if self.user_handle.is_some() {
                if let Some(hq) = &self.handle_query {
                    #[cfg(not(target_os = "android"))]
                    {
                        self.status_checker = StatusChecker::new(
                            hq.socket().clone(),
                            self.device_keypair.clone(),
                            self.contact_pubkeys.clone(),
                            self.event_proxy.clone(),
                        )
                        .ok();
                    }
                    #[cfg(target_os = "android")]
                    {
                        self.status_checker = StatusChecker::new(
                            hq.socket().clone(),
                            self.device_keypair.clone(),
                            self.contact_pubkeys.clone(),
                        )
                        .ok();
                    }
                    crate::log_info("UI: Status checker initialized after attestation");

                    // Start WebSocket client for real-time peer IP updates (desktop only)
                    #[cfg(not(target_os = "android"))]
                    {
                        use crate::network::PeerUpdateClient;
                        self.peer_update_client =
                            Some(PeerUpdateClient::new(self.event_proxy.clone()));
                        crate::log_info("UI: PeerUpdateClient started for real-time IP updates");
                    }

                    // Broadcast StatusPing to all peers so they learn our IP (NAT hole punching)
                    if !initial_peers.is_empty() {
                        if let Some(ref checker) = self.status_checker {
                            for peer in &initial_peers {
                                checker.ping(peer.ip, peer.device_pubkey.clone());
                            }
                            crate::log_info(&format!(
                                "Network: Initial broadcast ping to {} peer(s)",
                                initial_peers.len()
                            ));
                        }
                    }
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
        use crate::crypto::clutch;
        use crate::network::status::{ClutchRequest, ClutchRequestType, StatusUpdate};
        use crate::types::ClutchState;

        let checker = match &self.status_checker {
            Some(c) => c,
            None => return false,
        };

        // Get our handle_proof for CLUTCH (needed for initiator check and routing)
        let our_handle_proof = match self.user_handle_proof {
            Some(hp) => hp,
            None => return false, // Can't do CLUTCH without our handle_proof
        };

        // Get our private identity_seed for seed derivation
        // Formula: BLAKE3(VsfType::x(handle).flatten()) - VSF normalized for Unicode safety
        // SECURITY: This is PRIVATE and never sent over the wire!
        let our_identity_seed = match &self.user_handle {
            Some(h) => crate::storage::contacts::derive_identity_seed(h),
            None => return false, // Can't do CLUTCH without our handle
        };

        let mut changed = false;
        while let Some(update) = checker.try_recv() {
            match update {
                StatusUpdate::Online {
                    peer_pubkey,
                    is_online,
                    peer_addr,
                } => {
                    // Find matching contact and update status
                    for contact in &mut self.contacts {
                        if contact.public_identity == peer_pubkey {
                            // Update IP from the ping/pong source address
                            if let Some(addr) = peer_addr {
                                if contact.ip != Some(addr) {
                                    crate::log_info(&format!(
                                        "Status: Updated {} IP from ping/pong: {:?} -> {}",
                                        contact.handle, contact.ip, addr
                                    ));
                                    contact.ip = Some(addr);
                                }
                            }

                            if contact.is_online != is_online {
                                contact.is_online = is_online;
                                changed = true;
                                crate::log_info(&format!(
                                    "Status: {} is now {}",
                                    contact.handle,
                                    if is_online { "ONLINE" } else { "offline" }
                                ));
                            }

                            // Fetch avatar by handle if we don't have it yet and contact is online
                            if is_online && contact.avatar_pixels.is_none() {
                                crate::log_info(&format!(
                                    "Avatar: {} is online, fetching avatar by handle from FGTW",
                                    contact.handle
                                ));
                                #[cfg(not(target_os = "android"))]
                                crate::avatar::download_avatar_background(
                                    contact.handle.as_str().to_string(),
                                    self.contact_avatar_tx.clone(),
                                    Some(self.event_proxy.clone()),
                                );
                                #[cfg(target_os = "android")]
                                crate::avatar::download_avatar_background(
                                    contact.handle.as_str().to_string(),
                                    self.contact_avatar_tx.clone(),
                                    None,
                                );
                            }

                            // Trigger CLUTCH if contact is online and we're in Pending state
                            // Parallel v2: Both parties send ClutchOffer immediately
                            if is_online && contact.clutch_state == ClutchState::Pending {
                                let their_handle_proof = contact.handle_proof;

                                // Generate ephemeral keypair and send ClutchOffer
                                let (secret, pubkey) = clutch::generate_x25519_ephemeral();
                                contact.clutch_our_ephemeral_secret = Some(secret);
                                contact.clutch_our_ephemeral_pubkey = Some(pubkey);
                                contact.clutch_state = ClutchState::OfferSent;
                                changed = true;

                                if let Some(ip) = contact.ip {
                                    crate::log_info(&format!(
                                        "CLUTCH: Sending offer to {} (parallel v2)",
                                        contact.handle
                                    ));
                                    checker.send_clutch(ClutchRequest {
                                        peer_addr: ip,
                                        our_handle_proof,
                                        their_handle_proof,
                                        message: ClutchRequestType::Offer {
                                            ephemeral_pubkey: pubkey,
                                        },
                                    });
                                }
                            }
                            break;
                        }
                    }
                }
                // Parallel v2: ClutchOffer handler
                StatusUpdate::ClutchOffer {
                    from_handle_proof,
                    to_handle_proof,
                    ephemeral_pubkey,
                    sender_addr,
                } => {
                    // Check if this is addressed to us
                    if to_handle_proof != our_handle_proof {
                        continue;
                    }

                    crate::log_info(&format!(
                        "CLUTCH: Received Offer from {} (from_hp: {}...)",
                        sender_addr,
                        hex::encode(&from_handle_proof[..8])
                    ));

                    // Find contact by their handle_proof
                    let mut found_contact = false;
                    for contact in &mut self.contacts {
                        #[cfg(feature = "development")]
                        crate::log_info(&format!(
                            "CLUTCH: Comparing with contact '{}' (hp: {}...)",
                            contact.handle,
                            hex::encode(&contact.handle_proof[..8])
                        ));
                        if contact.handle_proof == from_handle_proof {
                            found_contact = true;
                            // Store their ephemeral pubkey
                            contact.clutch_their_ephemeral_pubkey = Some(ephemeral_pubkey);
                            contact.ip = Some(sender_addr); // Update IP in case it changed

                            // If Complete and receiving new Offer, peer lost state - re-key
                            // Reset to Pending so we can re-run CLUTCH with fresh ephemerals
                            if contact.clutch_state == ClutchState::Complete {
                                crate::log_info(&format!(
                                    "CLUTCH: Re-key requested by {} (they sent new Offer while we're Complete)",
                                    contact.handle
                                ));
                                // Clear old CLUTCH state, keep relationship_seed until new one is derived
                                contact.clutch_state = ClutchState::Pending;
                                contact.clutch_our_ephemeral_secret = None;
                                contact.clutch_our_ephemeral_pubkey = None;
                                // Keep their new ephemeral, will use it below
                            }

                            // If we're in Pending, we haven't sent our offer yet - send it now
                            if contact.clutch_state == ClutchState::Pending {
                                let (secret, pubkey) = clutch::generate_x25519_ephemeral();
                                contact.clutch_our_ephemeral_secret = Some(secret);
                                contact.clutch_our_ephemeral_pubkey = Some(pubkey);
                                contact.clutch_state = ClutchState::OfferSent;
                                changed = true;

                                crate::log_info(&format!(
                                    "CLUTCH: Sending offer back to {}",
                                    contact.handle
                                ));
                                checker.send_clutch(ClutchRequest {
                                    peer_addr: sender_addr,
                                    our_handle_proof,
                                    their_handle_proof: contact.handle_proof,
                                    message: ClutchRequestType::Offer {
                                        ephemeral_pubkey: pubkey,
                                    },
                                });
                            }

                            // Check if we have both pubkeys - if so, complete the ceremony
                            if let (Some(our_secret), Some(our_pub), Some(their_pub)) = (
                                &contact.clutch_our_ephemeral_secret,
                                &contact.clutch_our_ephemeral_pubkey,
                                &contact.clutch_their_ephemeral_pubkey,
                            ) {
                                // Both parties have exchanged offers - derive seed
                                // v3: Include device pubkeys for identity binding
                                let our_device_pub = *self.device_keypair.public.as_bytes();
                                let their_device_pub = *contact.public_identity.as_bytes();
                                let result = clutch::clutch_complete_parallel(
                                    &our_device_pub,
                                    &their_device_pub,
                                    &our_identity_seed,
                                    &contact.handle_hash,
                                    our_secret,
                                    our_pub,
                                    their_pub,
                                );
                                // Generate dual pads from relationship seed BEFORE moving it
                                // Use KDF to expand seed into two 1MB pads
                                let send_pad_key = blake3::derive_key(
                                    "photon.send_pad.v1",
                                    result.seed.as_bytes(),
                                );
                                let recv_pad_key = blake3::derive_key(
                                    "photon.recv_pad.v1",
                                    result.seed.as_bytes(),
                                );

                                contact.relationship_seed = Some(result.seed);
                                contact.clutch_state = ClutchState::Complete;

                                // Expand each 32-byte key into 1MB pad using repeated hashing
                                let mut send_pad = Vec::with_capacity(1_048_576);
                                let mut recv_pad = Vec::with_capacity(1_048_576);

                                for i in 0u32..(1_048_576 / 32) {
                                    let mut hasher = blake3::Hasher::new();
                                    hasher.update(&send_pad_key);
                                    hasher.update(&i.to_le_bytes());
                                    send_pad.extend_from_slice(hasher.finalize().as_bytes());

                                    let mut hasher = blake3::Hasher::new();
                                    hasher.update(&recv_pad_key);
                                    hasher.update(&i.to_le_bytes());
                                    recv_pad.extend_from_slice(hasher.finalize().as_bytes());
                                }

                                contact.send_pad = Some(send_pad);
                                contact.recv_pad = Some(recv_pad);

                                crate::log_info(&format!(
                                    "CLUTCH: Generated dual 1MB pads for {}",
                                    contact.handle
                                ));

                                // Zeroize and clear ephemeral secrets
                                if let Some(ref mut secret) = contact.clutch_our_ephemeral_secret {
                                    secret.zeroize();
                                }
                                contact.clutch_our_ephemeral_secret = None;
                                contact.clutch_our_ephemeral_pubkey = None;
                                contact.clutch_their_ephemeral_pubkey = None;
                                changed = true;

                                crate::log_info(&format!(
                                    "CLUTCH: Complete with {} (v3 device-bound + dual pads)",
                                    contact.handle
                                ));

                                // Save contact to persist relationship seed and clutch_state
                                if let Some(identity_seed) = self.user_identity_seed.as_ref() {
                                    let device_secret = self.device_keypair.secret.as_bytes();
                                    if let Err(e) = crate::storage::contacts::save_contact(
                                        contact,
                                        identity_seed,
                                        device_secret,
                                    ) {
                                        crate::log_error(&format!(
                                            "Failed to save contact after CLUTCH: {}",
                                            e
                                        ));
                                    } else {
                                        crate::log_info(&format!(
                                            "CLUTCH: Saved {} state to disk",
                                            contact.handle
                                        ));
                                    }
                                }

                                // Lower handle_proof sends ClutchComplete with proof
                                if clutch::is_clutch_initiator(
                                    &our_handle_proof,
                                    &contact.handle_proof,
                                ) {
                                    crate::log_info(
                                        "CLUTCH: We have lower handle_proof - sending proof",
                                    );
                                    checker.send_clutch(ClutchRequest {
                                        peer_addr: sender_addr,
                                        our_handle_proof,
                                        their_handle_proof: contact.handle_proof,
                                        message: ClutchRequestType::Complete {
                                            proof: result.proof,
                                        },
                                    });
                                }
                            }
                            break;
                        }
                    }
                    #[cfg(feature = "development")]
                    if !found_contact {
                        crate::log_info(&format!(
                            "CLUTCH: No contact found for handle_proof {}... (checked {} contacts)",
                            hex::encode(&from_handle_proof[..8]),
                            self.contacts.len()
                        ));
                    }
                }
                // v1 legacy messages - yeahnah, panic and burn
                StatusUpdate::ClutchInit { sender_addr, .. }
                | StatusUpdate::ClutchResponse { sender_addr, .. } => {
                    crate::log_error(&format!(
                        "CLUTCH: Received v1 legacy message from {} - REJECTED (v3 only)",
                        sender_addr
                    ));
                }
                StatusUpdate::ClutchComplete {
                    from_handle_proof,
                    to_handle_proof,
                    proof,
                } => {
                    // Check if this is addressed to us
                    if to_handle_proof != our_handle_proof {
                        continue;
                    }

                    crate::log_info(&format!(
                        "CLUTCH: Received Complete (from_hp: {}...)",
                        hex::encode(&from_handle_proof[..8])
                    ));

                    // Find contact by cached handle_proof
                    // In v3 parallel exchange, contact should already be Complete
                    for contact in &self.contacts {
                        if contact.handle_proof == from_handle_proof
                            && contact.clutch_state == ClutchState::Complete
                        {
                            if let Some(seed) = &contact.relationship_seed {
                                if clutch::verify_clutch_proof(seed, &proof) {
                                    crate::log_info(&format!(
                                        "CLUTCH: Proof verified for {}",
                                        contact.handle
                                    ));
                                } else {
                                    crate::log_error(&format!(
                                        "CLUTCH: Proof verification FAILED for {}!",
                                        contact.handle
                                    ));
                                }
                            }
                            break;
                        }
                    }
                }
                StatusUpdate::ChatMessage {
                    from_handle_proof,
                    sequence,
                    ciphertext,
                    sender_addr,
                } => {
                    // Find contact by handle_proof
                    let contact_info = self.contacts.iter().enumerate().find_map(|(idx, c)| {
                        if c.handle_proof == from_handle_proof {
                            Some((
                                idx,
                                c.id.clone(),
                                c.send_pad.clone(),
                                c.recv_pad.clone(),
                                c.handle.to_string(),
                            ))
                        } else {
                            None
                        }
                    });

                    if let Some((contact_idx, contact_id, Some(send_pad), Some(recv_pad), handle)) =
                        contact_info
                    {
                        crate::log_info(&format!(
                            "CHAT: Received message from {} (seq {}), {} bytes",
                            handle,
                            sequence,
                            ciphertext.len()
                        ));

                        // Get or create message chain for this contact
                        let chain = self
                            .message_chains
                            .entry(contact_id)
                            .or_insert_with(|| MessageChain::new(send_pad, recv_pad));

                        // Decrypt the message
                        let encrypted = EncryptedMessage {
                            sequence,
                            ciphertext,
                        };

                        match chain.decrypt(&encrypted) {
                            Ok((message, plaintext_hash)) => {
                                let text = String::from_utf8_lossy(&message.payload).to_string();
                                crate::log_info(&format!("CHAT: Decrypted: \"{}\"", text));

                                // Add to contact's message list
                                if let Some(contact) = self.contacts.get_mut(contact_idx) {
                                    contact.messages.push(ChatMessage::new(text, false));
                                    // Auto-scroll to bottom to show new message
                                    contact.message_scroll_offset = 0.0;
                                }
                                changed = true;

                                // Send ACK back with bidirectional weave binding
                                if let Some(ref checker) = self.status_checker {
                                    if let Some(our_hp) = self.user_handle_proof {
                                        // Get our last ACK'd message hash for weave binding
                                        let our_last_acked_hash = chain.get_last_acked_hash();
                                        checker.send_ack(crate::network::status::AckRequest {
                                            peer_addr: sender_addr,
                                            our_handle_proof: our_hp,
                                            sequence,
                                            plaintext_hash,
                                            our_last_acked_hash,
                                        });
                                        crate::log_info(&format!(
                                            "CHAT: Sent ACK for seq {} to {} (weave: {})",
                                            sequence,
                                            sender_addr,
                                            hex::encode(&our_last_acked_hash[..8])
                                        ));
                                    }
                                }
                            }
                            Err(e) => {
                                crate::log_error(&format!(
                                    "CHAT: Decryption failed for seq {}: {}",
                                    sequence, e
                                ));
                            }
                        }
                    }
                }
                StatusUpdate::MessageAck {
                    from_handle_proof,
                    sequence,
                    plaintext_hash,
                    sender_last_acked,
                } => {
                    // Find contact by handle_proof
                    let contact_info = self.contacts.iter().enumerate().find_map(|(idx, c)| {
                        if c.handle_proof == from_handle_proof {
                            Some((idx, c.id.clone(), c.handle.to_string()))
                        } else {
                            None
                        }
                    });

                    if let Some((contact_idx, contact_id, handle)) = contact_info {
                        crate::log_info(&format!(
                            "CHAT: ACK received from {} for seq {} (weave: {})",
                            handle,
                            sequence,
                            hex::encode(&sender_last_acked[..8])
                        ));

                        // Update chain with ACK - rotates pads using weave hash for bidirectional binding
                        if let Some(chain) = self.message_chains.get_mut(&contact_id) {
                            if let Err(e) =
                                chain.receive_ack(sequence, &plaintext_hash, &sender_last_acked)
                            {
                                crate::log_error(&format!("CHAT: Failed to process ACK: {}", e));
                            }
                        }

                        // Mark message as delivered in UI
                        if let Some(contact) = self.contacts.get_mut(contact_idx) {
                            // Find the message by sequence (it's the Nth outgoing message)
                            // For now, mark recent outgoing messages as delivered
                            for msg in contact.messages.iter_mut().rev() {
                                if msg.is_outgoing && !msg.delivered {
                                    msg.delivered = true;
                                    changed = true;
                                    break;
                                }
                            }
                        }
                    }
                }

                // PLTP large transfer received (for full CLUTCH key exchange)
                StatusUpdate::PLTPReceived { peer_addr, data } => {
                    crate::log_info(&format!(
                        "PLTP: Received {} bytes from {}",
                        data.len(),
                        peer_addr
                    ));
                    // TODO: Parse as ClutchFullOffer or ClutchKemResponse and process
                    // For now, just log that we received it
                }

                // PLTP outbound transfer completed
                StatusUpdate::PLTPSendComplete { peer_addr } => {
                    crate::log_info(&format!(
                        "PLTP: Outbound transfer to {} completed",
                        peer_addr
                    ));
                    // TODO: Track completion for full CLUTCH flow
                }

                // Full CLUTCH offer received (~548KB with all 8 pubkeys)
                StatusUpdate::ClutchFullOfferReceived {
                    from_handle_proof,
                    payload,
                    sender_addr,
                } => {
                    use crate::crypto::clutch::{
                        generate_all_ephemeral_keypairs, ClutchFullOfferPayload,
                        ClutchKemResponsePayload,
                    };
                    use crate::types::ClutchState;

                    crate::log_info(&format!(
                        "CLUTCH: Received full offer ({} bytes) from {}",
                        payload.len(),
                        sender_addr
                    ));

                    // Parse the payload
                    let their_offer = match ClutchFullOfferPayload::from_bytes(&payload) {
                        Some(offer) => offer,
                        None => {
                            crate::log_error("CLUTCH: Failed to parse full offer payload");
                            continue;
                        }
                    };

                    // Find contact by handle_proof
                    for contact in &mut self.contacts {
                        if contact.handle_proof == from_handle_proof {
                            contact.ip = Some(sender_addr); // Update IP

                            // Store their offer
                            contact.clutch_their_offer = Some(their_offer.clone());

                            // State machine logic
                            match contact.clutch_state {
                                ClutchState::Pending => {
                                    // We haven't started yet - generate keys and send our offer
                                    crate::log_info(&format!(
                                        "CLUTCH: Generating 8 ephemeral keypairs for {}",
                                        contact.handle
                                    ));
                                    let keypairs = generate_all_ephemeral_keypairs();
                                    let our_offer =
                                        ClutchFullOfferPayload::from_keypairs(&keypairs);
                                    contact.clutch_our_keypairs = Some(keypairs);
                                    contact.clutch_state = ClutchState::OfferReceived;
                                    changed = true;

                                    // Send our offer
                                    checker.send_full_offer(
                                        sender_addr,
                                        our_handle_proof,
                                        our_offer.to_bytes(),
                                    );
                                    crate::log_info(&format!(
                                        "CLUTCH: Sent full offer to {}",
                                        contact.handle
                                    ));
                                }
                                ClutchState::KeysGenerated | ClutchState::OfferSent => {
                                    // We already generated/sent - now we have both
                                    contact.clutch_state = ClutchState::OffersExchanged;
                                    changed = true;
                                    crate::log_info(&format!(
                                        "CLUTCH: Both offers exchanged with {}",
                                        contact.handle
                                    ));
                                }
                                ClutchState::OfferReceived => {
                                    // Duplicate, ignore
                                }
                                ClutchState::Complete => {
                                    // They're re-keying - reset and start fresh
                                    crate::log_info(&format!(
                                        "CLUTCH: Re-key requested by {} - resetting",
                                        contact.handle
                                    ));
                                    contact.clutch_state = ClutchState::Pending;
                                    contact.clutch_our_keypairs = None;
                                    contact.clutch_their_offer = None;
                                    contact.clutch_our_kem_secrets = None;
                                    contact.clutch_their_kem_secrets = None;
                                    // Re-process as Pending
                                    let keypairs = generate_all_ephemeral_keypairs();
                                    let our_offer =
                                        ClutchFullOfferPayload::from_keypairs(&keypairs);
                                    contact.clutch_our_keypairs = Some(keypairs);
                                    contact.clutch_their_offer = Some(their_offer.clone());
                                    contact.clutch_state = ClutchState::OfferReceived;
                                    changed = true;

                                    checker.send_full_offer(
                                        sender_addr,
                                        our_handle_proof,
                                        our_offer.to_bytes(),
                                    );
                                }
                                _ => {}
                            }

                            // If both offers exchanged, generate and send KEM response
                            if contact.clutch_state == ClutchState::OffersExchanged
                                || contact.clutch_state == ClutchState::OfferReceived
                            {
                                if let (Some(ref their_offer), Some(ref _our_keys)) =
                                    (&contact.clutch_their_offer, &contact.clutch_our_keypairs)
                                {
                                    // Encapsulate to their public keys
                                    let (kem_response, our_secrets) =
                                        ClutchKemResponsePayload::encapsulate_to_peer(their_offer);
                                    contact.clutch_our_kem_secrets = Some(our_secrets);
                                    contact.clutch_state = ClutchState::KemSent;
                                    changed = true;

                                    checker.send_kem_response(
                                        sender_addr,
                                        our_handle_proof,
                                        kem_response.to_bytes(),
                                    );
                                    crate::log_info(&format!(
                                        "CLUTCH: Sent KEM response to {}",
                                        contact.handle
                                    ));
                                }
                            }
                            break;
                        }
                    }
                }

                // CLUTCH KEM response received (~17KB with 4 ciphertexts)
                StatusUpdate::ClutchKemResponseReceived {
                    from_handle_proof,
                    payload,
                    sender_addr,
                } => {
                    use crate::crypto::clutch::{
                        clutch_complete_full, p256_ecdh, p384_ecdh, secp256k1_ecdh, x25519_ecdh,
                        ClutchKemResponsePayload, ClutchKemSharedSecrets, ClutchSharedSecrets,
                    };
                    use crate::types::ClutchState;

                    crate::log_info(&format!(
                        "CLUTCH: Received KEM response ({} bytes) from {}",
                        payload.len(),
                        sender_addr
                    ));

                    // Parse the payload
                    let their_kem = match ClutchKemResponsePayload::from_bytes(&payload) {
                        Some(kem) => kem,
                        None => {
                            crate::log_error("CLUTCH: Failed to parse KEM response payload");
                            continue;
                        }
                    };

                    // Find contact by handle_proof
                    for contact in &mut self.contacts {
                        if contact.handle_proof == from_handle_proof {
                            contact.ip = Some(sender_addr); // Update IP

                            // Decapsulate their KEM response using our secret keys
                            if let Some(ref our_keys) = contact.clutch_our_keypairs {
                                let their_secrets = ClutchKemSharedSecrets::decapsulate_from_peer(
                                    &their_kem, our_keys,
                                );
                                contact.clutch_their_kem_secrets = Some(their_secrets);

                                match contact.clutch_state {
                                    ClutchState::KemSent => {
                                        // We sent, they responded - now we have both
                                        contact.clutch_state = ClutchState::KemReceived;
                                        changed = true;
                                    }
                                    _ => {
                                        // Unexpected state but store it anyway
                                        contact.clutch_state = ClutchState::KemReceived;
                                        changed = true;
                                    }
                                }

                                // Check if ceremony is complete (both KEM responses exchanged)
                                if contact.clutch_our_kem_secrets.is_some()
                                    && contact.clutch_their_kem_secrets.is_some()
                                {
                                    if let (
                                        Some(ref their_offer),
                                        Some(ref our_keys),
                                        Some(ref our_kem_secrets),
                                        Some(ref their_kem_secrets),
                                    ) = (
                                        &contact.clutch_their_offer,
                                        &contact.clutch_our_keypairs,
                                        &contact.clutch_our_kem_secrets,
                                        &contact.clutch_their_kem_secrets,
                                    ) {
                                        crate::log_info(&format!(
                                            "CLUTCH: Completing full 8-algorithm ceremony with {}",
                                            contact.handle
                                        ));

                                        // Compute EC shared secrets (same both directions)
                                        let x25519_shared = x25519_ecdh(
                                            &our_keys.x25519_secret,
                                            &their_offer.x25519_public,
                                        );
                                        let p384_shared = p384_ecdh(
                                            &our_keys.p384_secret,
                                            &their_offer.p384_public,
                                        );
                                        let secp256k1_shared = secp256k1_ecdh(
                                            &our_keys.secp256k1_secret,
                                            &their_offer.secp256k1_public,
                                        );
                                        let p256_shared = p256_ecdh(
                                            &our_keys.p256_secret,
                                            &their_offer.p256_public,
                                        );

                                        // Determine low/high ordering by handle hash
                                        let we_are_low = our_identity_seed < contact.handle_hash;

                                        // Build shared secrets struct with proper ordering
                                        let secrets = if we_are_low {
                                            ClutchSharedSecrets {
                                                low_x25519: x25519_shared,
                                                high_x25519: x25519_shared,
                                                low_p384: p384_shared.clone(),
                                                high_p384: p384_shared,
                                                low_secp256k1: secp256k1_shared.clone(),
                                                high_secp256k1: secp256k1_shared,
                                                low_p256: p256_shared.clone(),
                                                high_p256: p256_shared,
                                                // KEM: low = our encap (we→them), high = their encap (them→us)
                                                low_frodo: our_kem_secrets.frodo.clone(),
                                                high_frodo: their_kem_secrets.frodo.clone(),
                                                low_ntru: our_kem_secrets.ntru.clone(),
                                                high_ntru: their_kem_secrets.ntru.clone(),
                                                low_mceliece: our_kem_secrets.mceliece.clone(),
                                                high_mceliece: their_kem_secrets.mceliece.clone(),
                                                low_hqc: our_kem_secrets.hqc.clone(),
                                                high_hqc: their_kem_secrets.hqc.clone(),
                                            }
                                        } else {
                                            ClutchSharedSecrets {
                                                low_x25519: x25519_shared,
                                                high_x25519: x25519_shared,
                                                low_p384: p384_shared.clone(),
                                                high_p384: p384_shared,
                                                low_secp256k1: secp256k1_shared.clone(),
                                                high_secp256k1: secp256k1_shared,
                                                low_p256: p256_shared.clone(),
                                                high_p256: p256_shared,
                                                // KEM: low = their encap (them→us), high = our encap (we→them)
                                                low_frodo: their_kem_secrets.frodo.clone(),
                                                high_frodo: our_kem_secrets.frodo.clone(),
                                                low_ntru: their_kem_secrets.ntru.clone(),
                                                high_ntru: our_kem_secrets.ntru.clone(),
                                                low_mceliece: their_kem_secrets.mceliece.clone(),
                                                high_mceliece: our_kem_secrets.mceliece.clone(),
                                                low_hqc: their_kem_secrets.hqc.clone(),
                                                high_hqc: our_kem_secrets.hqc.clone(),
                                            }
                                        };

                                        // Complete the ceremony - derive dual 1MB pads
                                        let our_device_pub = *self.device_keypair.public.as_bytes();
                                        let their_device_pub = *contact.public_identity.as_bytes();
                                        let result = clutch_complete_full(
                                            &our_device_pub,
                                            &their_device_pub,
                                            &our_identity_seed,
                                            &contact.handle_hash,
                                            &secrets,
                                        );

                                        // Assign pads based on who is low/high
                                        if we_are_low {
                                            contact.send_pad = Some(result.low_pad);
                                            contact.recv_pad = Some(result.high_pad);
                                        } else {
                                            contact.send_pad = Some(result.high_pad);
                                            contact.recv_pad = Some(result.low_pad);
                                        }

                                        contact.clutch_state = ClutchState::Complete;
                                        changed = true;

                                        crate::log_info(&format!(
                                            "CLUTCH: Ceremony complete with {}! Generated 2MB pads (proof: {}...)",
                                            contact.handle,
                                            hex::encode(&result.proof[..8])
                                        ));

                                        // Zeroize sensitive material
                                        if let Some(ref mut keys) = contact.clutch_our_keypairs {
                                            keys.zeroize();
                                        }
                                        contact.clutch_our_keypairs = None;
                                        contact.clutch_their_offer = None;
                                        if let Some(ref mut secrets) =
                                            contact.clutch_our_kem_secrets
                                        {
                                            secrets.zeroize();
                                        }
                                        if let Some(ref mut secrets) =
                                            contact.clutch_their_kem_secrets
                                        {
                                            secrets.zeroize();
                                        }
                                        contact.clutch_our_kem_secrets = None;
                                        contact.clutch_their_kem_secrets = None;
                                    }
                                }
                            } else {
                                crate::log_error(&format!(
                                    "CLUTCH: Received KEM response but no keypairs for {}",
                                    contact.handle
                                ));
                            }
                            break;
                        }
                    }
                }

                // LAN peer discovered via broadcast (NAT hairpinning workaround)
                StatusUpdate::LanPeerDiscovered {
                    handle_proof,
                    local_ip,
                    port,
                } => {
                    // Find contact by handle_proof and store their LAN IP + port
                    for contact in &mut self.contacts {
                        if contact.handle_proof == handle_proof {
                            let old_local = contact.local_ip;
                            let old_port = contact.local_port;
                            contact.local_ip = Some(local_ip);
                            contact.local_port = Some(port);
                            if old_local != Some(local_ip) || old_port != Some(port) {
                                crate::log_info(&format!(
                                    "LAN: Discovered {} at local {}:{}",
                                    contact.handle, local_ip, port
                                ));
                                changed = true;
                            }
                            break;
                        }
                    }
                }
            }
        }

        // Proactive CLUTCH initiation: attempt full CLUTCH for contacts with IPs but in Pending state
        // This bypasses UDP online detection, using TCP directly
        // Prefers local_ip for same-LAN peers (NAT hairpinning workaround)
        let mut clutch_to_initiate: Option<(usize, std::net::SocketAddr, String, bool)> = None;
        for idx in 0..self.contacts.len() {
            let contact = &self.contacts[idx];
            if contact.clutch_state == ClutchState::Pending {
                // Prefer local_ip if we discovered them on LAN (avoids NAT hairpinning)
                if let (Some(local_ip), Some(local_port)) = (contact.local_ip, contact.local_port) {
                    // Use local IP + port from LAN discovery
                    let addr =
                        std::net::SocketAddr::new(std::net::IpAddr::V4(local_ip), local_port);
                    clutch_to_initiate =
                        Some((idx, addr, contact.handle.as_str().to_string(), true));
                    break;
                } else if let Some(ip) = contact.ip {
                    clutch_to_initiate =
                        Some((idx, ip, contact.handle.as_str().to_string(), false));
                    break; // Only try one contact per frame
                }
            }
        }

        if let Some((idx, ip, handle_str, is_local)) = clutch_to_initiate {
            use crate::crypto::clutch::{generate_all_ephemeral_keypairs, ClutchFullOfferPayload};

            if let Some(hp) = self.user_handle_proof {
                if is_local {
                    crate::log_info(&format!(
                        "CLUTCH: Initiating with {} via LAN {} (NAT hairpin workaround)",
                        handle_str, ip
                    ));
                } else {
                    crate::log_info(&format!(
                        "CLUTCH: Proactively initiating full ceremony with {} (has IP, bypassing UDP)",
                        handle_str
                    ));
                }

                // Generate all 8 ephemeral keypairs (~512KB secret, ~548KB public)
                crate::log_info(&format!(
                    "CLUTCH: Generating 8 ephemeral keypairs for {} (this may take a moment)...",
                    handle_str
                ));
                let keypairs = generate_all_ephemeral_keypairs();
                let our_offer = ClutchFullOfferPayload::from_keypairs(&keypairs);
                let payload_bytes = our_offer.to_bytes();

                crate::log_info(&format!(
                    "CLUTCH: Generated {} byte full offer for {}",
                    payload_bytes.len(),
                    handle_str
                ));

                // Update contact state
                if let Some(contact) = self.contacts.get_mut(idx) {
                    contact.clutch_our_keypairs = Some(keypairs);
                    contact.clutch_state = ClutchState::OfferSent;
                    changed = true;
                }

                // Send via TCP (uses inner Arc clone, safe to call after mut borrow)
                checker.send_full_offer(ip, hp, payload_bytes);

                crate::log_info(&format!(
                    "CLUTCH: Sent full offer to {} (8 algorithms, ~548KB)",
                    handle_str
                ));
            }
        }

        changed
    }

    /// Send a message to the currently selected contact
    /// Returns true if message was sent successfully
    pub fn send_message_to_selected_contact(&mut self, message_text: &str) -> bool {
        use crate::network::status::MessageRequest;
        use crate::types::ChatMessage;

        // Get selected contact
        let contact_idx = match self.selected_contact {
            Some(idx) => idx,
            None => return false,
        };

        // Get contact info we need
        let (contact_id, ip, send_pad, recv_pad) = {
            let contact = match self.contacts.get(contact_idx) {
                Some(c) => c,
                None => return false,
            };

            // Must have completed CLUTCH
            if contact.clutch_state != crate::types::ClutchState::Complete {
                crate::log_info(&format!(
                    "Cannot send to {}: CLUTCH not complete",
                    contact.handle
                ));
                return false;
            }

            // Must have dual pads
            let send_pad = match &contact.send_pad {
                Some(p) => p.clone(),
                None => {
                    crate::log_info(&format!(
                        "Cannot send to {}: pads not initialized (CLUTCH incomplete)",
                        contact.handle
                    ));
                    return false;
                }
            };
            let recv_pad = match &contact.recv_pad {
                Some(p) => p.clone(),
                None => {
                    crate::log_info(&format!(
                        "Cannot send to {}: pads not initialized (CLUTCH incomplete)",
                        contact.handle
                    ));
                    return false;
                }
            };

            // Prefer local_ip for same-LAN peers (NAT hairpinning workaround)
            let ip = if let (Some(local_ip), Some(local_port)) =
                (contact.local_ip, contact.local_port)
            {
                // Use local IP + port from LAN discovery
                std::net::SocketAddr::new(std::net::IpAddr::V4(local_ip), local_port)
            } else {
                match contact.ip {
                    Some(ip) => ip,
                    None => {
                        crate::log_info(&format!("Cannot send to {}: no IP", contact.handle));
                        return false;
                    }
                }
            };

            (contact.id.clone(), ip, send_pad, recv_pad)
        };

        // Get or create message chain for this contact
        // Clone pads for closure
        let send_pad_clone = send_pad.clone();
        let recv_pad_clone = recv_pad.clone();
        let chain = self
            .message_chains
            .entry(contact_id.clone())
            .or_insert_with(|| MessageChain::new(send_pad_clone, recv_pad_clone));

        // Encrypt the message
        let payload = message_text.as_bytes();
        let encrypted = match chain.encrypt(payload) {
            Ok(e) => e,
            Err(e) => {
                crate::log_error(&format!("Failed to encrypt message: {}", e));
                return false;
            }
        };

        // Get our handle proof
        let our_handle_proof = match self.user_handle_proof {
            Some(hp) => hp,
            None => return false,
        };

        // Send via StatusChecker
        if let Some(ref checker) = self.status_checker {
            checker.send_message(MessageRequest {
                peer_addr: ip,
                our_handle_proof,
                sequence: encrypted.sequence,
                ciphertext: encrypted.ciphertext,
            });

            // Add to contact's message list
            if let Some(contact) = self.contacts.get_mut(contact_idx) {
                contact.messages.push(ChatMessage::new(
                    message_text.to_string(),
                    true, // is_outgoing
                ));
                // Auto-scroll to bottom to show new message
                contact.message_scroll_offset = 0.0;
            }

            crate::log_info(&format!(
                "CHAT: Sent message (seq {}) to {}",
                encrypted.sequence, ip
            ));
            return true;
        }

        false
    }

    /// Check for completed avatar downloads and update contacts or user avatar
    /// Returns true if any avatars were updated
    pub fn check_avatar_downloads(&mut self) -> bool {
        let mut changed = false;
        while let Ok(result) = self.contact_avatar_rx.try_recv() {
            // Check if this is the user's own avatar
            if let Some(ref user_handle) = self.user_handle {
                if user_handle == &result.handle {
                    if let Some(pixels) = &result.pixels {
                        let display_pixels =
                            crate::display_profile::DisplayConverter::new().convert_avatar(pixels);
                        self.avatar_pixels = Some(display_pixels.clone());
                        self.avatar_scaled = None; // Force re-scale
                        changed = true;
                        crate::log_info(&format!(
                            "Avatar: User avatar loaded ({} bytes)",
                            pixels.len()
                        ));
                        // Also update the contact entry for self (if user added themselves)
                        for contact in &mut self.contacts {
                            if contact.handle.as_str() == result.handle {
                                contact.avatar_pixels = Some(display_pixels.clone());
                                contact.avatar_scaled = None;
                                contact.avatar_scaled_diameter = 0;
                                break;
                            }
                        }
                    }
                    continue;
                }
            }

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

    /// Spawn background thread to generate CLUTCH keypairs for a contact.
    /// McEliece key generation is slow (~100ms+), so we do it off the main thread.
    /// Results are received via clutch_keygen_rx and processed in check_clutch_keygens().
    pub fn spawn_clutch_keygen(&self, contact_id: ContactId) {
        use crate::crypto::clutch::generate_all_ephemeral_keypairs;

        let tx = self.clutch_keygen_tx.clone();
        #[cfg(not(target_os = "android"))]
        let proxy = self.event_proxy.clone();

        std::thread::spawn(move || {
            crate::log_info("CLUTCH: Background keypair generation started...");
            let keypairs = generate_all_ephemeral_keypairs();
            crate::log_info("CLUTCH: Background keypair generation complete");

            let _ = tx.send(ClutchKeygenResult {
                contact_id,
                keypairs,
            });

            // Wake the event loop so it processes the result
            #[cfg(not(target_os = "android"))]
            let _ = proxy.send_event(super::PhotonEvent::ClutchKeygenComplete);
        });
    }

    /// Check for completed background CLUTCH keygen results
    /// Returns true if any contacts were updated
    pub fn check_clutch_keygens(&mut self) -> bool {
        use crate::crypto::clutch::ClutchFullOfferPayload;
        use crate::types::ClutchState;

        let mut changed = false;

        while let Ok(result) = self.clutch_keygen_rx.try_recv() {
            // Find the contact and update it
            for contact in &mut self.contacts {
                if contact.id == result.contact_id {
                    crate::log_info(&format!(
                        "CLUTCH: Keypairs ready for {} - state -> KeysGenerated",
                        contact.handle
                    ));

                    // Store keypairs and update state
                    contact.clutch_our_keypairs = Some(result.keypairs);
                    contact.clutch_state = ClutchState::KeysGenerated;
                    changed = true;

                    // If we already have an IP, immediately send the offer
                    if let Some(ip) = contact.ip {
                        if let Some(ref keypairs) = contact.clutch_our_keypairs {
                            if let Some(hp) = self.user_handle_proof {
                                let offer = ClutchFullOfferPayload::from_keypairs(keypairs);
                                let payload = offer.to_bytes();

                                crate::log_info(&format!(
                                    "CLUTCH: Sending offer to {} ({} bytes)",
                                    contact.handle,
                                    payload.len()
                                ));

                                if let Some(ref checker) = self.status_checker {
                                    checker.send_full_offer(ip, hp, payload);
                                    contact.clutch_state = ClutchState::OfferSent;
                                }
                            }
                        }
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
            // Prefer local_ip for same-LAN peers (NAT hairpinning workaround)
            let addr = if let (Some(local_ip), Some(local_port)) =
                (contact.local_ip, contact.local_port)
            {
                Some(std::net::SocketAddr::new(
                    std::net::IpAddr::V4(local_ip),
                    local_port,
                ))
            } else {
                contact.ip
            };
            if let Some(ip) = addr {
                checker.ping(ip, contact.public_identity.clone());
                pinged += 1;
            }
        }
        if pinged > 0 {
            crate::log_info(&format!("Status: Pinged {} contact(s)", pinged));
        }

        // Send LAN broadcast for local peer discovery (NAT hairpinning workaround)
        // This lets peers on the same LAN discover each other's local IPs
        if let (Some(handle_proof), Some(hq)) = (self.user_handle_proof, &self.handle_query) {
            checker.send_lan_broadcast(handle_proof, hq.port());
        }
    }

    /// Ping a specific contact by index (for entering conversation)
    pub fn ping_contact(&mut self, contact_idx: usize) {
        let checker = match &self.status_checker {
            Some(c) => c,
            None => return,
        };

        if contact_idx < self.contacts.len() {
            let contact = &self.contacts[contact_idx];
            // Prefer local_ip for same-LAN peers (NAT hairpinning workaround)
            let addr = if let (Some(local_ip), Some(local_port)) =
                (contact.local_ip, contact.local_port)
            {
                Some(std::net::SocketAddr::new(
                    std::net::IpAddr::V4(local_ip),
                    local_port,
                ))
            } else {
                contact.ip
            };
            if let Some(ip) = addr {
                checker.ping(ip, contact.public_identity.clone());
                crate::log_info(&format!(
                    "Status: Pinged {} on conversation enter",
                    contact.handle
                ));
            }
        }
    }

    /// Trigger peer IP refresh (FGTW query + ping all contacts)
    /// Called when returning to contacts screen for snappy updates
    pub fn trigger_peer_refresh(&mut self) {
        crate::log_info("Network: Triggering peer refresh on screen return");

        // Refresh FGTW peer table
        if let Some(hq) = &self.handle_query {
            let _ = hq.refresh();
        }

        // Ping all contacts to refresh their status
        self.ping_contacts();
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

    /// Force an immediate FGTW refresh (called when FCM peer update received)
    pub fn force_fgtw_refresh(&mut self) {
        if matches!(self.app_state, AppState::Ready) {
            if let Some(hq) = &self.handle_query {
                if hq.refresh() {
                    crate::log_info("Network: FCM-triggered FGTW refresh");
                    // Reset timer so we don't double-refresh
                    self.next_fgtw_refresh =
                        std::time::Instant::now() + std::time::Duration::from_secs(60);
                }
            }
        }
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

        // Broadcast StatusPing to all peers so they learn our new IP (NAT hole punching)
        if let Some(ref checker) = self.status_checker {
            for peer in &result.peers {
                checker.ping(peer.ip, peer.device_pubkey.clone());
            }
            crate::log_info(&format!(
                "Network: Broadcast ping to {} peer(s)",
                result.peers.len()
            ));
        }

        // Update contact IPs from fresh peer data
        // Also send CLUTCH offer if we have keys ready but haven't sent yet
        use crate::crypto::clutch::ClutchFullOfferPayload;
        use crate::types::ClutchState;

        let mut updated = 0;
        let mut offers_to_send: Vec<(std::net::SocketAddr, Vec<u8>)> = Vec::new();
        let our_handle_proof = self.user_handle_proof;

        for peer in &result.peers {
            for contact in &mut self.contacts {
                if contact.public_identity == peer.device_pubkey {
                    let was_none = contact.ip.is_none();
                    if contact.ip != Some(peer.ip) {
                        crate::log_info(&format!(
                            "Network: Updated {} IP: {:?} -> {}",
                            contact.handle, contact.ip, peer.ip
                        ));
                        contact.ip = Some(peer.ip);
                        updated += 1;
                    }

                    // If we just got an IP and have keys ready, queue offer to send
                    if was_none && contact.clutch_state == ClutchState::KeysGenerated {
                        if let Some(ref keypairs) = contact.clutch_our_keypairs {
                            let offer = ClutchFullOfferPayload::from_keypairs(keypairs);
                            offers_to_send.push((peer.ip, offer.to_bytes()));
                            contact.clutch_state = ClutchState::OfferSent;
                            crate::log_info(&format!(
                                "CLUTCH: Queueing offer for {} (just got IP)",
                                contact.handle
                            ));
                        }
                    }
                    break;
                }
            }
        }

        // Send queued offers (after releasing mutable borrow on contacts)
        if let Some(hp) = our_handle_proof {
            if let Some(ref checker) = self.status_checker {
                for (ip, payload) in offers_to_send {
                    checker.send_full_offer(ip, hp, payload);
                }
            }
        }

        if updated > 0 {
            crate::log_info(&format!("Network: Updated {} contact IP(s)", updated));
        }

        updated > 0
    }

    /// Check for peer updates from WebSocket (real-time IP changes)
    /// Returns true if any contact IP was updated
    #[cfg(not(target_os = "android"))]
    pub fn check_peer_updates(&mut self) -> bool {
        let Some(ref client) = self.peer_update_client else {
            return false;
        };

        let mut updated = false;

        // Process all pending updates
        while let Some(update) = client.try_recv() {
            crate::log_info(&format!(
                "PeerUpdate: Received IP update for {}:{} (handle_proof: {}...)",
                update.ip,
                update.port,
                &hex::encode(&update.handle_proof[..4])
            ));

            // Update matching contact by device_pubkey
            for contact in &mut self.contacts {
                if contact.public_identity.as_bytes() == &update.device_pubkey {
                    let new_ip = format!("{}:{}", update.ip, update.port)
                        .parse::<std::net::SocketAddr>()
                        .ok();

                    if contact.ip != new_ip {
                        crate::log_info(&format!(
                            "PeerUpdate: Updated {} IP: {:?} -> {:?}",
                            contact.handle, contact.ip, new_ip
                        ));
                        contact.ip = new_ip;
                        updated = true;
                    }
                    break;
                }
            }
        }

        updated
    }

    /// Initiate full 8-algorithm CLUTCH with a contact by index.
    /// Returns true if initiation started successfully.
    ///
    /// This generates all 8 ephemeral keypairs (~512KB secret, ~548KB public)
    /// and sends the full offer via TCP.
    #[allow(dead_code)]
    pub fn initiate_full_clutch(&mut self, contact_idx: usize, checker: &StatusChecker) -> bool {
        use crate::crypto::clutch::{generate_all_ephemeral_keypairs, ClutchFullOfferPayload};
        use crate::types::ClutchState;

        // Get our identity (handle hash)
        let _our_identity_seed = match &self.user_identity_seed {
            Some(s) => *s,
            None => return false,
        };

        // Get our handle_proof
        let our_handle_proof = match &self.user_handle_proof {
            Some(hp) => *hp,
            None => return false,
        };

        // Get contact info
        let contact = match self.contacts.get_mut(contact_idx) {
            Some(c) => c,
            None => return false,
        };

        // Must be in Pending state
        if contact.clutch_state != ClutchState::Pending {
            crate::log_info(&format!(
                "Cannot initiate full CLUTCH with {}: state is {:?}",
                contact.handle, contact.clutch_state
            ));
            return false;
        }

        // Must have IP
        let ip = match contact.ip {
            Some(ip) => ip,
            None => {
                crate::log_info(&format!(
                    "Cannot initiate full CLUTCH with {}: no IP",
                    contact.handle
                ));
                return false;
            }
        };

        // Generate all 8 ephemeral keypairs
        crate::log_info(&format!(
            "CLUTCH: Generating 8 ephemeral keypairs for {} (this may take a moment)...",
            contact.handle
        ));
        let keypairs = generate_all_ephemeral_keypairs();
        let our_offer = ClutchFullOfferPayload::from_keypairs(&keypairs);
        let payload_bytes = our_offer.to_bytes();

        crate::log_info(&format!(
            "CLUTCH: Generated {} byte full offer for {}",
            payload_bytes.len(),
            contact.handle
        ));

        // Store our keypairs
        contact.clutch_our_keypairs = Some(keypairs);
        contact.clutch_state = ClutchState::OfferSent;

        // Send via TCP
        checker.send_full_offer(ip, our_handle_proof, payload_bytes);

        crate::log_info(&format!(
            "CLUTCH: Sent full offer to {} (8 algorithms, ~548KB)",
            contact.handle
        ));

        true
    }
}
