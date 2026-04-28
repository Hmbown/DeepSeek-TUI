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
#[allow(dead_code)]
pub const COMPOSER_BG: Color = DEEPSEEK_SLATE;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UiTheme {
    pub name: &'static str,
    pub composer_bg: Color,
    pub selection_bg: Color,
    pub header_bg: Color,
}

pub const UI_THEME: UiTheme = UiTheme {
    name: "whale",
    composer_bg: DEEPSEEK_SLATE,
    selection_bg: SELECTION_BG,
    header_bg: DEEPSEEK_INK,
};

// === Color depth + brightness helpers (v0.6.6 UI redesign) ===

/// Terminal color depth, used to gate truecolor surfaces (e.g. reasoning bg
/// tints) on terminals that can't render them faithfully.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorDepth {
    /// 16-color terminals (macOS Terminal.app default, dumb tmux setups).
    /// Background tints distort the named-palette mapping, so we drop them.
    Ansi16,
    /// 256-color terminals — RGB→256 fallback is faithful enough.
    Ansi256,
    /// True-color (24-bit) — render the palette verbatim.
    TrueColor,
}

impl ColorDepth {
    /// Detect the active terminal's color depth. Honors `COLORTERM`
    #[allow(dead_code)]
    /// (truecolor / 24bit) first, then falls back to `TERM`. Defaults to
    /// `TrueColor` because most modern terminals support it; the conservative
    /// fallback is `Ansi16` so background tints disappear safely.
    #[must_use]
    pub fn detect() -> Self {
        if let Ok(ct) = std::env::var("COLORTERM") {
            let ct = ct.to_ascii_lowercase();
            if ct.contains("truecolor") || ct.contains("24bit") {
                return Self::TrueColor;
            }
        }
        let term = std::env::var("TERM").unwrap_or_default();
        let term = term.to_ascii_lowercase();
        if term.contains("256") {
            Self::Ansi256
        } else if term.is_empty() || term == "dumb" {
            Self::Ansi16
        } else {
            // Conservative default for unknown TERM strings — most modern
            // terminals advertise truecolor, but if we're wrong here, dropping
            // bg tints is the safe failure mode.
            Self::TrueColor
        }
    }
}

/// Adapt a foreground color to the terminal's color depth.
#[allow(dead_code)]
///
/// On TrueColor, `color` passes through. On Ansi256 we let ratatui's renderer
/// down-convert (it does this already). On Ansi16 we strip RGB to a near
/// named color so semantic intent survives even on legacy terminals.
#[must_use]
pub fn adapt_color(color: Color, depth: ColorDepth) -> Color {
    match (color, depth) {
        (_, ColorDepth::TrueColor | ColorDepth::Ansi256) => color,
        (Color::Rgb(r, g, b), ColorDepth::Ansi16) => nearest_ansi16(r, g, b),
        _ => color,
    }
}

/// Adapt a background color. On Ansi16 terminals background tints are noisy,
#[allow(dead_code)]
/// so we drop them to `Color::Reset` rather than attempt a coarse named-color
/// match — a quiet background reads cleaner than a wrong one.
#[must_use]
pub fn adapt_bg(color: Color, depth: ColorDepth) -> Color {
    match depth {
        ColorDepth::TrueColor | ColorDepth::Ansi256 => color,
        ColorDepth::Ansi16 => Color::Reset,
    }
}

/// Mix two RGB colors at `alpha` (0.0 = `bg`, 1.0 = `fg`). Anything that's not
#[allow(dead_code)]
/// RGB falls back to `fg` — there's no meaningful alpha blend on a named
/// palette entry.
#[must_use]
pub fn blend(fg: Color, bg: Color, alpha: f32) -> Color {
    let alpha = alpha.clamp(0.0, 1.0);
    match (fg, bg) {
        (Color::Rgb(fr, fg_, fb), Color::Rgb(br, bg_, bb)) => {
            let mix = |a: u8, b: u8| -> u8 {
                let a = f32::from(a);
                let b = f32::from(b);
                (b + (a - b) * alpha).round().clamp(0.0, 255.0) as u8
            };
            Color::Rgb(mix(fr, br), mix(fg_, bg_), mix(fb, bb))
        }
        _ => fg,
    }
}

