mod metrics;

use std::io::{self, Read};
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};
use clap::{Args, CommandFactory, Parser, Subcommand, ValueEnum};
use clap_complete::{Shell, generate};
use deepseek_agent::ModelRegistry;
use deepseek_app_server::{
    AppServerOptions, run as run_app_server, run_stdio as run_app_server_stdio,
};
use deepseek_config::{CliRuntimeOverrides, ConfigStore, ProviderKind, ResolvedRuntimeOptions};
use deepseek_execpolicy::{AskForApproval, ExecPolicyContext, ExecPolicyEngine};
use deepseek_mcp::{McpServerDefinition, run_stdio_server};
use deepseek_secrets::Secrets;
use deepseek_state::{StateStore, ThreadListFilters};

#[derive(Debug, Clone, Copy, ValueEnum)]
enum ProviderArg {
    Deepseek,
    NvidiaNim,
    Openai,
    Openrouter,
    Novita,
}

impl From<ProviderArg> for ProviderKind {
    fn from(value: ProviderArg) -> Self {
        match value {
            ProviderArg::Deepseek => ProviderKind::Deepseek,
            ProviderArg::NvidiaNim => ProviderKind::NvidiaNim,
            ProviderArg::Openai => ProviderKind::Openai,
            ProviderArg::Openrouter => ProviderKind::Openrouter,
            ProviderArg::Novita => ProviderKind::Novita,
        }
    }
}

#[derive(Debug, Parser)]
#[command(
    name = "deepseek",
    version,
    bin_name = "deepseek",
    override_usage = "deepseek [OPTIONS] [PROMPT]\n       deepseek [OPTIONS] <COMMAND> [ARGS]"
)]
struct Cli {
    #[arg(long)]
    config: Option<PathBuf>,
    #[arg(long)]
    profile: Option<String>,
    #[arg(
        long,
        value_enum,
        help = "Advanced provider selector for non-TUI registry/config commands"
    )]
    provider: Option<ProviderArg>,
    #[arg(long)]
    model: Option<String>,
    #[arg(long = "output-mode")]
    output_mode: Option<String>,
    #[arg(long = "log-level")]
    log_level: Option<String>,
    #[arg(long)]
    telemetry: Option<bool>,
    #[arg(long)]
    approval_policy: Option<String>,
    #[arg(long)]
    sandbox_mode: Option<String>,
    #[arg(long)]
    api_key: Option<String>,
    #[arg(long)]
    base_url: Option<String>,
    #[arg(long = "no-alt-screen")]
    no_alt_screen: bool,
    #[arg(long = "mouse-capture", conflicts_with = "no_mouse_capture")]
    mouse_capture: bool,
    #[arg(long = "no-mouse-capture", conflicts_with = "mouse_capture")]
    no_mouse_capture: bool,
    #[arg(long = "skip-onboarding")]
    skip_onboarding: bool,
    #[arg(
        short = 'p',
        long = "prompt",
        value_name = "PROMPT",
        conflicts_with = "prompt"
    )]
    prompt_flag: Option<String>,
    #[arg(value_name = "PROMPT")]
    prompt: Option<String>,
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Debug, Subcommand)]
enum Commands {
    /// Run interactive/non-interactive flows via the TUI binary.
    Run(RunArgs),
    /// Run DeepSeek TUI diagnostics.
    Doctor(TuiPassthroughArgs),
    /// List live DeepSeek API models via the TUI binary.
    Models(TuiPassthroughArgs),
    /// List saved TUI sessions.
    Sessions(TuiPassthroughArgs),
    /// Resume a saved TUI session.
    Resume(TuiPassthroughArgs),
    /// Fork a saved TUI session.
    Fork(TuiPassthroughArgs),
    /// Create a default AGENTS.md in the current directory.
    Init(TuiPassthroughArgs),
    /// Bootstrap MCP config and/or skills directories.
    Setup(TuiPassthroughArgs),
    /// Run the DeepSeek TUI non-interactive agent command.
    Exec(TuiPassthroughArgs),
    /// Run a DeepSeek-powered code review over a git diff.
    Review(TuiPassthroughArgs),
    /// Apply a patch file or stdin to the working tree.
    Apply(TuiPassthroughArgs),
    /// Run the offline TUI evaluation harness.
    Eval(TuiPassthroughArgs),
    /// Manage TUI MCP servers.
    Mcp(TuiPassthroughArgs),
    /// Inspect TUI feature flags.
    Features(TuiPassthroughArgs),
    /// Run a local TUI server.
    Serve(TuiPassthroughArgs),
    /// Generate shell completions for the TUI binary.
    Completions(TuiPassthroughArgs),
    /// Save a DeepSeek API key to the shared config.
    Login(LoginArgs),
    /// Remove saved authentication state.
    Logout,
    /// Manage authentication credentials and provider mode.
    Auth(AuthArgs),
    /// Run MCP server mode over stdio.
    McpServer,
    /// Read/write/list config values.
    Config(ConfigArgs),
    /// Resolve or list available models across providers.
    Model(ModelArgs),
    /// Manage thread/session metadata and resume/fork flows.
    Thread(ThreadArgs),
    /// Evaluate sandbox/approval policy decisions.
    Sandbox(SandboxArgs),
    /// Run the app-server transport.
    AppServer(AppServerArgs),
    /// Generate shell completions.
    Completion {
        #[arg(value_enum)]
        shell: Shell,
    },
    /// Print a usage rollup from the audit log and session store.
    Metrics(MetricsArgs),
}

#[derive(Debug, Args)]
struct MetricsArgs {
    /// Emit machine-readable JSON.
    #[arg(long)]
    json: bool,
    /// Restrict to events newer than this duration (e.g. 7d, 24h, 30m, now-2h).
    #[arg(long, value_name = "DURATION")]
    since: Option<String>,
}

#[derive(Debug, Args)]
struct RunArgs {
    #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
    args: Vec<String>,
}

#[derive(Debug, Args, Clone)]
struct TuiPassthroughArgs {
    #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
    args: Vec<String>,
}

#[derive(Debug, Args)]
struct LoginArgs {
    #[arg(long, value_enum, default_value_t = ProviderArg::Deepseek, hide = true)]
    provider: ProviderArg,
    #[arg(long)]
    api_key: Option<String>,
    #[arg(long, default_value_t = false, hide = true)]
    chatgpt: bool,
    #[arg(long, default_value_t = false, hide = true)]
    device_code: bool,
    #[arg(long, hide = true)]
    token: Option<String>,
}

#[derive(Debug, Args)]
struct AuthArgs {
    #[command(subcommand)]
    command: AuthCommand,
}

#[derive(Debug, Subcommand)]
enum AuthCommand {
    /// Show current provider, env vars, and config-file presence.
    Status,
    /// Save an API key to the OS keyring (never written to disk in
    /// plaintext). Reads from `--api-key`, `--api-key-stdin`, or
    /// prompts on stdin when neither is given. Does not echo the key.
    Set {
        #[arg(long, value_enum)]
        provider: ProviderArg,
        /// Inline value (discouraged — appears in shell history).
        #[arg(long)]
        api_key: Option<String>,
        /// Read the key from stdin instead of prompting.
        #[arg(long = "api-key-stdin", default_value_t = false)]
        api_key_stdin: bool,
    },
    /// Report whether a provider has a key configured. Never prints
    /// the value; just `set` / `not set` plus the source layer.
    Get {
        #[arg(long, value_enum)]
        provider: ProviderArg,
    },
    /// Delete a provider's key from the OS keyring (and from the
    /// plaintext config slot, if present, for parity).
    Clear {
        #[arg(long, value_enum)]
        provider: ProviderArg,
    },
    /// List all known providers with their auth state, without
    /// revealing keys.
    List,
    /// Migrate plaintext `api_key` values from `~/.deepseek/config.toml`
    /// into the OS keyring, then strip them from the file.
    Migrate {
        /// Don't actually write anything; print what would change.
        #[arg(long, default_value_t = false)]
        dry_run: bool,
    },
}

