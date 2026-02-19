# DeepSeek CLI

A terminal interface for the [DeepSeek platform](https://platform.deepseek.com).  
Current release: [v0.3.21](https://github.com/Hmbown/DeepSeek-TUI/releases/tag/v0.3.21).

[![CI](https://github.com/Hmbown/DeepSeek-TUI/actions/workflows/ci.yml/badge.svg)](https://github.com/Hmbown/DeepSeek-TUI/actions/workflows/ci.yml)
[![crates.io](https://img.shields.io/crates/v/deepseek-tui)](https://crates.io/crates/deepseek-tui)

Not affiliated with DeepSeek Inc.

DeepSeek CLI lets you run and control DeepSeek models from your terminal with file editing, shell execution, web lookup, task orchestration, and sub-agent workflows.

## Getting started

1. Install

```bash
# From crates.io (requires Rust 1.85+)
cargo install deepseek-tui --locked

# Or build from source
git clone https://github.com/Hmbown/DeepSeek-TUI.git
cd DeepSeek-TUI
cargo install --path . --locked
```

2. Add API key

Create `~/.deepseek/config.toml`:

```toml
api_key = "YOUR_DEEPSEEK_API_KEY"
```

3. Run

```bash
deepseek
```

Use **Tab** to switch modes, **F1** for help, and **Esc** to cancel a running request.

## Modes

| Mode | Behavior | Approval |
|------|----------|----------|
| `Plan` | Design-first, proposes a plan first | Manual for writes and shell |
| `Agent` | Multi-step autonomous tool use | File writes auto-approved, shell is manual |
| `YOLO` | Full auto-approve | All tools auto-approved |

Use `/set mode normal` to return to manual mode for all actions.

## What it can do

- Workspace file operations: read, edit, search, and patch files
- Shell execution with timeout and interactive support
- Web search and content capture with citations
- Git inspection, task lists, and PR/issue workflows
- Sub-agent orchestration and background execution
- MCP integration for external tool servers
- Runtime API (`deepseek serve --http`) for external clients

## Key commands

```bash
deepseek                         # interactive mode
deepseek -p "Explain ... in 2 sentences"  # one-shot prompt
deepseek exec --auto "Fix all clippy warnings in this project"  # agentic execution
deepseek serve --http            # start local runtime API
deepseek models                  # list available models
deepseek doctor                  # environment and config checks
```

## Configuration

Defaults can be stored in `~/.deepseek/config.toml`:

```toml
api_key = "sk-..."
default_text_model = "deepseek-reasoner" # optional (or "deepseek-chat")
allow_shell = true                     # optional (sandboxed by default)
max_subagents = 3                      # optional (1-20)
```

Overrides:

- `DEEPSEEK_API_KEY` (API key; highest priority)
- `DEEPSEEK_BASE_URL` (default: `https://api.deepseek.com`)

See [config.example.toml](config.example.toml) and [docs/CONFIGURATION.md](docs/CONFIGURATION.md).

## API Runtime

`deepseek serve --http` starts a local HTTP/SSE service on `127.0.0.1:7878` for multi-turn conversations, task queues, and event streaming.
See [docs/RUNTIME_API.md](docs/RUNTIME_API.md).

## Standalone app (web + desktop)

The Next.js + Tauri app lives in `apps/deepseek-app`.

```bash
pnpm install
pnpm deepseek-app:web:dev
pnpm deepseek-app:desktop:dev
```

It uses the same runtime API endpoint (`deepseek serve --http`).

## Docs

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

## Contributors

- Hunter Bown (`@Hmbown`)

## License

MIT

DeepSeek is a trademark of DeepSeek Inc. This is an unofficial, community-run project.
