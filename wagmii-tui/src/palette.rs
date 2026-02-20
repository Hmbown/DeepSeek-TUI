//! Wagmii color palette and semantic roles.

use ratatui::style::Color;

pub const WAGMII_BLUE_RGB: (u8, u8, u8) = (53, 120, 229); // #3578E5
pub const WAGMII_SKY_RGB: (u8, u8, u8) = (106, 174, 242);
#[allow(dead_code)]
pub const WAGMII_AQUA_RGB: (u8, u8, u8) = (54, 187, 212);
#[allow(dead_code)]
pub const WAGMII_NAVY_RGB: (u8, u8, u8) = (24, 63, 138);
pub const WAGMII_INK_RGB: (u8, u8, u8) = (11, 21, 38);
pub const WAGMII_SLATE_RGB: (u8, u8, u8) = (18, 28, 46);
pub const WAGMII_RED_RGB: (u8, u8, u8) = (226, 80, 96);

// New semantic colors
pub const BORDER_COLOR_RGB: (u8, u8, u8) = (42, 74, 127); // #2A4A7F

pub const WAGMII_BLUE: Color = Color::Rgb(
    WAGMII_BLUE_RGB.0,
    WAGMII_BLUE_RGB.1,
    WAGMII_BLUE_RGB.2,
);
pub const WAGMII_SKY: Color =
    Color::Rgb(WAGMII_SKY_RGB.0, WAGMII_SKY_RGB.1, WAGMII_SKY_RGB.2);
#[allow(dead_code)]
pub const WAGMII_AQUA: Color = Color::Rgb(
    WAGMII_AQUA_RGB.0,
    WAGMII_AQUA_RGB.1,
    WAGMII_AQUA_RGB.2,
);
#[allow(dead_code)]
pub const WAGMII_NAVY: Color = Color::Rgb(
    WAGMII_NAVY_RGB.0,
    WAGMII_NAVY_RGB.1,
    WAGMII_NAVY_RGB.2,
);
pub const WAGMII_INK: Color =
    Color::Rgb(WAGMII_INK_RGB.0, WAGMII_INK_RGB.1, WAGMII_INK_RGB.2);
pub const WAGMII_SLATE: Color = Color::Rgb(
    WAGMII_SLATE_RGB.0,
    WAGMII_SLATE_RGB.1,
    WAGMII_SLATE_RGB.2,
);
pub const WAGMII_RED: Color =
    Color::Rgb(WAGMII_RED_RGB.0, WAGMII_RED_RGB.1, WAGMII_RED_RGB.2);

pub const TEXT_BODY: Color = Color::White;
pub const TEXT_SECONDARY: Color = Color::Rgb(192, 192, 192); // #C0C0C0
pub const TEXT_HINT: Color = Color::Rgb(160, 160, 160); // #A0A0A0
pub const TEXT_ACCENT: Color = WAGMII_SKY;
pub const FOOTER_HINT: Color = Color::Rgb(180, 190, 208); // #B4BED0
pub const SELECTION_TEXT: Color = Color::White;

// Compatibility aliases for existing call sites.
pub const TEXT_PRIMARY: Color = TEXT_BODY;
pub const TEXT_MUTED: Color = TEXT_SECONDARY;
pub const TEXT_DIM: Color = TEXT_HINT;

// New semantic colors for UI theming
pub const BORDER_COLOR: Color =
    Color::Rgb(BORDER_COLOR_RGB.0, BORDER_COLOR_RGB.1, BORDER_COLOR_RGB.2);
#[allow(dead_code)]
pub const ACCENT_PRIMARY: Color = WAGMII_BLUE; // #3578E5
#[allow(dead_code)]
pub const ACCENT_SECONDARY: Color = TEXT_ACCENT; // #6AAEF2
#[allow(dead_code)]
pub const BACKGROUND_LIGHT: Color = Color::Rgb(30, 47, 71); // #1E2F47
#[allow(dead_code)]
pub const BACKGROUND_DARK: Color = Color::Rgb(13, 26, 48); // #0D1A30
#[allow(dead_code)]
pub const STATUS_NEUTRAL: Color = Color::Rgb(160, 160, 160); // #A0A0A0

// Legacy status colors - keep for backward compatibility
pub const STATUS_SUCCESS: Color = WAGMII_SKY;
pub const STATUS_WARNING: Color = Color::Rgb(255, 170, 60); // Amber
pub const STATUS_ERROR: Color = WAGMII_RED;
#[allow(dead_code)]
pub const STATUS_INFO: Color = WAGMII_BLUE;

// Mode-specific accent colors for mode badges
pub const MODE_NORMAL: Color = Color::Rgb(192, 192, 192); // #C0C0C0
pub const MODE_AGENT: Color = Color::Rgb(80, 150, 255); // Bright blue
pub const MODE_YOLO: Color = Color::Rgb(255, 100, 100); // Warning red
pub const MODE_PLAN: Color = Color::Rgb(255, 170, 60); // Orange

pub const SELECTION_BG: Color = Color::Rgb(26, 44, 74);
pub const COMPOSER_BG: Color = WAGMII_SLATE;

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
            composer_bg: WAGMII_INK,
            selection_bg: SELECTION_BG,
            header_bg: WAGMII_INK,
        },
        "light" => UiTheme {
            name: "light",
            composer_bg: Color::Rgb(26, 38, 58),
            selection_bg: SELECTION_BG,
            header_bg: WAGMII_SLATE,
        },
        "whale" => UiTheme {
            name: "whale",
            composer_bg: WAGMII_SLATE,
            selection_bg: SELECTION_BG,
            header_bg: WAGMII_INK,
        },
        _ => UiTheme {
            name: "default",
            composer_bg: COMPOSER_BG,
            selection_bg: SELECTION_BG,
            header_bg: WAGMII_INK,
        },
    }
}