#[derive(Debug, Args)]
struct ConfigArgs {
    #[command(subcommand)]
    command: ConfigCommand,
}

#[derive(Debug, Subcommand)]
enum ConfigCommand {
    Get { key: String },
    Set { key: String, value: String },
    Unset { key: String },
    List,
    Path,
}

#[derive(Debug, Args)]
struct ModelArgs {
    #[command(subcommand)]
    command: ModelCommand,
}

#[derive(Debug, Subcommand)]
enum ModelCommand {
    List {
        #[arg(long, value_enum)]
        provider: Option<ProviderArg>,
    },
    Resolve {
        model: Option<String>,
        #[arg(long, value_enum)]
        provider: Option<ProviderArg>,
    },
}

#[derive(Debug, Args)]
struct ThreadArgs {
    #[command(subcommand)]
    command: ThreadCommand,
}

#[derive(Debug, Subcommand)]
enum ThreadCommand {
    List {
        #[arg(long, default_value_t = false)]
        all: bool,
        #[arg(long)]
        limit: Option<usize>,
    },
    Read {
        thread_id: String,
    },
    Resume {
        thread_id: String,
    },
    Fork {
        thread_id: String,
    },
    Archive {
        thread_id: String,
    },
    Unarchive {
        thread_id: String,
    },
    SetName {
        thread_id: String,
        name: String,
    },
}

#[derive(Debug, Args)]
struct SandboxArgs {
    #[command(subcommand)]
    command: SandboxCommand,
}

#[derive(Debug, Subcommand)]
enum SandboxCommand {
    Check {
        command: String,
        #[arg(long, value_enum, default_value_t = ApprovalModeArg::OnRequest)]
        ask: ApprovalModeArg,
    },
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum ApprovalModeArg {
    UnlessTrusted,
    OnFailure,
    OnRequest,
    Never,
}

impl From<ApprovalModeArg> for AskForApproval {
    fn from(value: ApprovalModeArg) -> Self {
        match value {
            ApprovalModeArg::UnlessTrusted => AskForApproval::UnlessTrusted,
            ApprovalModeArg::OnFailure => AskForApproval::OnFailure,
            ApprovalModeArg::OnRequest => AskForApproval::OnRequest,
            ApprovalModeArg::Never => AskForApproval::Never,
        }
    }
}

#[derive(Debug, Args)]
struct AppServerArgs {
    #[arg(long, default_value = "127.0.0.1")]
    host: String,
    #[arg(long, default_value_t = 8787)]
    port: u16,
    #[arg(long)]
    config: Option<PathBuf>,
    #[arg(long, default_value_t = false)]
    stdio: bool,
}

const MCP_SERVER_DEFINITIONS_KEY: &str = "mcp.server_definitions";

pub fn run_cli() -> std::process::ExitCode {
    match run() {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("error: {err}");
            std::process::ExitCode::FAILURE
        }
    }
}

fn run() -> Result<()> {
    let mut cli = Cli::parse();

    let mut store = ConfigStore::load(cli.config.clone())?;
    let runtime_overrides = CliRuntimeOverrides {
        provider: cli.provider.map(Into::into),
        model: cli.model.clone(),
        api_key: cli.api_key.clone(),
        base_url: cli.base_url.clone(),
        auth_mode: None,
        output_mode: cli.output_mode.clone(),
        log_level: cli.log_level.clone(),
        telemetry: cli.telemetry,
        approval_policy: cli.approval_policy.clone(),
        sandbox_mode: cli.sandbox_mode.clone(),
    };
    let resolved_runtime = store.config.resolve_runtime_options(&runtime_overrides);

    let command = cli.command.take();

    match command {
        Some(Commands::Run(args)) => delegate_to_tui(&cli, &resolved_runtime, args.args),
        Some(Commands::Doctor(args)) => {
            delegate_to_tui(&cli, &resolved_runtime, tui_args("doctor", args))
        }
        Some(Commands::Models(args)) => {
            delegate_to_tui(&cli, &resolved_runtime, tui_args("models", args))
        }
        Some(Commands::Sessions(args)) => {
            delegate_to_tui(&cli, &resolved_runtime, tui_args("sessions", args))
        }
        Some(Commands::Resume(args)) => {
            delegate_to_tui(&cli, &resolved_runtime, tui_args("resume", args))
        }
        Some(Commands::Fork(args)) => {
            delegate_to_tui(&cli, &resolved_runtime, tui_args("fork", args))
        }
        Some(Commands::Init(args)) => {
            delegate_to_tui(&cli, &resolved_runtime, tui_args("init", args))
        }
        Some(Commands::Setup(args)) => {
            delegate_to_tui(&cli, &resolved_runtime, tui_args("setup", args))
        }
        Some(Commands::Exec(args)) => {
            delegate_to_tui(&cli, &resolved_runtime, tui_args("exec", args))
        }
        Some(Commands::Review(args)) => {
            delegate_to_tui(&cli, &resolved_runtime, tui_args("review", args))
        }
        Some(Commands::Apply(args)) => {
            delegate_to_tui(&cli, &resolved_runtime, tui_args("apply", args))
        }
        Some(Commands::Eval(args)) => {
            delegate_to_tui(&cli, &resolved_runtime, tui_args("eval", args))
        }
        Some(Commands::Mcp(args)) => {
            delegate_to_tui(&cli, &resolved_runtime, tui_args("mcp", args))
        }
        Some(Commands::Features(args)) => {
            delegate_to_tui(&cli, &resolved_runtime, tui_args("features", args))
        }
        Some(Commands::Serve(args)) => {
            delegate_to_tui(&cli, &resolved_runtime, tui_args("serve", args))
        }
        Some(Commands::Completions(args)) => {
            delegate_to_tui(&cli, &resolved_runtime, tui_args("completions", args))
        }
        Some(Commands::Login(args)) => run_login_command(&mut store, args),
        Some(Commands::Logout) => run_logout_command(&mut store),
        Some(Commands::Auth(args)) => run_auth_command(&mut store, args.command),
        Some(Commands::McpServer) => run_mcp_server_command(&mut store),
        Some(Commands::Config(args)) => run_config_command(&mut store, args.command),
        Some(Commands::Model(args)) => run_model_command(args.command),
        Some(Commands::Thread(args)) => run_thread_command(args.command),
        Some(Commands::Sandbox(args)) => run_sandbox_command(args.command),
        Some(Commands::AppServer(args)) => run_app_server_command(args),
        Some(Commands::Completion { shell }) => {
            let mut cmd = Cli::command();
            generate(shell, &mut cmd, "deepseek", &mut io::stdout());
            Ok(())
        }
        Some(Commands::Metrics(args)) => run_metrics_command(args),
        None => {
            let mut forwarded = Vec::new();
            if let Some(prompt) = cli.prompt_flag.clone().or_else(|| cli.prompt.clone()) {
                forwarded.push("--prompt".to_string());
                forwarded.push(prompt);
            }
            delegate_to_tui(&cli, &resolved_runtime, forwarded)
        }
    }
}

fn tui_args(command: &str, args: TuiPassthroughArgs) -> Vec<String> {
    let mut forwarded = Vec::with_capacity(args.args.len() + 1);
    forwarded.push(command.to_string());
    forwarded.extend(args.args);
    forwarded
}

