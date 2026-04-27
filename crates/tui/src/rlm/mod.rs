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
//! Key invariants:
//! - P is stored as a REPL variable, NEVER in the LLM's context window.
//! - Only metadata about state/stdout goes to the LLM — constant-size context.
//! - The LLM generates Python code, not free text.
//! - The REPL exposes `llm_query()` (one-shot child) and `sub_rlm()` (recursive
//!   RLM call); both are serviced by an in-process HTTP sidecar so Python can
//!   call them synchronously via `urllib`.

pub mod prompt;
pub mod sidecar;
pub mod turn;

pub use prompt::rlm_system_prompt;
pub use turn::{RlmTurnResult, run_rlm_turn};
