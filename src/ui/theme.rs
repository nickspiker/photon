// Global theme colors and constants
// All colors are vectors with last channel as alpha for future multichannel support

// Window edge colors
pub const WINDOW_LIGHT_EDGE: [u8; 4] = [68, 65, 55, 255];
pub const WINDOW_SHADOW_EDGE: [u8; 4] = [43, 52, 55, 255];
pub const WINDOW_CONTROLS_BG: [u8; 4] = [30, 30, 30, 255]; // Background behind window control buttons
pub const WINDOW_CONTROLS_HAIRLINE: [u8; 4] = [68, 65, 55, 255]; // Hairline separators between buttons

// Background colors
pub const BACKGROUND: [u8; 4] = [6, 8, 9, 255];
pub const BACKGROUND_ADDER: [u8; 3] = [16, 17, 20]; // Added to background texture variation
pub const LAUNCH_SCREEN_BG: [u8; 4] = [18, 18, 24, 255]; // Launch screen background

// UI element colors
pub const LIGHT_EDGE: [u8; 4] = [96, 96, 96, 255];
pub const SHADOW_EDGE: [u8; 4] = [32, 32, 32, 255];
pub const FILL: [u8; 4] = [64, 64, 64, 255];

// Text colors
pub const FONT_HINT: [u8; 4] = [50, 60, 64, 255];
pub const FONT_LABEL: [u8; 4] = [128, 128, 128, 255];

// Button colors
pub const BUTTON_BASE: [u8; 4] = [64, 64, 64, 255];
pub const BUTTON_LIGHT_EDGE: [u8; 4] = [96, 96, 96, 255];
pub const BUTTON_SHADOW_EDGE: [u8; 4] = [32, 32, 32, 255];
pub const BUTTON_HAIRLINE: [u8; 4] = [50, 50, 50, 255]; // Hairline separators between buttons

// Button glyphs (base colors, not deltas)
pub const CLOSE_GLYPH: [u8; 4] = [128, 32, 32, 255];
pub const MAXIMIZE_GLYPH: [u8; 4] = [72, 107, 58, 255];
pub const MAXIMIZE_GLYPH_INTERIOR: [u8; 4] = [40, 45, 46, 255];
pub const MINIMIZE_GLYPH: [u8; 4] = [51, 48, 199, 255];

// Button hover deltas (applied on hover, negated on unhover)
pub const CLOSE_HOVER: [i8; 4] = [33, -3, -7, 0]; // Red
pub const MAXIMIZE_HOVER: [i8; 4] = [-6, 16, -6, 0]; // Green
pub const MINIMIZE_HOVER: [i8; 4] = [-9, -6, 37, 0]; // Blue

// Textbox colors
pub const TEXTBOX_LIGHT_EDGE: [u8; 4] = [68, 65, 55, 255];
pub const TEXTBOX_SHADOW_EDGE: [u8; 4] = [43, 52, 55, 255];
pub const TEXTBOX_FILL: [u8; 4] = [6, 8, 9, 255];

// Logo colors (grayscale for glow/highlight, RGBA for final text)
pub const LOGO_GLOW_GRAY: u8 = 192; // Logo glow effect (grayscale)
pub const LOGO_HIGHLIGHT_GRAY: u8 = 128; // Logo highlight (grayscale)
pub const LOGO_TEXT: [u8; 4] = [0, 0, 0, 255]; // Logo text color

// Font families
pub const FONT_LOGO: &str = "Oxanium";
pub const FONT_UI: &str = "Josefin Slab";
pub const FONT_USER_CONTENT: &str = "Open Sans";
