//! REPL runtime for paper-spec RLM (Zhang et al., arXiv:2512.24601).
//!
//! Manages a persistent Python subprocess that can execute code blocks,
//! call `llm_query()` for recursive sub-LLM calls, and return results
//! via `FINAL()` / `FINAL_VAR()` patterns.
//!
//! ## Architecture
//!
//! - `PythonRuntime` — owns the Python subprocess lifecycle, sends code
//!   via stdin, collects stdout/stderr with truncation.
//! - `LlmQueryFn` — injected into the Python namespace as `llm_query(prompt)`.
//!   Calls back to Rust which dispatches a one-shot DeepSeek API completion.
//! - `ReplOutput` — parsed result from a REPL execution round, carrying
//!   stdout text, whether a FINAL was detected, and any error signals.

pub mod runtime;
pub mod sandbox;

pub use runtime::PythonRuntime;
pub use sandbox::{ReplOutput, inject_llm_query_fn, parse_final};

use std::sync::Arc;
use tokio::sync::Mutex;

/// Shared handle to a long-lived Python REPL session.
pub type SharedRepl = Arc<Mutex<Option<PythonRuntime>>>;

/// Create a new shared REPL handle (initially uninitialized — lazy start).
pub fn new_shared_repl() -> SharedRepl {
    Arc::new(Mutex::new(None))
}
