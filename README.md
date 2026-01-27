# DeepSeek CLI 🤖

Your AI-powered terminal companion for DeepSeek models

[![CI](https://github.com/Hmbown/DeepSeek-TUI/actions/workflows/ci.yml/badge.svg)](https://github.com/Hmbown/DeepSeek-TUI/actions/workflows/ci.yml)
[![crates.io](https://img.shields.io/crates/v/deepseek-tui)](https://crates.io/crates/deepseek-tui)

Unofficial terminal UI (TUI) + CLI for the [DeepSeek platform](https://platform.deepseek.com) — chat with DeepSeek models and collaborate with AI assistants that can read, write, execute, and plan with approval-gated tool access.

**Not affiliated with DeepSeek Inc.**

## ✨ Features

- **Interactive TUI** with multiple modes (Normal, Plan, Agent, YOLO, RLM, Duo)
- **Comprehensive tool access** – File operations, shell execution, task management, and sub-agent systems
- **File operations**: List directories, read/write/edit files, apply patches, search files with regex
- **Shell execution**: Run commands with timeout support, background execution with task management
- **Task management**: Todo lists, implementation plans, persistent notes
- **Sub-agent system**: Spawn, coordinate, and cancel background agents (including swarms)
- **Web search**: Integrated web search with DuckDuckGo
- **Multi‑model support** – DeepSeek‑Reasoner, DeepSeek‑Chat, and other DeepSeek models
- **Context‑aware** – loads project‑specific instructions from `AGENTS.md`
- **Session management** – resume, fork, and search past conversations
- **Skills system** – reusable workflows stored as `SKILL.md` directories
- **Model Context Protocol (MCP)** – integrate external tool servers
- **Sandboxed execution** (macOS) for safe shell commands
- **Git integration** – code review, patch application, diff analysis
- **Cross‑platform** – works on macOS, Linux, and Windows

## 🚀 Quick Start

1. **Get an API key** from [https://platform.deepseek.com](https://platform.deepseek.com)
2. **Install and run**:

```bash
# Install via Cargo
cargo install deepseek-tui --locked

# Set your API key
export DEEPSEEK_API_KEY="YOUR_DEEPSEEK_API_KEY"

# Start chatting
deepseek
```

3. Press `F1` or type `/help` for the in‑app command list.

If anything looks off, run `deepseek doctor` to diagnose configuration issues.

## 📦 Installation

### From crates.io

```bash
cargo install deepseek-tui --locked
```

### Build from source

```bash
git clone https://github.com/Hmbown/DeepSeek-TUI.git
cd DeepSeek-TUI
cargo build --release
./target/release/deepseek --help
```

### Direct download

Download a prebuilt binary from [GitHub Releases](https://github.com/Hmbown/DeepSeek-TUI/releases) and put it on your `PATH` as `deepseek`.

## ⚙️ Configuration

On first run, the TUI can prompt for your API key and save it to `~/.deepseek/config.toml`. You can also create the file manually:

```toml
# ~/.deepseek/config.toml
api_key = "YOUR_DEEPSEEK_API_KEY"   # must be non‑empty
default_text_model = "deepseek-reasoner" # optional
allow_shell = false                 # optional
max_subagents = 3                   # optional (1‑20)
```

Useful environment variables:

- `DEEPSEEK_API_KEY` (overrides `api_key`)
- `DEEPSEEK_BASE_URL` (default: `https://api.deepseek.com`; China users may use `https://api.deepseeki.com`)
- `DEEPSEEK_PROFILE` (selects `[profiles.<name>]` from the config; errors if missing)
- `DEEPSEEK_CONFIG_PATH` (override config path)
- `DEEPSEEK_MCP_CONFIG`, `DEEPSEEK_SKILLS_DIR`, `DEEPSEEK_NOTES_PATH`, `DEEPSEEK_MEMORY_PATH`, `DEEPSEEK_ALLOW_SHELL`, `DEEPSEEK_MAX_SUBAGENTS`

See `config.example.toml` and `docs/CONFIGURATION.md` for a full reference.

## 🎮 Modes

In the TUI, press `Tab` to cycle modes: **Normal → Plan → Agent → YOLO → RLM → Duo → Normal**.

| Mode | Description | Approval Behavior |
|------|-------------|-------------------|
| **Normal** | Chat; asks before file writes or shell | Manual approval for writes & shell |
| **Plan** | Design‑first prompting; same approvals as Normal | Manual approval for writes & shell |
| **Agent** | Multi‑step tool use; asks before shell | Manual approval for shell, auto‑approve file writes |
| **YOLO** | Enables shell + trust + auto‑approves all tools (dangerous) | Auto‑approve all tools |
| **RLM** | Externalized context + REPL helpers; auto‑approves tools (best for large files) | Auto‑approve tools |
| **Duo** | Player‑coach autocoding with iterative validation (based on g3 paper) | Depends on phase |

Approval behavior is mode‑dependent, but you can also override it at runtime with `/set approval_mode auto|suggest|never`.

## 🛠️ Tools

DeepSeek CLI exposes a comprehensive set of tools to the model across 5 categories, with 16+ individual tools available, all with approval gating based on the current mode.

### Tool Categories

#### File Operations
- **`list_dir`** – List directory contents with file/directory metadata
- **`read_file`** – Read UTF‑8 files from the workspace
- **`write_file`** – Create or overwrite files
- **`edit_file`** – Search and replace text in files
- **`apply_patch`** – Apply unified diff patches with fuzzy matching
- **`grep_files`** – Search files by regex pattern with context lines
- **`web_search`** – Search the web and return concise results

#### Shell Execution
- **`exec_shell`** – Run shell commands with timeout support
- **Background execution** – Run commands in background with task ID return

#### Task Management
- **`todo_write`** – Create and update todo lists with status tracking
- **`update_plan`** – Manage structured implementation plans
- **`note`** – Append persistent notes across sessions

#### Sub‑Agents
- **`agent_spawn`** – Create background sub‑agents for focused tasks
- **`agent_swarm`** – Launch a dependency‑aware swarm of sub‑agents
- **`agent_result`** – Retrieve results from sub‑agents
- **`agent_list`** – List all active and completed agents
- **`agent_cancel`** – Cancel running sub‑agents

### System Behavior

- **Workspace boundary**: File tools are restricted to `--workspace` unless you enable `/trust` (YOLO enables trust automatically).
- **Approvals**: The TUI requests approval depending on mode and tool category (file writes, shell).
- **Web search**: `web_search` uses DuckDuckGo HTML results and is auto‑approved.
- **Skills**: Reusable workflows stored as `SKILL.md` directories (default: `~/.deepseek/skills`). Use `/skills` and `/skill <name>`.
- **MCP**: Load external tool servers via `~/.deepseek/mcp.json` (supports `servers` and `mcpServers`). MCP tools currently execute without TUI approval prompts, so only enable servers you trust. See `docs/MCP.md`.

## 🧠 RLM (Reasoning & Large‑scale Memory)

RLM mode is designed for "too big for context" tasks: large files, whole‑doc sweeps, and big pasted blocks.

- Auto‑switch triggers: "largest file", explicit "RLM", large file requests, and large pastes.
- Shortcut: `/rlm` (or `/aleph`) enters RLM mode directly.
- In **RLM mode**, `/load @path` loads a file into the external context store (outside RLM mode, `/load` loads a saved chat JSON).
- Use `/repl` to enter expression mode (e.g. `search("pattern")`, `lines(1, 80)`).
- Power tools: `rlm_load`, `rlm_exec`, `rlm_status`, `rlm_query`.

`rlm_query` can be expensive: prefer batching and check `/status` if you're doing lots of sub‑queries.

## 👥 Duo Mode

> **Note:** Duo mode is experimental and may not work correctly in all cases. Use with caution.

Duo mode implements the player‑coach autocoding paradigm for iterative development with built‑in validation:

- **Player**: implements requirements (builder role)
- **Coach**: validates implementation against requirements (critic role)
- Tools: `duo_init`, `duo_player`, `duo_coach`, `duo_advance`, `duo_status`

Workflow: `init → player → coach → advance → (repeat until approved)`

## 📚 Examples

### Interactive chat

```bash
deepseek
```

### One‑shot prompt (non‑interactive)

```bash
deepseek -p "Write a haiku about Rust"
```

### Agentic execution with tool access

```bash
deepseek exec --auto "Fix lint errors in the current directory"
```

### Resume latest session

```bash
deepseek --continue
```

### Work on a specific project

```bash
deepseek --workspace /path/to/project
```

### Review staged git changes

```bash
deepseek review --staged
```

### Apply a patch file

```bash
deepseek apply patch.diff
```

### List saved sessions

```bash
deepseek sessions --limit 50
```

### Generate shell completions

```bash
deepseek completions zsh > _deepseek
deepseek completions bash > deepseek.bash
deepseek completions fish > deepseek.fish
```

## 🔧 Troubleshooting

### No API key
Set `DEEPSEEK_API_KEY` environment variable or run `deepseek` and complete onboarding.

### Config not found
Check `~/.deepseek/config.toml` (or `DEEPSEEK_CONFIG_PATH`).

### Wrong region / base URL
Set `DEEPSEEK_BASE_URL` to `https://api.deepseeki.com` (China).

### Session issues
Run `deepseek sessions` and try `deepseek --resume latest`.

### MCP tools missing
Validate `~/.deepseek/mcp.json` (or `DEEPSEEK_MCP_CONFIG`) and restart.

### Sandbox errors (macOS)
Ensure `/usr/bin/sandbox-exec` exists (comes with macOS). For other platforms, sandboxing is limited.

## 📖 Documentation

- `docs/CONFIGURATION.md` – Complete configuration reference
- `docs/MCP.md` – Model Context Protocol guide
- `docs/ARCHITECTURE.md` – Project architecture
- `docs/RLM.md` – RLM mode deep‑dive
- `docs/MODES.md` – Mode comparison and usage
- `CONTRIBUTING.md` – How to contribute to the project

## 🧪 Development

```bash
cargo build
cargo test
cargo fmt
cargo clippy
```

See `CONTRIBUTING.md` for detailed guidelines.

## 📄 License

MIT

---

DeepSeek is a trademark of DeepSeek Inc. This is an unofficial project.
