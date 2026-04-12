//! DeepSeek color palette and semantic roles.

use ratatui::style::Color;

pub const DEEPSEEK_BLUE_RGB: (u8, u8, u8) = (53, 120, 229); // #3578E5
pub const DEEPSEEK_SKY_RGB: (u8, u8, u8) = (106, 174, 242);
#[allow(dead_code)]
pub const DEEPSEEK_AQUA_RGB: (u8, u8, u8) = (54, 187, 212);
#[allow(dead_code)]
pub const DEEPSEEK_NAVY_RGB: (u8, u8, u8) = (24, 63, 138);
pub const DEEPSEEK_INK_RGB: (u8, u8, u8) = (11, 21, 38);
pub const DEEPSEEK_SLATE_RGB: (u8, u8, u8) = (18, 28, 46);
pub const DEEPSEEK_RED_RGB: (u8, u8, u8) = (226, 80, 96);

// New semantic colors
pub const BORDER_COLOR_RGB: (u8, u8, u8) = (42, 74, 127); // #2A4A7F

pub const DEEPSEEK_BLUE: Color = Color::Rgb(
    DEEPSEEK_BLUE_RGB.0,
    DEEPSEEK_BLUE_RGB.1,
    DEEPSEEK_BLUE_RGB.2,
);
pub const DEEPSEEK_SKY: Color =
    Color::Rgb(DEEPSEEK_SKY_RGB.0, DEEPSEEK_SKY_RGB.1, DEEPSEEK_SKY_RGB.2);
#[allow(dead_code)]
pub const DEEPSEEK_AQUA: Color = Color::Rgb(
    DEEPSEEK_AQUA_RGB.0,
    DEEPSEEK_AQUA_RGB.1,
    DEEPSEEK_AQUA_RGB.2,
);
#[allow(dead_code)]
pub const DEEPSEEK_NAVY: Color = Color::Rgb(
    DEEPSEEK_NAVY_RGB.0,
    DEEPSEEK_NAVY_RGB.1,
    DEEPSEEK_NAVY_RGB.2,
);
pub const DEEPSEEK_INK: Color =
    Color::Rgb(DEEPSEEK_INK_RGB.0, DEEPSEEK_INK_RGB.1, DEEPSEEK_INK_RGB.2);
pub const DEEPSEEK_SLATE: Color = Color::Rgb(
    DEEPSEEK_SLATE_RGB.0,
    DEEPSEEK_SLATE_RGB.1,
    DEEPSEEK_SLATE_RGB.2,
);
pub const DEEPSEEK_RED: Color =
    Color::Rgb(DEEPSEEK_RED_RGB.0, DEEPSEEK_RED_RGB.1, DEEPSEEK_RED_RGB.2);

pub const TEXT_BODY: Color = Color::White;
pub const TEXT_SECONDARY: Color = Color::Rgb(192, 192, 192); // #C0C0C0
pub const TEXT_HINT: Color = Color::Rgb(160, 160, 160); // #A0A0A0
pub const TEXT_ACCENT: Color = DEEPSEEK_SKY;
pub const SELECTION_TEXT: Color = Color::White;
pub const TEXT_SOFT: Color = Color::Rgb(214, 223, 235); // #D6DFEB

// Compatibility aliases for existing call sites.
pub const TEXT_PRIMARY: Color = TEXT_BODY;
pub const TEXT_MUTED: Color = TEXT_SECONDARY;
pub const TEXT_DIM: Color = TEXT_HINT;

// New semantic colors for UI theming
pub const BORDER_COLOR: Color =
    Color::Rgb(BORDER_COLOR_RGB.0, BORDER_COLOR_RGB.1, BORDER_COLOR_RGB.2);
