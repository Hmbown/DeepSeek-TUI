//! Palette audit tests to prevent color drift.
//!
//! These tests ensure that deprecated colors (like DEEPSEEK_AQUA) are not used
//! directly in user-visible code. The palette should only use DeepSeek brand
//! colors: blue, sky, red (plus neutral shades).

use std::fs;
use std::path::Path;

/// Colors that should not be used directly in TUI code.
/// Use semantic aliases (STATUS_SUCCESS, STATUS_WARNING, etc.) instead.
const DEPRECATED_DIRECT_COLORS: &[&str] = &["DEEPSEEK_AQUA"];

/// Patterns that indicate proper usage (in palette.rs definitions)
const ALLOWED_PATTERNS: &[&str] = &["pub const DEEPSEEK_AQUA", "DEEPSEEK_AQUA_RGB"];

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

    // Verify STATUS_SUCCESS is set to DEEPSEEK_SKY
    assert!(
        content.contains("pub const STATUS_SUCCESS: Color = DEEPSEEK_SKY;"),
        "STATUS_SUCCESS should use DEEPSEEK_SKY, not DEEPSEEK_AQUA"
    );
}

#[test]
fn verify_brand_colors_defined() {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let palette_path = Path::new(manifest_dir).join("src/palette.rs");
    let content = fs::read_to_string(&palette_path).expect("Failed to read palette.rs");

    // Verify primary brand colors are defined (check for the constant names with values)
    assert!(
        content.contains("DEEPSEEK_BLUE_RGB: (u8, u8, u8) = (53, 120, 229);"),
        "DEEPSEEK_BLUE should be #3578E5"
    );
    assert!(
        content.contains("DEEPSEEK_SKY_RGB: (u8, u8, u8) = (106, 174, 242);"),
        "DEEPSEEK_SKY should be #6AAEF2"
    );
    assert!(
        content.contains("DEEPSEEK_RED_RGB: (u8, u8, u8) = (226, 80, 96);"),
        "DEEPSEEK_RED should be #E25060"
    );
}
