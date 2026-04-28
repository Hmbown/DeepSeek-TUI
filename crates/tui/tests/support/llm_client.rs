//! Test-only mirror of the production `llm_client` module surface.
//!
//! The integration test under `tests/integration_mock_llm.rs` includes this
//! file as `mod llm_client` and `mock.rs` as the nested submodule. Doing it
//! this way means `mock.rs`'s `super::{LlmClient, StreamEventBox}` paths
//! resolve cleanly — they refer to the trait + alias declared right here.
//!
//! The trait shape MUST stay 1:1 with the real one in
//! `crates/tui/src/llm_client/mod.rs`. If the production trait grows a method,
//! mirror it here so `mock.rs` (the same source file shipped in the binary)
//! still satisfies it.

use anyhow::Result;
use std::pin::Pin;

use crate::models::{MessageRequest, MessageResponse, StreamEvent};

pub type StreamEventBox =
    Pin<Box<dyn futures_util::Stream<Item = Result<StreamEvent>> + Send + 'static>>;

#[allow(async_fn_in_trait, dead_code)]
pub trait LlmClient: Send + Sync {
    fn provider_name(&self) -> &'static str;
    fn model(&self) -> &str;
    async fn create_message(&self, request: MessageRequest) -> Result<MessageResponse>;
    async fn create_message_stream(&self, request: MessageRequest) -> Result<StreamEventBox>;
    async fn health_check(&self) -> Result<bool> {
        Ok(true)
    }
}

#[path = "../../src/llm_client/mock.rs"]
pub mod mock;
