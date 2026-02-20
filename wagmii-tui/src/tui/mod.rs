//! Terminal UI (TUI) module for `Wagmii` CLI.

// === Submodules ===

pub mod app;
pub mod approval;
pub mod clipboard;
pub mod command_palette;
pub mod diff_render;
pub mod event_broker;
pub mod history;
pub mod markdown_render;
pub mod onboarding;
pub mod pager;
pub mod paste_burst;
pub mod plan_prompt;
pub mod scrolling;
pub mod selection;
pub mod session_picker;
pub mod streaming;
pub mod transcript;
pub mod ui;
mod ui_text;
pub mod user_input;
pub mod views;
pub mod widgets;

// === Re-exports ===

pub use app::TuiOptions;
pub use ui::run_tui;
