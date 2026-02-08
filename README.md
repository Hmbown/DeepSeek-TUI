# DeepSeek CLI

An unofficial terminal UI and CLI for the [DeepSeek platform](https://platform.deepseek.com).

[![CI](https://github.com/Hmbown/DeepSeek-TUI/actions/workflows/ci.yml/badge.svg)](https://github.com/Hmbown/DeepSeek-TUI/actions/workflows/ci.yml)
[![crates.io](https://img.shields.io/crates/v/deepseek-tui)](https://crates.io/crates/deepseek-tui)

Chat with DeepSeek models directly from your terminal. The assistant can read and write files, run shell commands, search the web, manage tasks, and coordinate sub-agents — all with configurable approval gating.

**Not affiliated with DeepSeek Inc.**

## Getting Started

### 1. Install

Choose one of:

```bash
# From crates.io (requires Rust 1.85+)
cargo install deepseek-tui --locked

# Or build from source
git clone https://github.com/Hmbown/DeepSeek-TUI.git
cd DeepSeek-TUI
cargo build --release
# binary is at ./target/release/deepseek
```

Prebuilt binaries are also available on [GitHub Releases](https://github.com/Hmbown/DeepSeek-TUI/releases).

### 2. Set your API key

Get a key from [platform.deepseek.com](https://platform.deepseek.com), then:

On first run, the TUI can prompt for your API key and save it to `~/.deepseek/config.toml`. You can also create the file manually:

```toml
# ~/.deepseek/config.toml
api_key = "YOUR_DEEPSEEK_API_KEY"   # must be non‑empty
default_text_model = "deepseek-v3.2" # optional
allow_shell = false                 # optional
max_subagents = 3                   # optional (1‑20)
```

Alternatively, run `deepseek` and the onboarding wizard will prompt you to enter and save the key.

### 3. Run

```bash
deepseek
```

On first launch the TUI opens in **Agent** mode. Press **Tab** to switch modes, **F1** or type `/help` to see all commands, and **Esc** to cancel a running request.

### 4. Optional setup

```bash
# Bootstrap MCP server config and skills templates
deepseek setup

# Verify your environment
deepseek doctor
```

## Keyboard Shortcuts

| Key | Action |
|-----|--------|
| `Enter` | Send message |
| `Alt+Enter` / `Ctrl+J` | Insert newline |
| `Tab` | Cycle modes (Plan / Agent / YOLO) |
| `Esc` | Cancel request / clear input |
| `Ctrl+C` | Cancel request or exit |
| `Ctrl+R` | Search past sessions |
| `F1` or `Ctrl+/` | Toggle help overlay |
| `PageUp` / `PageDown` | Scroll transcript |
| `Alt+Up` / `Alt+Down` | Scroll transcript (small) |
| `l` (empty input) | Open last message in pager |

## Modes

Press `Tab` to cycle modes: **Plan -> Agent -> YOLO -> Plan**.

| Mode | Description | Approvals |
|------|-------------|-----------|
| **Plan** | Design-first prompting; produces a plan before implementing | Manual for writes and shell |
| **Agent** | Multi-step autonomous tool use | Auto-approve file writes, manual for shell |
| **YOLO** | Full auto-approve (use with caution) | All tools auto-approved |

Normal mode is also available (chat-only with manual approval for everything) and can be selected via `Esc` from Agent mode or `/set mode normal`.

Override approval behavior at runtime: `/set approval_mode auto|suggest|never`.

## Tools

The model has access to 25+ tools across these categories:

### File Operations
- `list_dir` / `read_file` / `write_file` / `edit_file` — basic file I/O within the workspace
- `apply_patch` — apply unified diffs with fuzzy matching
- `grep_files` / `file_search` — search files by regex or name

### Shell Execution
- `exec_shell` — run commands with timeout support and background execution
- `exec_shell_wait` / `exec_shell_interact` — wait on or send input to running commands

### Web
- `web.run` — multi-command browser (search / open / click / find / screenshot / image_query) with citation support
- `web_search` — quick DuckDuckGo search when citations are not needed

### Task Management
- `todo_write` — create and track task lists with status
- `update_plan` — structured implementation plans
- `note` — persistent cross-session notes

### Sub-Agents
- `agent_spawn` / `agent_swarm` — launch background agents or dependency-aware swarms
- `agent_result` / `agent_list` / `agent_cancel` — manage running agents

### Structured Data
- `weather` / `finance` / `sports` / `time` / `calculator`

### Interaction
- `request_user_input` — ask the user structured or multiple-choice questions
- `multi_tool_use.parallel` — execute multiple read-only tools in parallel

All file tools respect the `--workspace` boundary unless `/trust` is enabled (YOLO enables trust automatically). MCP tools execute without TUI approval prompts, so only enable servers you trust.

## Configuration

The TUI stores its config at `~/.deepseek/config.toml`:

```toml
api_key = "sk-..."
default_text_model = "deepseek-reasoner"  # optional
allow_shell = false                       # optional
max_subagents = 3                         # optional (1-20)
```

### Environment Variables

| Variable | Purpose |
|----------|---------|
| `DEEPSEEK_API_KEY` | API key (overrides config file) |
| `DEEPSEEK_BASE_URL` | API endpoint (default: `https://api.deepseek.com`) |
| `DEEPSEEK_PROFILE` | Select a `[profiles.<name>]` section from config |
| `DEEPSEEK_CONFIG_PATH` | Override config file location |

Additional overrides: `DEEPSEEK_MCP_CONFIG`, `DEEPSEEK_SKILLS_DIR`, `DEEPSEEK_NOTES_PATH`, `DEEPSEEK_MEMORY_PATH`, `DEEPSEEK_ALLOW_SHELL`, `DEEPSEEK_MAX_SUBAGENTS`.

See `config.example.toml` and `docs/CONFIGURATION.md` for the full reference.

## Examples

```bash
# Interactive chat (default)
deepseek

# One-shot prompt (non-interactive, prints and exits)
deepseek -p "Explain the borrow checker in two sentences"

# Agentic execution with auto-approve
deepseek exec --auto "Fix all clippy warnings in this project"

# Resume latest session
deepseek --continue

# Work on a specific project directory
deepseek --workspace /path/to/project

# Review staged git changes
deepseek review --staged

# List saved sessions
deepseek sessions --limit 50

# Shell completions
deepseek completions zsh > _deepseek
deepseek completions bash > deepseek.bash
deepseek completions fish > deepseek.fish
```

## Troubleshooting

| Problem | Fix |
|---------|-----|
| No API key | Set `DEEPSEEK_API_KEY` or run `deepseek` to complete onboarding |
| Config not found | Check `~/.deepseek/config.toml` (or `DEEPSEEK_CONFIG_PATH`) |
| Wrong region | Set `DEEPSEEK_BASE_URL` to `https://api.deepseeki.com` (China) |
| Session issues | Run `deepseek sessions` then `deepseek --resume latest` |
| Skills missing | Run `deepseek setup --skills` (add `--local` for workspace-local) |
| MCP tools missing | Run `deepseek mcp init`, then restart |
| Sandbox errors (macOS) | Run `deepseek doctor` to confirm sandbox availability |

## Documentation

- [Configuration Reference](docs/CONFIGURATION.md)
- [Architecture](docs/ARCHITECTURE.md)
- [Mode Comparison](docs/MODES.md)
- [MCP Integration](docs/MCP.md)
- [Contributing](CONTRIBUTING.md)

## Development

```bash
cargo build
cargo test
cargo clippy
cargo fmt
```

See [CONTRIBUTING.md](CONTRIBUTING.md) for detailed guidelines.

## License

MIT

---

DeepSeek is a trademark of DeepSeek Inc. This is an unofficial, community-driven project.