fn run_login_command(store: &mut ConfigStore, args: LoginArgs) -> Result<()> {
    let provider: ProviderKind = args.provider.into();
    store.config.provider = provider;

    if args.chatgpt {
        let token = match args.token {
            Some(token) => token,
            None => read_api_key_from_stdin()?,
        };
        store.config.auth_mode = Some("chatgpt".to_string());
        store.config.chatgpt_access_token = Some(token);
        store.config.device_code_session = None;
        store.save()?;
        println!("logged in using chatgpt token mode ({})", provider.as_str());
        return Ok(());
    }

    if args.device_code {
        let token = match args.token {
            Some(token) => token,
            None => read_api_key_from_stdin()?,
        };
        store.config.auth_mode = Some("device_code".to_string());
        store.config.device_code_session = Some(token);
        store.config.chatgpt_access_token = None;
        store.save()?;
        println!(
            "logged in using device code session mode ({})",
            provider.as_str()
        );
        return Ok(());
    }

    let api_key = match args.api_key {
        Some(v) => v,
        None => read_api_key_from_stdin()?,
    };
    store.config.auth_mode = Some("api_key".to_string());
    store.config.providers.for_provider_mut(provider).api_key = Some(api_key);
    if provider == ProviderKind::Deepseek {
        store.config.api_key = store.config.providers.deepseek.api_key.clone();
        if store.config.default_text_model.is_none() {
            store.config.default_text_model = Some(
                store
                    .config
                    .providers
                    .deepseek
                    .model
                    .clone()
                    .unwrap_or_else(|| "deepseek-v4-pro".to_string()),
            );
        }
    }
    store.save()?;
    if provider == ProviderKind::Deepseek {
        println!(
            "logged in using API key mode (deepseek). This also updates the shared deepseek-tui config."
        );
    } else {
        println!("logged in using API key mode ({})", provider.as_str());
    }
    Ok(())
}

fn run_logout_command(store: &mut ConfigStore) -> Result<()> {
    store.config.api_key = None;
    store.config.providers.deepseek.api_key = None;
    store.config.providers.nvidia_nim.api_key = None;
    store.config.providers.openai.api_key = None;
    store.config.auth_mode = None;
    store.config.chatgpt_access_token = None;
    store.config.device_code_session = None;
    store.save()?;
    println!("logged out");
    Ok(())
}

/// Map [`ProviderKind`] to the canonical keyring slot name (`-a` arg
/// in `security find-generic-password`).
fn keyring_slot(provider: ProviderKind) -> &'static str {
    match provider {
        ProviderKind::Deepseek => "deepseek",
        ProviderKind::NvidiaNim => "nvidia-nim",
        ProviderKind::Openai => "openai",
        ProviderKind::Openrouter => "openrouter",
        ProviderKind::Novita => "novita",
    }
}

/// Provider order used by the `auth list` and `auth status` outputs.
const PROVIDER_LIST: [ProviderKind; 5] = [
    ProviderKind::Deepseek,
    ProviderKind::NvidiaNim,
    ProviderKind::Openrouter,
    ProviderKind::Novita,
    ProviderKind::Openai,
];

fn provider_env_set(provider: ProviderKind) -> bool {
    deepseek_secrets::env_for(keyring_slot(provider)).is_some()
}

fn provider_config_set(store: &ConfigStore, provider: ProviderKind) -> bool {
    let slot = store
        .config
        .providers
        .for_provider(provider)
        .api_key
        .as_ref();
    let root = (provider == ProviderKind::Deepseek)
        .then_some(store.config.api_key.as_ref())
        .flatten();
    slot.or(root).is_some_and(|v| !v.trim().is_empty())
}

fn run_auth_command(store: &mut ConfigStore, command: AuthCommand) -> Result<()> {
    run_auth_command_with_secrets(store, command, &Secrets::auto_detect())
}

fn run_auth_command_with_secrets(
    store: &mut ConfigStore,
    command: AuthCommand,
    secrets: &Secrets,
) -> Result<()> {
    match command {
        AuthCommand::Status => {
            println!("provider: {}", store.config.provider.as_str());
            println!("keyring backend: {}", secrets.backend_name());
            for provider in PROVIDER_LIST {
                let slot = keyring_slot(provider);
                let keyring_set = secrets
                    .get(slot)
                    .ok()
                    .flatten()
                    .is_some_and(|v| !v.trim().is_empty());
                let env_set = provider_env_set(provider);
                let file_set = provider_config_set(store, provider);
                println!(
                    "{slot} auth: keyring={}, env={}, config={}",
                    keyring_set, env_set, file_set
                );
            }
            Ok(())
        }
        AuthCommand::Set {
            provider,
            api_key,
            api_key_stdin,
        } => {
            let provider: ProviderKind = provider.into();
            let slot = keyring_slot(provider);
            let api_key = match (api_key, api_key_stdin) {
                (Some(v), _) => v,
                (None, true) => read_api_key_from_stdin()?,
                (None, false) => prompt_api_key(slot)?,
            };
            secrets
                .set(slot, &api_key)
                .with_context(|| format!("failed to write {slot} key to keyring"))?;
            // Don't print the key. Don't echo length.
            println!("saved API key for {slot} to {}", secrets.backend_name());
            Ok(())
        }
        AuthCommand::Get { provider } => {
            let provider: ProviderKind = provider.into();
            let slot = keyring_slot(provider);
            let in_keyring = secrets
                .get(slot)
                .ok()
                .flatten()
                .is_some_and(|v| !v.trim().is_empty());
            let in_env = provider_env_set(provider);
            let in_file = provider_config_set(store, provider);
            // Report the highest-priority source that has it.
            let resolved = secrets.resolve(slot).is_some() || in_file;
            if resolved {
                let source = if in_keyring {
                    "keyring"
                } else if in_env {
                    "env"
                } else {
                    "config-file"
                };
                println!("{slot}: set (source: {source})");
            } else {
                println!("{slot}: not set");
            }
            Ok(())
        }
        AuthCommand::Clear { provider } => {
            let provider: ProviderKind = provider.into();
            let slot = keyring_slot(provider);
            secrets
                .delete(slot)
                .with_context(|| format!("failed to delete {slot} key from keyring"))?;
            // Also clear the plaintext slot in config.toml for parity.
            store.config.providers.for_provider_mut(provider).api_key = None;
            if provider == ProviderKind::Deepseek {
                store.config.api_key = None;
            }
            store.save()?;
            println!("cleared API key for {slot}");
            Ok(())
        }
        AuthCommand::List => {
            println!("keyring backend: {}", secrets.backend_name());
            println!("provider     keyring  env  config");
            for provider in PROVIDER_LIST {
                let slot = keyring_slot(provider);
                let kr = secrets
                    .get(slot)
                    .ok()
                    .flatten()
                    .is_some_and(|v| !v.trim().is_empty());
                let env = provider_env_set(provider);
                let file = provider_config_set(store, provider);
                println!(
                    "{slot:<12}  {}        {}     {}",
                    yes_no(kr),
                    yes_no(env),
                    yes_no(file)
                );
            }
            Ok(())
        }
        AuthCommand::Migrate { dry_run } => run_auth_migrate(store, secrets, dry_run),
    }
}

fn yes_no(b: bool) -> &'static str {
    if b { "yes" } else { "no " }
}

fn prompt_api_key(slot: &str) -> Result<String> {
    use std::io::{IsTerminal, Write};
    eprint!("Enter API key for {slot}: ");
    io::stderr().flush().ok();
    if !io::stdin().is_terminal() {
        // Non-interactive: read directly without prompting twice.
        return read_api_key_from_stdin();
    }
    let mut buf = String::new();
    io::stdin()
        .read_line(&mut buf)
        .context("failed to read API key from stdin")?;
    let key = buf.trim().to_string();
    if key.is_empty() {
        bail!("empty API key provided");
    }
    Ok(key)
}

