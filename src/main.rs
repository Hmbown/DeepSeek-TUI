//! CLI entry point for the `DeepSeek` client.

// Allow these clippy lints for now - can be fixed later
#![allow(clippy::collapsible_if)]
#![allow(clippy::get_first)]
#![allow(clippy::field_reassign_with_default)]
#![allow(clippy::redundant_closure)]
#![allow(clippy::ptr_arg)]
#![allow(clippy::useless_conversion)]
#![allow(clippy::iter_cloned_collect)]
#![allow(clippy::manual_range_contains)]
#![allow(clippy::unnecessary_lazy_evaluations)]
// Allow rustdoc warnings for now
#![allow(rustdoc::bare_urls)]
#![allow(rustdoc::invalid_html_tags)]

use std::io::{self, IsTerminal, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use anyhow::{Result, bail};
use clap::{Args, CommandFactory, Parser, Subcommand};
use clap_complete::{Shell, generate};
use dotenvy::dotenv;
use tempfile::NamedTempFile;
use wait_timeout::ChildExt;

mod client;
mod command_safety;
mod commands;
mod compaction;
mod config;
mod core;
mod duo;
mod execpolicy;
mod features;
mod hooks;
mod llm_client;
mod logging;
mod mcp;
mod mcp_server;
mod models;
mod palette;
mod pricing;
mod project_context;
mod project_doc;
mod prompts;
mod responses_api_proxy;
mod rlm;
mod sandbox;
mod session_manager;
mod settings;
mod skills;
mod tools;
mod tui;
mod ui;
mod utils;

use crate::config::Config;
use crate::llm_client::LlmClient;
use crate::mcp::{McpConfig, McpPool};
use crate::models::{ContentBlock, Message, MessageRequest, SystemPrompt};
use crate::session_manager::{SessionManager, create_saved_session};
use crate::tui::history::{summarize_tool_args, summarize_tool_output};

#[derive(Parser, Debug)]
#[command(
    name = "deepseek",
    author,
    version,
    about = "DeepSeek CLI - Chat with DeepSeek",
    long_about = "Unofficial CLI for the DeepSeek API.\n\nJust run 'deepseek' to start chatting.\n\nNot affiliated with DeepSeek Inc."
)]
struct Cli {
    /// Subcommand to run
    #[command(subcommand)]
    command: Option<Commands>,

    #[command(flatten)]
    feature_toggles: FeatureToggles,

    /// Send a one-shot prompt (non-interactive)
    #[arg(short, long)]
    prompt: Option<String>,

    /// YOLO mode: enable agent tools + shell execution
    #[arg(long)]
    yolo: bool,

    /// Maximum number of concurrent sub-agents (1-5)
    #[arg(long)]
    max_subagents: Option<usize>,

    /// Path to config file
    #[arg(long)]
    config: Option<PathBuf>,

    /// Enable verbose logging
    #[arg(short, long)]
    verbose: bool,

    /// Config profile name
    #[arg(long)]
    profile: Option<String>,

    /// Workspace directory for file operations
    #[arg(short, long)]
    workspace: Option<PathBuf>,

    /// Resume a previous session by ID or prefix
    #[arg(short, long)]
    resume: Option<String>,

    /// Continue the most recent session
    #[arg(short = 'c', long = "continue")]
    continue_session: bool,

    /// Disable the alternate screen buffer (inline mode)
    #[arg(long = "no-alt-screen")]
    no_alt_screen: bool,

    /// Skip onboarding screens
    #[arg(long)]
    skip_onboarding: bool,
}

#[derive(Subcommand, Debug, Clone)]
#[allow(clippy::large_enum_variant)]
enum Commands {
    /// Run system diagnostics and check configuration
    Doctor,
    /// Generate shell completions
    Completions {
        /// Shell to generate completions for
        #[arg(value_enum)]
        shell: Shell,
    },
    /// List saved sessions
    Sessions {
        /// Maximum number of sessions to display
        #[arg(short, long, default_value = "20")]
        limit: usize,
        /// Search sessions by title
        #[arg(short, long)]
        search: Option<String>,
    },
    /// Create default AGENTS.md in current directory
    Init,
    /// Save a DeepSeek API key to the config file
    Login {
        /// API key to store (otherwise read from stdin)
        #[arg(long)]
        api_key: Option<String>,
    },
    /// Remove the saved API key
    Logout,
    /// Run a non-interactive prompt
    Exec(ExecArgs),
    /// Run a code review over a git diff
    Review(ReviewArgs),
    /// Apply a patch file (or stdin) to the working tree
    Apply(ApplyArgs),
    /// Manage MCP servers
    Mcp {
        #[command(subcommand)]
        command: McpCommand,
    },
    /// Execpolicy tooling
    Execpolicy(ExecpolicyCommand),
    /// Inspect feature flags
    Features(FeaturesCli),
    /// Run a command inside the sandbox
    Sandbox(SandboxArgs),
    /// Run a local server (e.g. MCP)
    Serve(ServeArgs),
    /// Resume a previous session by ID (use --last for most recent)
    Resume {
        /// Conversation/session id (UUID or prefix)
        #[arg(value_name = "SESSION_ID")]
        session_id: Option<String>,
        /// Continue the most recent session without a picker
        #[arg(long = "last", default_value_t = false, conflicts_with = "session_id")]
        last: bool,
    },
    /// Fork a previous session by ID (use --last for most recent)
    Fork {
        /// Conversation/session id (UUID or prefix)
        #[arg(value_name = "SESSION_ID")]
        session_id: Option<String>,
        /// Fork the most recent session without a picker
        #[arg(long = "last", default_value_t = false, conflicts_with = "session_id")]
        last: bool,
    },
    /// Internal: run the responses API proxy.
    #[command(hide = true)]
    ResponsesApiProxy(responses_api_proxy::Args),
}

