//! OSC 8 hyperlink emission and stripping.
//!
//! Modern terminals (iTerm2, Terminal.app 13+, Ghostty, Kitty, WezTerm,
//! Alacritty, recent gnome-terminal/konsole) make a substring clickable when
//! it is wrapped in:
//!
//! ```text
//! \x1b]8;;TARGET\x1b\\LABEL\x1b]8;;\x1b\\
//! ```
//!
//! Terminals that don't understand the sequence simply render the visible
//! `LABEL` and ignore the escape. So emitting OSC 8 is a strict UX upgrade for
//! supporting terminals and a no-op for the rest.
//!
//! The TUI emits these inside `Span::content` strings so the existing
//! ratatui pipeline carries them through. The tradeoff is that the clipboard
//! / selection extraction path must strip the codes before handing text to the
//! user — that's what [`strip_into`] is for.

use std::sync::atomic::{AtomicBool, Ordering};

const OSC8_PREFIX: &str = "\x1b]8;;";
const OSC8_TERMINATOR: &str = "\x1b\\";

/// Process-wide enable flag. `true` by default. Set once at app init from
/// `[ui] osc8_links` (when present) and read by the renderer.
static ENABLED: AtomicBool = AtomicBool::new(true);

/// Set the process-wide OSC 8 enable flag. Intended to be called once at
/// startup; subsequent calls take effect immediately.
pub fn set_enabled(enabled: bool) {
    ENABLED.store(enabled, Ordering::Relaxed);
}

/// Whether OSC 8 hyperlink emission is currently enabled.
#[must_use]
pub fn enabled() -> bool {
    ENABLED.load(Ordering::Relaxed)
}

/// Wrap `label` so it links to `target` in OSC 8-aware terminals. The returned
/// string contains the full `\x1b]8;;TARGET\x1b\LABEL\x1b]8;;\x1b\` payload.
///
/// Does **not** check [`enabled()`]; callers wanting the runtime gate should
/// branch on it before calling this. That keeps the helper test-friendly.
#[must_use]
pub fn wrap_link(target: &str, label: &str) -> String {
    let mut out = String::with_capacity(target.len() + label.len() + 12);
    out.push_str(OSC8_PREFIX);
    out.push_str(target);
    out.push_str(OSC8_TERMINATOR);
    out.push_str(label);
    out.push_str(OSC8_PREFIX);
    out.push_str(OSC8_TERMINATOR);
    out
}

/// Strip OSC 8 escape sequences from `s` into `out`, preserving the visible
/// label text. Other escapes (color, style) pass through untouched. The
/// implementation handles both the standard `ESC \` and the lone `BEL`
/// terminators that some emitters use.
pub fn strip_into(s: &str, out: &mut String) {
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        // Look for the OSC 8 prefix `ESC ] 8 ;`
        if i + 4 <= bytes.len()
            && bytes[i] == 0x1b
            && bytes[i + 1] == b']'
            && bytes[i + 2] == b'8'
            && bytes[i + 3] == b';'
        {
            // Skip until the string terminator (ESC \) or BEL.
            let mut j = i + 4;
            while j < bytes.len() {
                if bytes[j] == 0x07 {
                    j += 1;
                    break;
                }
                if bytes[j] == 0x1b && j + 1 < bytes.len() && bytes[j + 1] == b'\\' {
                    j += 2;
                    break;
                }
                j += 1;
            }
            i = j;
            continue;
        }
        out.push(bytes[i] as char);
        i += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// Serialize tests that read or write the `ENABLED` flag so they don't
    /// race each other under cargo's default parallel test runner.
    static FLAG_GUARD: Mutex<()> = Mutex::new(());

    fn strip(s: &str) -> String {
        let mut out = String::with_capacity(s.len());
        strip_into(s, &mut out);
        out
    }

    #[test]
    fn wrap_link_shape_is_osc_8_compliant() {
        let wrapped = wrap_link("https://example.com", "click me");
        assert_eq!(
            wrapped,
            "\x1b]8;;https://example.com\x1b\\click me\x1b]8;;\x1b\\"
        );
    }

    #[test]
    fn strip_removes_wrapper_keeps_label() {
        let wrapped = wrap_link("https://example.com", "click me");
        assert_eq!(strip(&wrapped), "click me");
    }

    #[test]
    fn strip_handles_bel_terminator() {
        let wrapped = "\x1b]8;;https://example.com\x07click me\x1b]8;;\x07";
        assert_eq!(strip(wrapped), "click me");
    }

    #[test]
    fn strip_passes_through_text_with_no_escapes() {
        let plain = "no escapes here";
        assert_eq!(strip(plain), plain);
    }

    #[test]
    fn strip_preserves_non_osc_8_escapes() {
        // Color escape stays in place; only OSC 8 wrappers are removed.
        let mixed = format!(
            "\x1b[31mred\x1b[0m {wrapped}",
            wrapped = wrap_link("https://example.com", "click")
        );
        assert_eq!(strip(&mixed), "\x1b[31mred\x1b[0m click");
    }

    #[test]
    fn enabled_is_true_by_default_when_untouched() {
        // Hold the flag guard so we observe the initial state, not a value
        // mid-flight from `set_enabled_round_trips`. The flag *defaults* to
        // true at static init and tests in this module are the only writers.
        let _g = FLAG_GUARD.lock().unwrap_or_else(|e| e.into_inner());
        assert!(enabled());
    }

    #[test]
    fn set_enabled_round_trips() {
        let _g = FLAG_GUARD.lock().unwrap_or_else(|e| e.into_inner());
        let prior = enabled();
        set_enabled(false);
        assert!(!enabled());
        set_enabled(true);
        assert!(enabled());
        set_enabled(prior);
    }
}