/// Move plaintext keys from config.toml into the keyring. Stays
/// idempotent: rerunning is a no-op once the file is clean.
fn run_auth_migrate(store: &mut ConfigStore, secrets: &Secrets, dry_run: bool) -> Result<()> {
    let mut migrated: Vec<(ProviderKind, &'static str)> = Vec::new();
    let mut warnings: Vec<String> = Vec::new();

    for provider in PROVIDER_LIST {
        let slot = keyring_slot(provider);
        let from_provider_block = store
            .config
            .providers
            .for_provider(provider)
            .api_key
            .clone()
            .filter(|v| !v.trim().is_empty());
        let from_root = (provider == ProviderKind::Deepseek)
            .then(|| store.config.api_key.clone())
            .flatten()
            .filter(|v| !v.trim().is_empty());
        let value = from_provider_block.or(from_root);
        let Some(value) = value else { continue };

        if let Ok(Some(existing)) = secrets.get(slot)
            && existing == value
        {
            // Already migrated; safe to strip the file slot.
        } else if dry_run {
            migrated.push((provider, slot));
            continue;
        } else if let Err(err) = secrets.set(slot, &value) {
            warnings.push(format!("skipped {slot}: failed to write to keyring: {err}"));
            continue;
        }
        if !dry_run {
            store.config.providers.for_provider_mut(provider).api_key = None;
            if provider == ProviderKind::Deepseek {
                store.config.api_key = None;
            }
        }
        migrated.push((provider, slot));
    }

    if !dry_run && !migrated.is_empty() {
        store
            .save()
            .context("failed to write updated config.toml")?;
    }

    println!("keyring backend: {}", secrets.backend_name());
    if migrated.is_empty() {
        println!("nothing to migrate (config.toml has no plaintext api_key entries)");
    } else {
        println!(
            "{} {} provider key(s):",
            if dry_run { "would migrate" } else { "migrated" },
            migrated.len()
        );
        for (_, slot) in &migrated {
            println!("  - {slot}");
        }
        if !dry_run {
            println!(
                "config.toml at {} no longer contains api_key entries for migrated providers.",
                store.path().display()
            );
        }
    }
    for w in warnings {
        eprintln!("warning: {w}");
    }
    Ok(())
}

fn run_config_command(store: &mut ConfigStore, command: ConfigCommand) -> Result<()> {
    match command {
        ConfigCommand::Get { key } => {
            if let Some(value) = store.config.get_value(&key) {
                println!("{value}");
                return Ok(());
            }
            bail!("key not found: {key}");
        }
        ConfigCommand::Set { key, value } => {
            store.config.set_value(&key, &value)?;
            store.save()?;
            println!("set {key}");
            Ok(())
        }
        ConfigCommand::Unset { key } => {
            store.config.unset_value(&key)?;
            store.save()?;
            println!("unset {key}");
            Ok(())
        }
        ConfigCommand::List => {
            for (key, value) in store.config.list_values() {
                println!("{key} = {value}");
            }
            Ok(())
        }
        ConfigCommand::Path => {
            println!("{}", store.path().display());
            Ok(())
        }
    }
}

fn run_model_command(command: ModelCommand) -> Result<()> {
    let registry = ModelRegistry::default();
    match command {
        ModelCommand::List { provider } => {
            let filter = provider.map(ProviderKind::from);
            for model in registry.list().into_iter().filter(|m| match filter {
                Some(p) => m.provider == p,
                None => true,
            }) {
                println!("{} ({})", model.id, model.provider.as_str());
            }
            Ok(())
        }
        ModelCommand::Resolve { model, provider } => {
            let resolved = registry.resolve(model.as_deref(), provider.map(ProviderKind::from));
            println!("requested: {}", resolved.requested.unwrap_or_default());
            println!("resolved: {}", resolved.resolved.id);
            println!("provider: {}", resolved.resolved.provider.as_str());
            println!("used_fallback: {}", resolved.used_fallback);
            Ok(())
        }
    }
}

fn run_thread_command(command: ThreadCommand) -> Result<()> {
    let state = StateStore::open(None)?;
    match command {
        ThreadCommand::List { all, limit } => {
            let threads = state.list_threads(ThreadListFilters {
                include_archived: all,
                limit,
            })?;
            for thread in threads {
                println!(
                    "{} | {} | {} | {}",
                    thread.id,
                    thread
                        .name
                        .clone()
                        .unwrap_or_else(|| "(unnamed)".to_string()),
                    thread.model_provider,
                    thread.cwd.display()
                );
            }
            Ok(())
        }
        ThreadCommand::Read { thread_id } => {
            let thread = state.get_thread(&thread_id)?;
            println!("{}", serde_json::to_string_pretty(&thread)?);
            Ok(())
        }
        ThreadCommand::Resume { thread_id } => {
            let args = vec!["resume".to_string(), thread_id];
            delegate_simple_tui(args)
        }
        ThreadCommand::Fork { thread_id } => {
            let args = vec!["fork".to_string(), thread_id];
            delegate_simple_tui(args)
        }
        ThreadCommand::Archive { thread_id } => {
            state.mark_archived(&thread_id)?;
            println!("archived {thread_id}");
            Ok(())
        }
        ThreadCommand::Unarchive { thread_id } => {
            state.mark_unarchived(&thread_id)?;
            println!("unarchived {thread_id}");
            Ok(())
        }
        ThreadCommand::SetName { thread_id, name } => {
            let mut thread = state
                .get_thread(&thread_id)?
                .with_context(|| format!("thread not found: {thread_id}"))?;
            thread.name = Some(name);
            thread.updated_at = chrono::Utc::now().timestamp();
            state.upsert_thread(&thread)?;
            println!("renamed {thread_id}");
            Ok(())
        }
    }
}

fn run_sandbox_command(command: SandboxCommand) -> Result<()> {
    match command {
        SandboxCommand::Check { command, ask } => {
            let engine = ExecPolicyEngine::new(Vec::new(), vec!["rm -rf".to_string()]);
            let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
            let decision = engine.check(ExecPolicyContext {
                command: &command,
                cwd: &cwd.display().to_string(),
                ask_for_approval: ask.into(),
                sandbox_mode: Some("workspace-write"),
            })?;
            println!("{}", serde_json::to_string_pretty(&decision)?);
            Ok(())
        }
    }
}

fn run_app_server_command(args: AppServerArgs) -> Result<()> {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .context("failed to create tokio runtime")?;
    if args.stdio {
        return runtime.block_on(run_app_server_stdio(args.config));
    }
    let listen: SocketAddr = format!("{}:{}", args.host, args.port)
        .parse()
        .with_context(|| {
            format!(
                "invalid app-server listen address {}:{}",
                args.host, args.port
            )
        })?;
    runtime.block_on(run_app_server(AppServerOptions {
        listen,
        config_path: args.config,
    }))
}

fn run_mcp_server_command(store: &mut ConfigStore) -> Result<()> {
    let persisted = load_mcp_server_definitions(store);
    let updated = run_stdio_server(persisted)?;
    persist_mcp_server_definitions(store, &updated)
}

fn load_mcp_server_definitions(store: &ConfigStore) -> Vec<McpServerDefinition> {
    let Some(raw) = store.config.get_value(MCP_SERVER_DEFINITIONS_KEY) else {
        return Vec::new();
    };

    match parse_mcp_server_definitions(&raw) {
        Ok(definitions) => definitions,
        Err(err) => {
            eprintln!(
                "warning: failed to parse persisted MCP server definitions ({}): {}",
                MCP_SERVER_DEFINITIONS_KEY, err
            );
            Vec::new()
        }
    }
}

fn parse_mcp_server_definitions(raw: &str) -> Result<Vec<McpServerDefinition>> {
    if let Ok(parsed) = serde_json::from_str::<Vec<McpServerDefinition>>(raw) {
        return Ok(parsed);
    }

    let unwrapped: String = serde_json::from_str(raw)
        .with_context(|| format!("invalid JSON payload at key {MCP_SERVER_DEFINITIONS_KEY}"))?;
    serde_json::from_str::<Vec<McpServerDefinition>>(&unwrapped).with_context(|| {
        format!("invalid MCP server definition list in key {MCP_SERVER_DEFINITIONS_KEY}")
    })
}

fn persist_mcp_server_definitions(
    store: &mut ConfigStore,
    definitions: &[McpServerDefinition],
) -> Result<()> {
    let encoded =
        serde_json::to_string(definitions).context("failed to encode MCP server definitions")?;
    store
        .config
        .set_value(MCP_SERVER_DEFINITIONS_KEY, &encoded)?;
    store.save()
}

fn delegate_to_tui(
    cli: &Cli,
    resolved_runtime: &ResolvedRuntimeOptions,
    passthrough: Vec<String>,
) -> Result<()> {
    let tui = locate_sibling_tui_binary()?;

    let mut cmd = Command::new(tui);
    if let Some(config) = cli.config.as_ref() {
        cmd.arg("--config").arg(config);
    }
    if let Some(profile) = cli.profile.as_ref() {
        cmd.arg("--profile").arg(profile);
    }
    if cli.no_alt_screen {
        cmd.arg("--no-alt-screen");
    }
    if cli.mouse_capture {
        cmd.arg("--mouse-capture");
    }
    if cli.no_mouse_capture {
        cmd.arg("--no-mouse-capture");
    }
    if cli.skip_onboarding {
        cmd.arg("--skip-onboarding");
    }
    cmd.args(passthrough);

    if !matches!(
        resolved_runtime.provider,
        ProviderKind::Deepseek
            | ProviderKind::NvidiaNim
            | ProviderKind::Openrouter
            | ProviderKind::Novita
    ) {
        bail!(
            "The interactive TUI supports DeepSeek, NVIDIA NIM, OpenRouter, and Novita providers. Remove --provider {} or use `deepseek model ...` for provider registry inspection.",
            resolved_runtime.provider.as_str()
        );
    }

    cmd.env("DEEPSEEK_MODEL", &resolved_runtime.model);
    cmd.env("DEEPSEEK_BASE_URL", &resolved_runtime.base_url);
    cmd.env("DEEPSEEK_PROVIDER", resolved_runtime.provider.as_str());
    if let Some(api_key) = resolved_runtime.api_key.as_ref() {
        cmd.env("DEEPSEEK_API_KEY", api_key);
    }

    if let Some(model) = cli.model.as_ref() {
        cmd.env("DEEPSEEK_MODEL", model);
    }
    if let Some(output_mode) = cli.output_mode.as_ref() {
        cmd.env("DEEPSEEK_OUTPUT_MODE", output_mode);
    }
    if let Some(log_level) = cli.log_level.as_ref() {
        cmd.env("DEEPSEEK_LOG_LEVEL", log_level);
    }
    if let Some(telemetry) = cli.telemetry {
        cmd.env("DEEPSEEK_TELEMETRY", telemetry.to_string());
    }
    if let Some(policy) = cli.approval_policy.as_ref() {
        cmd.env("DEEPSEEK_APPROVAL_POLICY", policy);
    }
    if let Some(mode) = cli.sandbox_mode.as_ref() {
        cmd.env("DEEPSEEK_SANDBOX_MODE", mode);
    }
    if let Some(api_key) = cli.api_key.as_ref() {
        cmd.env("DEEPSEEK_API_KEY", api_key);
    }
    if let Some(base_url) = cli.base_url.as_ref() {
        cmd.env("DEEPSEEK_BASE_URL", base_url);
    }

    let status = cmd.status().context("failed to spawn deepseek-tui")?;
    match status.code() {
        Some(code) => std::process::exit(code),
        None => bail!("deepseek-tui terminated by signal"),
    }
}

fn delegate_simple_tui(args: Vec<String>) -> Result<()> {
    let tui = locate_sibling_tui_binary()?;
    let status = Command::new(tui).args(args).status()?;
    match status.code() {
        Some(code) => std::process::exit(code),
        None => bail!("deepseek-tui terminated by signal"),
    }
}

/// Resolve the sibling `deepseek-tui` executable next to the running
/// dispatcher. Honours platform executable suffix (`.exe` on Windows) so
/// the npm-distributed Windows package — which ships
/// `bin/downloads/deepseek-tui.exe` — is found by `Path::exists` (#247).
///
/// `DEEPSEEK_TUI_BIN` is consulted first as an explicit override for
/// custom installs and CI test layouts. On Windows we additionally try
/// the suffix-less name as a fallback for users who already manually
/// renamed the file before this fix landed.
fn locate_sibling_tui_binary() -> Result<PathBuf> {
    if let Ok(override_path) = std::env::var("DEEPSEEK_TUI_BIN") {
        let candidate = PathBuf::from(override_path);
        if candidate.is_file() {
            return Ok(candidate);
        }
        bail!(
            "DEEPSEEK_TUI_BIN points at {}, which is not a regular file.",
            candidate.display()
        );
    }

    let current = std::env::current_exe().context("failed to locate current executable path")?;
    if let Some(found) = sibling_tui_candidate(&current) {
        return Ok(found);
    }

    // Build a stable error path so the user sees the platform-correct
    // expected name, not "deepseek-tui" on Windows.
    let expected = current.with_file_name(format!("deepseek-tui{}", std::env::consts::EXE_SUFFIX));
    bail!(
        "Companion `deepseek-tui` binary not found at {}.\n\
\n\
The `deepseek` dispatcher delegates interactive sessions to a sibling \
`deepseek-tui` binary. To fix this, install one of:\n\
  • npm:    npm install -g deepseek-tui            (downloads both binaries)\n\
  • cargo:  cargo install deepseek-tui-cli deepseek-tui --locked\n\
  • GitHub Releases: download BOTH `deepseek-<platform>` AND \
`deepseek-tui-<platform>` from https://github.com/Hmbown/DeepSeek-TUI/releases/latest \
and place them in the same directory.\n\
\n\
Or set DEEPSEEK_TUI_BIN to the absolute path of an existing `deepseek-tui` binary.",
        expected.display()
    );
}

/// Return the first existing sibling-binary path under any of the names
/// `deepseek-tui` might use on this platform. Pure function to keep
/// `locate_sibling_tui_binary` testable.
fn sibling_tui_candidate(dispatcher: &Path) -> Option<PathBuf> {
    // Primary: platform-correct name. EXE_SUFFIX is "" on Unix and ".exe"
    // on Windows.
    let primary =
        dispatcher.with_file_name(format!("deepseek-tui{}", std::env::consts::EXE_SUFFIX));
    if primary.is_file() {
        return Some(primary);
    }
    // Windows fallback: a user who manually renamed `.exe` away (per the
    // workaround in #247) still launches successfully under the new code.
    if cfg!(windows) {
        let suffixless = dispatcher.with_file_name("deepseek-tui");
        if suffixless.is_file() {
            return Some(suffixless);
        }
    }
    None
}

fn run_metrics_command(args: MetricsArgs) -> Result<()> {
    let since = match args.since.as_deref() {
        Some(s) => {
            Some(metrics::parse_since(s).with_context(|| format!("invalid --since value: {s:?}"))?)
        }
        None => None,
    };
    metrics::run(metrics::MetricsArgs {
        json: args.json,
        since,
    })
}

fn read_api_key_from_stdin() -> Result<String> {
    let mut input = String::new();
    io::stdin()
        .read_to_string(&mut input)
        .context("failed to read api key from stdin")?;
    let key = input.trim().to_string();
    if key.is_empty() {
        bail!("empty API key provided");
    }
    Ok(key)
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::error::ErrorKind;

    fn parse_ok(argv: &[&str]) -> Cli {
        Cli::try_parse_from(argv).unwrap_or_else(|err| panic!("parse failed for {argv:?}: {err}"))
    }

    fn help_for(argv: &[&str]) -> String {
        let err = Cli::try_parse_from(argv).expect_err("expected --help to short-circuit parsing");
        assert_eq!(err.kind(), ErrorKind::DisplayHelp);
        err.to_string()
    }

    #[test]
    fn clap_command_definition_is_consistent() {
        Cli::command().debug_assert();
    }

    #[test]
    fn parses_config_command_matrix() {
        let cli = parse_ok(&["deepseek", "config", "get", "provider"]);
        assert!(matches!(
            cli.command,
            Some(Commands::Config(ConfigArgs {
                command: ConfigCommand::Get { ref key }
            })) if key == "provider"
        ));

        let cli = parse_ok(&["deepseek", "config", "set", "model", "deepseek-v4-flash"]);
        assert!(matches!(
            cli.command,
            Some(Commands::Config(ConfigArgs {
                command: ConfigCommand::Set { ref key, ref value }
            })) if key == "model" && value == "deepseek-v4-flash"
        ));

        let cli = parse_ok(&["deepseek", "config", "unset", "model"]);
        assert!(matches!(
            cli.command,
            Some(Commands::Config(ConfigArgs {
                command: ConfigCommand::Unset { ref key }
            })) if key == "model"
        ));

        assert!(matches!(
            parse_ok(&["deepseek", "config", "list"]).command,
            Some(Commands::Config(ConfigArgs {
                command: ConfigCommand::List
            }))
        ));
        assert!(matches!(
            parse_ok(&["deepseek", "config", "path"]).command,
            Some(Commands::Config(ConfigArgs {
                command: ConfigCommand::Path
            }))
        ));
    }

    #[test]
    fn parses_model_command_matrix() {
        let cli = parse_ok(&["deepseek", "model", "list"]);
        assert!(matches!(
            cli.command,
            Some(Commands::Model(ModelArgs {
                command: ModelCommand::List { provider: None }
            }))
        ));

        let cli = parse_ok(&["deepseek", "model", "list", "--provider", "openai"]);
        assert!(matches!(
            cli.command,
            Some(Commands::Model(ModelArgs {
                command: ModelCommand::List {
                    provider: Some(ProviderArg::Openai)
                }
            }))
        ));

        let cli = parse_ok(&["deepseek", "model", "resolve", "deepseek-v4-flash"]);
        assert!(matches!(
            cli.command,
            Some(Commands::Model(ModelArgs {
                command: ModelCommand::Resolve {
                    model: Some(ref model),
                    provider: None
                }
            })) if model == "deepseek-v4-flash"
        ));

        let cli = parse_ok(&[
            "deepseek",
            "model",
            "resolve",
            "--provider",
            "deepseek",
            "deepseek-v4-pro",
        ]);
        assert!(matches!(
            cli.command,
            Some(Commands::Model(ModelArgs {
                command: ModelCommand::Resolve {
                    model: Some(ref model),
                    provider: Some(ProviderArg::Deepseek)
                }
            })) if model == "deepseek-v4-pro"
        ));
    }

    #[test]
    fn parses_thread_command_matrix() {
        let cli = parse_ok(&["deepseek", "thread", "list", "--all", "--limit", "50"]);
        assert!(matches!(
            cli.command,
            Some(Commands::Thread(ThreadArgs {
                command: ThreadCommand::List {
                    all: true,
                    limit: Some(50)
                }
            }))
        ));

        let cli = parse_ok(&["deepseek", "thread", "read", "thread-1"]);
        assert!(matches!(
            cli.command,
            Some(Commands::Thread(ThreadArgs {
                command: ThreadCommand::Read { ref thread_id }
            })) if thread_id == "thread-1"
        ));

        let cli = parse_ok(&["deepseek", "thread", "resume", "thread-2"]);
        assert!(matches!(
            cli.command,
            Some(Commands::Thread(ThreadArgs {
                command: ThreadCommand::Resume { ref thread_id }
            })) if thread_id == "thread-2"
        ));

        let cli = parse_ok(&["deepseek", "thread", "fork", "thread-3"]);
        assert!(matches!(
            cli.command,
            Some(Commands::Thread(ThreadArgs {
                command: ThreadCommand::Fork { ref thread_id }
            })) if thread_id == "thread-3"
        ));

        let cli = parse_ok(&["deepseek", "thread", "archive", "thread-4"]);
        assert!(matches!(
            cli.command,
            Some(Commands::Thread(ThreadArgs {
                command: ThreadCommand::Archive { ref thread_id }
            })) if thread_id == "thread-4"
        ));

        let cli = parse_ok(&["deepseek", "thread", "unarchive", "thread-5"]);
        assert!(matches!(
            cli.command,
            Some(Commands::Thread(ThreadArgs {
                command: ThreadCommand::Unarchive { ref thread_id }
            })) if thread_id == "thread-5"
        ));

        let cli = parse_ok(&["deepseek", "thread", "set-name", "thread-6", "My Thread"]);
        assert!(matches!(
            cli.command,
            Some(Commands::Thread(ThreadArgs {
                command: ThreadCommand::SetName {
                    ref thread_id,
                    ref name
                }
            })) if thread_id == "thread-6" && name == "My Thread"
        ));
    }

    #[test]
    fn parses_sandbox_app_server_and_completion_matrix() {
        let cli = parse_ok(&[
            "deepseek",
            "sandbox",
            "check",
            "echo hello",
            "--ask",
            "on-failure",
        ]);
        assert!(matches!(
            cli.command,
            Some(Commands::Sandbox(SandboxArgs {
                command: SandboxCommand::Check {
                    ref command,
                    ask: ApprovalModeArg::OnFailure
                }
            })) if command == "echo hello"
        ));

        let cli = parse_ok(&[
            "deepseek",
            "app-server",
            "--host",
            "0.0.0.0",
            "--port",
            "9999",
        ]);
        assert!(matches!(
            cli.command,
            Some(Commands::AppServer(AppServerArgs {
                ref host,
                port: 9999,
                stdio: false,
                ..
            })) if host == "0.0.0.0"
        ));

        let cli = parse_ok(&["deepseek", "app-server", "--stdio"]);
        assert!(matches!(
            cli.command,
            Some(Commands::AppServer(AppServerArgs { stdio: true, .. }))
        ));

        let cli = parse_ok(&["deepseek", "completion", "bash"]);
        assert!(matches!(
            cli.command,
            Some(Commands::Completion { shell: Shell::Bash })
        ));
    }

    #[test]
    fn parses_direct_tui_command_aliases() {
        let cli = parse_ok(&["deepseek", "doctor"]);
        assert!(matches!(
            cli.command,
            Some(Commands::Doctor(TuiPassthroughArgs { ref args })) if args.is_empty()
        ));

        let cli = parse_ok(&["deepseek", "models", "--json"]);
        assert!(matches!(
            cli.command,
            Some(Commands::Models(TuiPassthroughArgs { ref args })) if args == &["--json"]
        ));

        let cli = parse_ok(&["deepseek", "resume", "abc123"]);
        assert!(matches!(
            cli.command,
            Some(Commands::Resume(TuiPassthroughArgs { ref args })) if args == &["abc123"]
        ));

        let cli = parse_ok(&["deepseek", "setup", "--skills", "--local"]);
        assert!(matches!(
            cli.command,
            Some(Commands::Setup(TuiPassthroughArgs { ref args }))
                if args == &["--skills", "--local"]
        ));
    }

    #[test]
    fn deepseek_login_writes_tui_compatible_config() {
        let nanos = chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default();
        let path = std::env::temp_dir().join(format!(
            "deepseek-cli-login-test-{}-{nanos}.toml",
            std::process::id()
        ));
        let mut store = ConfigStore::load(Some(path.clone())).expect("store should load");

        run_login_command(
            &mut store,
            LoginArgs {
                provider: ProviderArg::Deepseek,
                api_key: Some("sk-test".to_string()),
                chatgpt: false,
                device_code: false,
                token: None,
            },
        )
        .expect("login should write config");

        assert_eq!(store.config.api_key.as_deref(), Some("sk-test"));
        assert_eq!(
            store.config.default_text_model.as_deref(),
            Some("deepseek-v4-pro")
        );
        let saved = std::fs::read_to_string(&path).expect("config should be written");
        assert!(saved.contains("api_key = \"sk-test\""));
        assert!(saved.contains("default_text_model = \"deepseek-v4-pro\""));

        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn parses_auth_subcommand_matrix() {
        let cli = parse_ok(&["deepseek", "auth", "set", "--provider", "deepseek"]);
        assert!(matches!(
            cli.command,
            Some(Commands::Auth(AuthArgs {
                command: AuthCommand::Set {
                    provider: ProviderArg::Deepseek,
                    api_key: None,
                    api_key_stdin: false,
                }
            }))
        ));

        let cli = parse_ok(&[
            "deepseek",
            "auth",
            "set",
            "--provider",
            "openrouter",
            "--api-key-stdin",
        ]);
        assert!(matches!(
            cli.command,
            Some(Commands::Auth(AuthArgs {
                command: AuthCommand::Set {
                    provider: ProviderArg::Openrouter,
                    api_key: None,
                    api_key_stdin: true,
                }
            }))
        ));

        let cli = parse_ok(&["deepseek", "auth", "get", "--provider", "novita"]);
        assert!(matches!(
            cli.command,
            Some(Commands::Auth(AuthArgs {
                command: AuthCommand::Get {
                    provider: ProviderArg::Novita
                }
            }))
        ));

        let cli = parse_ok(&["deepseek", "auth", "clear", "--provider", "nvidia-nim"]);
        assert!(matches!(
            cli.command,
            Some(Commands::Auth(AuthArgs {
                command: AuthCommand::Clear {
                    provider: ProviderArg::NvidiaNim
                }
            }))
        ));

        let cli = parse_ok(&["deepseek", "auth", "list"]);
        assert!(matches!(
            cli.command,
            Some(Commands::Auth(AuthArgs {
                command: AuthCommand::List
            }))
        ));

        let cli = parse_ok(&["deepseek", "auth", "migrate"]);
        assert!(matches!(
            cli.command,
            Some(Commands::Auth(AuthArgs {
                command: AuthCommand::Migrate { dry_run: false }
            }))
        ));

        let cli = parse_ok(&["deepseek", "auth", "migrate", "--dry-run"]);
        assert!(matches!(
            cli.command,
            Some(Commands::Auth(AuthArgs {
                command: AuthCommand::Migrate { dry_run: true }
            }))
        ));
    }

    #[test]
    fn auth_set_writes_to_keyring_and_not_to_config_file() {
        use deepseek_secrets::{InMemoryKeyringStore, KeyringStore};
        use std::sync::Arc;

        let nanos = chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default();
        let path = std::env::temp_dir().join(format!(
            "deepseek-cli-auth-set-test-{}-{nanos}.toml",
            std::process::id()
        ));
        let mut store = ConfigStore::load(Some(path.clone())).expect("store should load");
        let inner = Arc::new(InMemoryKeyringStore::new());
        let secrets = Secrets::new(inner.clone());

        run_auth_command_with_secrets(
            &mut store,
            AuthCommand::Set {
                provider: ProviderArg::Deepseek,
                api_key: Some("sk-keyring".to_string()),
                api_key_stdin: false,
            },
            &secrets,
        )
        .expect("set should succeed");

        assert_eq!(
            inner.get("deepseek").unwrap(),
            Some("sk-keyring".to_string())
        );
        // Plaintext config slot must not be written.
        assert!(store.config.api_key.is_none());
        assert!(store.config.providers.deepseek.api_key.is_none());
        let saved = std::fs::read_to_string(&path).unwrap_or_default();
        assert!(
            !saved.contains("sk-keyring"),
            "plaintext key leaked into config: {saved}"
        );

        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn auth_clear_removes_from_keyring_and_config() {
        use deepseek_secrets::{InMemoryKeyringStore, KeyringStore};
        use std::sync::Arc;

        let nanos = chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default();
        let path = std::env::temp_dir().join(format!(
            "deepseek-cli-auth-clear-test-{}-{nanos}.toml",
            std::process::id()
        ));
        let mut store = ConfigStore::load(Some(path.clone())).expect("store should load");
        store.config.api_key = Some("sk-stale".to_string());
        store.config.providers.deepseek.api_key = Some("sk-stale".to_string());
        store.save().unwrap();

        let inner = Arc::new(InMemoryKeyringStore::new());
        inner.set("deepseek", "sk-keyring").unwrap();
        let secrets = Secrets::new(inner.clone());

        run_auth_command_with_secrets(
            &mut store,
            AuthCommand::Clear {
                provider: ProviderArg::Deepseek,
            },
            &secrets,
        )
        .expect("clear should succeed");

        assert_eq!(inner.get("deepseek").unwrap(), None);
        assert!(store.config.api_key.is_none());
        assert!(store.config.providers.deepseek.api_key.is_none());

        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn auth_migrate_moves_plaintext_keys_into_keyring_and_strips_file() {
        use deepseek_secrets::{InMemoryKeyringStore, KeyringStore};
        use std::sync::Arc;

        let nanos = chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default();
        let path = std::env::temp_dir().join(format!(
            "deepseek-cli-auth-migrate-test-{}-{nanos}.toml",
            std::process::id()
        ));
        let mut store = ConfigStore::load(Some(path.clone())).expect("store should load");
        store.config.api_key = Some("sk-deep".to_string());
        store.config.providers.deepseek.api_key = Some("sk-deep".to_string());
        store.config.providers.openrouter.api_key = Some("or-key".to_string());
        store.config.providers.novita.api_key = Some("nv-key".to_string());
        store.save().unwrap();

        let inner = Arc::new(InMemoryKeyringStore::new());
        let secrets = Secrets::new(inner.clone());

        run_auth_command_with_secrets(
            &mut store,
            AuthCommand::Migrate { dry_run: false },
            &secrets,
        )
        .expect("migrate should succeed");

        assert_eq!(inner.get("deepseek").unwrap(), Some("sk-deep".to_string()));
        assert_eq!(inner.get("openrouter").unwrap(), Some("or-key".to_string()));
        assert_eq!(inner.get("novita").unwrap(), Some("nv-key".to_string()));

        // Config file must no longer contain the api keys.
        assert!(store.config.api_key.is_none());
        assert!(store.config.providers.deepseek.api_key.is_none());
        assert!(store.config.providers.openrouter.api_key.is_none());
        assert!(store.config.providers.novita.api_key.is_none());

        let saved = std::fs::read_to_string(&path).expect("config exists post-migrate");
        assert!(!saved.contains("sk-deep"), "plaintext leaked: {saved}");
        assert!(!saved.contains("or-key"), "plaintext leaked: {saved}");
        assert!(!saved.contains("nv-key"), "plaintext leaked: {saved}");

        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn auth_migrate_dry_run_does_not_modify_anything() {
        use deepseek_secrets::{InMemoryKeyringStore, KeyringStore};
        use std::sync::Arc;

        let nanos = chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default();
        let path = std::env::temp_dir().join(format!(
            "deepseek-cli-auth-migrate-dry-{}-{nanos}.toml",
            std::process::id()
        ));
        let mut store = ConfigStore::load(Some(path.clone())).expect("store should load");
        store.config.providers.openrouter.api_key = Some("or-stay".to_string());
        store.save().unwrap();

        let inner = Arc::new(InMemoryKeyringStore::new());
        let secrets = Secrets::new(inner.clone());

        run_auth_command_with_secrets(&mut store, AuthCommand::Migrate { dry_run: true }, &secrets)
            .expect("dry-run should succeed");

        assert_eq!(inner.get("openrouter").unwrap(), None);
        assert_eq!(
            store.config.providers.openrouter.api_key.as_deref(),
            Some("or-stay")
        );

        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn parses_global_override_flags() {
        let cli = parse_ok(&[
            "deepseek",
            "--provider",
            "openai",
            "--config",
            "/tmp/deepseek.toml",
            "--profile",
            "work",
            "--model",
            "gpt-4.1",
            "--output-mode",
            "json",
            "--log-level",
            "debug",
            "--telemetry",
            "true",
            "--approval-policy",
            "on-request",
            "--sandbox-mode",
            "workspace-write",
            "--base-url",
            "https://api.openai.com/v1",
            "--api-key",
            "sk-test",
            "--no-alt-screen",
            "--no-mouse-capture",
            "--skip-onboarding",
            "model",
            "resolve",
            "gpt-4.1",
        ]);

        assert!(matches!(cli.provider, Some(ProviderArg::Openai)));
        assert_eq!(cli.config, Some(PathBuf::from("/tmp/deepseek.toml")));
        assert_eq!(cli.profile.as_deref(), Some("work"));
        assert_eq!(cli.model.as_deref(), Some("gpt-4.1"));
        assert_eq!(cli.output_mode.as_deref(), Some("json"));
        assert_eq!(cli.log_level.as_deref(), Some("debug"));
        assert_eq!(cli.telemetry, Some(true));
        assert_eq!(cli.approval_policy.as_deref(), Some("on-request"));
        assert_eq!(cli.sandbox_mode.as_deref(), Some("workspace-write"));
        assert_eq!(cli.base_url.as_deref(), Some("https://api.openai.com/v1"));
        assert_eq!(cli.api_key.as_deref(), Some("sk-test"));
        assert!(cli.no_alt_screen);
        assert!(cli.no_mouse_capture);
        assert!(!cli.mouse_capture);
        assert!(cli.skip_onboarding);
    }

    #[test]
    fn parses_top_level_prompt_flag_for_canonical_one_shot() {
        let cli = parse_ok(&["deepseek", "-p", "Reply with exactly OK."]);

        assert_eq!(cli.prompt_flag.as_deref(), Some("Reply with exactly OK."));
        assert_eq!(cli.prompt, None);
    }

    #[test]
    fn root_help_surface_contains_expected_subcommands_and_globals() {
        let rendered = help_for(&["deepseek", "--help"]);

        for token in [
            "run",
            "doctor",
            "models",
            "sessions",
            "resume",
            "setup",
            "login",
            "logout",
            "auth",
            "mcp-server",
            "config",
            "model",
            "thread",
            "sandbox",
            "app-server",
            "completion",
            "metrics",
            "--provider",
            "--model",
            "--config",
            "--profile",
            "--output-mode",
            "--log-level",
            "--telemetry",
            "--base-url",
            "--api-key",
            "--approval-policy",
            "--sandbox-mode",
            "--no-alt-screen",
            "--mouse-capture",
            "--no-mouse-capture",
            "--skip-onboarding",
            "--prompt",
        ] {
            assert!(
                rendered.contains(token),
                "expected help to contain token: {token}"
            );
        }
    }

    #[test]
    fn subcommand_help_surfaces_are_stable() {
        let cases = [
            ("config", vec!["get", "set", "unset", "list", "path"]),
            ("model", vec!["list", "resolve"]),
            (
                "thread",
                vec![
                    "list",
                    "read",
                    "resume",
                    "fork",
                    "archive",
                    "unarchive",
                    "set-name",
                ],
            ),
            ("sandbox", vec!["check"]),
            (
                "app-server",
                vec!["--host", "--port", "--config", "--stdio"],
            ),
            ("completion", vec!["<SHELL>", "bash"]),
            ("metrics", vec!["--json", "--since"]),
        ];

        for (subcommand, expected_tokens) in cases {
            let argv = ["deepseek", subcommand, "--help"];
            let rendered = help_for(&argv);
            for token in expected_tokens {
                assert!(
                    rendered.contains(token),
                    "expected help for `{subcommand}` to include `{token}`"
                );
            }
        }
    }

    /// Regression for issue #247: on Windows the dispatcher must find the
    /// sibling `deepseek-tui.exe`, not bail out looking for an
    /// extension-less `deepseek-tui`. The candidate resolver also accepts
    /// the suffix-less name on Windows so users who manually renamed the
    /// file as a workaround keep working after the upgrade.
    #[test]
    fn sibling_tui_candidate_picks_platform_correct_name() {
        let dir = tempfile::TempDir::new().expect("tempdir");
        let dispatcher = dir
            .path()
            .join("deepseek")
            .with_extension(std::env::consts::EXE_EXTENSION);
        // Touch the dispatcher so its parent dir is the lookup root.
        std::fs::write(&dispatcher, b"").unwrap();

        // No sibling yet — resolver returns None.
        assert!(sibling_tui_candidate(&dispatcher).is_none());

        let target =
            dispatcher.with_file_name(format!("deepseek-tui{}", std::env::consts::EXE_SUFFIX));
        std::fs::write(&target, b"").unwrap();

        let found = sibling_tui_candidate(&dispatcher).expect("must locate sibling");
        assert_eq!(found, target, "primary platform-correct name wins");
    }

    /// Windows-only fallback: the user from #247 manually renamed the
    /// file to drop `.exe`. After the fix lands, that workaround must
    /// still resolve via the suffix-less fallback so they don't have to
    /// rename it back.
    #[cfg(windows)]
    #[test]
    fn sibling_tui_candidate_windows_falls_back_to_suffixless() {
        let dir = tempfile::TempDir::new().expect("tempdir");
        let dispatcher = dir.path().join("deepseek.exe");
        std::fs::write(&dispatcher, b"").unwrap();

        // Only the suffixless name exists — emulates the manual rename.
        let suffixless = dispatcher.with_file_name("deepseek-tui");
        std::fs::write(&suffixless, b"").unwrap();

        let found = sibling_tui_candidate(&dispatcher)
            .expect("Windows fallback must locate suffixless deepseek-tui");
        assert_eq!(found, suffixless);
    }

    /// `DEEPSEEK_TUI_BIN` overrides the discovery path. Useful for
    /// custom Windows install layouts and CI test rigs.
    #[test]
    fn locate_sibling_tui_binary_honours_env_override() {
        let dir = tempfile::TempDir::new().expect("tempdir");
        let custom = dir
            .path()
            .join(format!("custom-tui{}", std::env::consts::EXE_SUFFIX));
        std::fs::write(&custom, b"").unwrap();

        // Use a guard so even on test failure the env var clears.
        struct EnvGuard;
        impl Drop for EnvGuard {
            fn drop(&mut self) {
                // SAFETY: tests own this env key for the duration of the
                // guard; clearing on drop matches the documented teardown
                // pattern for `std::env::set_var` in single-threaded tests.
                unsafe { std::env::remove_var("DEEPSEEK_TUI_BIN") };
            }
        }
        let _g = EnvGuard;
        // SAFETY: same single-threaded scope contract as the guard above.
        unsafe { std::env::set_var("DEEPSEEK_TUI_BIN", &custom) };

        let resolved = locate_sibling_tui_binary().expect("override must resolve");
        assert_eq!(resolved, custom);
    }
}
