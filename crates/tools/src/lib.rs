use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use async_trait::async_trait;
use deepseek_protocol::{ToolKind, ToolOutput, ToolPayload};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::RwLock;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolSpec {
    pub name: String,
    pub input_schema: Value,
    pub output_schema: Value,
    pub supports_parallel_tool_calls: bool,
    pub timeout_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfiguredToolSpec {
    pub spec: ToolSpec,
    pub supports_parallel_tool_calls: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolCallSource {
    Direct,
    JsRepl,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub name: String,
    pub payload: ToolPayload,
    pub source: ToolCallSource,
    pub raw_tool_call_id: Option<String>,
}

impl ToolCall {
    pub fn execution_subject(&self, fallback_cwd: &str) -> (String, String, &'static str) {
        match &self.payload {
            ToolPayload::LocalShell { params } => (
                params.command.clone(),
                params
                    .cwd
                    .clone()
                    .unwrap_or_else(|| fallback_cwd.to_string()),
                "shell",
            ),
            _ => (self.name.clone(), fallback_cwd.to_string(), "tool"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ToolInvocation {
    pub call_id: String,
    pub tool_name: String,
    pub payload: ToolPayload,
    pub source: ToolCallSource,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum FunctionCallError {
    ToolNotFound { name: String },
    KindMismatch { expected: ToolKind, got: ToolKind },
    MutatingToolRejected { name: String },
    TimedOut { name: String, timeout_ms: u64 },
    Cancelled { name: String },
    ExecutionFailed { name: String, error: String },
}

#[async_trait]
pub trait ToolHandler: Send + Sync {
    fn kind(&self) -> ToolKind;
    fn matches_kind(&self, kind: ToolKind) -> bool {
        self.kind() == kind
    }
    fn is_mutating(&self) -> bool {
        false
    }
    async fn handle(
        &self,
        invocation: ToolInvocation,
    ) -> std::result::Result<ToolOutput, FunctionCallError>;
}

#[derive(Debug, Default)]
pub struct ToolCallRuntime {
    pub parallel_execution: Arc<RwLock<()>>,
}

#[derive(Default)]
pub struct ToolRegistry {
    handlers: HashMap<String, Arc<dyn ToolHandler>>,
    specs: HashMap<String, ConfiguredToolSpec>,
    runtime: ToolCallRuntime,
}

impl ToolRegistry {
    pub fn register(&mut self, spec: ToolSpec, handler: Arc<dyn ToolHandler>) -> Result<()> {
        let name = spec.name.clone();
        self.specs.insert(
            name.clone(),
            ConfiguredToolSpec {
                supports_parallel_tool_calls: spec.supports_parallel_tool_calls,
                spec,
            },
        );
        self.handlers.insert(name, handler);
        Ok(())
    }

    pub fn list_specs(&self) -> Vec<ConfiguredToolSpec> {
        self.specs.values().cloned().collect()
    }

    pub async fn dispatch(
        &self,
        call: ToolCall,
        allow_mutating: bool,
    ) -> std::result::Result<ToolOutput, FunctionCallError> {
        let handler = self.handlers.get(&call.name).cloned().ok_or_else(|| {
            FunctionCallError::ToolNotFound {
                name: call.name.clone(),
            }
        })?;
        let configured =
            self.specs
                .get(&call.name)
                .cloned()
                .ok_or_else(|| FunctionCallError::ToolNotFound {
                    name: call.name.clone(),
                })?;

        let payload_kind = tool_payload_kind(&call.payload);
        let expected = handler.kind();
        if !handler.matches_kind(payload_kind) {
            return Err(FunctionCallError::KindMismatch {
                expected,
                got: payload_kind,
            });
        }
        if handler.is_mutating() && !allow_mutating {
            return Err(FunctionCallError::MutatingToolRejected { name: call.name });
        }

        let invocation = ToolInvocation {
            call_id: call
                .raw_tool_call_id
                .clone()
                .unwrap_or_else(|| format!("tool-call-{}", uuid::Uuid::new_v4())),
            tool_name: call.name.clone(),
            payload: call.payload,
            source: call.source,
        };

        if configured.supports_parallel_tool_calls {
            let _guard = self.runtime.parallel_execution.read().await;
            self.execute_with_timeout(handler, configured.spec.timeout_ms, invocation)
                .await
        } else {
            let _guard = self.runtime.parallel_execution.write().await;
            self.execute_with_timeout(handler, configured.spec.timeout_ms, invocation)
                .await
        }
    }

    async fn execute_with_timeout(
        &self,
        handler: Arc<dyn ToolHandler>,
        timeout_ms: Option<u64>,
        invocation: ToolInvocation,
    ) -> std::result::Result<ToolOutput, FunctionCallError> {
        if let Some(timeout_ms) = timeout_ms {
            let name = invocation.tool_name.clone();
            match tokio::time::timeout(
                Duration::from_millis(timeout_ms),
                handler.handle(invocation),
            )
            .await
            {
                Ok(result) => result,
                Err(_) => Err(FunctionCallError::TimedOut { name, timeout_ms }),
            }
        } else {
            handler.handle(invocation).await
        }
    }
}

fn tool_payload_kind(payload: &ToolPayload) -> ToolKind {
    match payload {
        ToolPayload::Mcp { .. } => ToolKind::Mcp,
        ToolPayload::Function { .. }
        | ToolPayload::Custom { .. }
        | ToolPayload::LocalShell { .. } => ToolKind::Function,
    }
}
