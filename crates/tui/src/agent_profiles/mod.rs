//! Agent Profile system — named agent types with permission/tool inheritance.
//!
//! Agent profiles are a finer-grained layer within the existing
//! Plan / Agent / YOLO top-level modes. Each profile carries:
//! - Model and reasoning-effort overrides
//! - Tool allow/deny lists
//! - Path-level permission defaults (Allow / Deny / Ask)
//! - System-prompt extensions
//! - Auto-approve flag (YOLO-lite for implementer)
//!
//! Profiles are defined in:
//! - **Built-in**: hardcoded defaults for `general`, `explore`, `plan`,
//!   `implementer`, `reviewer`, `builder`.
//! - **User config**: `~/.deepseek/agents.toml` (overrides built-ins).
//! - **Project config**: `<workspace>/.deepseek/agents.toml` (overrides user).
//!
//! NOTE: Most types in this module are not yet wired into the TUI's
//! slash-command or sub-agent dispatch paths. They are kept public so
//! the integration work in a follow-up PR only touches the wiring, not
//! the data model.

pub mod profile;
pub mod manager;