/// Return the reasoning surface color tinted at 12% over the app background.
#[allow(dead_code)]
/// This is the headline reasoning treatment in v0.6.6; a 12% blend keeps the
/// warm bias subtle without competing with body text. Returns `None` when the
/// terminal can't render the bg faithfully.
#[must_use]
pub fn reasoning_surface_tint(depth: ColorDepth) -> Option<Color> {
    match depth {
        ColorDepth::Ansi16 => None,
        _ => Some(blend(SURFACE_REASONING, DEEPSEEK_INK, 0.12)),
    }
}

/// Pulse `color` between 30% and 100% brightness on a 2s cycle keyed off
/// `now_ms` (epoch ms). The minimum keeps the glyph readable at trough; the
/// maximum is the source color verbatim. Linear interpolation between them
/// reads as a slow heartbeat.
#[must_use]
pub fn pulse_brightness(color: Color, now_ms: u64) -> Color {
    // 2 s = 2000 ms full cycle; sin gives a smooth 0..1..0 swing.
    let phase = (now_ms % 2000) as f32 / 2000.0;
    let t = (phase * std::f32::consts::TAU).sin() * 0.5 + 0.5; // 0..1
    let alpha = 0.30 + t * 0.70; // 30%..100%
    match color {
        Color::Rgb(r, g, b) => {
            let s = |c: u8| -> u8 { ((f32::from(c)) * alpha).round().clamp(0.0, 255.0) as u8 };
            Color::Rgb(s(r), s(g), s(b))
        }
        other => other,
    }
}

/// Map an RGB triple to its closest ANSI-16 named color. Only used by
#[allow(dead_code)]
/// `adapt_color` on Ansi16 terminals; we lean on hue dominance + lightness so
/// brand colors land on the obviously-related named entry (sky → cyan, blue →
/// blue, red → red, etc.) rather than dithering around grey.
fn nearest_ansi16(r: u8, g: u8, b: u8) -> Color {
    let lum = (u16::from(r) + u16::from(g) + u16::from(b)) / 3;
    if lum < 24 {
        return Color::Black;
    }
    if r > 220 && g > 220 && b > 220 {
        return Color::White;
    }
    let bright = lum > 144;
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    if max.saturating_sub(min) < 16 {
        return if bright { Color::Gray } else { Color::DarkGray };
    }
    let dom = if r >= g && r >= b {
        if g > b + 24 {
            if bright { Color::LightYellow } else { Color::Yellow }
        } else if b > r.saturating_sub(24) {
            if bright { Color::LightMagenta } else { Color::Magenta }
        } else if bright {
            Color::LightRed
        } else {
            Color::Red
        }
    } else if g >= r && g >= b {
        if b > r + 24 {
            if bright { Color::LightCyan } else { Color::Cyan }
        } else if bright {
            Color::LightGreen
        } else {
            Color::Green
        }
    } else if r > g + 24 {
        if bright { Color::LightMagenta } else { Color::Magenta }
    } else if g > r + 24 {
        if bright { Color::LightCyan } else { Color::Cyan }
    } else if bright {
        Color::LightBlue
    } else {
        Color::Blue
    };
    dom
}

#[cfg(test)]
mod tests {
    use super::{
        ACCENT_REASONING_LIVE, ColorDepth, DEEPSEEK_INK, DEEPSEEK_RED, DEEPSEEK_SKY,
        SURFACE_REASONING, adapt_bg, adapt_color, blend, nearest_ansi16, pulse_brightness,
        reasoning_surface_tint,
    };
    use ratatui::style::Color;

