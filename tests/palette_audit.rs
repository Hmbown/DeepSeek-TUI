//! Palette audit tests to prevent color drift.
//!
//! These tests ensure that deprecated colors (like WAGMII_AQUA) are not used
//! directly in user-visible code. The palette should only use Wagmii brand
//! colors: blue, sky, red (plus neutral shades).

use std::fs;
use std::path::Path;

use ratatui::style::Color;

#[path = "../src/palette.rs"]
#[allow(dead_code)]
mod palette;

/// Colors that should not be used directly in TUI code.
/// Use semantic aliases (STATUS_SUCCESS, STATUS_WARNING, etc.) instead.
const DEPRECATED_DIRECT_COLORS: &[&str] = &["WAGMII_AQUA"];

/// Patterns that indicate proper usage (in palette.rs definitions)
const ALLOWED_PATTERNS: &[&str] = &["pub const WAGMII_AQUA", "WAGMII_AQUA_RGB"];

fn color_to_rgb(color: Color) -> (u8, u8, u8) {
    match color {
        Color::Rgb(r, g, b) => (r, g, b),
        Color::Black => (0, 0, 0),
        Color::White => (255, 255, 255),
        Color::Gray => (128, 128, 128),
        Color::DarkGray => (169, 169, 169),
        Color::Red => (255, 0, 0),
        Color::LightRed => (255, 102, 102),
        Color::Green => (0, 255, 0),
        Color::LightGreen => (102, 255, 102),
        Color::Yellow => (255, 255, 0),
        Color::LightYellow => (255, 255, 153),
        Color::Blue => (0, 0, 255),
        Color::LightBlue => (102, 153, 255),
        Color::Magenta => (255, 0, 255),
        Color::LightMagenta => (255, 153, 255),
        Color::Cyan => (0, 255, 255),
        Color::LightCyan => (153, 255, 255),
        _ => panic!("unsupported color variant for contrast test: {:?}", color),
    }
}

fn linearize_srgb(component: u8) -> f64 {
    let srgb = f64::from(component) / 255.0;
    if srgb <= 0.04045 {
        srgb / 12.92
    } else {
        ((srgb + 0.055) / 1.055).powf(2.4)
    }
}

fn relative_luminance(color: Color) -> f64 {
    let (r, g, b) = color_to_rgb(color);
    0.2126 * linearize_srgb(r) + 0.7152 * linearize_srgb(g) + 0.0722 * linearize_srgb(b)
}

fn contrast_ratio(foreground: Color, background: Color) -> f64 {
    let fg = relative_luminance(foreground);
    let bg = relative_luminance(background);
    if fg >= bg {
        (fg + 0.05) / (bg + 0.05)
    } else {
        (bg + 0.05) / (fg + 0.05)
    }
}

fn assert_min_contrast(label: &str, foreground: Color, background: Color, min_ratio: f64) {
    let ratio = contrast_ratio(foreground, background);
    assert!(
        ratio >= min_ratio,
        "{label} contrast {ratio:.2} is below minimum {min_ratio:.2}"
    );
}

/// Audit a single file for deprecated color usage.
fn audit_file(path: &Path, violations: &mut Vec<String>) {
    let content = match fs::read_to_string(path) {
        Ok(c) => c,
        Err(_) => return,
    };

    for (line_num, line) in content.lines().enumerate() {
        for deprecated in DEPRECATED_DIRECT_COLORS {
            // Check for palette::DEPRECATED usage
            let pattern = format!("palette::{}", deprecated);
            if line.contains(&pattern) {
                // Skip if this is an allowed pattern (definition)
                let is_allowed = ALLOWED_PATTERNS.iter().any(|p| line.contains(p));
                if !is_allowed {
                    violations.push(format!(
                        "{}:{}: direct use of {} (use semantic alias instead)",
                        path.display(),
                        line_num + 1,
                        deprecated
                    ));
                }
            }
        }
    }
}

