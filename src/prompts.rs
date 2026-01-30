//! System prompts for different modes.
//! NOTE: Prompt building is currently handled directly in engine - these are for future refactoring.

#![allow(dead_code)]

use crate::models::SystemPrompt;
use crate::project_context::{ProjectContext, load_project_context_with_parents};
use crate::tui::app::AppMode;
use std::path::Path;

// Prompt files loaded at compile time
pub const BASE_PROMPT: &str = include_str!("prompts/base.txt");
pub const NORMAL_PROMPT: &str = include_str!("prompts/normal.txt");
pub const AGENT_PROMPT: &str = include_str!("prompts/agent.txt");
pub const PLAN_PROMPT: &str = include_str!("prompts/plan.txt");
pub const RLM_PROMPT: &str = include_str!("prompts/rlm.txt");
pub const DUO_PROMPT: &str = include_str!("prompts/duo.txt");

/// Get the system prompt for a specific mode
pub fn system_prompt_for_mode(mode: AppMode) -> SystemPrompt {
    let text = match mode {
        AppMode::Normal => NORMAL_PROMPT,
        AppMode::Agent | AppMode::Yolo => AGENT_PROMPT,
        AppMode::Plan => PLAN_PROMPT,
        AppMode::Rlm => RLM_PROMPT,
        AppMode::Duo => DUO_PROMPT,
    };
    SystemPrompt::Text(text.trim().to_string())
}

/// Get the system prompt for a specific mode with project context
pub fn system_prompt_for_mode_with_context(
    mode: AppMode,
    workspace: &Path,
    working_set_summary: Option<&str>,
    rlm_summary: Option<&str>,
    duo_summary: Option<&str>,
) -> SystemPrompt {
    let base_prompt = match mode {
        AppMode::Normal => NORMAL_PROMPT,
        AppMode::Agent | AppMode::Yolo => AGENT_PROMPT,
        AppMode::Plan => PLAN_PROMPT,
        AppMode::Rlm => RLM_PROMPT,
        AppMode::Duo => DUO_PROMPT,
    };

    // Load project context from workspace
    let project_context = load_project_context_with_parents(workspace);

    // Combine base prompt with project context
    let mut full_prompt = if let Some(project_block) = project_context.as_system_block() {
        format!("{}\n\n{}", base_prompt.trim(), project_block)
    } else {
        // Fallback: Generate an automatic project map summary
        let summary = crate::utils::summarize_project(workspace);
        let tree = crate::utils::project_tree(workspace, 2); // Shallow tree for prompt
        format!(
            "{}\n\n### Project Structure (Automatic Map)\n**Summary:** {}\n\n**Tree:**\n```\n{}\n```",
            base_prompt.trim(),
            summary,
            tree
        )
    };

    if let Some(summary) = working_set_summary
        && !summary.trim().is_empty()
    {
        full_prompt = format!("{full_prompt}\n\n{summary}");
    }

    if mode == AppMode::Rlm {
        let summary = rlm_summary.unwrap_or("No RLM contexts loaded.");
        full_prompt = format!("{full_prompt}\n\nRLM Context Summary:\n{summary}");
    }

    if mode == AppMode::Duo {
        let summary = duo_summary.unwrap_or("No Duo contexts loaded.");
        full_prompt = format!("{full_prompt}\n\nDuo Context Summary:\n{summary}");
    }

    SystemPrompt::Text(full_prompt)
}

/// Build a system prompt with explicit project context
pub fn build_system_prompt(base: &str, project_context: Option<&ProjectContext>) -> SystemPrompt {
    let full_prompt =
        match project_context.and_then(super::project_context::ProjectContext::as_system_block) {
            Some(project_block) => format!("{}\n\n{}", base.trim(), project_block),
            None => base.trim().to_string(),
        };
    SystemPrompt::Text(full_prompt)
}

// Legacy functions for backwards compatibility
pub fn base_system_prompt() -> SystemPrompt {
    SystemPrompt::Text(BASE_PROMPT.trim().to_string())
}

pub fn normal_system_prompt() -> SystemPrompt {
    SystemPrompt::Text(NORMAL_PROMPT.trim().to_string())
}

pub fn agent_system_prompt() -> SystemPrompt {
    SystemPrompt::Text(AGENT_PROMPT.trim().to_string())
}

pub fn plan_system_prompt() -> SystemPrompt {
    SystemPrompt::Text(PLAN_PROMPT.trim().to_string())
}
