//! Model-visible memory tools: `memory_search`, `memory_save`, `memory_list`,
//! `memory_forget`.
//!
//! These tools give the model structured access to the persistent memory
//! store. They are registered separately from the MVP `remember` tool and
//! work with the TOML-backed memory system (`crate::memory::store`).

use async_trait::async_trait;
use serde_json::{Value, json};

use crate::memory::store::{self, MemoryConfidence, new_memory};
use super::spec::{
    ApprovalRequirement, ToolCapability, ToolContext, ToolError, ToolResult, ToolSpec, required_str,
    optional_str,
};

// ── memory_search ──────────────────────────────────────────────────────────

pub struct MemorySearchTool;

#[async_trait]
impl ToolSpec for MemorySearchTool {
    fn name(&self) -> &'static str {
        "memory_search"
    }

    fn description(&self) -> &'static str {
        "Search the persistent memory store for facts about the user, \
         project conventions, architectural decisions, and known issues. \
         Memories are stored from previous sessions and manual saves. \
         Use this before asking the user something they've already told you, \
         or to recall project-specific conventions."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "Keywords to search for in memory content and tags."
                }
            },
            "required": ["query"]
        })
    }

    fn capabilities(&self) -> Vec<ToolCapability> {
        vec![ToolCapability::ReadOnly]
    }

    fn approval_requirement(&self) -> ApprovalRequirement {
        ApprovalRequirement::Auto
    }

    async fn execute(&self, input: Value, _context: &ToolContext) -> Result<ToolResult, ToolError> {
        let query = required_str(&input, "query")?;
        let workspace = &_context.workspace;
        let hash = store::project_hash(workspace);

        let memories = store::load_memories(Some(&hash), true)
            .map_err(|e| ToolError::execution_failed(format!("failed to load memories: {e}")))?;

        let results = store::search_memories(&memories, query);

        if results.is_empty() {
            return Ok(ToolResult::success("No matching memories found.".to_string()));
        }

        let lines: Vec<String> = results
            .iter()
            .map(|m| {
                let confidence = m.confidence.to_string();
                let tags = if m.tags.is_empty() {
                    String::new()
                } else {
                    format!(" [{}]", m.tags.join(", "))
                };
                format!("- [{confidence}]{tags} {}  (id: {})", m.content, m.id)
            })
            .collect();

        Ok(ToolResult::success(lines.join("\n")))
    }
}

// ── memory_save ────────────────────────────────────────────────────────────

pub struct MemorySaveTool;

#[async_trait]
impl ToolSpec for MemorySaveTool {
    fn name(&self) -> &'static str {
        "memory_save"
    }

    fn description(&self) -> &'static str {
        "Manually save a durable memory for future sessions. Use this when the \
         user explicitly tells you to remember something, or when you discover \
         a project convention worth persisting. Memories are stored per-project \
         and surface in future session prompts."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "content": {
                    "type": "string",
                    "description": "The fact, preference, or convention to remember. One sentence."
                },
                "tags": {
                    "type": "array",
                    "items": {"type": "string"},
                    "description": "Classification tags (e.g. [\"language\", \"style\"])."
                },
                "confidence": {
                    "type": "string",
                    "enum": ["high", "medium", "low"],
                    "description": "Confidence level. Defaults to 'medium'."
                }
            },
            "required": ["content"]
        })
    }

    fn capabilities(&self) -> Vec<ToolCapability> {
        vec![ToolCapability::WritesFiles]
    }

    fn approval_requirement(&self) -> ApprovalRequirement {
        ApprovalRequirement::Auto
    }

    async fn execute(&self, input: Value, _context: &ToolContext) -> Result<ToolResult, ToolError> {
        let content = required_str(&input, "content")?.to_string();
        let confidence: MemoryConfidence = optional_str(&input, "confidence")
            .unwrap_or("medium")
            .parse()
            .map_err(|e: String| ToolError::missing_field(&e))?;

        let tags: Vec<String> = input
            .get("tags")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();

        let workspace = &_context.workspace;
        let hash = Some(store::project_hash(workspace));

        let memory = new_memory(content.clone(), "manual".to_string(), confidence, tags, hash);

        store::save_memory(memory, Some(workspace), 0)
            .map_err(|e| ToolError::execution_failed(format!("failed to save memory: {e}")))?;

        Ok(ToolResult::success(format!("saved: {content}")))
    }
}

