//! Core engine module for `DeepSeek` CLI.
//!
//! This module provides the event-driven architecture that separates
//! the UI from the AI interaction logic:
//!
//! - `engine`: The main engine that processes operations
//! - `events`: Events emitted by the engine to the UI
//! - `ops`: Operations submitted by the UI to the engine
//! - `session`: Session state management
//! - `turn`: Turn context and tracking

pub mod capacity;
pub mod capacity_memory;
pub mod coherence;
pub mod engine;
pub mod events;
pub mod ops;
pub mod session;
pub mod tool_parser;
pub mod turn;

// Re-exports
