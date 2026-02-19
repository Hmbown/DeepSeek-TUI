//! Slash command registry and dispatch system
//!
//! This module provides a modular command system inspired by Codex-rs.
//! Commands are organized by category and dispatched through a central registry.

mod config;
mod core;
mod debug;
mod init;
mod note;
mod queue;
mod review;
mod session;
mod skills;
mod task;

use crate::tui::app::{App, AppAction};

/// Result of executing a command
#[derive(Debug, Clone)]
pub struct CommandResult {
    /// Optional message to display to the user
    pub message: Option<String>,
    /// Optional action for the app to take
    pub action: Option<AppAction>,
}

impl CommandResult {
    /// Create an empty result (command succeeded with no output)
    pub fn ok() -> Self {
        Self {
            message: None,
            action: None,
        }
    }

    /// Create a result with just a message
    pub fn message(msg: impl Into<String>) -> Self {
        Self {
            message: Some(msg.into()),
            action: None,
        }
    }

    /// Create a result with an action
    pub fn action(action: AppAction) -> Self {
        Self {
            message: None,
            action: Some(action),
        }
    }

    /// Create a result with both message and action
    #[allow(dead_code)]
    pub fn with_message_and_action(msg: impl Into<String>, action: AppAction) -> Self {
        Self {
            message: Some(msg.into()),
            action: Some(action),
        }
    }

    /// Create an error message result
    pub fn error(msg: impl Into<String>) -> Self {
        Self {
            message: Some(format!("Error: {}", msg.into())),
            action: None,
        }
    }
}

/// Command metadata for help and autocomplete
#[derive(Debug, Clone, Copy)]
pub struct CommandInfo {
    pub name: &'static str,
    pub aliases: &'static [&'static str],
    pub description: &'static str,
    pub usage: &'static str,
}

impl CommandInfo {
    pub fn requires_argument(&self) -> bool {
        self.usage.contains('<') || self.usage.contains('[')
    }

    pub fn palette_command(&self) -> String {
        if self.requires_argument() {
            format!("/{} ", self.name)
        } else {
            format!("/{}", self.name)
        }
    }

    pub fn palette_description(&self) -> String {
        if self.aliases.is_empty() {
            self.description.to_string()
        } else {
            format!("{}  aliases: {}", self.description, self.aliases.join(", "))
        }
    }
}

/// All registered commands
pub const COMMANDS: &[CommandInfo] = &[
    // Core commands
    CommandInfo {
        name: "help",
        aliases: &["?"],
        description: "Show help information",
        usage: "/help [command]",
    },
    CommandInfo {
        name: "clear",
        aliases: &[],
        description: "Clear conversation history",
        usage: "/clear",
    },
    CommandInfo {
        name: "exit",
        aliases: &["quit", "q"],
        description: "Exit the application",
        usage: "/exit",
    },
    CommandInfo {
        name: "model",
        aliases: &[],
        description: "Switch or view current model",
        usage: "/model [name]",
    },
    CommandInfo {
        name: "models",
        aliases: &[],
        description: "List available models from API",
        usage: "/models",
    },
    CommandInfo {
        name: "queue",
        aliases: &["queued"],
        description: "View or edit queued messages",
        usage: "/queue [list|edit <n>|drop <n>|clear]",
    },
    CommandInfo {
        name: "subagents",
        aliases: &["agents"],
        description: "List sub-agent status",
        usage: "/subagents",
    },
    CommandInfo {
        name: "deepseek",
        aliases: &["dashboard", "api"],
        description: "Show DeepSeek dashboard and docs links",
        usage: "/deepseek",
    },
    CommandInfo {
        name: "home",
        aliases: &["stats", "overview"],
        description: "Show home dashboard with stats and quick actions",
        usage: "/home",
    },
    CommandInfo {
        name: "note",
        aliases: &[],
        description: "Append note to persistent notes file (.deepseek/notes.md)",
        usage: "/note <text>",
    },
    CommandInfo {
        name: "task",
        aliases: &["tasks"],
        description: "Manage background tasks",
        usage: "/task [add <prompt>|list|show <id>|cancel <id>]",
    },
    // Session commands
    CommandInfo {
        name: "save",
        aliases: &[],
        description: "Save session to file",
        usage: "/save [path]",
    },
    CommandInfo {
        name: "sessions",
        aliases: &["resume"],
        description: "Open session picker",
        usage: "/sessions",
    },
    CommandInfo {
        name: "load",
        aliases: &[],
        description: "Load session from file",
        usage: "/load [path]",
    },
    CommandInfo {
        name: "compact",
        aliases: &[],
        description: "Trigger context compaction to free up space",
        usage: "/compact",
    },
    CommandInfo {
        name: "export",
        aliases: &[],
        description: "Export conversation to markdown",
        usage: "/export [path]",
    },
    // Config commands
    CommandInfo {
        name: "config",
        aliases: &[],
        description: "Display current configuration",
        usage: "/config",
    },
    CommandInfo {
        name: "set",
        aliases: &[],
        description: "Modify a setting",
        usage: "/set <key> <value>",
    },
    CommandInfo {
        name: "yolo",
        aliases: &[],
        description: "Enable YOLO mode (shell + trust + auto-approve)",
        usage: "/yolo",
    },
    CommandInfo {
        name: "normal",
        aliases: &[],
        description: "Switch to normal mode (no autonomous tool flow)",
        usage: "/normal",
    },
    CommandInfo {
        name: "agent",
        aliases: &[],
        description: "Switch to agent mode",
        usage: "/agent",
    },
    CommandInfo {
        name: "plan",
        aliases: &[],
        description: "Switch to plan mode and review suggested implementation steps",
        usage: "/plan",
    },
    CommandInfo {
        name: "trust",
        aliases: &[],
        description: "Enable trust mode (access files outside workspace)",
        usage: "/trust",
    },
    CommandInfo {
        name: "logout",
        aliases: &[],
        description: "Clear API key and return to setup",
        usage: "/logout",
    },
    // Debug commands
    CommandInfo {
        name: "tokens",
        aliases: &[],
        description: "Show token usage for session",
        usage: "/tokens",
    },
    CommandInfo {
        name: "system",
        aliases: &[],
        description: "Show current system prompt",
        usage: "/system",
    },
    CommandInfo {
        name: "context",
        aliases: &[],
        description: "Show context window usage",
        usage: "/context",
    },
    CommandInfo {
        name: "undo",
        aliases: &[],
        description: "Remove last message pair",
        usage: "/undo",
    },
    CommandInfo {
        name: "retry",
        aliases: &[],
        description: "Retry the last request",
        usage: "/retry",
    },
    CommandInfo {
        name: "init",
        aliases: &[],
        description: "Generate AGENTS.md for project",
        usage: "/init",
    },
    CommandInfo {
        name: "settings",
        aliases: &[],
        description: "Show persistent settings",
        usage: "/settings",
    },
    // Skills commands
    CommandInfo {
        name: "skills",
        aliases: &[],
        description: "List available skills",
        usage: "/skills",
    },
    CommandInfo {
        name: "skill",
        aliases: &[],
        description: "Activate a skill for next message",
        usage: "/skill <name>",
    },
    CommandInfo {
        name: "review",
        aliases: &[],
        description: "Run a structured code review on a file, diff, or PR",
        usage: "/review <target>",
    },
    // Debug/cost command
    CommandInfo {
        name: "cost",
        aliases: &[],
        description: "Show session cost breakdown",
        usage: "/cost",
    },
];

