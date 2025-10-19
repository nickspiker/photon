// Global theme colors and constants

// UI element colors
pub const LIGHT_EDGE: (u8, u8, u8) = (96, 96, 96);
pub const SHADOW_EDGE: (u8, u8, u8) = (32, 32, 32);
pub const FILL: (u8, u8, u8) = (64, 64, 64);

// Text colors
pub const TEXT_PLACEHOLDER: [u8; 4] = [128, 128, 128, 255]; // 50% grey for placeholder text
pub const TEXT_LABEL: [u8; 4] = [128, 128, 128, 0]; // 50% grey for labels

// Button colors
pub const BUTTON_BASE: (u8, u8, u8) = (64, 64, 64);
pub const BUTTON_LIGHT_EDGE: (u8, u8, u8) = (96, 96, 96);
pub const BUTTON_SHADOW_EDGE: (u8, u8, u8) = (32, 32, 32);

// Button hover deltas (applied on hover, negated on unhover)
pub const CLOSE_HOVER: (i8, i8, i8) = (33, -3, -7); // Red
pub const MAXIMIZE_HOVER: (i8, i8, i8) = (-6, 16, -6); // Green
pub const MINIMIZE_HOVER: (i8, i8, i8) = (-9, -6, 37); // Blue

// Font families
pub const FONT_LOGO: &str = "Oxanium";
pub const FONT_UI: &str = "Josefin Slab";
pub const FONT_USER_CONTENT: &str = "Open Sans";
