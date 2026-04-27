//! True Recursive Language Model (RLM) loop — paper-spec Algorithm 1.
//!
//! Implements the RLM inference paradigm from Zhang, Kraska, Khattab
//! (arXiv:2512.24601, §2 Algorithm 1):
//!
//! ```text
//! state ← InitREPL(prompt=P)
//! state ← AddFunction(state, sub_RLM)
//! hist ← [Metadata(state)]
//! while True:
//!     code ← LLM(hist)
//!     (state, stdout) ← REPL(state, code)
//!     hist ← hist ∥ code ∥ Metadata(stdout)
//!     if state[Final] is set:
//!         return state[Final]
//! ```
//!
//! Key departure from our previous "RLM-inspired" approach:
//! - P is stored as a REPL variable, NEVER in the LLM's context window
//! - Only metadata about state/stdout goes to the LLM — constant-size context
//! - The LLM generates Python code, not free text
//! - Recursion happens via llm_query() inside the code, not as tool calls
//!
//! ## Architecture
//!
//! The RLM loop is a standalone async function that the engine calls from
//! its event loop when it receives an `Op::RlmQuery`. It:
//! 1. Initialises a PythonRuntime with the prompt stored as `PROMPT`
//! 2. Builds a metadata-only context describing REPL state
//! 3. Calls the root LLM to generate code
//! 4. Executes the code in the REPL
//! 5. Checks for FINAL — if found, returns it
//! 6. Otherwise, feeds code + truncated stdout metadata back, loops

pub mod prompt;
pub mod turn;

pub use prompt::rlm_system_prompt;
pub use turn::run_rlm_turn;
