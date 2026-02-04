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

pub const TEXT_PRIMARY: Color = Color::White;
pub const TEXT_MUTED: Color = Color::DarkGray;
pub const TEXT_DIM: Color = Color::Gray;

pub const STATUS_SUCCESS: Color = DEEPSEEK_SKY;
pub const STATUS_WARNING: Color = DEEPSEEK_SKY;
pub const STATUS_ERROR: Color = DEEPSEEK_RED;
#[allow(dead_code)]
pub const STATUS_INFO: Color = DEEPSEEK_BLUE;

// Mode-specific accent colors for mode badges
pub const MODE_NORMAL: Color = Color::Gray;
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
            selection_bg: Color::Rgb(30, 52, 92),
            header_bg: DEEPSEEK_INK,
        },
        "light" => UiTheme {
            name: "light",
            composer_bg: Color::Rgb(26, 38, 58),
            selection_bg: Color::Rgb(38, 64, 112),
            header_bg: DEEPSEEK_SLATE,
        },
        "whale" => UiTheme {
            name: "whale",
            composer_bg: DEEPSEEK_SLATE,
            selection_bg: DEEPSEEK_NAVY,
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