/// Execute a slash command
pub fn execute(cmd: &str, app: &mut App) -> CommandResult {
    let parts: Vec<&str> = cmd.trim().splitn(2, ' ').collect();
    let command = parts[0].to_lowercase();
    let command = command.strip_prefix('/').unwrap_or(&command);
    let arg = parts.get(1).map(|s| s.trim());

    // Match command or alias
    match command {
        // Core commands
        "help" | "?" => core::help(app, arg),
        "clear" => core::clear(app),
        "exit" | "quit" | "q" => core::exit(),
        "model" => core::model(app, arg),
        "models" => core::models(app),
        "queue" | "queued" => queue::queue(app, arg),
        "subagents" | "agents" => core::subagents(app),
        "deepseek" | "dashboard" | "api" => core::deepseek_links(),
        "home" | "stats" | "overview" => core::home_dashboard(app),
        "note" => note::note(app, arg),
        "task" | "tasks" => task::task(app, arg),

        // Session commands
        "save" => session::save(app, arg),
        "sessions" | "resume" => session::sessions(app),
        "load" => session::load(app, arg),
        "compact" => session::compact(app),
        "export" => session::export(app, arg),

        // Config commands
        "config" => config::show_config(app),
        "settings" => config::show_settings(app),
        "set" => config::set_config(app, arg),
        "yolo" => config::yolo(app),
        "normal" => config::normal_mode(app),
        "agent" => config::agent_mode(app),
        "plan" => config::plan_mode(app),
        "trust" => config::trust(app),
        "logout" => config::logout(app),

        // Debug commands
        "tokens" => debug::tokens(app),
        "cost" => debug::cost(app),
        "system" => debug::system_prompt(app),
        "context" => debug::context(app),
        "undo" => debug::undo(app),
        "retry" => debug::retry(app),

        // Project commands
        "init" => init::init(app),

        // Skills commands
        "skills" => skills::list_skills(app),
        "skill" => skills::run_skill(app, arg),
        "review" => review::review(app, arg),

        _ => CommandResult::error(format!(
            "Unknown command: /{command}. Type /help for available commands."
        )),
    }
}

/// Get command info by name or alias
pub fn get_command_info(name: &str) -> Option<&'static CommandInfo> {
    let name = name.strip_prefix('/').unwrap_or(name);
    COMMANDS
        .iter()
        .find(|cmd| cmd.name == name || cmd.aliases.contains(&name))
}

/// Get all commands matching a prefix (for autocomplete)
#[allow(dead_code)]
pub fn commands_matching(prefix: &str) -> Vec<&'static CommandInfo> {
    let prefix = prefix.strip_prefix('/').unwrap_or(prefix).to_lowercase();
    COMMANDS
        .iter()
        .filter(|cmd| {
            cmd.name.starts_with(&prefix) || cmd.aliases.iter().any(|a| a.starts_with(&prefix))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    // No unit tests currently required for command routing.
}