// ── memory_list ────────────────────────────────────────────────────────────

pub struct MemoryListTool;

#[async_trait]
impl ToolSpec for MemoryListTool {
    fn name(&self) -> &'static str {
        "memory_list"
    }

    fn description(&self) -> &'static str {
        "List all memories for the current project (and optionally global \
         memories). Use this to see what the system already knows about \
         this project and user."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "include_global": {
                    "type": "boolean",
                    "description": "Also include global (non-project) memories. Default true."
                }
            },
            "required": []
        })
    }

    fn capabilities(&self) -> Vec<ToolCapability> {
        vec![ToolCapability::ReadOnly]
    }

    fn approval_requirement(&self) -> ApprovalRequirement {
        ApprovalRequirement::Auto
    }

    async fn execute(&self, input: Value, _context: &ToolContext) -> Result<ToolResult, ToolError> {
        let include_global = input
            .get("include_global")
            .and_then(|v| v.as_bool())
            .unwrap_or(true);

        let workspace = &_context.workspace;
        let hash = store::project_hash(workspace);

        let memories = store::load_memories(Some(&hash), include_global)
            .map_err(|e| ToolError::execution_failed(format!("failed to load memories: {e}")))?;

        if memories.is_empty() {
            return Ok(ToolResult::success("No memories stored yet. Use memory_save to add one, or wait for automatic extraction at session end.".to_string()));
        }

        let lines: Vec<String> = memories
            .iter()
            .map(|m| {
                let confidence = m.confidence.to_string();
                let scope = if m.project_hash.is_some() { "project" } else { "global" };
                let tags = if m.tags.is_empty() {
                    String::new()
                } else {
                    format!(" [{}]", m.tags.join(", "))
                };
                format!(
                    "- [{confidence}][{scope}]{tags} {}  (id: {})",
                    m.content, m.id
                )
            })
            .collect();

        let summary = format!(
            "{} memories for this project:\n\n{}",
            memories.len(),
            lines.join("\n")
        );

        Ok(ToolResult::success(summary))
    }
}

// ── memory_forget ──────────────────────────────────────────────────────────

pub struct MemoryForgetTool;

#[async_trait]
impl ToolSpec for MemoryForgetTool {
    fn name(&self) -> &'static str {
        "memory_forget"
    }

    fn description(&self) -> &'static str {
        "Delete a memory by its id. Use this when the user corrects a stored \
         fact, or when a convention is no longer relevant. Get the id from \
         memory_list or memory_search output."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "id": {
                    "type": "string",
                    "description": "The memory id to delete (from memory_list or memory_search)."
                }
            },
            "required": ["id"]
        })
    }

    fn capabilities(&self) -> Vec<ToolCapability> {
        vec![ToolCapability::WritesFiles]
    }

    fn approval_requirement(&self) -> ApprovalRequirement {
        ApprovalRequirement::Auto
    }

    async fn execute(&self, input: Value, _context: &ToolContext) -> Result<ToolResult, ToolError> {
        let id = required_str(&input, "id")?;
        let workspace = &_context.workspace;
        let hash = store::project_hash(workspace);

        let deleted = store::delete_memory(id, Some(&hash), 0)
            .map_err(|e| ToolError::execution_failed(format!("failed to delete memory: {e}")))?;

        if deleted {
            Ok(ToolResult::success(format!("deleted memory {id}")))
        } else {
            Ok(ToolResult::success(format!(
                "no memory found with id {id}"
            )))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn memory_search_requires_query() {
        let tool = MemorySearchTool;
        let schema = tool.input_schema();
        let required = schema["required"].as_array().unwrap();
        assert!(required.iter().any(|v| v.as_str() == Some("query")));
    }

    #[test]
    fn memory_save_requires_content() {
        let tool = MemorySaveTool;
        let schema = tool.input_schema();
        let required = schema["required"].as_array().unwrap();
        assert!(required.iter().any(|v| v.as_str() == Some("content")));
    }

    #[test]
    fn memory_forget_requires_id() {
        let tool = MemoryForgetTool;
        let schema = tool.input_schema();
        let required = schema["required"].as_array().unwrap();
        assert!(required.iter().any(|v| v.as_str() == Some("id")));
    }

    #[test]
    fn memory_list_has_no_required_fields() {
        let tool = MemoryListTool;
        let schema = tool.input_schema();
        let required = schema["required"].as_array().unwrap();
        assert!(required.is_empty());
    }
}