/// Recursively audit a directory for deprecated color usage.
fn audit_directory(dir: &Path, violations: &mut Vec<String>) {
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            audit_directory(&path, violations);
        } else if path.extension().is_some_and(|e| e == "rs") {
            // Skip palette.rs itself (where colors are defined)
            if path.file_name().is_some_and(|n| n == "palette.rs") {
                continue;
            }
            audit_file(&path, violations);
        }
    }
}

#[test]
fn audit_no_direct_aqua_usage() {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let src_dir = Path::new(manifest_dir).join("src");
    let mut violations = Vec::new();

    audit_directory(&src_dir, &mut violations);

    if !violations.is_empty() {
        let report = violations.join("\n");
        panic!(
            "Palette audit failed! Found {} direct uses of deprecated colors:\n{}",
            violations.len(),
            report
        );
    }
}

#[test]
fn verify_status_success_uses_sky() {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let palette_path = Path::new(manifest_dir).join("src/palette.rs");
    let content = fs::read_to_string(&palette_path).expect("Failed to read palette.rs");

    // Verify STATUS_SUCCESS is set to WAGMII_SKY
    assert!(
        content.contains("pub const STATUS_SUCCESS: Color = WAGMII_SKY;"),
        "STATUS_SUCCESS should use WAGMII_SKY, not WAGMII_AQUA"
    );
}

#[test]
fn verify_brand_colors_defined() {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let palette_path = Path::new(manifest_dir).join("src/palette.rs");
    let content = fs::read_to_string(&palette_path).expect("Failed to read palette.rs");

    // Verify primary brand colors are defined (check for the constant names with values)
    assert!(
        content.contains("WAGMII_BLUE_RGB: (u8, u8, u8) = (53, 120, 229);"),
        "WAGMII_BLUE should be #3578E5"
    );
    assert!(
        content.contains("WAGMII_SKY_RGB: (u8, u8, u8) = (106, 174, 242);"),
        "WAGMII_SKY should be #6AAEF2"
    );
    assert!(
        content.contains("WAGMII_RED_RGB: (u8, u8, u8) = (226, 80, 96);"),
        "WAGMII_RED should be #E25060"
    );
}

#[test]
fn contrast_guardrails_for_key_ui_pairs() {
    let min_readable = 4.5;

    assert_min_contrast(
        "TEXT_BODY on WAGMII_INK",
        palette::TEXT_BODY,
        palette::WAGMII_INK,
        min_readable,
    );
    assert_min_contrast(
        "TEXT_SECONDARY on WAGMII_INK",
        palette::TEXT_SECONDARY,
        palette::WAGMII_INK,
        min_readable,
    );
    assert_min_contrast(
        "TEXT_HINT on WAGMII_INK",
        palette::TEXT_HINT,
        palette::WAGMII_INK,
        min_readable,
    );
    assert_min_contrast(
        "STATUS_WARNING on WAGMII_INK",
        palette::STATUS_WARNING,
        palette::WAGMII_INK,
        min_readable,
    );
    assert_min_contrast(
        "STATUS_ERROR on WAGMII_INK",
        palette::STATUS_ERROR,
        palette::WAGMII_INK,
        min_readable,
    );
    assert_min_contrast(
        "FOOTER_HINT on WAGMII_INK",
        palette::FOOTER_HINT,
        palette::WAGMII_INK,
        min_readable,
    );
    assert_min_contrast(
        "FOOTER_HINT on WAGMII_SLATE",
        palette::FOOTER_HINT,
        palette::WAGMII_SLATE,
        min_readable,
    );

    for theme in ["default", "dark", "light", "whale"] {
        let selection_bg = palette::ui_theme(theme).selection_bg;
        assert_min_contrast(
            &format!("SELECTION_TEXT on {theme} selection"),
            palette::SELECTION_TEXT,
            selection_bg,
            min_readable,
        );
    }
}
