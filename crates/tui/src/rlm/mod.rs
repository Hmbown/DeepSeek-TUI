//! Recursive Language Model (RLM) loop — paper-spec Algorithm 1.
//!
//! Implements Zhang, Kraska & Khattab (arXiv:2512.24601, §2 Algorithm 1):
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
//! Invariants:
//! - `P` is held only as a REPL variable (`context` / `ctx`); never
//!   appears in the root LLM's window.
//! - The root LLM receives small metadata messages — length, preview,
//!   helper list, prior-round summary.
//! - Code rounds and sub-LLM calls travel over a single stdin/stdout
//!   pipe to a long-lived `python3 -u` subprocess. No HTTP sidecar.

pub mod bridge;
pub mod prompt;
pub mod turn;

pub use bridge::RlmBridge;
pub use prompt::rlm_system_prompt;
pub use turn::{RlmTermination, RlmTurnResult, run_rlm_turn, run_rlm_turn_with_root};
