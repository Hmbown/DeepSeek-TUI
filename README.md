# DeepSeek CLI

An unofficial terminal UI and CLI for the [DeepSeek platform](https://platform.deepseek.com).

[![CI](https://github.com/Hmbown/DeepSeek-TUI/actions/workflows/ci.yml/badge.svg)](https://github.com/Hmbown/DeepSeek-TUI/actions/workflows/ci.yml)
[![crates.io](https://img.shields.io/crates/v/deepseek-tui)](https://crates.io/crates/deepseek-tui)

Chat with DeepSeek models directly from your terminal. The assistant can read and write files, run shell commands, search the web, manage tasks, and coordinate sub-agents — all with configurable approval gating.

**Not affiliated with DeepSeek Inc.**

## Getting Started

### 1. Install

```bash
# From crates.io (requires Rust 1.85+)
cargo install deepseek-tui --locked

# Or build from source
git clone https://github.com/Hmbown/DeepSeek-TUI.git
cd DeepSeek-TUI
cargo install --path . --locked
```

Prebuilt binaries are also available on [GitHub Releases](https://github.com/Hmbown/DeepSeek-TUI/releases).

### 2. Set your API key

Get a key from [platform.deepseek.com](https://platform.deepseek.com). On first run the TUI will prompt you to enter and save it, or create the config manually:

```toml
# ~/.deepseek/config.toml
api_key = "YOUR_DEEPSEEK_API_KEY"
```

### 3. Run

```bash
deepseek
```

Press **Tab** to switch modes, **F1** for help, and **Esc** to cancel a running request.

## Modes

| Mode | Description | Approvals |
|------|-------------|-----------|
| **Plan** | Design-first prompting; produces a plan before implementing | Manual for writes and shell |
| **Agent** | Multi-step autonomous tool use | Auto-approve file writes, manual for shell |
| **YOLO** | Full auto-approve (use with caution) | All tools auto-approved |

Press `Tab` to cycle modes. Normal mode (manual approval for everything) is also available via `/set mode normal`.

## What It Can Do

The assistant has access to 30+ tools:

- **File operations** — read, write, edit, patch, search, and grep across your workspace
- **Shell execution** — run commands with timeout, background execution, and interactive I/O
- **Web browsing** — search the web, open pages, screenshot, and extract content with citations
- **Git** — inspect repo status, diffs, and staged changes
- **Code review** — structured review for files, diffs, or GitHub PRs
- **Sub-agents** — spawn background agents or coordinate agent swarms for parallel work
- **Task management** — to-do lists, implementation plans, persistent notes, and a background task queue
- **Structured data** — weather, finance, sports scores, time zones, and a calculator
- **MCP integration** — connect external tool servers via the [Model Context Protocol](docs/MCP.md)

All file tools respect the `--workspace` boundary unless `/trust` is enabled.

## Examples

```bash
# Interactive chat
deepseek

# One-shot prompt
deepseek -p "Explain the borrow checker in two sentences"

# Agentic execution with auto-approve
deepseek exec --auto "Fix all clippy warnings in this project"

# Resume latest session
deepseek --continue

# Work on a specific project
deepseek --workspace /path/to/project

# Review staged git changes
deepseek review --staged

# Start the runtime API server
deepseek serve --http

# Verify your environment
deepseek doctor
```

## Configuration

Config lives at `~/.deepseek/config.toml`:

```toml
api_key = "sk-..."
default_text_model = "deepseek-reasoner" # optional (or "deepseek-chat")
allow_shell = true                     # optional (sandboxed by default)
max_subagents = 3                      # optional (1-20)
```

Key environment variables:

| Variable | Purpose |
|----------|---------|
| `DEEPSEEK_API_KEY` | API key (overrides config file) |
| `DEEPSEEK_BASE_URL` | API endpoint (default: `https://api.deepseek.com`) |

See [`config.example.toml`](config.example.toml) and [`docs/CONFIGURATION.md`](docs/CONFIGURATION.md) for the full reference.

## Runtime API

`deepseek serve --http` starts a local HTTP/SSE API on `127.0.0.1:7878` for external clients. Supports threads, multi-turn conversations, task queues, and live event streaming.

See [`docs/RUNTIME_API.md`](docs/RUNTIME_API.md) for endpoints and usage.

## Keyboard Shortcuts

| Key | Action |
|-----|--------|
| `Enter` | Send message |
| `Alt+Enter` / `Ctrl+J` | Insert newline |
| `Tab` | Autocomplete or cycle modes |
| `Esc` | Cancel request / clear input |
| `Ctrl+C` | Cancel or exit |
| `Ctrl+K` | Command palette |
| `Ctrl+R` | Search past sessions |
| `F1` | Help overlay |
| `PageUp` / `PageDown` | Scroll transcript |

## Troubleshooting

| Problem | Fix |
|---------|-----|
| No API key | Set `DEEPSEEK_API_KEY` or run `deepseek` to complete onboarding |
| Config not found | Check `~/.deepseek/config.toml` (or set `DEEPSEEK_CONFIG_PATH`) |
| Wrong region | Set `DEEPSEEK_BASE_URL` to `https://api.deepseeki.com` (China) |
| Sandbox errors (macOS) | Run `deepseek doctor` |

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

See [CONTRIBUTING.md](CONTRIBUTING.md) for guidelines.

## License

MIT

---

DeepSeek is a trademark of DeepSeek Inc. This is an unofficial, community-driven project.