#[derive(Args, Debug, Clone)]
struct ExecArgs {
    /// Prompt to send to the model
    prompt: String,
    /// Override model for this run
    #[arg(long)]
    model: Option<String>,
    /// Enable agentic mode with tool access and auto-approvals
    #[arg(long, default_value_t = false)]
    auto: bool,
}

#[derive(Args, Debug, Default, Clone)]
struct FeatureToggles {
    /// Enable a feature (repeatable). Equivalent to `features.<name>=true`.
    #[arg(long = "enable", value_name = "FEATURE", action = clap::ArgAction::Append, global = true)]
    enable: Vec<String>,

    /// Disable a feature (repeatable). Equivalent to `features.<name>=false`.
    #[arg(long = "disable", value_name = "FEATURE", action = clap::ArgAction::Append, global = true)]
    disable: Vec<String>,
}

impl FeatureToggles {
    fn apply(&self, config: &mut Config) -> Result<()> {
        for feature in &self.enable {
            config.set_feature(feature, true)?;
        }
        for feature in &self.disable {
            config.set_feature(feature, false)?;
        }
        Ok(())
    }
}

#[derive(Args, Debug, Clone)]
struct ReviewArgs {
    /// Review staged changes instead of the working tree
    #[arg(long, conflicts_with = "base")]
    staged: bool,
    /// Base ref to diff against (e.g. origin/main)
    #[arg(long)]
    base: Option<String>,
    /// Limit diff to a specific path
    #[arg(long)]
    path: Option<PathBuf>,
    /// Override model for this review
    #[arg(long)]
    model: Option<String>,
    /// Maximum diff characters to include
    #[arg(long, default_value_t = 200_000)]
    max_chars: usize,
}

#[derive(Args, Debug, Clone)]
struct ApplyArgs {
    /// Patch file to apply (defaults to stdin)
    #[arg(value_name = "PATCH_FILE")]
    patch_file: Option<PathBuf>,
}

#[derive(Args, Debug, Clone)]
struct ServeArgs {
    /// Start MCP server over stdio
    #[arg(long)]
    mcp: bool,
}

#[derive(Subcommand, Debug, Clone)]
enum McpCommand {
    /// List configured MCP servers
    List,
    /// Connect to MCP servers and report status
    Connect {
        /// Optional server name to connect to
        #[arg(value_name = "SERVER")]
        server: Option<String>,
    },
    /// List tools discovered from MCP servers
    Tools {
        /// Optional server name to list tools for
        #[arg(value_name = "SERVER")]
        server: Option<String>,
    },
}

#[derive(Args, Debug, Clone)]
struct ExecpolicyCommand {
    #[command(subcommand)]
    command: ExecpolicySubcommand,
}

#[derive(Subcommand, Debug, Clone)]
enum ExecpolicySubcommand {
    /// Check execpolicy files against a command
    Check(execpolicy::ExecPolicyCheckCommand),
}

#[derive(Args, Debug, Clone)]
struct FeaturesCli {
    #[command(subcommand)]
    command: FeaturesSubcommand,
}

#[derive(Subcommand, Debug, Clone)]
enum FeaturesSubcommand {
    /// List known feature flags and their state
    List,
}

#[derive(Args, Debug, Clone)]
struct SandboxArgs {
    #[command(subcommand)]
    command: SandboxCommand,
}

