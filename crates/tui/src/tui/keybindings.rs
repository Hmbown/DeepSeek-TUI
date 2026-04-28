//! Documentation-only catalog of every user-facing keybinding.
//!
//! This module is the *single source of truth* for what shortcuts the help
//! overlay renders. The actual key handlers live in `tui/ui.rs` (and a few
//! sibling modules); they read keys directly off the crossterm event stream
//! and intentionally do **not** consult this catalog. The catalog exists so
//! that:
//!
//! 1. The help overlay (`tui/views/help.rs`) does not have to maintain a
//!    parallel list that silently rots when a handler is added or moved.
//! 2. New contributors have one place to look when answering "which keys are
//!    bound, and where do they go?"
//!
//! When you add or change a binding in `ui.rs`, **add or update the matching
//! entry here**. The compile-only side-effect of forgetting is a stale help
//! screen; there is no runtime crash, so the discipline lives in code review.
//!
//! Entries are grouped by `KeybindingSection`. The `chord` field is a
//! human-readable string formatted exactly the way it should appear in help —
//! we avoid storing `KeyBinding` values directly because many shortcuts are
//! pairs (`↑/↓`) or families (`Alt+1/2/3`) that don't map cleanly to a single
//! chord.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeybindingSection {
    Navigation,
    Editing,
    Submission,
    Modes,
    Sessions,
    Clipboard,
    Help,
}

