//! Tool system modules and re-exports.

#![allow(dead_code, unused_imports)]

// === Modules ===

pub mod apply_patch;
pub mod diagnostics;
pub mod duo;
pub mod calculator;
pub mod finance;
pub mod file;
pub mod file_search;
pub mod git;
pub mod sports;
pub mod time;
pub mod plan;
pub mod parallel;
pub mod project;
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
pub mod user_input;
pub mod web_search;
pub mod web_run;
pub mod weather;

// === Re-exports ===

// Re-export commonly used types from spec
pub use spec::ToolContext;

// Re-export registry types
pub use registry::{ToolRegistry, ToolRegistryBuilder};

// Re-export search tools
pub use file_search::FileSearchTool;
pub use search::GrepFilesTool;

// Re-export structured data tools
pub use calculator::CalculatorTool;
pub use finance::FinanceTool;
pub use sports::SportsTool;
pub use time::TimeTool;
pub use weather::WeatherTool;

// Re-export web search tools
pub use web_search::WebSearchTool;
pub use web_run::WebRunTool;

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
pub use shell::{ExecShellTool, ShellInteractTool, ShellWaitTool};

// Re-export subagent types
pub use subagent::SubAgent;

// Re-export test runner tool
pub use test_runner::RunTestsTool;

// Re-export todo types
pub use todo::TodoWriteTool;

// Re-export plan types
pub use plan::UpdatePlanTool;

// Re-export parallel/multi-tool types
pub use parallel::MultiToolUseParallelTool;

// Re-export user input tool/types
pub use user_input::{RequestUserInputTool, UserInputAnswer, UserInputRequest, UserInputResponse};

// Re-export RLM tools
pub use rlm::{RlmExecTool, RlmLoadTool, RlmQueryTool, RlmStatusTool};