#[derive(Subcommand, Debug, Clone)]
enum SandboxCommand {
    /// Run a command with sandboxing
    Run {
        /// Sandbox policy (danger-full-access, read-only, external-sandbox, workspace-write)
        #[arg(long, default_value = "workspace-write")]
        policy: String,
        /// Allow outbound network access
        #[arg(long)]
        network: bool,
        /// Additional writable roots (repeatable)
        #[arg(long, value_name = "PATH")]
        writable_root: Vec<PathBuf>,
        /// Exclude TMPDIR from writable paths
        #[arg(long)]
        exclude_tmpdir: bool,
        /// Exclude /tmp from writable paths
        #[arg(long)]
        exclude_slash_tmp: bool,
        /// Command working directory
        #[arg(long)]
        cwd: Option<PathBuf>,
        /// Timeout in milliseconds
        #[arg(long, default_value_t = 60_000)]
        timeout_ms: u64,
        /// Command and arguments to run
        #[arg(required = true, trailing_var_arg = true)]
        command: Vec<String>,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    dotenv().ok();
    let cli = Cli::parse();
    logging::set_verbose(cli.verbose);

    // Handle subcommands first
    if let Some(command) = cli.command.clone() {
        return match command {
            Commands::Doctor => {
                run_doctor().await;
                Ok(())
            }
            Commands::Completions { shell } => {
                generate_completions(shell);
                Ok(())
            }
            Commands::Sessions { limit, search } => list_sessions(limit, search),
            Commands::Init => init_project(),
            Commands::Login { api_key } => run_login(api_key),
            Commands::Logout => run_logout(),
            Commands::Exec(args) => {
                let config = load_config_from_cli(&cli)?;
                let model = args
                    .model
                    .or_else(|| config.default_text_model.clone())
                    .unwrap_or_else(|| "deepseek-reasoner".to_string());
                if args.auto || cli.yolo {
                    let workspace = cli.workspace.clone().unwrap_or_else(|| {
                        std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
                    });
                    let max_subagents = cli
                        .max_subagents
                        .map_or_else(|| config.max_subagents(), |value| value.clamp(1, 5));
                    let auto_mode = args.auto || cli.yolo;
                    run_exec_agent(
                        &config,
                        &model,
                        &args.prompt,
                        workspace,
                        max_subagents,
                        true,
                        auto_mode,
                    )
                    .await
                } else {
                    run_one_shot(&config, &model, &args.prompt).await
                }
            }
            Commands::Review(args) => {
                let config = load_config_from_cli(&cli)?;
                run_review(&config, args).await
            }
            Commands::Apply(args) => run_apply(args),
            Commands::Mcp { command } => {
                let config = load_config_from_cli(&cli)?;
                run_mcp_command(&config, command).await
            }
            Commands::Execpolicy(command) => run_execpolicy_command(command),
            Commands::Features(command) => {
                let config = load_config_from_cli(&cli)?;
                run_features_command(&config, command)
            }
            Commands::Sandbox(args) => run_sandbox_command(args),
            Commands::Serve(args) => {
                let workspace = cli.workspace.clone().unwrap_or_else(|| {
                    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
                });
                if args.mcp {
                    mcp_server::run_mcp_server(workspace)
                } else {
                    bail!("No server mode specified. Use --mcp.")
                }
            }
            Commands::Resume { session_id, last } => {
                let config = load_config_from_cli(&cli)?;
                let resume_id = resolve_session_id(session_id, last)?;
                run_interactive(&cli, &config, Some(resume_id)).await
            }
            Commands::Fork { session_id, last } => {
                let config = load_config_from_cli(&cli)?;
                let new_session_id = fork_session(session_id, last)?;
                run_interactive(&cli, &config, Some(new_session_id)).await
            }
            Commands::ResponsesApiProxy(args) => {
                responses_api_proxy::run_main(args)?;
                Ok(())
            }
        };
    }

    // One-shot prompt mode
    let config = load_config_from_cli(&cli)?;
    if let Some(prompt) = cli.prompt {
        let model = config
            .default_text_model
            .clone()
            .unwrap_or_else(|| "deepseek-reasoner".to_string());
        return run_one_shot(&config, &model, &prompt).await;
    }

    // Handle session resume
    let resume_session_id = if cli.continue_session {
        // Get most recent session
        match session_manager::SessionManager::default_location() {
            Ok(manager) => manager.get_latest_session().ok().flatten().map(|m| m.id),
            Err(_) => None,
        }
    } else {
        cli.resume.clone()
    };

    // Default: Interactive TUI
    // --yolo starts in YOLO mode (shell + trust + auto-approve)
    run_interactive(&cli, &config, resume_session_id).await
}

/// Generate shell completions for the given shell
fn generate_completions(shell: Shell) {
    let mut cmd = Cli::command();
    let name = cmd.get_name().to_string();
    generate(shell, &mut cmd, name, &mut io::stdout());
}

/// Run system diagnostics
async fn run_doctor() {
    use crate::palette;
    use colored::Colorize;

    let (blue_r, blue_g, blue_b) = palette::DEEPSEEK_BLUE_RGB;
    let (sky_r, sky_g, sky_b) = palette::DEEPSEEK_SKY_RGB;
    let (aqua_r, aqua_g, aqua_b) = palette::DEEPSEEK_SKY_RGB;
    let (red_r, red_g, red_b) = palette::DEEPSEEK_RED_RGB;

    println!(
        "{}",
        "DeepSeek CLI Doctor"
            .truecolor(blue_r, blue_g, blue_b)
            .bold()
    );
    println!("{}", "==================".truecolor(sky_r, sky_g, sky_b));
    println!();

    // Version info
    println!("{}", "Version Information:".bold());
    println!("  deepseek-cli: {}", env!("CARGO_PKG_VERSION"));
    println!("  rust: {}", rustc_version());
    println!();

    // Check configuration
    println!("{}", "Configuration:".bold());
    let config_dir =
        dirs::home_dir().map_or_else(|| PathBuf::from(".deepseek"), |h| h.join(".deepseek"));

    let config_file = config_dir.join("config.toml");
    if config_file.exists() {
        println!(
            "  {} config.toml found at {}",
            "✓".truecolor(aqua_r, aqua_g, aqua_b),
            config_file.display()
        );
    } else {
        println!(
            "  {} config.toml not found (will use defaults)",
            "!".truecolor(sky_r, sky_g, sky_b)
        );
    }

    // Check API keys
    println!();
    println!("{}", "API Keys:".bold());
    let has_api_key = if std::env::var("DEEPSEEK_API_KEY").is_ok() {
        println!(
            "  {} DEEPSEEK_API_KEY is set",
            "✓".truecolor(aqua_r, aqua_g, aqua_b)
        );
        true
    } else {
        let key_in_config = Config::load(None, None)
            .ok()
            .and_then(|c| c.deepseek_api_key().ok())
            .is_some();
        if key_in_config {
            println!(
                "  {} DeepSeek API key found in config",
                "✓".truecolor(aqua_r, aqua_g, aqua_b)
            );
            true
        } else {
            println!(
                "  {} DeepSeek API key not configured",
                "✗".truecolor(red_r, red_g, red_b)
            );
            println!("    Run 'deepseek' to configure interactively, or set DEEPSEEK_API_KEY");
            false
        }
    };

    // API connectivity test
    println!();
    println!("{}", "API Connectivity:".bold());
    if has_api_key {
        print!("  {} Testing connection to DeepSeek API...", "·".dimmed());
        // Flush to show progress immediately
        use std::io::Write;
        std::io::stdout().flush().ok();

        match test_api_connectivity().await {
            Ok(model) => {
                println!(
                    "\r  {} API connection successful (model: {})",
                    "✓".truecolor(aqua_r, aqua_g, aqua_b),
                    model
                );
            }
            Err(e) => {
                let error_msg = e.to_string();
                println!(
                    "\r  {} API connection failed",
                    "✗".truecolor(red_r, red_g, red_b)
                );
                // Provide helpful diagnostics based on error type
                if error_msg.contains("401") || error_msg.contains("Unauthorized") {
                    println!("    Invalid API key. Check your DEEPSEEK_API_KEY or config.toml");
                } else if error_msg.contains("403") || error_msg.contains("Forbidden") {
                    println!(
                        "    API key lacks permissions. Verify key is active at platform.deepseek.com"
                    );
                } else if error_msg.contains("timeout") || error_msg.contains("Timeout") {
                    println!("    Connection timed out. Check your network connection");
                } else if error_msg.contains("dns") || error_msg.contains("resolve") {
                    println!("    DNS resolution failed. Check your network connection");
                } else if error_msg.contains("connect") {
                    println!("    Connection failed. Check firewall settings or try again");
                } else {
                    println!("    Error: {}", error_msg);
                }
            }
        }
    } else {
        println!("  {} Skipped (no API key configured)", "·".dimmed());
    }

    // Check MCP configuration
    println!();
    println!("{}", "MCP Servers:".bold());
    let mcp_config = config_dir.join("mcp.json");
    if mcp_config.exists() {
        println!("  {} mcp.json found", "✓".truecolor(aqua_r, aqua_g, aqua_b));
        if let Ok(content) = std::fs::read_to_string(&mcp_config)
            && let Ok(config) = serde_json::from_str::<crate::mcp::McpConfig>(&content)
        {
            if config.servers.is_empty() {
                println!("  {} 0 server(s) configured", "·".dimmed());
            } else {
                println!(
                    "  {} {} server(s) configured",
                    "·".dimmed(),
                    config.servers.len()
                );
                for name in config.servers.keys() {
                    println!("    - {name}");
                }
            }
        }
    } else {
        println!("  {} mcp.json not found (no MCP servers)", "·".dimmed());
    }

    // Check skills directory
    println!();
    println!("{}", "Skills:".bold());
    let skills_dir = config_dir.join("skills");
    if skills_dir.exists() {
        let skill_count = std::fs::read_dir(skills_dir)
            .map(|entries| entries.filter_map(std::result::Result::ok).count())
            .unwrap_or(0);
        println!(
            "  {} skills directory found ({} items)",
            "✓".truecolor(aqua_r, aqua_g, aqua_b),
            skill_count
        );
    } else {
        println!("  {} skills directory not found", "·".dimmed());
    }

    // Platform-specific checks
    println!();
    println!("{}", "Platform:".bold());
    println!("  OS: {}", std::env::consts::OS);
    println!("  Arch: {}", std::env::consts::ARCH);

    #[cfg(target_os = "macos")]
    {
        if std::path::Path::new("/usr/bin/sandbox-exec").exists() {
            println!(
                "  {} macOS sandbox available",
                "✓".truecolor(aqua_r, aqua_g, aqua_b)
            );
        } else {
            println!(
                "  {} macOS sandbox not available",
                "!".truecolor(sky_r, sky_g, sky_b)
            );
        }
    }

    println!();
    println!(
        "{}",
        "All checks complete!"
            .truecolor(aqua_r, aqua_g, aqua_b)
            .bold()
    );
}

fn run_execpolicy_command(command: ExecpolicyCommand) -> Result<()> {
    match command.command {
        ExecpolicySubcommand::Check(cmd) => cmd.run().map_err(Into::into),
    }
}

fn run_features_command(config: &Config, command: FeaturesCli) -> Result<()> {
    match command.command {
        FeaturesSubcommand::List => run_features_list(config),
    }
}

fn stage_str(stage: features::Stage) -> &'static str {
    match stage {
        features::Stage::Experimental => "experimental",
        features::Stage::Beta => "beta",
        features::Stage::Stable => "stable",
        features::Stage::Deprecated => "deprecated",
        features::Stage::Removed => "removed",
    }
}

