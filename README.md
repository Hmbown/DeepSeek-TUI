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
cargo install --path . --locked
# installs `deepseek` to ~/.cargo/bin (ensure it is on your PATH)
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
| `Tab` | Autocomplete slash command (or cycle modes) |
| `Esc` | Cancel request / clear input |
| `Ctrl+C` | Cancel request or exit |
| `Ctrl+K` | Open command palette |
| `Ctrl+R` | Search past sessions |
| `F1` or `Ctrl+/` | Toggle help overlay |
| `PageUp` / `PageDown` | Scroll transcript |
| `Alt+Up` / `Alt+Down` | Scroll transcript (small) |
| `l` (empty input) | Open last message in pager |
| `v` (empty input) | Open selected/latest tool details |

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

The model has access to 30+ tools across these categories:

### File Operations
- `list_dir` / `read_file` / `write_file` / `edit_file` — basic file I/O within the workspace
- `apply_patch` — apply unified diffs with fuzzy matching
- `grep_files` / `file_search` — search files by regex or name
- `git_status` / `git_diff` — inspect repository status and changes

### Shell Execution
- `exec_shell` — run commands with timeout support and background execution
- `exec_shell_wait` / `exec_wait`, `exec_shell_interact` / `exec_interact` — wait on or send input to running commands

### Web & Browsing
- `web.run` — multi-command browser (search / open / click / find / screenshot / image_query) with citation support. Note: the tool name is `web.run` (single dot), not `web..run`.
- `web_search` — quick DuckDuckGo search when citations are not needed

### Task & Project Management
- `todo_write` — create and track task lists with status
- `update_plan` — structured implementation plans
- `note` — persistent cross-session notes
- `/task add|list|show|cancel` — persistent background task queue with timeline visibility
- `project_map` — high-level project structure visualization

### Code Analysis & Review
- `review` — structured code review for files, git diffs, or GitHub PRs
- `run_tests` — run `cargo test` with optional arguments
- `diagnostics` — report workspace, git, sandbox, and toolchain info

### Sub-Agent Orchestration
- `agent_spawn` / `delegate_to_agent` — launch background agents for focused tasks
- `agent_swarm` — orchestrate multiple sub-agents with dependencies
- `agent_result` / `agent_list` / `agent_cancel` / `agent_wait` / `wait` / `send_input` — manage running agents
- `multi_tool_use.parallel` — execute multiple read-only tools in parallel

### Structured Data
- `weather` — daily weather forecast for a location
- `finance` — latest price for stocks, funds, indices, or cryptocurrency
- `sports` — schedules or standings for a league
- `time` — current time for a UTC offset
- `calculator` — evaluate basic arithmetic expressions

### Interaction
- `request_user_input` — ask the user structured or multiple-choice questions

### MCP Integration (when configured)
- `mcp_read_resource`, `mcp_get_prompt` — read context from external MCP servers
- `list_mcp_resources`, `list_mcp_resource_templates` — explore available MCP resources

All file tools respect the `--workspace` boundary unless `/trust` is enabled (YOLO enables trust automatically). MCP tools use the same approval pipeline as built-in tools; only trusted MCP servers should be configured.

**Note on token tracking**: DeepSeek models have a 128k context window. If token counts appear inflated (e.g., >128k), this is likely a tracking bug; use `/compact` to summarize earlier context and free up space.

## Configuration

The TUI stores its config at `~/.deepseek/config.toml`:

```toml
api_key = "sk-..."
default_text_model = "deepseek-v3.2"      # optional
allow_shell = false                       # optional
max_subagents = 3                         # optional (1-20)
```

Any valid DeepSeek model ID is accepted for `default_text_model` (for example, future IDs such as `deepseek-v4-mini` once available).

### Environment Variables

