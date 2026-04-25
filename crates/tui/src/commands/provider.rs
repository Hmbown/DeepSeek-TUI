//! Provider switching: flip between DeepSeek and NVIDIA NIM at runtime.

use crate::config::{ApiProvider, normalize_model_name};
use crate::tui::app::{App, AppAction};

use super::CommandResult;

/// Switch or view the current LLM backend.
///
/// Accepts `<provider> [model]` so you can flip backend and model in one
/// shot, e.g. `/provider nim flash` lands you on
/// `deepseek-ai/deepseek-v4-flash`. The optional model accepts shorthand
/// (`flash`, `pro`, `v4-flash`, `v4-pro`) or any normal DeepSeek model ID.
pub fn provider(app: &mut App, args: Option<&str>) -> CommandResult {
    let trimmed = args.map(str::trim).filter(|s| !s.is_empty());
    let Some(args) = trimmed else {
        return CommandResult::message(format!(
            "Current provider: {}\n\
             Active model:     {}\n\
             Available:        deepseek, nvidia-nim\n\
             Usage:            /provider <name> [model]\n\
             Examples:         /provider nim flash      → NIM v4-flash (recommended)\n\
                               /provider nim pro        → NIM v4-pro (currently DEGRADED)\n\
                               /provider deepseek       → DeepSeek native, default model\n\
             Tip: NIM needs NVIDIA_API_KEY (or [providers.nvidia_nim].api_key in config.toml).",
            app.api_provider.as_str(),
            app.model
        ));
    };

    let mut parts = args.split_whitespace();
    let name = parts.next().unwrap_or("");
    let model_arg = parts.next();

    let Some(target) = ApiProvider::parse(name) else {
        return CommandResult::error(format!(
            "Unknown provider '{name}'. Expected: deepseek, nvidia-nim."
        ));
    };

    let model = match model_arg {
        None => None,
        Some(raw) => match normalize_model_name(&expand_model_alias(raw)) {
            Some(normalized) => Some(normalized),
            None => {
                return CommandResult::error(format!(
                    "Invalid model '{raw}'. Try: flash, pro, deepseek-v4-flash, deepseek-v4-pro."
                ));
            }
        },
    };

    if target == app.api_provider && model.is_none() {
        return CommandResult::message(format!("Already on provider: {}", target.as_str()));
    }

    CommandResult::action(AppAction::SwitchProvider {
        provider: target,
        model,
    })
}

fn expand_model_alias(name: &str) -> String {
    match name.trim().to_ascii_lowercase().as_str() {
        "pro" | "v4-pro" => "deepseek-v4-pro".to_string(),
        "flash" | "v4-flash" => "deepseek-v4-flash".to_string(),
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::tui::app::TuiOptions;
    use std::path::PathBuf;

    fn create_test_app() -> App {
        let options = TuiOptions {
            model: "deepseek-v4-pro".to_string(),
            workspace: PathBuf::from("."),
            allow_shell: false,
            use_alt_screen: true,
            use_mouse_capture: false,
            max_subagents: 1,
            skills_dir: PathBuf::from("."),
            memory_path: PathBuf::from("memory.md"),
            notes_path: PathBuf::from("notes.txt"),
            mcp_config_path: PathBuf::from("mcp.json"),
            use_memory: false,
            start_in_agent_mode: false,
            skip_onboarding: true,
            yolo: false,
            resume_session_id: None,
        };
        App::new(options, &Config::default())
    }

    #[test]
    fn no_args_shows_current_provider_and_usage() {
        let mut app = create_test_app();
        let result = provider(&mut app, None);
        let msg = result.message.expect("expected info message");
        assert!(msg.contains("Current provider:"));
        assert!(msg.contains("deepseek"));
        assert!(msg.contains("Available:"));
        assert!(msg.contains("nvidia-nim"));
        assert!(msg.contains("/provider nim flash"));
        assert!(result.action.is_none());
    }

    #[test]
    fn unknown_provider_returns_error() {
        let mut app = create_test_app();
        let result = provider(&mut app, Some("openai"));
        let msg = result.message.expect("expected error message");
        assert!(msg.contains("Unknown provider"));
        assert!(result.action.is_none());
    }

    #[test]
    fn switching_to_active_provider_without_model_is_a_noop() {
        let mut app = create_test_app();
        let result = provider(&mut app, Some("deepseek"));
        let msg = result.message.expect("expected message");
        assert!(msg.contains("Already on provider"));
        assert!(result.action.is_none());
    }

    #[test]
    fn switch_to_nim_emits_action_without_model_override() {
        let mut app = create_test_app();
        let result = provider(&mut app, Some("nvidia-nim"));
        assert!(result.message.is_none());
        match result.action {
            Some(AppAction::SwitchProvider { provider, model }) => {
                assert_eq!(provider, ApiProvider::NvidiaNim);
                assert_eq!(model, None);
            }
            other => panic!("expected SwitchProvider action, got {other:?}"),
        }
    }

    #[test]
    fn nim_flash_shorthand_emits_action_with_model_override() {
        let mut app = create_test_app();
        let result = provider(&mut app, Some("nim flash"));
        match result.action {
            Some(AppAction::SwitchProvider { provider, model }) => {
                assert_eq!(provider, ApiProvider::NvidiaNim);
                assert_eq!(model.as_deref(), Some("deepseek-v4-flash"));
            }
            other => panic!("expected SwitchProvider action, got {other:?}"),
        }
    }

    #[test]
    fn nim_pro_shorthand_emits_action_with_model_override() {
        let mut app = create_test_app();
        let result = provider(&mut app, Some("nim pro"));
        match result.action {
            Some(AppAction::SwitchProvider { provider, model }) => {
                assert_eq!(provider, ApiProvider::NvidiaNim);
                assert_eq!(model.as_deref(), Some("deepseek-v4-pro"));
            }
            other => panic!("expected SwitchProvider action, got {other:?}"),
        }
    }

    #[test]
    fn switch_to_active_provider_with_new_model_still_emits_action() {
        let mut app = create_test_app();
        let result = provider(&mut app, Some("deepseek flash"));
        match result.action {
            Some(AppAction::SwitchProvider { provider, model }) => {
                assert_eq!(provider, ApiProvider::Deepseek);
                assert_eq!(model.as_deref(), Some("deepseek-v4-flash"));
            }
            other => panic!("expected SwitchProvider action, got {other:?}"),
        }
    }

    #[test]
    fn invalid_model_returns_error() {
        let mut app = create_test_app();
        let result = provider(&mut app, Some("nim gpt-4"));
        let msg = result.message.expect("expected error message");
        assert!(msg.contains("Invalid model"));
        assert!(result.action.is_none());
    }
}