fn run_features_list(config: &Config) -> Result<()> {
    let features = config.features();
    println!("feature\tstage\tenabled");
    for spec in features::FEATURES {
        let enabled = features.enabled(spec.id);
        println!("{}\t{}\t{enabled}", spec.key, stage_str(spec.stage));
    }
    Ok(())
}

/// Test API connectivity by making a minimal request
async fn test_api_connectivity() -> Result<String> {
    use crate::client::DeepSeekClient;
    use crate::models::{ContentBlock, Message, MessageRequest};

    let config = Config::load(None, None)?;
    let client = DeepSeekClient::new(&config)?;
    let model = client.model().to_string();

    // Minimal request: single word prompt, 1 max token
    let request = MessageRequest {
        model: model.clone(),
        messages: vec![Message {
            role: "user".to_string(),
            content: vec![ContentBlock::Text {
                text: "hi".to_string(),
                cache_control: None,
            }],
        }],
        max_tokens: 1,
        system: None,
        tools: None,
        tool_choice: None,
        metadata: None,
        thinking: None,
        stream: Some(false),
        temperature: None,
        top_p: None,
    };

    // Use tokio timeout to catch hanging requests
    let timeout_duration = std::time::Duration::from_secs(15);
    match tokio::time::timeout(timeout_duration, client.create_message(request)).await {
        Ok(Ok(_response)) => Ok(model),
        Ok(Err(e)) => Err(e),
        Err(_) => anyhow::bail!("Request timeout after 15 seconds"),
    }
}

fn rustc_version() -> String {
    // Try to get rustc version, fall back to "unknown"
    std::process::Command::new("rustc")
        .arg("--version")
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map_or_else(|| "unknown".to_string(), |s| s.trim().to_string())
}

