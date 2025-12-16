//! Centralized layout geometry for UI elements.
//!
//! All sizes are computed as ratios of `min_dim` (the smaller of width/height).
//! This ensures consistent proportions regardless of window size or aspect ratio.

use super::app::AppState;

/// Button geometry (square buttons with centered icon)
#[derive(Clone, Copy, Debug, Default)]
pub struct ButtonLayout {
    pub center_x: usize,
    pub center_y: usize,
    pub size: usize,      // Button diameter (square, so one dimension)
    pub icon_size: usize, // Inner icon size (typically size * 3/4)
}

/// Textbox geometry including optional button
#[derive(Clone, Debug, Default)]
pub struct TextboxLayout {
    pub x: usize,           // Left edge
    pub y: usize,           // Vertical center
    pub width: usize,       // Total width
    pub height: usize,      // Total height
    pub font_size: f32,     // Text rendering size
    pub content_width: usize, // Usable width for text (width minus button area if present)
    pub button: Option<ButtonLayout>, // Send/Query button (None on Launch)
    pub inset: usize,       // Button inset from textbox edge
}

/// Top-level UI layout computed once per resize/state change
#[derive(Clone, Debug, Default)]
pub struct Layout {
    pub textbox: TextboxLayout,
    pub center_x: usize, // Screen center X
    pub center_y: usize, // Screen center Y
}

impl Layout {
    /// Compute layout from current dimensions and app state
    pub fn compute(width: u32, height: u32, min_dim: usize, app_state: &AppState) -> Self {
        let width = width as usize;
        let height = height as usize;
        let center_x = width / 2;
        let center_y = height / 2;

        // Textbox geometry (ratios from existing code)
        let margin = min_dim / 8;
        let box_width = width - margin * 2;
        let box_height = min_dim / 8;
        let font_size = min_dim as f32 / 16.0;

        // Textbox Y varies by state
        let textbox_y = match app_state {
            AppState::Ready | AppState::Searching => min_dim * 5 / 8,
            AppState::Conversation => height - box_height * 3 / 2,
            _ => height * 5 / 8, // Launch states
        };

        // Button present on Ready/Searching/Conversation, NOT on Launch
        let has_button = matches!(
            app_state,
            AppState::Ready | AppState::Searching | AppState::Conversation
        );

        let inset = box_height / 16;
        let button_size = box_height * 7 / 8;

        let (button, content_width) = if has_button {
            let textbox_bottom = textbox_y + box_height / 2;
            let button = ButtonLayout {
                center_x: center_x + box_width / 2 - inset - button_size / 2,
                center_y: textbox_bottom - inset - button_size / 2,
                size: button_size,
                icon_size: button_size * 3 / 4,
            };
            // Content width excludes button area
            let content_width = box_width - button_size - inset * 2;
            (Some(button), content_width)
        } else {
            (None, box_width)
        };

        Self {
            textbox: TextboxLayout {
                x: margin,
                y: textbox_y,
                width: box_width,
                height: box_height,
                font_size,
                content_width,
                button,
                inset,
            },
            center_x,
            center_y,
        }
    }
}
