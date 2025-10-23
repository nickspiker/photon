// Mouse input handling for PhotonApp

use crate::debug_println;
use crate::ui::app::HoveredButton;

use super::app::{
    HandleStatus, PhotonApp, ResizeEdge, HIT_CHALLENGE_BUTTON, HIT_CLOSE_BUTTON,
    HIT_HANDLE_TEXTBOX, HIT_MAXIMIZE_BUTTON, HIT_MINIMIZE_BUTTON, HIT_NONE, HIT_PRIMARY_BUTTON,
    HIT_RECOVER_BUTTON,
};
use rand::Rng;
use winit::event::{ElementState, MouseButton};
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
                            let was_focused = self.current_text_state.textbox_focused;

                            // Focus the textbox and set blinkey position based on click location
                            self.current_text_state.textbox_focused = true;

                            // Unhover textbox when activating it (hover effects interfere with blinkey)
                            if self.hovered_button == HoveredButton::Textbox {
                                debug_println!("  Unhovering textbox on activation");
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
                            let old_blinkey_index = self.current_text_state.blinkey_index;
                            if !self.current_text_state.chars.is_empty() {
                                let center_x = self.width as usize / 2;
                                let total_text_width: usize = self.current_text_state.width;
                                let text_half = total_text_width / 2;
                                let text_start_x = center_x as f32 - text_half as f32
                                    + self.current_text_state.scroll_offset;

                                // Find which character was clicked
                                let click_x = mouse_x as f32;
                                let mut x_offset = text_start_x;
                                let mut found_position = false;

                                for (i, &char_width) in
                                    self.current_text_state.widths.iter().enumerate()
                                {
                                    let char_center = x_offset + char_width as f32 / 2.0;
                                    if click_x < char_center {
                                        self.current_text_state.blinkey_index = i;
                                        found_position = true;
                                        break;
                                    }
                                    x_offset += char_width as f32;
                                }

                                if !found_position {
                                    self.current_text_state.blinkey_index =
                                        self.current_text_state.chars.len();
                                }
                            } else {
                                self.current_text_state.blinkey_index = 0;
                                self.current_text_state.scroll_offset = 0.0;
                            }

                            let text: String = self.current_text_state.chars.iter().collect();
                            debug_println!("CLICK: textbox @ mouse=({}, {}), was_focused={}, blinkey: {} -> {}, text=\"{}\" (len={})",
                                     mouse_x, mouse_y, was_focused, old_blinkey_index,
                                     self.current_text_state.blinkey_index, text, text.len());

                            // Calculate blinkey pixel position (needed before drawing)
                            let margin = self.min_dim / 8;
                            let box_height = self.min_dim / 8;
                            let center_x = self.width as usize / 2;
                            let center_y = self.height as usize * 4 / 7;
                            let font_size = self.font_size();

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

                            // Calculate NEW blinkey position
                            let blinkey_pixel_offset: usize =
                                if self.current_text_state.blinkey_index > 0 {
                                    self.current_text_state.widths
                                        [..self.current_text_state.blinkey_index]
                                        .iter()
                                        .sum()
                                } else {
                                    0
                                };
                            let total_text_width: usize = self.current_text_state.width;
                            let text_half = total_text_width / 2;
                            self.blinkey_pixel_x = (center_x as f32 - text_half as f32
                                + self.current_text_state.scroll_offset
                                + blinkey_pixel_offset as f32)
                                as usize;
                            self.blinkey_pixel_y =
                                (center_y as f32 - box_height as f32 * 0.25) as usize;

                            // Draw blinkey at NEW position (or start if first focus)
                            if !was_focused {
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
                        HIT_PRIMARY_BUTTON => {
                            // Primary button click: "Query", "Attest", or "Recover / Challenge"
                            let handle: String = self.current_text_state.chars.iter().collect();
                            match self.handle_status {
                                HandleStatus::Empty => {
                                    // "Query" button clicked - start network query
                                    debug_println!("Querying handle: {}", handle);
                                    self.query_handle();
                                    self.window_dirty = true;
                                }
                                HandleStatus::Checking => {
                                    // Query already in progress - ignore clicks
                                    debug_println!("Query already in progress, ignoring click");
                                }
                                HandleStatus::Unattested => {
                                    // "Attest" button clicked
                                    debug_println!("Attesting handle: {}", handle);
                                    // TODO: Implement attestation logic (create new identity)
                                }
                                HandleStatus::AlreadyAttested => {
                                    // "Recover / Challenge" button clicked - show dual choice screen
                                    self.handle_status = HandleStatus::RecoverOrChallenge;
                                    self.window_dirty = true;
                                }
                                _ => {
                                    // Shouldn't happen (Checking or RecoverOrChallenge states don't show primary button)
                                    debug_println!("Primary button clicked in unexpected state");
                                }
                            }
                            return;
                        }
                        HIT_RECOVER_BUTTON => {
                            // "Recover" button clicked (I'm recovering my own identity)
                            let handle: String = self.current_text_state.chars.iter().collect();
                            debug_println!("Recovering handle: {}", handle);
                            // TODO: Implement recovery flow (reconstruct from trust circle)
                            return;
                        }
                        HIT_CHALLENGE_BUTTON => {
                            // "Challenge" button clicked (proving earlier attestation)
                            let handle: String = self.current_text_state.chars.iter().collect();
                            debug_println!("Challenging attestation for handle: {}", handle);
                            // TODO: Implement challenge flow (prove earlier attestation)
                            return;
                        }
                        _ => {
                            // Clicked outside textbox, unfocus it
                            if self.current_text_state.textbox_focused {
                                self.current_text_state.textbox_focused = false;

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

                                // If textbox is empty, need to redraw to show infinity placeholder
                                if self.current_text_state.chars.is_empty() {
                                    self.text_dirty = true;
                                }
                            }
                        }
                    }
                }

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
                } else {
                    // Not on a resize edge, start window drag
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
            }
            ElementState::Released => {
                self.mouse_button_pressed = false;

                // End selection
                if self.is_mouse_selecting {
                    self.is_mouse_selecting = false;
                    self.selection_last_update_time = None;

                    // State transition: blinkey OFF -> ON (immediate-mode)
                    if !self.blinkey_visible && self.current_text_state.textbox_focused {
                        // Recalculate blinkey position first
                        let margin = self.min_dim / 8;
                        let box_height = self.min_dim / 8;
                        let center_x = self.width as usize / 2;
                        let center_y = self.height as usize * 4 / 7;
                        let font_size = self.font_size();

                        let blinkey_pixel_offset: usize = if self.current_text_state.blinkey_index
                            > 0
                        {
                            self.current_text_state.widths[..self.current_text_state.blinkey_index]
                                .iter()
                                .sum()
                        } else {
                            0
                        };
                        let total_text_width: usize = self.current_text_state.width;
                        let text_half = total_text_width / 2;
                        self.blinkey_pixel_x = (center_x as f32 - text_half as f32
                            + self.current_text_state.scroll_offset
                            + blinkey_pixel_offset as f32)
                            as usize;
                        self.blinkey_pixel_y =
                            (center_y as f32 - box_height as f32 * 0.25) as usize;

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
                let margin = self.min_dim / 8;
                let box_left = margin;
                let box_right = self.width as usize - margin;
                let mouse_x = self.mouse_x as f32;

                // Clamp mouse position to textbox bounds for blinkey calculation
                let clamped_mouse_x = mouse_x.clamp(box_left as f32, box_right as f32) as usize;

                // Find which character is at (clamped) mouse position
                let center_x = self.width as usize / 2;
                let total_text_width: usize = self.current_text_state.width;
                let text_half = total_text_width / 2;
                let text_start_x =
                    center_x as f32 - text_half as f32 + self.current_text_state.scroll_offset;

                let click_x = clamped_mouse_x as f32;
                let mut x_offset = text_start_x;
                let mut found_position = false;

                for (i, &char_width) in self.current_text_state.widths.iter().enumerate() {
                    let char_center = x_offset + char_width as f32 / 2.0;
                    if click_x < char_center {
                        self.current_text_state.blinkey_index = i;
                        found_position = true;
                        break;
                    }
                    x_offset += char_width as f32;
                }

                if !found_position {
                    self.current_text_state.blinkey_index = self.current_text_state.chars.len();
                }

                self.selection_dirty = true;
                self.controls_dirty = true;

                return true; // Request redraw
            }

            // Check button hover state using hitmap
            let old_hovered = self.hovered_button;

            // Get hit test value at mouse position
            let mouse_x = self.mouse_x as usize;
            let mouse_y = self.mouse_y as usize;
            let element_id = if mouse_x < self.width as usize && mouse_y < self.height as usize {
                let hit_idx = mouse_y * self.width as usize + mouse_x;
                let element_id = self.hit_test_map[hit_idx];
                debug_println!("MOUSE: pos=({}, {}), hit_idx={}, element_id={}", mouse_x, mouse_y, hit_idx, element_id);

                self.hovered_button = match element_id {
                    HIT_CLOSE_BUTTON => HoveredButton::Close,
                    HIT_MAXIMIZE_BUTTON => HoveredButton::Maximize,
                    HIT_MINIMIZE_BUTTON => HoveredButton::Minimize,
                    HIT_HANDLE_TEXTBOX => {
                        // Don't hover textbox when it's focused (would interfere with blinkey)
                        if self.current_text_state.textbox_focused {
                            debug_println!("HOVER: Textbox is focused, not hovering");
                            HoveredButton::None
                        } else {
                            debug_println!("HOVER: Textbox not focused, hovering");
                            HoveredButton::Textbox
                        }
                    }
                    HIT_PRIMARY_BUTTON | HIT_RECOVER_BUTTON | HIT_CHALLENGE_BUTTON => {
                        HoveredButton::QueryButton
                    }
                    _ => HoveredButton::None,
                };
                element_id
            } else {
                self.hovered_button = HoveredButton::None;
                HIT_NONE
            };

            // Update blinkey icon based on hover position
            // Check what we're hovering over
            let blinkey = if self.hovered_button != HoveredButton::None {
                CursorIcon::Pointer
            } else if element_id == HIT_PRIMARY_BUTTON
                || element_id == HIT_RECOVER_BUTTON
                || element_id == HIT_CHALLENGE_BUTTON
            {
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
            window.set_cursor(blinkey);

            // Return true if hover state changed
            if old_hovered != self.hovered_button {
                self.controls_dirty = true;
            }
            // Return true if hover state changed
            old_hovered != self.hovered_button
        }
    }
}