/// List saved sessions
fn list_sessions(limit: usize, search: Option<String>) -> Result<()> {
    use crate::palette;
    use colored::Colorize;
    use session_manager::{SessionManager, format_session_line};

    let (blue_r, blue_g, blue_b) = palette::DEEPSEEK_BLUE_RGB;
    let (sky_r, sky_g, sky_b) = palette::DEEPSEEK_SKY_RGB;
    let (aqua_r, aqua_g, aqua_b) = palette::DEEPSEEK_SKY_RGB;

    let manager = SessionManager::default_location()?;

    let sessions = if let Some(query) = search {
        manager.search_sessions(&query)?
    } else {
        manager.list_sessions()?
    };

    if sessions.is_empty() {
        println!("{}", "No sessions found.".truecolor(sky_r, sky_g, sky_b));
        println!(
            "Start a new session with: {}",
            "deepseek".truecolor(blue_r, blue_g, blue_b)
        );
        return Ok(());
    }

    println!(
        "{}",
        "Saved Sessions".truecolor(blue_r, blue_g, blue_b).bold()
    );
    println!("{}", "==============".truecolor(sky_r, sky_g, sky_b));
    println!();

    for (i, session) in sessions.iter().take(limit).enumerate() {
        let line = format_session_line(session);
        if i == 0 {
            println!("  {} {}", "*".truecolor(aqua_r, aqua_g, aqua_b), line);
        } else {
            println!("    {line}");
        }
    }

    let total = sessions.len();
    if total > limit {
        println!();
        println!(
            "  {} more session(s). Use --limit to show more.",
            total - limit
        );
    }

    println!();
    println!(
        "Resume with: {} {}",
        "deepseek --resume".truecolor(blue_r, blue_g, blue_b),
        "<session-id>".dimmed()
    );
    println!(
        "Continue latest: {}",
        "deepseek --continue".truecolor(blue_r, blue_g, blue_b)
    );

    Ok(())
}

/// Initialize a new project with AGENTS.md
fn init_project() -> Result<()> {
    use crate::palette;
    use colored::Colorize;
    use project_context::create_default_agents_md;

    let (sky_r, sky_g, sky_b) = palette::DEEPSEEK_SKY_RGB;
    let (aqua_r, aqua_g, aqua_b) = palette::DEEPSEEK_SKY_RGB;
    let (red_r, red_g, red_b) = palette::DEEPSEEK_RED_RGB;

    let workspace = std::env::current_dir()?;
    let agents_path = workspace.join("AGENTS.md");

    if agents_path.exists() {
        println!(
            "{} AGENTS.md already exists at {}",
            "!".truecolor(sky_r, sky_g, sky_b),
            agents_path.display()
        );
        return Ok(());
    }

    match create_default_agents_md(&workspace) {
        Ok(path) => {
            println!(
                "{} Created {}",
                "✓".truecolor(aqua_r, aqua_g, aqua_b),
                path.display()
            );
            println!();
            println!("Edit this file to customize how the AI agent works with your project.");
            println!("The instructions will be loaded automatically when you run deepseek.");
        }
        Err(e) => {
            println!(
                "{} Failed to create AGENTS.md: {}",
                "✗".truecolor(red_r, red_g, red_b),
                e
            );
        }
    }

    Ok(())
}

fn load_config_from_cli(cli: &Cli) -> Result<Config> {
    let profile = cli
        .profile
        .clone()
        .or_else(|| std::env::var("DEEPSEEK_PROFILE").ok());
    let mut config = Config::load(cli.config.clone(), profile.as_deref())?;
    cli.feature_toggles.apply(&mut config)?;
    Ok(config)
}

fn read_api_key_from_stdin() -> Result<String> {
    let mut stdin = io::stdin();
    if stdin.is_terminal() {
        bail!("No API key provided. Pass --api-key or pipe one via stdin.");
    }
    let mut buffer = String::new();
    stdin.read_to_string(&mut buffer)?;
    let api_key = buffer.trim().to_string();
    if api_key.is_empty() {
        bail!("No API key provided via stdin.");
    }
    Ok(api_key)
}

fn run_login(api_key: Option<String>) -> Result<()> {
    let api_key = match api_key {
        Some(key) => key,
        None => read_api_key_from_stdin()?,
    };
    let path = config::save_api_key(&api_key)?;
    println!("Saved API key to {}", path.display());
    Ok(())
}

fn run_logout() -> Result<()> {
    config::clear_api_key()?;
    println!("Cleared saved API key.");
    Ok(())
}

fn resolve_session_id(session_id: Option<String>, last: bool) -> Result<String> {
    if last {
        return Ok("latest".to_string());
    }
    if let Some(id) = session_id {
        return Ok(id);
    }
    pick_session_id()
}

fn fork_session(session_id: Option<String>, last: bool) -> Result<String> {
    let manager = SessionManager::default_location()?;
    let saved = if last {
        let Some(meta) = manager.get_latest_session()? else {
            bail!("No saved sessions found.");
        };
        manager.load_session(&meta.id)?
    } else {
        let id = resolve_session_id(session_id, false)?;
        manager.load_session_by_prefix(&id)?
    };

    let system_prompt = saved
        .system_prompt
        .as_ref()
        .map(|text| SystemPrompt::Text(text.clone()));
    let forked = create_saved_session(
        &saved.messages,
        &saved.metadata.model,
        &saved.metadata.workspace,
        saved.metadata.total_tokens,
        system_prompt.as_ref(),
    );
    manager.save_session(&forked)?;
    Ok(forked.metadata.id)
}

fn pick_session_id() -> Result<String> {
    let manager = SessionManager::default_location()?;
    let sessions = manager.list_sessions()?;
    if sessions.is_empty() {
        bail!("No saved sessions found.");
    }

    println!("Select a session to resume:");
    for (idx, session) in sessions.iter().enumerate() {
        println!("  {:>2}. {} ({})", idx + 1, session.title, session.id);
    }
    print!("Enter a number (or press Enter to cancel): ");
    io::stdout().flush()?;

    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    let input = input.trim();
    if input.is_empty() {
        bail!("No session selected.");
    }
    let idx: usize = input
        .parse()
        .map_err(|_| anyhow::anyhow!("Invalid input"))?;
    let session = sessions
        .get(idx.saturating_sub(1))
        .ok_or_else(|| anyhow::anyhow!("Selection out of range"))?;
    Ok(session.id.clone())
}

