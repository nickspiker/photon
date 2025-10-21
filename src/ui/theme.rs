// Global theme colours and constants
// All colours are u32 in packed ARGB format: 0xAARRGGBB

// Window edge colours
pub const WINDOW_LIGHT_EDGE: u32 = 0xFF_44_41_37;
pub const WINDOW_SHADOW_EDGE: u32 = 0xFF_2B_34_37;
pub const WINDOW_CONTROLS_BG: u32 = 0xFF_1E_1E_1E; // Background behind window control buttons
pub const WINDOW_CONTROLS_HAIRLINE: u32 = 0xFF_44_41_37; // Hairline separators between buttons

// Background colours
pub const BACKGROUND: u32 = 0xFF_06_08_09;
pub const BACKGROUND_ADDER: [u8; 3] = [16, 17, 20]; // Added to background texture variation (RGB only)
pub const LAUNCH_SCREEN_BG: u32 = 0xFF_12_12_18; // Launch screen background

// UI element colours
pub const LIGHT_EDGE: u32 = 0xFF_60_60_60;
pub const SHADOW_EDGE: u32 = 0xFF_20_20_20;
pub const FILL: u32 = 0xFF_40_40_40;

// Text colours
pub const FONT_LABEL: u32 = 0xFF_80_80_80;
pub const CURSOR_BRIGHTNESS: f32 = 100.; // Cursor wave brightness multiplier
pub const TEXT_BRIGHTNESS: u8 = 0xD0;

// Button colours
pub const BUTTON_BASE: u32 = 0xFF_40_40_40;
pub const BUTTON_LIGHT_EDGE: u32 = 0xFF_60_60_60;
pub const BUTTON_SHADOW_EDGE: u32 = 0xFF_20_20_20;
pub const BUTTON_HAIRLINE: u32 = 0xFF_32_32_32; // Hairline separators between buttons

// Button glyphs (base colours, not deltas)
pub const CLOSE_GLYPH: u32 = 0xFF_80_20_20;
pub const MAXIMIZE_GLYPH: u32 = 0xFF_48_6B_3A;
pub const MAXIMIZE_GLYPH_INTERIOR: u32 = 0xFF_28_2D_2E;
pub const MINIMIZE_GLYPH: u32 = 0xFF_33_30_C7;

// Button hover deltas (applied on hover, negated on unhover)
pub const CLOSE_HOVER: [i8; 4] = [33, -3, -7, 0]; // Red (A, R, G, B)
pub const MAXIMIZE_HOVER: [i8; 4] = [-6, 16, -6, 0]; // Green (A, R, G, B)
pub const MINIMIZE_HOVER: [i8; 4] = [-9, -6, 37, 0]; // Blue (A, R, G, B)

// Textbox colours
pub const TEXTBOX_LIGHT_EDGE: u32 = 0xFF_44_41_37;
pub const TEXTBOX_SHADOW_EDGE: u32 = 0xFF_2B_34_37;
pub const TEXTBOX_FILL: u32 = 0xFF_06_08_09;

// Logo colours (grayscale for glow/highlight)
pub const LOGO_GLOW_GRAY: u8 = 192; // Logo glow effect (grayscale)
pub const LOGO_HIGHLIGHT_GRAY: u8 = 128; // Logo highlight (grayscale)
pub const LOGO_TEXT: u32 = 0xFF_00_00_00; // Logo text colour

// Font families and weights
pub const FONT_LOGO: &str = "Oxanium";
pub const FONT_UI: &str = "Josefin Slab";
pub const FONT_USER_CONTENT: &str = "Open Sans";
pub const FONT_WEIGHT_USER_CONTENT: u16 = 400; // Font weight for user-entered text
