//! Tool system modules and re-exports.

#![allow(dead_code, unused_imports)]

// === Modules ===

pub mod apply_patch;
pub mod diagnostics;
pub mod duo;
pub mod file;
pub mod file_search;
pub mod git;
pub mod plan;
pub mod registry;
pub mod review;
pub mod rlm;
pub mod search;
pub mod shell;
pub mod spec;
pub mod subagent;
pub mod swarm;
pub mod test_runner;
pub mod todo;
pub mod web_search;

// === Re-exports ===

// Re-export commonly used types from spec
pub use spec::ToolContext;

// Re-export registry types
pub use registry::{ToolRegistry, ToolRegistryBuilder};

// Re-export search tools
pub use file_search::FileSearchTool;
pub use search::GrepFilesTool;

// Re-export web search tools
pub use web_search::WebSearchTool;

// Re-export patch tools
pub use apply_patch::ApplyPatchTool;

// Re-export review tools
pub use review::{ReviewOutput, ReviewTool};

// Re-export file tools
pub use file::{EditFileTool, ListDirTool, ReadFileTool, WriteFileTool};

// Re-export diagnostics tool
pub use diagnostics::DiagnosticsTool;

// Re-export git tools
pub use git::{GitDiffTool, GitStatusTool};

// Re-export shell types
pub use shell::ExecShellTool;

// Re-export subagent types
pub use subagent::SubAgent;

// Re-export test runner tool
pub use test_runner::RunTestsTool;

// Re-export todo types
pub use todo::TodoWriteTool;

// Re-export plan types
pub use plan::UpdatePlanTool;

// Re-export RLM tools
pub use rlm::{RlmExecTool, RlmLoadTool, RlmQueryTool, RlmStatusTool};
