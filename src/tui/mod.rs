//! Terminal UI (TUI) module for `DeepSeek` CLI.

// === Submodules ===

pub mod app;
pub mod approval;
pub mod clipboard;
pub mod diff_render;
pub mod event_broker;
pub mod history;
pub mod markdown_render;
pub mod onboarding;
pub mod pager;
pub mod paste_burst;
pub mod scrolling;
pub mod selection;
pub mod session_picker;
pub mod streaming;
pub mod transcript;
pub mod ui;
pub mod user_input;
pub mod views;
pub mod widgets;

// === Re-exports ===

pub use app::TuiOptions;
pub use ui::run_tui;