async fn run_review(config: &Config, args: ReviewArgs) -> Result<()> {
    use crate::client::DeepSeekClient;

    let diff = collect_diff(&args)?;
    if diff.trim().is_empty() {
        bail!("No diff to review.");
    }

    let model = args
        .model
        .or_else(|| config.default_text_model.clone())
        .unwrap_or_else(|| "deepseek-reasoner".to_string());

    let system = SystemPrompt::Text(
        "You are a senior code reviewer. Focus on bugs, risks, behavioral regressions, and missing tests. \
Provide findings ordered by severity with file references, then open questions, then a brief summary."
            .to_string(),
    );
    let user_prompt =
        format!("Review the following diff and provide feedback:\n\n{diff}\n\nEnd of diff.");

    let client = DeepSeekClient::new(config)?;
    let request = MessageRequest {
        model,
        messages: vec![Message {
            role: "user".to_string(),
            content: vec![ContentBlock::Text {
                text: user_prompt,
                cache_control: None,
            }],
        }],
        max_tokens: 4096,
        system: Some(system),
        tools: None,
        tool_choice: None,
        metadata: None,
        thinking: None,
        stream: Some(false),
        temperature: Some(0.2),
        top_p: Some(0.9),
    };

    let response = client.create_message(request).await?;
    for block in response.content {
        if let ContentBlock::Text { text, .. } = block {
            println!("{text}");
        }
    }
    Ok(())
}

fn collect_diff(args: &ReviewArgs) -> Result<String> {
    let mut cmd = Command::new("git");
    cmd.arg("diff");
    if args.staged {
        cmd.arg("--cached");
    }
    if let Some(base) = &args.base {
        cmd.arg(format!("{base}...HEAD"));
    }
    if let Some(path) = &args.path {
        cmd.arg("--").arg(path);
    }

    let output = cmd
        .output()
        .map_err(|e| anyhow::anyhow!("Failed to run git diff. Is git installed? ({})", e))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("git diff failed: {}", stderr.trim());
    }
    let mut diff = String::from_utf8_lossy(&output.stdout).to_string();
    if diff.len() > args.max_chars {
        diff = crate::utils::truncate_with_ellipsis(&diff, args.max_chars, "\n...[truncated]\n");
    }
    Ok(diff)
}

