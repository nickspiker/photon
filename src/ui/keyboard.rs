// Keyboard input handling for PhotonApp

use crate::debug_println;
use crate::ui::theme;

use super::app::{HandleStatus, PhotonApp};
use rand::Rng;
use unicode_normalization::UnicodeNormalization;
use winit::{
    event::{ElementState, KeyEvent},
    keyboard::{Key, NamedKey},
};

/// Check if a character passes the NFC round-trip test
/// Only rule: must round-trip cleanly through NFC normalization as a single char
fn is_char_valid(ch: char) -> bool {
    let normalized: String = ch.to_string().nfc().collect();

    // Must produce exactly 1 char after normalization
    if normalized.chars().count() != 1 {
        return false;
    }

    // Must round-trip (input == normalized output)
    let round_trip_char = normalized.chars().next().unwrap();
    round_trip_char == ch
}

impl PhotonApp {
    pub fn handle_keyboard(&mut self, event: KeyEvent) {
        if event.state == ElementState::Pressed {
            // Toggle debug visualizations with Ctrl shortcuts
            if self.modifiers.control_key() {
                if let Key::Character(ref c) = event.logical_key {
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

                    // Only process clipboard shortcuts if textbox is focused
                    if self.current_text_state.textbox_focused {
                        // Ctrl+A: Select all
                        if c.eq_ignore_ascii_case("a") {
                            if !self.current_text_state.chars.is_empty() {
                                self.current_text_state.selection_anchor = Some(0);
                                self.current_text_state.cursor_index =
                                    self.current_text_state.chars.len();
                                self.selection_dirty = true;
                                self.controls_dirty = true;
                            }
                            return;
                        }

                        // Ctrl+C: Copy
                        if c.eq_ignore_ascii_case("c") {
                            if let Some(selected_text) = self.get_selected_text() {
                                if let Ok(mut clipboard) = arboard::Clipboard::new() {
                                    let _ = clipboard.set_text(selected_text);
                                }
                            }
                            return;
                        }

                        // Ctrl+X: Cut
                        if c.eq_ignore_ascii_case("x") {
                            if let Some(selected_text) = self.get_selected_text() {
                                // Try to copy to clipboard first
                                let clipboard_ok = arboard::Clipboard::new()
                                    .and_then(|mut clip| clip.set_text(selected_text))
                                    .is_ok();

                                // Only delete if clipboard succeeded (or you don't care about failures)
                                if clipboard_ok {
                                    self.delete_selection();
                                    self.handle_status = HandleStatus::Empty;
                                    self.text_dirty = true;
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
                        if c.eq_ignore_ascii_case("v") {
                            if let Ok(mut clipboard) = arboard::Clipboard::new() {
                                if let Ok(text) = clipboard.get_text() {
                                    // Delete selection if it exists
                                    if self.current_text_state.selection_anchor.is_some() {
                                        self.delete_selection();
                                    }

                                    // Calculate widths for pasted text
                                    let font_size = self.font_size();
                                    let widths: Vec<usize> = text
                                        .chars()
                                        .map(|ch| {
                                            self.text_renderer.measure_text_width(
                                                &ch.to_string(),
                                                font_size,
                                                theme::FONT_WEIGHT_USER_CONTENT,
                                                theme::FONT_USER_CONTENT,
                                            ) as usize
                                        })
                                        .collect();

                                    // Insert pasted text at cursor
                                    let insert_idx = self.current_text_state.cursor_index;
                                    self.current_text_state
                                        .insert_str(insert_idx, &text, &widths);
                                    self.current_text_state.cursor_index += widths.len();
                                    self.handle_status = HandleStatus::Empty;
                                    self.text_dirty = true;
                                    self.controls_dirty = true;
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
                    if self.current_text_state.cursor_index > 0 {
                        // Start selection if Shift held and no selection exists
                        if shift_held && self.current_text_state.selection_anchor.is_none() {
                            self.current_text_state.selection_anchor =
                                Some(self.current_text_state.cursor_index);
                        }

                        self.current_text_state.cursor_index -= 1;

                        // Clear selection if Shift not held
                        if !shift_held {
                            self.current_text_state.selection_anchor = None;
                        }
                        self.selection_dirty = true;
                        self.controls_dirty = true;
                    }
                    return;
                }
                Key::Named(NamedKey::ArrowRight) => {
                    if self.current_text_state.cursor_index < self.current_text_state.chars.len() {
                        // Start selection if Shift held and no selection exists
                        if shift_held && self.current_text_state.selection_anchor.is_none() {
                            self.current_text_state.selection_anchor =
                                Some(self.current_text_state.cursor_index);
                        }

                        self.current_text_state.cursor_index += 1;

                        // Clear selection if Shift not held
                        if !shift_held {
                            self.current_text_state.selection_anchor = None;
                        }
                        self.selection_dirty = true;
                        self.controls_dirty = true;
                    }
                    return;
                }
                Key::Named(NamedKey::Home) | Key::Named(NamedKey::ArrowUp) => {
                    if self.current_text_state.cursor_index != 0 {
                        // Start selection if Shift held and no selection exists
                        if shift_held && self.current_text_state.selection_anchor.is_none() {
                            self.current_text_state.selection_anchor =
                                Some(self.current_text_state.cursor_index);
                        }

                        self.current_text_state.cursor_index = 0;

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
                    if self.current_text_state.cursor_index != end_pos {
                        // Start selection if Shift held and no selection exists
                        if shift_held && self.current_text_state.selection_anchor.is_none() {
                            self.current_text_state.selection_anchor =
                                Some(self.current_text_state.cursor_index);
                        }

                        self.current_text_state.cursor_index = end_pos;

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
                    // If selection exists, delete it; otherwise delete char before cursor
                    if self.current_text_state.selection_anchor.is_some() {
                        debug_println!("BACKSPACE: deleting selection");
                        self.delete_selection();
                        self.handle_status = HandleStatus::Empty;
                        self.text_dirty = true;
                        self.selection_dirty = true;
                    } else if self.current_text_state.cursor_index > 0 {
                        let idx = self.current_text_state.cursor_index - 1;
                        let deleted_char = self.current_text_state.chars[idx];
                        debug_println!("BACKSPACE: deleting '{}' at index {}, cursor: {} -> {}",
                                 deleted_char, idx, self.current_text_state.cursor_index, self.current_text_state.cursor_index - 1);
                        self.current_text_state.remove(idx);
                        self.current_text_state.cursor_index -= 1;
                        let text: String = self.current_text_state.chars.iter().collect();
                        debug_println!("  Text now: \"{}\" (len={})", text, text.len());
                        self.handle_status = HandleStatus::Empty;
                        self.text_dirty = true;
                        self.selection_dirty = true;
                        self.controls_dirty = true;
                    }
                    return;
                }
                Key::Named(NamedKey::Delete) => {
                    // If selection exists, delete it; otherwise delete char at cursor
                    if self.current_text_state.selection_anchor.is_some() {
                        self.delete_selection();
                        self.handle_status = HandleStatus::Empty;
                        self.text_dirty = true;
                        self.selection_dirty = true;
                    } else if self.current_text_state.cursor_index
                        < self.current_text_state.chars.len()
                    {
                        self.current_text_state
                            .remove(self.current_text_state.cursor_index);
                        self.handle_status = HandleStatus::Empty;
                        self.text_dirty = true;
                        self.selection_dirty = true;
                        self.controls_dirty = true;
                    }
                    return;
                }
                Key::Named(NamedKey::Enter) => {
                    if !self.current_text_state.chars.is_empty() {
                        self.submit_username();
                    }
                    return;
                }
                Key::Named(NamedKey::Escape) => {
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
                    // Validate character: must round-trip cleanly through NFC
                    if !is_char_valid(ch) {
                        // Silently reject invalid characters
                        continue;
                    }

                    // Measure character width
                    let width = self.text_renderer.measure_text_width(
                        &ch.to_string(),
                        font_size,
                        theme::FONT_WEIGHT_USER_CONTENT,
                        theme::FONT_USER_CONTENT,
                    ) as usize;

                    // Insert character with its width
                    let cursor_idx = self.current_text_state.cursor_index;
                    debug_println!("INSERT: adding '{}' at index {}, width={}, cursor: {} -> {}",
                             ch, cursor_idx, width, cursor_idx, cursor_idx + 1);
                    self.current_text_state.insert(cursor_idx, ch, width);
                    self.current_text_state.cursor_index += 1;
                    let text: String = self.current_text_state.chars.iter().collect();
                    debug_println!("  Text now: \"{}\" (len={})", text, text.len());
                }
                self.handle_status = HandleStatus::Empty;
                self.text_dirty = true;
                self.controls_dirty = true;
            }
        }
    }
}