#[allow(dead_code)]
pub const ACCENT_PRIMARY: Color = DEEPSEEK_BLUE; // #3578E5
#[allow(dead_code)]
pub const ACCENT_SECONDARY: Color = TEXT_ACCENT; // #6AAEF2
#[allow(dead_code)]
pub const BACKGROUND_LIGHT: Color = Color::Rgb(30, 47, 71); // #1E2F47
#[allow(dead_code)]
pub const BACKGROUND_DARK: Color = Color::Rgb(13, 26, 48); // #0D1A30
#[allow(dead_code)]
pub const STATUS_NEUTRAL: Color = Color::Rgb(160, 160, 160); // #A0A0A0
#[allow(dead_code)]
pub const SURFACE_PANEL: Color = Color::Rgb(21, 33, 52); // #152134
#[allow(dead_code)]
pub const SURFACE_ELEVATED: Color = Color::Rgb(28, 42, 64); // #1C2A40
#[allow(dead_code)]
pub const SURFACE_REASONING: Color = Color::Rgb(54, 44, 26); // #362C1A
#[allow(dead_code)]
pub const SURFACE_REASONING_ACTIVE: Color = Color::Rgb(68, 53, 28); // #44351C
#[allow(dead_code)]
pub const SURFACE_TOOL: Color = Color::Rgb(24, 39, 60); // #18273C
#[allow(dead_code)]
pub const SURFACE_TOOL_ACTIVE: Color = Color::Rgb(29, 48, 73); // #1D3049
#[allow(dead_code)]
pub const SURFACE_SUCCESS: Color = Color::Rgb(22, 56, 63); // #16383F
#[allow(dead_code)]
pub const SURFACE_ERROR: Color = Color::Rgb(63, 27, 36); // #3F1B24
pub const ACCENT_REASONING_LIVE: Color = Color::Rgb(146, 198, 248); // #92C6F8
pub const ACCENT_TOOL_LIVE: Color = Color::Rgb(133, 184, 234); // #85B8EA
pub const ACCENT_TOOL_ISSUE: Color = Color::Rgb(192, 143, 153); // #C08F99
pub const TEXT_TOOL_OUTPUT: Color = Color::Rgb(205, 216, 228); // #CDD8E4

// Legacy status colors - keep for backward compatibility
pub const STATUS_SUCCESS: Color = DEEPSEEK_SKY;
pub const STATUS_WARNING: Color = Color::Rgb(255, 170, 60); // Amber
pub const STATUS_ERROR: Color = DEEPSEEK_RED;
#[allow(dead_code)]
pub const STATUS_INFO: Color = DEEPSEEK_BLUE;

// Mode-specific accent colors for mode badges
pub const MODE_AGENT: Color = Color::Rgb(80, 150, 255); // Bright blue
pub const MODE_YOLO: Color = Color::Rgb(255, 100, 100); // Warning red
pub const MODE_PLAN: Color = Color::Rgb(255, 170, 60); // Orange

pub const SELECTION_BG: Color = Color::Rgb(26, 44, 74);
pub const COMPOSER_BG: Color = DEEPSEEK_SLATE;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UiTheme {
    pub name: &'static str,
    pub composer_bg: Color,
    pub selection_bg: Color,
    pub header_bg: Color,
}

pub fn ui_theme(name: &str) -> UiTheme {
    match name.to_ascii_lowercase().as_str() {
        "dark" => UiTheme {
            name: "dark",
            composer_bg: DEEPSEEK_INK,
            selection_bg: SELECTION_BG,
            header_bg: DEEPSEEK_INK,
        },
        "light" => UiTheme {
            name: "light",
            composer_bg: Color::Rgb(26, 38, 58),
            selection_bg: SELECTION_BG,
            header_bg: DEEPSEEK_SLATE,
        },
        "whale" => UiTheme {
            name: "whale",
            composer_bg: DEEPSEEK_SLATE,
            selection_bg: SELECTION_BG,
            header_bg: DEEPSEEK_INK,
        },
        _ => UiTheme {
            name: "default",
            composer_bg: COMPOSER_BG,
            selection_bg: SELECTION_BG,
            header_bg: DEEPSEEK_INK,
        },
    }
}
