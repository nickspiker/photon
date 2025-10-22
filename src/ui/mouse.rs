// Mouse input handling for PhotonApp

use crate::ui::app::HoveredButton;

use super::app::{
    HandleStatus, PhotonApp, ResizeEdge, HIT_CHALLENGE_BUTTON, HIT_CLOSE_BUTTON,
    HIT_HANDLE_TEXTBOX, HIT_MAXIMIZE_BUTTON, HIT_MINIMIZE_BUTTON, HIT_NONE, HIT_PRIMARY_BUTTON,
    HIT_RECOVER_BUTTON,
};
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
                            // Focus the textbox and set cursor position based on click location
                            self.current_text_state.textbox_focused = true;
                            self.controls_dirty = true; // Focus changed - need to show cursor

                            // If textbox is empty, need to redraw to remove infinity placeholder
                            if self.current_text_state.chars.is_empty() {
                                self.text_dirty = true;
                            }

                            // Clear selection when clicking
                            self.current_text_state.selection_anchor = None;

                            // Calculate click position relative to text
                            if !self.current_text_state.chars.is_empty() {
                                let center_x = self.width as usize / 2;
                                let total_text_width: usize = self.current_text_state.width;
                                let text_half = total_text_width / 2;
                                let text_start_x = center_x as isize - text_half as isize
                                    + self.current_text_state.scroll_offset;

                                // Find which character was clicked
                                let click_x = mouse_x as isize;
                                let mut x_offset = text_start_x;
                                let mut found_position = false;

                                for (i, &char_width) in
                                    self.current_text_state.widths.iter().enumerate()
                                {
                                    let char_center = x_offset + char_width as isize / 2;
                                    if click_x < char_center {
                                        self.current_text_state.cursor_index = i;
                                        found_position = true;
                                        break;
                                    }
                                    x_offset += char_width as isize;
                                }

                                if !found_position {
                                    self.current_text_state.cursor_index =
                                        self.current_text_state.chars.len();
                                }
                            } else {
                                self.current_text_state.cursor_index = 0;
                                self.current_text_state.scroll_offset = 0;
                            }

                            return;
                        }
                        HIT_PRIMARY_BUTTON => {
                            // Primary button click: "Attest" or "Recover / Challenge"
                            let handle: String = self.current_text_state.chars.iter().collect();
                            match self.handle_status {
                                HandleStatus::Unattested => {
                                    // "Attest" button clicked
                                    println!("Attesting handle: {}", handle);
                                    // TODO: Implement attestation logic (create new identity)
                                }
                                HandleStatus::AlreadyAttested => {
                                    // "Recover / Challenge" button clicked - show dual choice screen
                                    self.handle_status = HandleStatus::RecoverOrChallenge;
                                    self.window_dirty = true;
                                }
                                _ => {
                                    // Shouldn't happen (Checking or RecoverOrChallenge states don't show primary button)
                                    println!("Primary button clicked in unexpected state");
                                }
                            }
                            return;
                        }
                        HIT_RECOVER_BUTTON => {
                            // "Recover" button clicked (I'm recovering my own identity)
                            let handle: String = self.current_text_state.chars.iter().collect();
                            println!("Recovering handle: {}", handle);
                            // TODO: Implement recovery flow (reconstruct from trust circle)
                            return;
                        }
                        HIT_CHALLENGE_BUTTON => {
                            // "Challenge" button clicked (proving earlier attestation)
                            let handle: String = self.current_text_state.chars.iter().collect();
                            println!("Challenging attestation for handle: {}", handle);
                            // TODO: Implement challenge flow (prove earlier attestation)
                            return;
                        }
                        _ => {
                            // Clicked outside textbox, unfocus it
                            if self.current_text_state.textbox_focused {
                                self.current_text_state.textbox_focused = false;
                                self.controls_dirty = true; // Focus changed - need to hide cursor

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
    ) -> bool {
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
            false // No redraw needed for window move
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
            false // No redraw needed for window resize (resize event handles it)
        } else {
            // Check button hover state using hitmap
            let old_hovered = self.hovered_button;

            // Get hit test value at mouse position
            let mouse_x = self.mouse_x as usize;
            let mouse_y = self.mouse_y as usize;
            let element_id = if mouse_x < self.width as usize && mouse_y < self.height as usize {
                let hit_idx = mouse_y * self.width as usize + mouse_x;
                let element_id = self.hit_test_map[hit_idx];

                self.hovered_button = match element_id {
                    HIT_CLOSE_BUTTON => HoveredButton::Close,
                    HIT_MAXIMIZE_BUTTON => HoveredButton::Maximize,
                    HIT_MINIMIZE_BUTTON => HoveredButton::Minimize,
                    _ => HoveredButton::None,
                };
                element_id
            } else {
                self.hovered_button = HoveredButton::None;
                HIT_NONE
            };

            // Update cursor icon based on hover position
            // Check what we're hovering over
            let cursor = if self.hovered_button != HoveredButton::None {
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
            window.set_cursor(cursor);

            // Return true if hover state changed
            if old_hovered != self.hovered_button {
                self.controls_dirty = true;
            }
            // Return true if hover state changed
            old_hovered != self.hovered_button
        }
    }
}
