// Mouse input handling for PhotonApp

use crate::debug_println;
use crate::ui::app::{HoveredButton, TextLayout};

use super::app::{
    AppState, LaunchState, PhotonApp, ResizeEdge, HIT_AVATAR, HIT_BACK_HEADER, HIT_CLOSE_BUTTON,
    HIT_CONTACT_BASE, HIT_HANDLE_TEXTBOX, HIT_MAXIMIZE_BUTTON, HIT_MINIMIZE_BUTTON, HIT_NONE,
    HIT_PRIMARY_BUTTON,
};
use winit::event::{ElementState, MouseButton, MouseScrollDelta};
use winit::window::{CursorIcon, Window};

impl PhotonApp {
    pub fn handle_mouse_click(
        &mut self,
        window: &Window,
        state: ElementState,
        _button: MouseButton,
    ) {
        match state {
            ElementState::Pressed => {
                self.mouse_button_pressed = true;

                // Clear avatar hint on any click
                if self.show_avatar_hint {
                    self.show_avatar_hint = false;
                    self.window_dirty = true;
                }

                // Priority order: window controls > resize edges > other UI elements > window drag
                let mouse_x = self.mouse_x as usize;
                let mouse_y = self.mouse_y as usize;

                // Check window control buttons FIRST (highest priority)
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
                        _ => {} // Not a window control, continue checking
                    }
                }

                // Check resize edges SECOND (higher priority than other UI elements)
                let edge = self.get_resize_edge(self.mouse_x, self.mouse_y);
                if edge != ResizeEdge::None {
                    self.is_dragging_resize = true;
                    self.resize_edge = edge;
                    self.drag_start_size = (self.width, self.height);

                    // Store the window position and global blinkey position at drag start
                    if let Some(window_pos) = window.outer_position().ok() {
                        self.drag_start_window_pos = (window_pos.x, window_pos.y);

                        // Calculate global blinkey position from window-relative position
                        let blinkey_screen_x = window_pos.x as f64 + self.mouse_x as f64;
                        let blinkey_screen_y = window_pos.y as f64 + self.mouse_y as f64;
                        self.drag_start_blinkey_screen_pos = (blinkey_screen_x, blinkey_screen_y);
                    }
                    return;
                }

                // Check other UI elements (textbox, buttons, contacts, back header)
                if mouse_x < self.width as usize && mouse_y < self.height as usize {
                    let hit_idx = mouse_y * self.width as usize + mouse_x;
                    let element_id = self.hit_test_map[hit_idx];

                    match element_id {
                        HIT_HANDLE_TEXTBOX => {
                            // Don't allow textbox focus during attestation - handle is locked in
                            if matches!(self.app_state, AppState::Launch(LaunchState::Attesting)) {
                                return;
                            }

                            let was_focused = self.current_text_state.textbox_focused;

                            // Focus the textbox and set blinkey position based on click location
                            self.current_text_state.textbox_focused = true;

                            // Unhover textbox when activating it (hover effects interfere with blinkey)
                            if self.hovered_button == HoveredButton::Textbox {
                                self.prev_hovered_button = HoveredButton::Textbox;
                                self.hovered_button = HoveredButton::None;
                                self.controls_dirty = true; // Trigger unhover rendering
                            } else {
                                debug_println!(
                                    "  Textbox not hovered on activation: {:?}",
                                    self.hovered_button
                                );
                            }

                            // Reset blink timer on focus gain to prevent rapid catch-up blinks
                            let next_blink = self.next_blink_wake_time();
                            self.next_blinkey_blink_time = next_blink;
                            debug_println!("  Reset blink timer");

                            // If textbox is empty, need to redraw to remove infinity placeholder
                            if self.current_text_state.chars.is_empty() {
                                self.text_dirty = true;
                            }

                            // Calculate click position relative to text (sets blinkey_index)
                            self.current_text_state.blinkey_index =
                                self.blinkey_index_from_x(mouse_x as f32);
                            if self.current_text_state.chars.is_empty() {
                                self.current_text_state.scroll_offset = 0.0;
                            }

                            // Calculate blinkey pixel position using TextLayout (single source of truth)
                            let box_width = self.textbox_width();
                            let box_height = self.textbox_height();
                            let textbox_y = self.textbox_center_y();
                            let font_size = self.text_layout.font_size;

                            let new_blinkey_x =
                                self.text_layout.blinkey_x(&self.current_text_state);
                            let new_blinkey_y = self.text_layout.blinkey_y();

                            // Lock buffer for blinkey update (immediate-mode)
                            let mut buffer = self.renderer.lock_buffer();
                            let pixels = buffer.as_mut();

                            // If blinkey already visible, undraw at OLD position first
                            if was_focused && self.blinkey_visible {
                                Self::undraw_blinkey(
                                    pixels,
                                    self.width as usize,
                                    self.blinkey_pixel_x,
                                    self.blinkey_pixel_y,
                                    &mut self.blinkey_visible,
                                    &mut self.blinkey_wave_top_bright,
                                    font_size as usize,
                                );
                            }

                            self.blinkey_pixel_x = new_blinkey_x;
                            self.blinkey_pixel_y = new_blinkey_y;

                            // Draw blinkey at NEW position (or start if first focus)
                            if !was_focused {
                                // Add textbox glow on focus
                                debug_println!("Textbox glow: ON");
                                let scroll = if matches!(self.app_state, AppState::Ready | AppState::Searching) {
                                    self.contacts_scroll_offset
                                } else {
                                    0
                                };
                                Self::apply_textbox_glow(
                                    pixels,
                                    &self.textbox_mask,
                                    self.width as usize,
                                    textbox_y as isize + scroll,
                                    box_width,
                                    box_height,
                                    true,
                                    self.glow_colour, // Use current glow colour (preserves status colour)
                                );

                                Self::start_blinkey(
                                    pixels,
                                    self.width as usize,
                                    self.blinkey_pixel_x,
                                    self.blinkey_pixel_y,
                                    &mut self.blinkey_visible,
                                    &mut self.blinkey_wave_top_bright,
                                    font_size as usize,
                                );
                            } else {
                                Self::draw_blinkey(
                                    pixels,
                                    self.width as usize,
                                    self.blinkey_pixel_x,
                                    self.blinkey_pixel_y,
                                    &mut self.blinkey_visible,
                                    &mut self.blinkey_wave_top_bright,
                                    font_size as usize,
                                );
                            }
                            buffer.present().unwrap();

                            // Prepare for potential drag selection - set anchor to click position
                            // is_mouse_selecting will be set to true when mouse actually moves (in handle_mouse_move)
                            self.current_text_state.selection_anchor =
                                Some(self.current_text_state.blinkey_index);
                            self.selection_dirty = true;

                            return;
                        }
                        HIT_BACK_HEADER => {
                            // Back button in conversation header - return to contacts
                            self.app_state = AppState::Ready;
                            let eff_ru = self.effective_ru();
                            self.text_layout = TextLayout::new(
                                self.width as usize,
                                self.height as usize,
                                self.span,
                                eff_ru,
                                &self.app_state,
                            );
                            self.layout = super::app::Layout::new(
                                self.width as usize,
                                self.height as usize,
                                self.span,
                                eff_ru,
                                &self.app_state,
                            );
                            self.selected_contact = None;
                            self.window_dirty = true;
                            self.reset_textbox();

                            // Clear hover states when transitioning screens
                            // Set both current and prev to None to avoid differential rendering artifacts
                            self.prev_hovered_button = HoveredButton::None;
                            self.hovered_button = HoveredButton::None;
                            self.prev_hovered_contact = None;
                            self.hovered_contact = None;

                            return;
                        }
                        HIT_PRIMARY_BUTTON => {
                            // Primary button click: "Query", "Attest", or "Recover / Challenge"

                            // Defocus textbox on button click
                            if self.current_text_state.textbox_focused {
                                self.current_text_state.textbox_focused = false;

                                if self.blinkey_visible {
                                    let box_width = self.textbox_width();
                                    let box_height = self.textbox_height();
                                    let textbox_y = self.textbox_center_y();
                                    let font_size = self.font_size();
                                    let scroll = if matches!(self.app_state, AppState::Ready | AppState::Searching) {
                                        self.contacts_scroll_offset
                                    } else {
                                        0
                                    };
                                    let mut buffer = self.renderer.lock_buffer();
                                    let pixels = buffer.as_mut();

                                    Self::apply_textbox_glow(
                                        pixels,
                                        &self.textbox_mask,
                                        self.width as usize,
                                        textbox_y as isize + scroll,
                                        box_width,
                                        box_height,
                                        false,
                                        self.glow_colour, // Must match the colour that was added
                                    );

                                    Self::stop_blinkey(
                                        pixels,
                                        self.width as usize,
                                        self.blinkey_pixel_x,
                                        self.blinkey_pixel_y,
                                        &mut self.blinkey_visible,
                                        &mut self.blinkey_wave_top_bright,
                                        font_size as usize,
                                    );
                                }
                            }

                            let handle: String = self.current_text_state.chars.iter().collect();
                            match &self.app_state {
                                AppState::Launch(launch_state) => {
                                    match launch_state {
                                        LaunchState::Fresh => {
                                            // "Attest" button clicked - start attestation
                                            debug_println!("Attesting handle: {}", handle);
                                            self.start_attestation();
                                            self.window_dirty = true;
                                        }
                                        LaunchState::Attesting => {
                                            // Attestation already in progress - ignore clicks
                                            debug_println!(
                                                "Attestation already in progress, ignoring click"
                                            );
                                        }
                                        LaunchState::Error(_) => {
                                            // Error state doesn't show button, shouldn't reach here
                                            debug_println!("Primary button clicked in error state (unexpected)");
                                        }
                                    }
                                }
                                AppState::Ready => {
                                    // "Add" button clicked - check if contact exists first
                                    let handle_lower = handle.to_lowercase();
                                    let existing_idx = self.contacts.iter().position(|c| {
                                        c.handle.as_str().to_lowercase() == handle_lower
                                    });

                                    if let Some(idx) = existing_idx {
                                        // Contact already exists - open conversation with them
                                        debug_println!("Contact {} already exists, opening conversation", handle);
                                        self.selected_contact = Some(idx);
                                        self.app_state = AppState::Conversation;
                                        self.reset_textbox();
                                        let eff_ru = self.effective_ru();
                                        self.text_layout = super::app::TextLayout::new(
                                            self.width as usize,
                                            self.height as usize,
                                            self.span,
                                            eff_ru,
                                            &self.app_state,
                                        );
                                        self.layout = super::app::Layout::new(
                                            self.width as usize,
                                            self.height as usize,
                                            self.span,
                                            eff_ru,
                                            &self.app_state,
                                        );
                                    } else {
                                        // New handle - search network
                                        debug_println!("Querying handle: {}", handle);
                                        self.start_handle_search(&handle);
                                    }
                                    self.window_dirty = true;
                                }
                                AppState::Conversation => {
                                    // "Send" button clicked - send message
                                    let message: String =
                                        self.current_text_state.chars.iter().collect();
                                    if !message.is_empty() {
                                        debug_println!("Send button clicked, sending: {}", message);
                                        if self.send_message_to_selected_contact(&message) {
                                            // Clear textbox after successful send
                                            self.reset_textbox();
                                            self.window_dirty = true;
                                        }
                                    }
                                }
                                _ => {}
                            }
                            return;
                        }
                        HIT_AVATAR => {
                            // Avatar clicked - show hint text
                            if matches!(self.app_state, AppState::Ready | AppState::Searching) {
                                debug_println!("Avatar clicked - showing upload hint");
                                self.show_avatar_hint = true;
                                self.window_dirty = true;
                            }
                            return;
                        }
                        id if id >= HIT_CONTACT_BASE => {
                            // Contact clicked - enter conversation view
                            let contact_idx = (id - HIT_CONTACT_BASE) as usize;
                            if contact_idx < self.contacts.len() {
                                debug_println!(
                                    "Contact clicked: {} (index {})",
                                    self.contacts[contact_idx].handle,
                                    contact_idx
                                );

                                // Ping this specific contact when entering conversation
                                self.ping_contact(contact_idx);

                                // Fetch avatar if we don't have it (don't require online status)
                                if self.contacts[contact_idx].avatar_pixels.is_none() {
                                    let handle =
                                        self.contacts[contact_idx].handle.as_str().to_string();
                                    eprintln!(
                                        "Avatar: Entering conversation with {}, fetching avatar",
                                        handle
                                    );
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

                                self.selected_contact = Some(contact_idx);
                                self.app_state = AppState::Conversation;
                                let eff_ru = self.effective_ru();
                                self.text_layout = TextLayout::new(
                                    self.width as usize,
                                    self.height as usize,
                                    self.span,
                                    eff_ru,
                                    &self.app_state,
                                );
                                self.layout = super::app::Layout::new(
                                    self.width as usize,
                                    self.height as usize,
                                    self.span,
                                    eff_ru,
                                    &self.app_state,
                                );
                                self.window_dirty = true;
                                self.reset_textbox();

                                // Clear hover states when transitioning screens
                                // Set both current and prev to None to avoid differential rendering artifacts
                                self.prev_hovered_button = HoveredButton::None;
                                self.hovered_button = HoveredButton::None;
                                self.prev_hovered_contact = None;
                                self.hovered_contact = None;
                            }
                            return;
                        }
                        _ => {
                            // Clicked outside textbox, unfocus it
                            if self.current_text_state.textbox_focused {
                                self.current_text_state.textbox_focused = false;

                                // State transition: blinkey ON -> OFF (immediate-mode)
                                if self.blinkey_visible {
                                    let box_width = self.textbox_width();
                                    let box_height = self.textbox_height();
                                    let textbox_y = self.textbox_center_y();
                                    let font_size = self.font_size();
                                    let scroll = if matches!(self.app_state, AppState::Ready | AppState::Searching) {
                                        self.contacts_scroll_offset
                                    } else {
                                        0
                                    };
                                    let mut buffer = self.renderer.lock_buffer();
                                    let pixels = buffer.as_mut();

                                    // Remove textbox glow on unfocus
                                    debug_println!("Textbox glow: OFF");
                                    Self::apply_textbox_glow(
                                        pixels,
                                        &self.textbox_mask,
                                        self.width as usize,
                                        textbox_y as isize + scroll,
                                        box_width,
                                        box_height,
                                        false,
                                        self.glow_colour, // Must match the colour that was added
                                    );

                                    Self::stop_blinkey(
                                        pixels,
                                        self.width as usize,
                                        self.blinkey_pixel_x,
                                        self.blinkey_pixel_y,
                                        &mut self.blinkey_visible,
                                        &mut self.blinkey_wave_top_bright,
                                        font_size as usize,
                                    );
                                    buffer.present().unwrap();
                                }

                                // If textbox is empty, need to redraw to show infinity placeholder
                                if self.current_text_state.chars.is_empty() {
                                    self.text_dirty = true;
                                }
                            }

                            // Hide avatar hint when clicking outside avatar
                            if self.show_avatar_hint {
                                self.show_avatar_hint = false;
                                self.window_dirty = true;
                            }
                        }
                    }
                }

                // Not on a resize edge or UI element, start window drag
                self.is_dragging_move = true;

                // Store the window position and global blinkey position at drag start
                if let Some(window_pos) = window.outer_position().ok() {
                    self.drag_start_window_pos = (window_pos.x, window_pos.y);

                    // Calculate global blinkey position from window-relative position
                    let blinkey_screen_x = window_pos.x as f64 + self.mouse_x as f64;
                    let blinkey_screen_y = window_pos.y as f64 + self.mouse_y as f64;
                    self.drag_start_blinkey_screen_pos = (blinkey_screen_x, blinkey_screen_y);
                }
            }
            ElementState::Released => {
                self.mouse_button_pressed = false;

                // End selection
                if self.is_mouse_selecting {
                    self.is_mouse_selecting = false;
                    self.selection_last_update_time = None;

                    // State transition: blinkey OFF -> ON (immediate-mode)
                    if !self.blinkey_visible && self.current_text_state.textbox_focused {
                        // Recalculate blinkey position using TextLayout (single source of truth)
                        let font_size = self.text_layout.font_size;

                        self.blinkey_pixel_x = self.text_layout.blinkey_x(&self.current_text_state);
                        self.blinkey_pixel_y = self.text_layout.blinkey_y();

                        let mut buffer = self.renderer.lock_buffer();
                        let pixels = buffer.as_mut();
                        Self::start_blinkey(
                            pixels,
                            self.width as usize,
                            self.blinkey_pixel_x,
                            self.blinkey_pixel_y,
                            &mut self.blinkey_visible,
                            &mut self.blinkey_wave_top_bright,
                            font_size as usize,
                        );
                        buffer.present().unwrap();
                    }

                    // Reset blink timer to prevent immediate blink after selection ends
                    self.next_blinkey_blink_time = self.next_blink_wake_time();

                    // If anchor == blinkey, it was just a click (not a drag), clear selection
                    if self.current_text_state.selection_anchor
                        == Some(self.current_text_state.blinkey_index)
                    {
                        self.current_text_state.selection_anchor = None;
                        self.selection_dirty = true;
                    }
                } else {
                    // Mouse released without dragging - clear anchor if it's just a click
                    if self.current_text_state.selection_anchor
                        == Some(self.current_text_state.blinkey_index)
                    {
                        self.current_text_state.selection_anchor = None;
                        self.selection_dirty = true;
                    }
                }

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
    ) -> bool {
        self.mouse_x = position.x as f32;
        self.mouse_y = position.y as f32;

        // Handle window move dragging
        if self.is_dragging_move {
            // Get current global blinkey position
            if let Some(window_pos) = window.outer_position().ok() {
                let current_blinkey_screen_x = window_pos.x as f64 + self.mouse_x as f64;
                let current_blinkey_screen_y = window_pos.y as f64 + self.mouse_y as f64;

                // Calculate delta in global screen space
                let dx = (current_blinkey_screen_x - self.drag_start_blinkey_screen_pos.0) as i32;
                let dy = (current_blinkey_screen_y - self.drag_start_blinkey_screen_pos.1) as i32;

                // Move window
                let new_x = self.drag_start_window_pos.0 + dx;
                let new_y = self.drag_start_window_pos.1 + dy;
                let _ = window.set_outer_position(winit::dpi::PhysicalPosition::new(new_x, new_y));
            }
            false // No redraw needed for window move
        } else if self.is_dragging_resize {
            // Get current global blinkey position
            if let Some(window_pos) = window.outer_position().ok() {
                let current_blinkey_screen_x = window_pos.x as f64 + self.mouse_x as f64;
                let current_blinkey_screen_y = window_pos.y as f64 + self.mouse_y as f64;

                // Calculate delta in global screen space
                let dx = (current_blinkey_screen_x - self.drag_start_blinkey_screen_pos.0) as f32;
                let dy = (current_blinkey_screen_y - self.drag_start_blinkey_screen_pos.1) as f32;

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
            // Start mouse selection if we have an anchor, button is pressed, and mouse moved
            if !self.is_mouse_selecting
                && self.mouse_button_pressed
                && self.current_text_state.selection_anchor.is_some()
            {
                self.is_mouse_selecting = true;

                // State transition: blinkey ON -> OFF (immediate-mode)
                if self.blinkey_visible {
                    let font_size = self.font_size();
                    let mut buffer = self.renderer.lock_buffer();
                    let pixels = buffer.as_mut();
                    Self::stop_blinkey(
                        pixels,
                        self.width as usize,
                        self.blinkey_pixel_x,
                        self.blinkey_pixel_y,
                        &mut self.blinkey_visible,
                        &mut self.blinkey_wave_top_bright,
                        font_size as usize,
                    );
                    buffer.present().unwrap();
                }
            }

            // Handle drag selection
            if self.is_mouse_selecting && !self.current_text_state.chars.is_empty() {
                let box_left = self.text_layout.usable_left;
                let box_right = self.text_layout.usable_right;

                // Clamp mouse position to textbox bounds for blinkey calculation
                let clamped_mouse_x = self.mouse_x.clamp(box_left as f32, box_right as f32);

                self.current_text_state.blinkey_index = self.blinkey_index_from_x(clamped_mouse_x);

                self.selection_dirty = true;
                self.controls_dirty = true;

                return true; // Request redraw
            }

            // Priority order: window controls > resize edges > other UI elements
            let old_hovered = self.hovered_button;
            let old_hovered_contact = self.hovered_contact;

            let mouse_x = self.mouse_x as usize;
            let mouse_y = self.mouse_y as usize;

            // Check window control buttons FIRST (highest priority)
            let element_id = if mouse_x < self.width as usize && mouse_y < self.height as usize {
                let hit_idx = mouse_y * self.width as usize + mouse_x;
                self.hit_test_map[hit_idx]
            } else {
                HIT_NONE
            };

            let is_window_control = matches!(
                element_id,
                HIT_CLOSE_BUTTON | HIT_MAXIMIZE_BUTTON | HIT_MINIMIZE_BUTTON
            );

            if is_window_control {
                // On a window control - show hover and pointer cursor
                self.hovered_button = match element_id {
                    HIT_CLOSE_BUTTON => HoveredButton::Close,
                    HIT_MAXIMIZE_BUTTON => HoveredButton::Maximize,
                    HIT_MINIMIZE_BUTTON => HoveredButton::Minimize,
                    _ => HoveredButton::None,
                };
                self.hovered_contact = None;
                window.set_cursor(CursorIcon::Pointer);
            } else {
                // Check resize edges SECOND (higher priority than other UI elements)
                let edge = self.get_resize_edge(self.mouse_x, self.mouse_y);

                if edge != ResizeEdge::None {
                    // On a resize edge - clear hover states and show resize cursor
                    self.hovered_button = HoveredButton::None;
                    self.hovered_contact = None;

                    let cursor = match edge {
                        ResizeEdge::Top | ResizeEdge::Bottom => CursorIcon::NsResize,
                        ResizeEdge::Left | ResizeEdge::Right => CursorIcon::EwResize,
                        ResizeEdge::TopLeft | ResizeEdge::BottomRight => CursorIcon::NwseResize,
                        ResizeEdge::TopRight | ResizeEdge::BottomLeft => CursorIcon::NeswResize,
                        ResizeEdge::None => CursorIcon::Default, // Won't happen due to check above
                    };
                    window.set_cursor(cursor);
                } else {
                    // Check other UI elements (textbox, buttons, contacts, back header)
                    self.hovered_button = match element_id {
                        HIT_HANDLE_TEXTBOX => {
                            // Don't hover textbox when it's focused (would interfere with blinkey)
                            if self.current_text_state.textbox_focused {
                                HoveredButton::None
                            } else {
                                HoveredButton::Textbox
                            }
                        }
                        HIT_PRIMARY_BUTTON => HoveredButton::QueryButton,
                        HIT_BACK_HEADER => HoveredButton::BackHeader,
                        _ => HoveredButton::None,
                    };

                    // Check if hovering over a contact
                    if element_id >= HIT_CONTACT_BASE {
                        self.hovered_contact = Some((element_id - HIT_CONTACT_BASE) as usize);
                    } else {
                        self.hovered_contact = None;
                    }

                    // Update cursor icon based on hover position
                    let cursor = if self.hovered_button != HoveredButton::None {
                        CursorIcon::Pointer
                    } else if element_id == HIT_PRIMARY_BUTTON {
                        CursorIcon::Pointer
                    } else if self.hovered_contact.is_some() {
                        CursorIcon::Pointer
                    } else if element_id == HIT_HANDLE_TEXTBOX {
                        CursorIcon::Text
                    } else {
                        CursorIcon::Default
                    };
                    window.set_cursor(cursor);
                }
            }

            // Return true if hover state changed (button or contact)
            let hover_changed =
                old_hovered != self.hovered_button || old_hovered_contact != self.hovered_contact;
            if hover_changed {
                self.controls_dirty = true;
            }
            hover_changed
        }
    }

    pub fn handle_mouse_wheel(&mut self, delta: MouseScrollDelta) -> bool {
        // Extract scroll amount (pixels or lines)
        let scroll_pixels = match delta {
            MouseScrollDelta::LineDelta(_x, y) => {
                // Line scrolling: convert lines to pixels (~20 pixels per line)
                y * 20.0
            }
            MouseScrollDelta::PixelDelta(pos) => {
                // Pixel scrolling: use y directly
                pos.y as f32
            }
        };

        // Ctrl+scroll = zoom (works in any state)
        // Logarithmic scaling: 1 step = multiply by 33/32
        if self.modifiers.control_key() {
            // scroll_pixels: ~20 per line notch, variable for pixel delta (touchpad)
            // Convert to steps: 1 notch = 1 step
            let steps = scroll_pixels / 20.0;
            self.adjust_zoom(steps);
            return true;
        }

        // Contacts scroll (Ready/Searching states)
        if matches!(self.app_state, AppState::Ready | AppState::Searching) {
            self.contacts_scroll_offset += scroll_pixels as isize;

            // Clamp scroll to content bounds (unless debug mode is enabled)
            if !self.debug {
                // Calculate content height: user section + contact rows
                let ru = self.effective_ru();
                let span = self.span as usize;
                let contacts_block = super::app::PixelRegion {
                    x: 0,
                    y: 0,
                    w: self.width as usize,
                    h: self.height as usize,
                };
                let layout = super::app::ContactsUnifiedLayout::new(&contacts_block, span, ru, 0);

                // User section height (from top to separator bottom)
                let user_section_bottom = layout.separator.y + layout.separator.h;

                // Count visible contacts (respecting search filter)
                let filter_text: String = self.current_text_state.chars.iter().collect();
                let filter_lower = filter_text.to_lowercase();
                let num_contacts = if filter_lower.is_empty() {
                    self.contacts.len()
                } else {
                    self.contacts
                        .iter()
                        .filter(|c| c.handle.as_str().to_lowercase().contains(&filter_lower))
                        .count()
                };

                // Total content height = user section + (contact rows × row height) + padding + version row
                let contacts_height = num_contacts * layout.row_height;
                let total_content_height = user_section_bottom + contacts_height + (2 * layout.row_height);

                // Scroll limits:
                // - max_scroll_up (positive): user section top at screen top → 0
                // - max_scroll_down (negative): content bottom at screen bottom
                let max_scroll_up: isize = 0;
                let max_scroll_down: isize =
                    -((total_content_height as isize - self.height as isize).max(0));

                self.contacts_scroll_offset =
                    self.contacts_scroll_offset.clamp(max_scroll_down, max_scroll_up);
            }

            // Defocus textbox if it scrolls off-screen
            let tl = &self.text_layout;
            let scrolled_textbox_cy = tl.center_y as isize + self.contacts_scroll_offset;
            let half_box_h = (tl.box_height / 2) as isize;

            // Textbox must be FULLY on-screen to stay focused
            if scrolled_textbox_cy < half_box_h || scrolled_textbox_cy >= self.height as isize - half_box_h {
                if self.current_text_state.textbox_focused {
                    self.current_text_state.textbox_focused = false;
                    self.blinkey_visible = false;
                }
            }

            self.window_dirty = true;
            return true;
        }

        // Message scroll only works in conversation view
        if self.app_state != AppState::Conversation {
            return false;
        }

        let Some(contact_idx) = self.selected_contact else {
            return false;
        };

        // Calculate scroll bounds BEFORE mutable borrow
        let line_height = (self.font_size() as f32 * 1.5) as usize;
        let padding = self.span / 32;
        let box_height = self.textbox_height();
        let center_y = self.height as usize / 2;
        let textbox_y = center_y + (center_y / 4);
        let message_area_top = (box_height as f32 * 1.5) as usize;
        let message_area_bottom = textbox_y - (box_height as f32 * 0.6) as usize;
        let visible_height = message_area_bottom - message_area_top;

        // Apply scroll to contact's message area
        if let Some(contact) = self.contacts.get_mut(contact_idx) {
            // Positive scroll = scroll up (show older messages)
            // Negative scroll = scroll down (show newer messages)
            contact.message_scroll_offset += scroll_pixels;

            let total_height = contact.messages.len() * line_height + padding * 2;

            // Clamp scroll offset to valid range
            // Max scroll up: 0 (no offset, messages at natural position)
            // Max scroll down: -(total_height - visible_height) if content taller than viewport
            let max_scroll_down = -((total_height as i32 - visible_height as i32).max(0) as f32);
            contact.message_scroll_offset =
                contact.message_scroll_offset.clamp(max_scroll_down, 0.0);

            self.window_dirty = true;
            return true;
        }

        false
    }
}
