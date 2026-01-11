// Keyboard input handling for PhotonApp

use crate::ui::theme;
use crate::{debug_println, DEBUG_ENABLED};
use std::sync::atomic::Ordering;

use super::app::{AppState, LaunchState, PhotonApp, TextLayout};
use rand::Rng;
use winit::{
    event::{ElementState, KeyEvent},
    keyboard::{Key, NamedKey},
};

impl PhotonApp {
    pub fn handle_keyboard(&mut self, event: KeyEvent) {
        if event.state == ElementState::Pressed {
            // Toggle debug visualizations with Ctrl shortcuts (development builds only)
            #[cfg(feature = "debug-keys")]
            if self.modifiers.control_key() {
                if let Key::Character(ref c) = event.logical_key {
                    // Ctrl+D: Toggle debug mode (counters + debug_println!)
                    if c.eq_ignore_ascii_case("d") {
                        self.debug = !self.debug;
                        // Also toggle the global atomic debug flag for debug_println!
                        DEBUG_ENABLED.store(self.debug, Ordering::Relaxed);
                        self.window_dirty = true; // Force redraw
                        return; // Don't process as regular input
                    }

                    // Ctrl+H: Toggle hit test map
                    if c.eq_ignore_ascii_case("h") {
                        self.debug_hit_test = !self.debug_hit_test;
                        self.show_textbox_mask = false; // Clear other debug state
                        self.window_dirty = true; // Force redraw to show visualization
                                                  // Generate new random colours for each hit area
                        let mut rng = rand::thread_rng();
                        self.debug_hit_colours.clear();
                        for _ in 0..=255u8 {
                            let r = rng.gen();
                            let g = rng.gen();
                            let b = rng.gen();
                            self.debug_hit_colours.push((r, g, b));
                        }
                        return; // Don't process as regular input
                    }

                    // Ctrl+T: Toggle textbox mask visualization
                    if c.eq_ignore_ascii_case("t") {
                        self.show_textbox_mask = !self.show_textbox_mask;
                        self.debug_hit_test = false; // Clear other debug state
                        self.window_dirty = true; // Force redraw to show visualization
                        return; // Don't process as regular input
                    }
                }
            }

            // Ctrl++/- for zoom (available in all states)
            // Uses logarithmic scaling: 1 step = multiply by 33/32
            if self.modifiers.control_key() {
                if let Key::Character(ref c) = event.logical_key {
                    // Ctrl++ or Ctrl+= : Zoom in (1 step)
                    if c == "+" || c == "=" {
                        self.adjust_zoom(1.0);
                        return;
                    }
                    // Ctrl+- : Zoom out (1 step)
                    if c == "-" {
                        self.adjust_zoom(-1.0);
                        return;
                    }
                    // Ctrl+0 : Reset zoom to 100%
                    if c == "0" {
                        self.ru = 1.0;
                        self.window_dirty = true;
                        self.recalculate_char_widths();
                        return;
                    }
                }
            }

            // Clipboard and text editing Ctrl shortcuts
            if self.modifiers.control_key() {
                if let Key::Character(ref c) = event.logical_key {
                    // Only process clipboard shortcuts if textbox is focused
                    if self.current_text_state.textbox_focused {
                        // Ctrl+A: Select all
                        if c.eq_ignore_ascii_case("a") {
                            if !self.current_text_state.chars.is_empty() {
                                self.current_text_state.selection_anchor = Some(0);
                                self.current_text_state.blinkey_index =
                                    self.current_text_state.chars.len();
                                self.selection_dirty = true;
                                self.controls_dirty = true;
                            }
                            return;
                        }

                        // Ctrl+C: Copy
                        #[cfg(not(target_os = "redox"))]
                        if c.eq_ignore_ascii_case("c") {
                            if let Some(selected_text) = self.get_selected_text() {
                                if let Ok(mut clipboard) = arboard::Clipboard::new() {
                                    let _ = clipboard.set_text(selected_text);
                                }
                            }
                            return;
                        }

                        // Ctrl+X: Cut
                        #[cfg(not(target_os = "redox"))]
                        if c.eq_ignore_ascii_case("x") {
                            if let Some(selected_text) = self.get_selected_text() {
                                // Try to copy to clipboard first
                                let clipboard_ok = arboard::Clipboard::new()
                                    .and_then(|mut clip| clip.set_text(selected_text))
                                    .is_ok();

                                // Only delete if clipboard succeeded (or you don't care about failures)
                                if clipboard_ok {
                                    self.delete_selection();
                                    if matches!(self.app_state, AppState::Launch(_)) {
                                        self.set_launch_state(LaunchState::Fresh);
                                    }
                                    if self.search_result.is_some() {
                                        self.window_dirty = true;
                                    }
                                    self.text_dirty = true;
                                    self.glow_colour = theme::GLOW_DEFAULT; // Reset glow to white on text change
                                    self.search_result = None; // Clear search result on text change
                                    self.selection_dirty = true;
                                    self.controls_dirty = true; // Cursor position changed
                                } else {
                                    // Optional: show error to user that clipboard failed
                                    log::warn!("Failed to copy to clipboard, not cutting");
                                }
                            }
                            return;
                        }

                        // Ctrl+V: Paste
                        #[cfg(not(target_os = "redox"))]
                        if c.eq_ignore_ascii_case("v") {
                            if let Ok(mut clipboard) = arboard::Clipboard::new() {
                                if let Ok(text) = clipboard.get_text() {
                                    self.paste_text(&text);
                                }
                            }
                            return;
                        }
                    }
                }
                // If Ctrl is held, don't process any other input
                return;
            }

            // All text editing requires textbox focus
            if !self.current_text_state.textbox_focused {
                return;
            }

            let shift_held = self.modifiers.shift_key();

            match event.logical_key {
                Key::Named(NamedKey::ArrowLeft) => {
                    // If there's a selection and Shift not held, go to left edge of selection
                    if let Some(anchor) = self.current_text_state.selection_anchor {
                        if !shift_held {
                            let left = anchor.min(self.current_text_state.blinkey_index);
                            self.current_text_state.blinkey_index = left;
                            self.current_text_state.selection_anchor = None;
                            self.selection_dirty = true;
                            self.controls_dirty = true;
                            return;
                        }
                    }

                    if self.current_text_state.blinkey_index > 0 {
                        // Start selection if Shift held and no selection exists
                        if shift_held && self.current_text_state.selection_anchor.is_none() {
                            self.current_text_state.selection_anchor =
                                Some(self.current_text_state.blinkey_index);
                        }

                        self.current_text_state.blinkey_index -= 1;
                        self.selection_dirty = true;
                        self.controls_dirty = true;
                    }
                    return;
                }
                Key::Named(NamedKey::ArrowRight) => {
                    // If there's a selection and Shift not held, go to right edge of selection
                    if let Some(anchor) = self.current_text_state.selection_anchor {
                        if !shift_held {
                            let right = anchor.max(self.current_text_state.blinkey_index);
                            self.current_text_state.blinkey_index = right;
                            self.current_text_state.selection_anchor = None;
                            self.selection_dirty = true;
                            self.controls_dirty = true;
                            return;
                        }
                    }

                    if self.current_text_state.blinkey_index < self.current_text_state.chars.len() {
                        // Start selection if Shift held and no selection exists
                        if shift_held && self.current_text_state.selection_anchor.is_none() {
                            self.current_text_state.selection_anchor =
                                Some(self.current_text_state.blinkey_index);
                        }

                        self.current_text_state.blinkey_index += 1;
                        self.selection_dirty = true;
                        self.controls_dirty = true;
                    }
                    return;
                }
                Key::Named(NamedKey::Home) | Key::Named(NamedKey::ArrowUp) => {
                    if self.current_text_state.blinkey_index != 0 {
                        // Start selection if Shift held and no selection exists
                        if shift_held && self.current_text_state.selection_anchor.is_none() {
                            self.current_text_state.selection_anchor =
                                Some(self.current_text_state.blinkey_index);
                        }

                        self.current_text_state.blinkey_index = 0;

                        // Clear selection if Shift not held
                        if !shift_held {
                            self.current_text_state.selection_anchor = None;
                        }
                        self.selection_dirty = true;
                        self.controls_dirty = true;
                    }
                    return;
                }
                Key::Named(NamedKey::End) | Key::Named(NamedKey::ArrowDown) => {
                    let end_pos = self.current_text_state.chars.len();
                    if self.current_text_state.blinkey_index != end_pos {
                        // Start selection if Shift held and no selection exists
                        if shift_held && self.current_text_state.selection_anchor.is_none() {
                            self.current_text_state.selection_anchor =
                                Some(self.current_text_state.blinkey_index);
                        }

                        self.current_text_state.blinkey_index = end_pos;

                        // Clear selection if Shift not held
                        if !shift_held {
                            self.current_text_state.selection_anchor = None;
                        }
                        self.selection_dirty = true;
                        self.controls_dirty = true;
                    }
                    return;
                }
                Key::Named(NamedKey::Backspace) => {
                    // If selection exists, delete it; otherwise delete char before blinkey
                    if self.current_text_state.selection_anchor.is_some() {
                        debug_println!("BACKSPACE: deleting selection");
                        self.delete_selection();
                        if matches!(self.app_state, AppState::Launch(_)) {
                            if !matches!(self.app_state, AppState::Launch(LaunchState::Fresh)) {
                                self.window_dirty = true; // Force redraw to update button
                            }
                            self.set_launch_state(LaunchState::Fresh);
                        }
                        if matches!(self.app_state, AppState::Ready | AppState::Searching) {
                            self.window_dirty = true; // Redraw for contact filtering
                            self.contacts_scroll_offset = 0; // Reset scroll when filter changes
                        }
                        if self.search_result.is_some() {
                            self.window_dirty = true;
                        }
                        self.text_dirty = true;
                        self.glow_colour = theme::GLOW_DEFAULT; // Reset glow to white on text change
                        self.search_result = None; // Clear search result on text change
                        self.selection_dirty = true;
                    } else if self.current_text_state.blinkey_index > 0 {
                        let idx = self.current_text_state.blinkey_index - 1;
                        let deleted_char = self.current_text_state.chars[idx];
                        debug_println!(
                            "BACKSPACE: deleting '{}' at index {}, blinkey: {} -> {}",
                            deleted_char,
                            idx,
                            self.current_text_state.blinkey_index,
                            self.current_text_state.blinkey_index - 1
                        );
                        self.current_text_state.remove(idx);
                        self.current_text_state.blinkey_index -= 1;
                        let text: String = self.current_text_state.chars.iter().collect();
                        debug_println!("  Text now: \"{}\" (len={})", text, text.len());
                        if matches!(self.app_state, AppState::Launch(_)) {
                            if !matches!(self.app_state, AppState::Launch(LaunchState::Fresh)) {
                                self.window_dirty = true; // Force redraw to update button
                            }
                            self.set_launch_state(LaunchState::Fresh);
                        }
                        if matches!(self.app_state, AppState::Ready | AppState::Searching) {
                            self.window_dirty = true; // Redraw for contact filtering
                            self.contacts_scroll_offset = 0; // Reset scroll when filter changes
                        }
                        if self.search_result.is_some() {
                            self.window_dirty = true;
                        }
                        self.text_dirty = true;
                        self.glow_colour = theme::GLOW_DEFAULT; // Reset glow to white on text change
                        self.search_result = None; // Clear search result on text change
                        self.selection_dirty = true;
                        self.controls_dirty = true;
                    }
                    return;
                }
                Key::Named(NamedKey::Delete) => {
                    // If selection exists, delete it; otherwise delete char at blinkey
                    if self.current_text_state.selection_anchor.is_some() {
                        self.delete_selection();
                        if matches!(self.app_state, AppState::Launch(_)) {
                            if !matches!(self.app_state, AppState::Launch(LaunchState::Fresh)) {
                                self.window_dirty = true; // Force redraw to update button
                            }
                            self.set_launch_state(LaunchState::Fresh);
                        }
                        if matches!(self.app_state, AppState::Ready | AppState::Searching) {
                            self.window_dirty = true; // Redraw for contact filtering
                            self.contacts_scroll_offset = 0; // Reset scroll when filter changes
                        }
                        if self.search_result.is_some() {
                            self.window_dirty = true;
                        }
                        self.text_dirty = true;
                        self.glow_colour = theme::GLOW_DEFAULT; // Reset glow to white on text change
                        self.search_result = None; // Clear search result on text change
                        self.selection_dirty = true;
                    } else if self.current_text_state.blinkey_index
                        < self.current_text_state.chars.len()
                    {
                        self.current_text_state
                            .remove(self.current_text_state.blinkey_index);
                        if matches!(self.app_state, AppState::Launch(_)) {
                            if !matches!(self.app_state, AppState::Launch(LaunchState::Fresh)) {
                                self.window_dirty = true; // Force redraw to update button
                            }
                            self.set_launch_state(LaunchState::Fresh);
                        }
                        if matches!(self.app_state, AppState::Ready | AppState::Searching) {
                            self.window_dirty = true; // Redraw for contact filtering
                            self.contacts_scroll_offset = 0; // Reset scroll when filter changes
                        }
                        if self.search_result.is_some() {
                            self.window_dirty = true;
                        }
                        self.text_dirty = true;
                        self.glow_colour = theme::GLOW_DEFAULT; // Reset glow to white on text change
                        self.search_result = None; // Clear search result on text change
                        self.selection_dirty = true;
                        self.controls_dirty = true;
                    }
                    return;
                }
                Key::Named(NamedKey::Enter) => {
                    if !self.current_text_state.chars.is_empty() {
                        match &self.app_state {
                            AppState::Launch(_) => {
                                // In Launch state: attest this handle
                                self.start_attestation();
                            }
                            AppState::Ready => {
                                // In Ready state: add contact (same as + button)
                                let handle: String =
                                    self.current_text_state.chars.iter().collect();
                                let handle_lower = handle.to_lowercase();

                                // Check if contact already exists (exact match)
                                let existing_idx = self.contacts.iter().position(|c| {
                                    c.handle.as_str().to_lowercase() == handle_lower
                                });

                                if let Some(idx) = existing_idx {
                                    // Contact already exists - open conversation
                                    self.selected_contact = Some(idx);
                                    self.app_state = AppState::Conversation;
                                    self.reset_textbox();
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
                                } else {
                                    // New handle - search network
                                    self.start_handle_search(&handle);
                                }
                                self.window_dirty = true;
                            }
                            AppState::Searching => {
                                // Already searching, ignore Enter
                            }
                            AppState::Conversation => {
                                // In Conversation state: send message
                                let message: String =
                                    self.current_text_state.chars.iter().collect();
                                if self.send_message_to_selected_contact(&message) {
                                    // Clear textbox after successful send
                                    self.reset_textbox();
                                    self.window_dirty = true;
                                }
                            }
                            AppState::Connected { .. } => {
                                // In Connected state: send message (TODO)
                                // For now, do nothing
                            }
                        }
                    }
                    return;
                }
                Key::Named(NamedKey::Escape) => {
                    // In Conversation view: go back to Ready view
                    if matches!(self.app_state, AppState::Conversation) {
                        // Trigger peer IP refresh when returning to contacts screen
                        self.trigger_peer_refresh();

                        self.app_state = AppState::Ready;
                        self.text_layout = TextLayout::new(
                            self.width as usize,
                            self.height as usize,
                            self.span,
                            self.effective_ru(),
                            &self.app_state,
                        );
                        self.selected_contact = None;
                        self.window_dirty = true;
                        self.reset_textbox();
                        return;
                    }
                    // Clear selection on Escape
                    if self.current_text_state.selection_anchor.is_some() {
                        self.current_text_state.selection_anchor = None;
                        self.selection_dirty = true;
                    }
                    return;
                }
                _ => {}
            }

            // Handle text input using event.text field (includes space and all printable chars)
            // Named keys return early above, so we only reach here for actual text input
            if let Some(text) = &event.text {
                // Delete selection first if it exists
                if self.current_text_state.selection_anchor.is_some() {
                    self.delete_selection();
                }

                let font_size = self.font_size();
                for ch in text.chars() {
                    // VSF handles normalization, accept all chars

                    // Measure character width
                    let width = self.text_renderer.measure_text_width(
                        &ch.to_string(),
                        font_size,
                        theme::FONT_WEIGHT_USER_CONTENT,
                        theme::FONT_USER_CONTENT,
                    ) as usize;

                    // Insert character with its width
                    let blinkey_idx = self.current_text_state.blinkey_index;
                    debug_println!(
                        "INSERT: adding '{}' at index {}, width={}, blinkey: {} -> {}",
                        ch,
                        blinkey_idx,
                        width,
                        blinkey_idx,
                        blinkey_idx + 1
                    );
                    self.current_text_state.insert(blinkey_idx, ch, width);
                    self.current_text_state.blinkey_index += 1;
                    let text: String = self.current_text_state.chars.iter().collect();
                    debug_println!("  Text now: \"{}\" (len={})", text, text.len());
                }
                // Only reset to Fresh state if we're in Launch mode
                // Don't touch app_state if we're in Ready or Connected
                if matches!(self.app_state, AppState::Launch(_)) {
                    if !matches!(self.app_state, AppState::Launch(LaunchState::Fresh)) {
                        self.window_dirty = true; // Force redraw to update button
                    }
                    self.set_launch_state(LaunchState::Fresh);
                }
                // Force redraw for contact filtering in Ready/Searching states
                if matches!(self.app_state, AppState::Ready | AppState::Searching) {
                    self.window_dirty = true;
                    self.contacts_scroll_offset = 0; // Reset scroll when filter changes
                }
                // Clear search result and force redraw if we had a result showing
                if self.search_result.is_some() {
                    self.window_dirty = true;
                }
                self.text_dirty = true;
                self.glow_colour = theme::GLOW_DEFAULT; // Reset glow to white on text change
                self.search_result = None; // Clear search result on text change
                self.controls_dirty = true;
            }
        }
    }
}
