//! Tool system modules and re-exports.

pub mod apply_patch;
pub mod diagnostics;
pub mod file;
pub mod file_search;
pub mod git;
pub mod parallel;
pub mod plan;
pub mod project;
pub mod registry;
pub mod review;
pub mod search;
pub mod shell;
mod shell_output;
pub mod spec;
pub mod subagent;
pub mod swarm;
pub mod test_runner;
pub mod todo;
pub mod user_input;
pub mod web_run;
pub mod web_search;

pub use registry::{ToolRegistry, ToolRegistryBuilder};
pub use review::ReviewOutput;
pub use spec::ToolContext;
pub use user_input::UserInputResponse;