fn run_apply(args: ApplyArgs) -> Result<()> {
    let patch = if let Some(path) = args.patch_file {
        std::fs::read_to_string(&path)
            .map_err(|e| anyhow::anyhow!("Failed to read patch {}: {}", path.display(), e))?
    } else {
        read_patch_from_stdin()?
    };
    if patch.trim().is_empty() {
        bail!("Patch is empty.");
    }

    let mut tmp = NamedTempFile::new()?;
    tmp.write_all(patch.as_bytes())?;
    let tmp_path = tmp.path().to_path_buf();

    let output = Command::new("git")
        .arg("apply")
        .arg("--whitespace=nowarn")
        .arg(&tmp_path)
        .output()
        .map_err(|e| anyhow::anyhow!("Failed to run git apply: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("git apply failed: {}", stderr.trim());
    }
    println!("Applied patch successfully.");
    Ok(())
}

fn read_patch_from_stdin() -> Result<String> {
    let mut stdin = io::stdin();
    if stdin.is_terminal() {
        bail!("No patch file provided and stdin is empty.");
    }
    let mut buffer = String::new();
    stdin.read_to_string(&mut buffer)?;
    Ok(buffer)
}

async fn run_mcp_command(config: &Config, command: McpCommand) -> Result<()> {
    let config_path = config.mcp_config_path();
    match command {
        McpCommand::List => {
            let cfg = load_mcp_config(&config_path)?;
            if cfg.servers.is_empty() {
                println!("No MCP servers configured in {}", config_path.display());
                return Ok(());
            }
            println!("MCP servers ({}):", cfg.servers.len());
            for (name, server) in cfg.servers {
                let status = if server.disabled {
                    "disabled"
                } else {
                    "enabled"
                };
                let args = if server.args.is_empty() {
                    "".to_string()
                } else {
                    format!(" {}", server.args.join(" "))
                };
                println!("  - {name} [{status}] {}{}", server.command, args);
            }
            Ok(())
        }
        McpCommand::Connect { server } => {
            let mut pool = McpPool::from_config_path(&config_path)?;
            if let Some(name) = server {
                pool.get_or_connect(&name).await?;
                println!("Connected to MCP server: {name}");
            } else {
                let errors = pool.connect_all().await;
                if errors.is_empty() {
                    println!("Connected to all configured MCP servers.");
                } else {
                    for (name, err) in errors {
                        eprintln!("Failed to connect {name}: {err}");
                    }
                }
            }
            Ok(())
        }
        McpCommand::Tools { server } => {
            let mut pool = McpPool::from_config_path(&config_path)?;
            if let Some(name) = server {
                let conn = pool.get_or_connect(&name).await?;
                if conn.tools().is_empty() {
                    println!("No tools found for MCP server: {name}");
                } else {
                    println!("Tools for {name}:");
                    for tool in conn.tools() {
                        println!(
                            "  - {}{}",
                            tool.name,
                            tool.description
                                .as_ref()
                                .map_or(String::new(), |d| format!(": {d}"))
                        );
                    }
                }
            } else {
                let _ = pool.connect_all().await;
                let tools = pool.all_tools();
                if tools.is_empty() {
                    println!("No MCP tools discovered.");
                } else {
                    println!("MCP tools:");
                    for (name, tool) in tools {
                        println!(
                            "  - {}{}",
                            name,
                            tool.description
                                .as_ref()
                                .map_or(String::new(), |d| format!(": {d}"))
                        );
                    }
                }
            }
            Ok(())
        }
    }
}

fn load_mcp_config(path: &Path) -> Result<McpConfig> {
    if !path.exists() {
        return Ok(McpConfig::default());
    }
    let contents = std::fs::read_to_string(path)
        .map_err(|e| anyhow::anyhow!("Failed to read MCP config {}: {}", path.display(), e))?;
    let cfg: McpConfig = serde_json::from_str(&contents)
        .map_err(|e| anyhow::anyhow!("Failed to parse MCP config: {e}"))?;
    Ok(cfg)
}

fn run_sandbox_command(args: SandboxArgs) -> Result<()> {
    use crate::sandbox::{CommandSpec, SandboxManager};

    let SandboxCommand::Run {
        policy,
        network,
        writable_root,
        exclude_tmpdir,
        exclude_slash_tmp,
        cwd,
        timeout_ms,
        command,
    } = args.command;

    let policy = parse_sandbox_policy(
        &policy,
        network,
        writable_root,
        exclude_tmpdir,
        exclude_slash_tmp,
    )?;
    let cwd = cwd.unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
    let timeout = Duration::from_millis(timeout_ms.clamp(1000, 600_000));

    let (program, args) = command
        .split_first()
        .ok_or_else(|| anyhow::anyhow!("Command is required"))?;
    let spec = CommandSpec::program(
        program,
        args.iter().cloned().collect(),
        cwd.clone(),
        timeout,
    )
    .with_policy(policy);
    let manager = SandboxManager::new();
    let exec_env = manager.prepare(&spec);

    let mut cmd = Command::new(exec_env.program());
    cmd.args(exec_env.args())
        .current_dir(&exec_env.cwd)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for (key, value) in &exec_env.env {
        cmd.env(key, value);
    }

    let mut child = cmd
        .spawn()
        .map_err(|e| anyhow::anyhow!("Failed to run command: {e}"))?;
    let stdout_handle = child
        .stdout
        .take()
        .ok_or_else(|| anyhow::anyhow!("stdout unavailable"))?;
    let stderr_handle = child
        .stderr
        .take()
        .ok_or_else(|| anyhow::anyhow!("stderr unavailable"))?;

    let timeout = exec_env.timeout;
    let stdout_thread = std::thread::spawn(move || {
        let mut reader = stdout_handle;
        let mut buf = Vec::new();
        let _ = reader.read_to_end(&mut buf);
        buf
    });
    let stderr_thread = std::thread::spawn(move || {
        let mut reader = stderr_handle;
        let mut buf = Vec::new();
        let _ = reader.read_to_end(&mut buf);
        buf
    });

    if let Some(status) = child.wait_timeout(timeout)? {
        let stdout = stdout_thread.join().unwrap_or_default();
        let stderr = stderr_thread.join().unwrap_or_default();
        let stderr_str = String::from_utf8_lossy(&stderr);
        let exit_code = status.code().unwrap_or(-1);
        let sandbox_type = exec_env.sandbox_type;
        let sandbox_denied = SandboxManager::was_denied(sandbox_type, exit_code, &stderr_str);

        if !stdout.is_empty() {
            print!("{}", String::from_utf8_lossy(&stdout));
        }
        if !stderr.is_empty() {
            eprint!("{}", stderr_str);
        }
        if sandbox_denied {
            eprintln!(
                "{}",
                SandboxManager::denial_message(sandbox_type, &stderr_str)
            );
        }

        if !status.success() {
            bail!("Command failed with exit code {exit_code}");
        }
    } else {
        let _ = child.kill();
        let _ = child.wait();
        bail!("Command timed out after {}ms", timeout.as_millis());
    }
    Ok(())
}

fn parse_sandbox_policy(
    policy: &str,
    network: bool,
    writable_root: Vec<PathBuf>,
    exclude_tmpdir: bool,
    exclude_slash_tmp: bool,
) -> Result<crate::sandbox::SandboxPolicy> {
    use crate::sandbox::SandboxPolicy;

    match policy {
        "danger-full-access" => Ok(SandboxPolicy::DangerFullAccess),
        "read-only" => Ok(SandboxPolicy::ReadOnly),
        "external-sandbox" => Ok(SandboxPolicy::ExternalSandbox {
            network_access: network,
        }),
        "workspace-write" => Ok(SandboxPolicy::WorkspaceWrite {
            writable_roots: writable_root,
            network_access: network,
            exclude_tmpdir,
            exclude_slash_tmp,
        }),
        other => bail!("Unknown sandbox policy: {other}"),
    }
}

fn should_use_alt_screen(cli: &Cli, config: &Config) -> bool {
    if cli.no_alt_screen {
        return false;
    }

    let mode = config
        .tui
        .as_ref()
        .and_then(|tui| tui.alternate_screen.as_deref())
        .unwrap_or("auto")
        .to_ascii_lowercase();

    match mode.as_str() {
        "always" => true,
        "never" => false,
        _ => !is_zellij(),
    }
}

fn is_zellij() -> bool {
    std::env::var_os("ZELLIJ").is_some()
}

async fn run_interactive(
    cli: &Cli,
    config: &Config,
    resume_session_id: Option<String>,
) -> Result<()> {
    let workspace = cli
        .workspace
        .clone()
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
    let model = config
        .default_text_model
        .clone()
        .unwrap_or_else(|| "deepseek-reasoner".to_string());
    let max_subagents = cli
        .max_subagents
        .map_or_else(|| config.max_subagents(), |value| value.clamp(1, 5));
    let use_alt_screen = should_use_alt_screen(cli, config);

    tui::run_tui(
        config,
        tui::TuiOptions {
            model,
            workspace,
            allow_shell: cli.yolo || config.allow_shell(),
            use_alt_screen,
            skills_dir: config.skills_dir(),
            memory_path: config.memory_path(),
            notes_path: config.notes_path(),
            mcp_config_path: config.mcp_config_path(),
            use_memory: false,
            start_in_agent_mode: cli.yolo,
            skip_onboarding: cli.skip_onboarding,
            yolo: cli.yolo, // YOLO mode auto-approves all tool executions
            resume_session_id,
            max_subagents,
        },
    )
    .await
}

async fn run_one_shot(config: &Config, model: &str, prompt: &str) -> Result<()> {
    use crate::client::DeepSeekClient;
    use crate::models::{ContentBlock, Message, MessageRequest};

    let client = DeepSeekClient::new(config)?;

    let request = MessageRequest {
        model: model.to_string(),
        messages: vec![Message {
            role: "user".to_string(),
            content: vec![ContentBlock::Text {
                text: prompt.to_string(),
                cache_control: None,
            }],
        }],
        max_tokens: 4096,
        system: None,
        tools: None,
        tool_choice: None,
        metadata: None,
        thinking: None,
        stream: Some(false),
        temperature: None,
        top_p: None,
    };

    let response = client.create_message(request).await?;

    for block in response.content {
        if let ContentBlock::Text { text, .. } = block {
            println!("{text}");
        }
    }

    Ok(())
}

async fn run_exec_agent(
    config: &Config,
    model: &str,
    prompt: &str,
    workspace: PathBuf,
    max_subagents: usize,
    auto_approve: bool,
    trust_mode: bool,
) -> Result<()> {
    use std::sync::{Arc, Mutex};

    use crate::compaction::CompactionConfig;
    use crate::core::engine::{EngineConfig, spawn_engine};
    use crate::core::events::Event;
    use crate::core::ops::Op;
    use crate::duo::DuoSession;
    use crate::rlm::RlmSession;
    use crate::tools::plan::PlanState;
    use crate::tools::todo::TodoList;
    use crate::tui::app::AppMode;

    let engine_config = EngineConfig {
        model: model.to_string(),
        workspace: workspace.clone(),
        allow_shell: auto_approve || config.allow_shell(),
        trust_mode,
        notes_path: config.notes_path(),
        mcp_config_path: config.mcp_config_path(),
        max_steps: 100,
        max_subagents,
        features: config.features(),
        rlm_session: Arc::new(Mutex::new(RlmSession::default())),
        duo_session: Arc::new(Mutex::new(DuoSession::new())),
        compaction: CompactionConfig::default(),
        todos: Arc::new(Mutex::new(TodoList::new())),
        plan_state: Arc::new(Mutex::new(PlanState::default())),
    };

    let engine_handle = spawn_engine(engine_config, config);
    let mode = if auto_approve {
        AppMode::Yolo
    } else {
        AppMode::Agent
    };

    engine_handle
        .send(Op::send(
            prompt,
            mode,
            model,
            auto_approve || config.allow_shell(),
            trust_mode,
        ))
        .await?;

    let mut stdout = io::stdout();
    let mut ends_with_newline = false;
    loop {
        let event = {
            let mut rx = engine_handle.rx_event.write().await;
            rx.recv().await
        };

        let Some(event) = event else {
            break;
        };

        match event {
            Event::MessageDelta { content, .. } => {
                print!("{content}");
                stdout.flush()?;
                ends_with_newline = content.ends_with('\n');
            }
            Event::MessageComplete { .. } => {
                if !ends_with_newline {
                    println!();
                }
            }
            Event::ToolCallStarted { name, input, .. } => {
                let summary = summarize_tool_args(&input);
                if let Some(summary) = summary {
                    eprintln!("tool: {name} ({summary})");
                } else {
                    eprintln!("tool: {name}");
                }
            }
            Event::ToolCallProgress { id, output } => {
                eprintln!("tool {id}: {}", summarize_tool_output(&output));
            }
            Event::ToolCallComplete { name, result, .. } => match result {
                Ok(output) => {
                    if name == "exec_shell" && !output.content.trim().is_empty() {
                        eprintln!("tool {name} completed");
                        eprintln!(
                            "--- stdout/stderr ---\n{}\n---------------------",
                            output.content
                        );
                    } else {
                        eprintln!(
                            "tool {name} completed: {}",
                            summarize_tool_output(&output.content)
                        );
                    }
                }
                Err(err) => {
                    eprintln!("tool {name} failed: {err}");
                }
            },
            Event::AgentSpawned { id, prompt } => {
                eprintln!("sub-agent {id} spawned: {}", summarize_tool_output(&prompt));
            }
            Event::AgentProgress { id, status } => {
                eprintln!("sub-agent {id}: {status}");
            }
            Event::AgentComplete { id, result } => {
                eprintln!(
                    "sub-agent {id} completed: {}",
                    summarize_tool_output(&result)
                );
            }
            Event::ApprovalRequired { id, .. } => {
                if auto_approve {
                    let _ = engine_handle.approve_tool_call(id).await;
                } else {
                    let _ = engine_handle.deny_tool_call(id).await;
                }
            }
            Event::ElevationRequired {
                tool_id,
                tool_name,
                denial_reason,
                ..
            } => {
                if auto_approve {
                    eprintln!("sandbox denied {tool_name}: {denial_reason} (auto-elevating)");
                    let policy = crate::sandbox::SandboxPolicy::DangerFullAccess;
                    let _ = engine_handle.retry_tool_with_policy(tool_id, policy).await;
                } else {
                    eprintln!("sandbox denied {tool_name}: {denial_reason}");
                    let _ = engine_handle.deny_tool_call(tool_id).await;
                }
            }
            Event::Error {
                message,
                recoverable: _,
            } => {
                eprintln!("error: {message}");
            }
            Event::TurnComplete { .. } => {
                let _ = engine_handle.send(Op::Shutdown).await;
                break;
            }
            _ => {}
        }
    }

    Ok(())
}