impl KeybindingSection {
    pub fn label(self) -> &'static str {
        match self {
            Self::Navigation => "Navigation",
            Self::Editing => "Input editing",
            Self::Submission => "Actions",
            Self::Modes => "Modes",
            Self::Sessions => "Sessions",
            Self::Clipboard => "Clipboard",
            Self::Help => "Help",
        }
    }

    /// Stable ordering for help rendering — matches the variant declaration
    /// order; explicit so adding a section forces a deliberate placement.
    pub fn rank(self) -> u8 {
        match self {
            Self::Navigation => 0,
            Self::Editing => 1,
            Self::Submission => 2,
            Self::Modes => 3,
            Self::Sessions => 4,
            Self::Clipboard => 5,
            Self::Help => 6,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct KeybindingEntry {
    pub chord: &'static str,
    pub description: &'static str,
    pub section: KeybindingSection,
}

/// Canonical list of keybindings shown in the help overlay.
///
/// Strings are written in the same notation the existing help screen uses so
/// readers can cross-reference with documentation: `Ctrl+X`, `Alt+X`,
/// `Shift+X`, `↑/↓`, `PgUp/PgDn`, etc. Help renderers may apply per-platform
/// substitutions (e.g. `⌥` for Alt on macOS) at render time, but the catalog
/// itself stores the portable form.
pub const KEYBINDINGS: &[KeybindingEntry] = &[
    // --- Navigation ---
    KeybindingEntry {
        chord: "↑ / ↓",
        description: "Scroll transcript or navigate input history",
        section: KeybindingSection::Navigation,
    },
    KeybindingEntry {
        chord: "Ctrl+↑ / Ctrl+↓",
        description: "Navigate input history",
        section: KeybindingSection::Navigation,
    },
    KeybindingEntry {
        chord: "Alt+↑ / Alt+↓",
        description: "Scroll transcript",
        section: KeybindingSection::Navigation,
    },
    KeybindingEntry {
        chord: "PgUp / PgDn",
        description: "Scroll transcript by page",
        section: KeybindingSection::Navigation,
    },
    KeybindingEntry {
        chord: "Home / End",
        description: "Jump to top / bottom of transcript",
        section: KeybindingSection::Navigation,
    },
    KeybindingEntry {
        chord: "g / G",
        description: "Jump to top / bottom (when input is empty)",
        section: KeybindingSection::Navigation,
    },
    KeybindingEntry {
        chord: "[ / ]",
        description: "Jump between tool output blocks",
        section: KeybindingSection::Navigation,
    },
    // --- Editing ---
    KeybindingEntry {
        chord: "← / →",
        description: "Move cursor in composer",
        section: KeybindingSection::Editing,
    },
    KeybindingEntry {
        chord: "Ctrl+A / Ctrl+E",
        description: "Jump to start / end of line",
        section: KeybindingSection::Editing,
    },
    KeybindingEntry {
        chord: "Backspace / Delete",
        description: "Delete character before / after the cursor",
        section: KeybindingSection::Editing,
    },
    KeybindingEntry {
        chord: "Ctrl+U",
        description: "Clear the current draft",
        section: KeybindingSection::Editing,
    },
    KeybindingEntry {
        chord: "Ctrl+J / Alt+Enter",
        description: "Insert a newline in the composer",
        section: KeybindingSection::Editing,
    },
    // --- Submission / actions ---
    KeybindingEntry {
        chord: "Enter",
        description: "Send the current draft",
        section: KeybindingSection::Submission,
    },
    KeybindingEntry {
        chord: "Esc",
        description: "Close menu, cancel request, discard draft, or clear input",
        section: KeybindingSection::Submission,
    },
    KeybindingEntry {
        chord: "Ctrl+C",
        description: "Cancel request, or exit when nothing is running",
        section: KeybindingSection::Submission,
    },
    KeybindingEntry {
        chord: "Ctrl+D",
        description: "Exit when input is empty",
        section: KeybindingSection::Submission,
    },
    KeybindingEntry {
        chord: "Ctrl+K",
        description: "Open the command palette",
        section: KeybindingSection::Submission,
    },
    KeybindingEntry {
        chord: "Ctrl+P",
        description: "Open the fuzzy file picker (insert @path on Enter)",
        section: KeybindingSection::Submission,
    },
    KeybindingEntry {
        chord: "l",
        description: "Open pager for the last message (when input is empty)",
        section: KeybindingSection::Submission,
    },
    KeybindingEntry {
        chord: "v",
        description: "Open details for the selected tool or message (when input is empty)",
        section: KeybindingSection::Submission,
    },
    KeybindingEntry {
        chord: "Alt+V",
        description: "Open tool-details pager",
        section: KeybindingSection::Submission,
    },
    KeybindingEntry {
        chord: "Ctrl+O",
        description: "Open thinking pager",
        section: KeybindingSection::Submission,
    },
    KeybindingEntry {
        chord: "Ctrl+T",
        description: "Open live transcript overlay (sticky-tail auto-scroll)",
        section: KeybindingSection::Submission,
    },
    KeybindingEntry {
        chord: "Esc Esc",
        description: "Backtrack to a previous user message (Left/Right step, Enter to rewind)",
        section: KeybindingSection::Submission,
    },
    // --- Modes ---
    KeybindingEntry {
        chord: "Tab / Shift+Tab",
        description: "Complete /command or cycle modes (Shift+Tab cycles reasoning effort)",
        section: KeybindingSection::Modes,
    },
    KeybindingEntry {
        chord: "Alt+1 / Alt+2 / Alt+3",
        description: "Jump directly to Plan / Agent / YOLO mode",
        section: KeybindingSection::Modes,
    },
    KeybindingEntry {
        chord: "Alt+P / Alt+A / Alt+Y",
        description: "Alternative jump to Plan / Agent / YOLO mode",
        section: KeybindingSection::Modes,
    },
    KeybindingEntry {
        chord: "Alt+! / Alt+@ / Alt+# / Alt+$ / Alt+)",
        description: "Focus Plan / Todos / Tasks / Agents / Auto sidebar",
        section: KeybindingSection::Modes,
    },
    KeybindingEntry {
        chord: "Ctrl+X",
        description: "Toggle between Plan and Agent modes",
        section: KeybindingSection::Modes,
    },
    // --- Sessions ---
    KeybindingEntry {
        chord: "Ctrl+R",
        description: "Open the session picker",
        section: KeybindingSection::Sessions,
    },
    // --- Clipboard ---
    KeybindingEntry {
        chord: "Ctrl+V",
        description: "Paste text or attach a clipboard image",
        section: KeybindingSection::Clipboard,
    },
    KeybindingEntry {
        chord: "Ctrl+Shift+C",
        description: "Copy the current selection (Cmd+C on macOS)",
        section: KeybindingSection::Clipboard,
    },
    KeybindingEntry {
        chord: "@path",
        description: "Add a local text file or directory to context",
        section: KeybindingSection::Clipboard,
    },
    // --- Help ---
    KeybindingEntry {
        chord: "?",
        description: "Open this help overlay (when input is empty)",
        section: KeybindingSection::Help,
    },
    KeybindingEntry {
        chord: "F1",
        description: "Toggle help overlay",
        section: KeybindingSection::Help,
    },
    KeybindingEntry {
        chord: "Ctrl+/",
        description: "Toggle help overlay",
        section: KeybindingSection::Help,
    },
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_is_non_empty_and_sections_have_entries() {
        assert!(!KEYBINDINGS.is_empty());
        // Every declared section should appear in the catalog at least once,
        // otherwise the help overlay would render an empty heading.
        let sections = [
            KeybindingSection::Navigation,
            KeybindingSection::Editing,
            KeybindingSection::Submission,
            KeybindingSection::Modes,
            KeybindingSection::Sessions,
            KeybindingSection::Clipboard,
            KeybindingSection::Help,
        ];
        for section in sections {
            assert!(
                KEYBINDINGS.iter().any(|entry| entry.section == section),
                "no entries for section {:?}",
                section
            );
        }
    }

    #[test]
    fn help_section_documents_question_mark() {
        // The whole point of #93 is that `?` opens this overlay; if the entry
        // ever disappears the user-facing discoverability promise breaks.
        assert!(
            KEYBINDINGS
                .iter()
                .any(|entry| entry.chord.contains('?') && entry.section == KeybindingSection::Help),
            "`?` must remain documented as the help-toggle chord"
        );
    }

    #[test]
    fn section_rank_is_a_total_order() {
        let sections = [
            KeybindingSection::Navigation,
            KeybindingSection::Editing,
            KeybindingSection::Submission,
            KeybindingSection::Modes,
            KeybindingSection::Sessions,
            KeybindingSection::Clipboard,
            KeybindingSection::Help,
        ];
        let mut ranks: Vec<u8> = sections.iter().map(|s| s.rank()).collect();
        ranks.sort_unstable();
        ranks.dedup();
        assert_eq!(ranks.len(), sections.len(), "ranks must be unique");
    }
}
