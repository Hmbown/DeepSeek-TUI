//! Vision model client for image and visual processing tasks.
//!
//! This module provides a dedicated client for vision-capable models
//! (e.g., GPT-4o, Claude 3, Gemini) to process images and visual content.
//! The vision model runs in subagent mode with independent session management.

pub mod client;
pub mod session;
pub mod tools;

pub use client::{VisionClient, VisionClientConfig, VisionRequest, VisionResponse};
pub use session::{VisionSession, VisionSessionHandle, VisionSessionManager};
pub use tools::{VisionAnalyzeTool, VisionOcrTool};
