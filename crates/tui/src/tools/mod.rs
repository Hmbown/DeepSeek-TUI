//! Tool system modules and re-exports.

pub mod apply_patch;
pub mod approval_cache;
pub mod automation;
pub mod diagnostics;
pub mod file;
pub mod file_search;
pub mod finance;

pub mod fetch_url;
pub mod git;
pub mod git_history;
pub mod github;
pub mod parallel;
pub mod plan;
pub mod project;
pub mod recall_archive;
pub mod registry;
pub mod revert_turn;
pub mod review;
pub mod rlm;
pub mod search;
pub mod shell;
mod shell_output;
pub mod spec;
pub mod subagent;
pub mod tasks;
pub mod test_runner;
pub mod todo;
pub mod truncate;
pub mod user_input;
pub mod validate_data;
pub mod web_run;
pub mod web_search;

pub use registry::{ToolRegistry, ToolRegistryBuilder};
pub use review::ReviewOutput;
pub use spec::ToolContext;
pub use user_input::UserInputResponse;