    #[test]
    fn adapt_color_passes_through_truecolor() {
        let c = Color::Rgb(53, 120, 229);
        assert_eq!(adapt_color(c, ColorDepth::TrueColor), c);
        assert_eq!(adapt_color(c, ColorDepth::Ansi256), c);
    }

    #[test]
    fn adapt_color_drops_to_named_on_ansi16() {
        // Sky: light blue with strong blue dominance and lum>144 → LightCyan.
        assert_eq!(
            adapt_color(DEEPSEEK_SKY, ColorDepth::Ansi16),
            Color::LightCyan
        );
        // Red: red-dominant, mid lum → Red (not the bright variant).
        assert_eq!(adapt_color(DEEPSEEK_RED, ColorDepth::Ansi16), Color::Red);
    }

    #[test]
    fn adapt_bg_disables_tints_on_ansi16() {
        assert_eq!(adapt_bg(SURFACE_REASONING, ColorDepth::Ansi16), Color::Reset);
        assert_eq!(
            adapt_bg(SURFACE_REASONING, ColorDepth::TrueColor),
            SURFACE_REASONING
        );
    }

    #[test]
    fn reasoning_tint_is_none_on_ansi16() {
        assert!(reasoning_surface_tint(ColorDepth::Ansi16).is_none());
        assert!(reasoning_surface_tint(ColorDepth::TrueColor).is_some());
    }

    #[test]
    fn blend_at_zero_returns_bg_at_one_returns_fg() {
        let fg = Color::Rgb(200, 100, 50);
        let bg = Color::Rgb(0, 0, 0);
        assert_eq!(blend(fg, bg, 0.0), bg);
        assert_eq!(blend(fg, bg, 1.0), fg);
    }

    #[test]
    fn blend_at_half_is_midpoint() {
        let mid = blend(Color::Rgb(200, 100, 0), Color::Rgb(0, 0, 0), 0.5);
        assert_eq!(mid, Color::Rgb(100, 50, 0));
    }

    #[test]
    fn pulse_brightness_swings_within_envelope() {
        // The pulse rides between 30%..100% — never below 30% of the source.
        let src = ACCENT_REASONING_LIVE;
        let mut min_r = u8::MAX;
        let mut max_r = 0u8;
        for ms in (0u64..2000).step_by(50) {
            if let Color::Rgb(r, _, _) = pulse_brightness(src, ms) {
                min_r = min_r.min(r);
                max_r = max_r.max(r);
            }
        }
        let Color::Rgb(src_r, _, _) = src else {
            panic!("expected RGB");
        };
        // Trough should land near 30% of source; crest near source itself.
        let lower = (f32::from(src_r) * 0.30).round() as u8;
        assert!(min_r <= lower + 2, "trough too high: {min_r}");
        assert!(max_r + 2 >= src_r, "crest too low: {max_r}");
    }

    #[test]
    fn pulse_passes_named_colors_unchanged() {
        // Named palette entries don't blend meaningfully — leave them alone.
        assert_eq!(pulse_brightness(Color::Reset, 0), Color::Reset);
        assert_eq!(pulse_brightness(Color::Cyan, 1234), Color::Cyan);
    }

    #[test]
    fn nearest_ansi16_routes_known_brand_colors() {
        // Blue at lum 134 with strong b-dominance lands on Cyan (pure named
        // Blue is too dark to read as the brand colour at this lightness).
        assert_eq!(nearest_ansi16(53, 120, 229), Color::Cyan);
        assert_eq!(nearest_ansi16(106, 174, 242), Color::LightCyan);
        assert_eq!(nearest_ansi16(226, 80, 96), Color::Red);
        assert_eq!(nearest_ansi16(11, 21, 38), Color::Black);
    }

    #[test]
    fn color_depth_detect_is_safe_without_env() {
        // Don't try to pin the result — env may be anything in CI. Just
        // exercise the path so a panic would surface.
        let _ = ColorDepth::detect();
        let _ = adapt_color(DEEPSEEK_INK, ColorDepth::detect());
    }
}