| Variable | Purpose |
|----------|---------|
| `DEEPSEEK_API_KEY` | API key (overrides config file) |
| `DEEPSEEK_BASE_URL` | API endpoint (default: `https://api.deepseek.com`) |
| `DEEPSEEK_PROFILE` | Select a `[profiles.<name>]` section from config |
| `DEEPSEEK_CONFIG_PATH` | Override config file location |

Additional overrides: `DEEPSEEK_MCP_CONFIG`, `DEEPSEEK_SKILLS_DIR`, `DEEPSEEK_NOTES_PATH`, `DEEPSEEK_MEMORY_PATH`, `DEEPSEEK_ALLOW_SHELL`, `DEEPSEEK_APPROVAL_POLICY`, `DEEPSEEK_SANDBOX_MODE`, `DEEPSEEK_MAX_SUBAGENTS`, `DEEPSEEK_ALLOW_INSECURE_HTTP`.

Optional local audit log (off by default): set `DEEPSEEK_TOOL_AUDIT_LOG=/path/to/audit.jsonl` to record tool approval decisions and tool outcomes as JSONL events.

See `config.example.toml` and `docs/CONFIGURATION.md` for the full reference.

## Examples

```bash
# Interactive chat (default)
deepseek

# One-shot prompt (non-interactive, prints and exits)
deepseek -p "Explain the borrow checker in two sentences"

# List models from the configured API endpoint
deepseek models

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

# Runtime API server (localhost by default)
deepseek serve --http --host 127.0.0.1 --port 7878

# MCP stdio server mode
deepseek serve --mcp
```

## Runtime API (HTTP/SSE)

`deepseek serve --http` starts a local runtime API for external clients.

Default bind: `127.0.0.1:7878`

Core endpoints:
- `GET /health`
- `GET /v1/sessions`
- `POST /v1/stream` (backward-compatible single-turn SSE wrapper)
- `POST /v1/threads`
- `GET /v1/threads`
- `GET /v1/threads/{id}`
- `POST /v1/threads/{id}/resume`
- `POST /v1/threads/{id}/fork`
- `POST /v1/threads/{id}/turns`
- `POST /v1/threads/{id}/turns/{turn_id}/steer`
- `POST /v1/threads/{id}/turns/{turn_id}/interrupt`
- `POST /v1/threads/{id}/compact`
- `GET /v1/threads/{id}/events` (SSE replay/live, optional `since_seq`)
- `GET /v1/tasks`
- `POST /v1/tasks`
- `GET /v1/tasks/{id}`
- `POST /v1/tasks/{id}/cancel`

Runtime semantics:
- explicit durable Thread/Turn/Item lifecycle with IDs and statuses
- multi-turn continuity on the same thread
- one active turn per thread (overlap rejected with `409`)
- interrupt transitions to terminal `interrupted` only after cleanup
- steer support for active turns
- compaction surfaced as first-class lifecycle items (`auto` + `manual`)
- replayable per-thread event timeline for API/TUI clients

Task queue semantics:
- durable task storage under `~/.deepseek/tasks` (override with `DEEPSEEK_TASKS_DIR`)
- restart-safe recovery (in-progress tasks are re-queued on startup)
- bounded worker pool via `deepseek serve --http --workers <1-8>`
- task execution linked to runtime thread/turn timelines

Security caveat:
- this server is local-first and assumes trusted local access
- no built-in auth/TLS/multi-user isolation
- do not expose it directly to untrusted networks without your own auth/proxy controls

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
| Finance tool returns no data | Currently, the finance tool relies on Stooq which may be unavailable; use `web.run` for financial data |
| Token/cost tracking inaccurate | This is a known bug; metrics are approximate. Use `/compact` to manage context |

## Documentation

- [Configuration Reference](docs/CONFIGURATION.md)
- [Architecture](docs/ARCHITECTURE.md)
- [Mode Comparison](docs/MODES.md)
- [MCP Integration](docs/MCP.md)
- [Runtime API](docs/RUNTIME_API.md)
- [Operations Runbook](docs/OPERATIONS_RUNBOOK.md)
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
