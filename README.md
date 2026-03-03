# DeepSeek TUI

A terminal-native TUI and CLI for [DeepSeek](https://platform.deepseek.com) models, built in Rust.

[![CI](https://github.com/Hmbown/DeepSeek-TUI/actions/workflows/ci.yml/badge.svg)](https://github.com/Hmbown/DeepSeek-TUI/actions/workflows/ci.yml)
[![crates.io](https://img.shields.io/crates/v/deepseek-tui)](https://crates.io/crates/deepseek-tui)

<p align="center">
  <img src="assets/hero.png" alt="DeepSeek CLI" width="800">
</p>

For DeepSeek models (current and future model IDs). Not affiliated with DeepSeek Inc.

## What is this

A terminal-native agent loop that gives DeepSeek the tools it needs to actually write code: file editing, shell execution, web search, git operations, task tracking, and MCP server integration. Coherence-aware memory compaction keeps long sessions on track without blowing up the context window.

Three modes:

- **Plan** — design-first, proposes before acting
- **Agent** — multi-step autonomous tool use
- **YOLO** — full auto-approve, no guardrails (preloads tools by default)

**Recent highlights**: workspace architecture (modular crates mirroring [Codex](https://github.com/openai/codex) layout), sub-agent orchestration (background workers, parallel tool calls, dependency-aware swarms), parallel tool execution (`multi_tool_use.parallel`), runtime HTTP/SSE API (`deepseek serve --http`), background task queue (`/task`), interactive configuration (`/config`), model discovery (`/models`), command palette (`Ctrl+K`), expandable tool payloads (`v`), persistent sidebar for live plan/todo/sub-agent state, and model context-window suffix hints (`-32k`, `-256k`).

## Install

```bash
# From crates.io (requires Rust 1.85+)
cargo install deepseek-tui --locked

# Or from source
git clone https://github.com/Hmbown/DeepSeek-TUI.git
cd DeepSeek-TUI
cargo install --path crates/tui --locked   # TUI (interactive terminal)
cargo install --path crates/cli --locked   # CLI (dispatcher + server)
```

## Setup

Create `~/.deepseek/config.toml`:

```toml
api_key = "YOUR_DEEPSEEK_API_KEY"
```

Then run:

```bash
deepseek-tui          # interactive TUI
# or
deepseek              # CLI dispatcher (delegates to deepseek-tui for interactive use)
```

**Tab** switches modes, **F1** opens help, **Esc** cancels a running request.

## Usage

```bash
deepseek-tui                                  # interactive TUI
deepseek-tui -p "explain this in 2 sentences" # one-shot prompt
deepseek-tui --yolo                           # agent mode, all tools auto-approved
deepseek doctor                               # check your setup
deepseek models                               # list available models
deepseek serve --http                         # start HTTP/SSE API server
```

Within the TUI, use `/config`, `/models`, `/task`, and `Ctrl+K` command palette.

## Workspace Architecture

```
crates/
  cli/          deepseek-tui-cli    → deepseek          CLI dispatcher + server
  tui/          deepseek-tui        → deepseek-tui      Interactive terminal UI
  app-server/   deepseek-app-server                      HTTP/SSE + JSON-RPC server
  core/         deepseek-core                            Agent loop + engine
  protocol/     deepseek-protocol                        Request/response framing
  config/       deepseek-config                          Configuration + profiles
  state/        deepseek-state                           SQLite session persistence
  tools/        deepseek-tools                           Tool registry + specs
  mcp/          deepseek-mcp                             MCP server integration
  hooks/        deepseek-hooks                           Lifecycle hooks
  execpolicy/   deepseek-execpolicy                      Approval policy engine
  agent/        deepseek-agent                           Model/provider registry
  tui-core/     deepseek-tui-core                        TUI state machine scaffold
```

## Model IDs

Common model IDs: `deepseek-chat`, `deepseek-reasoner`.

Any valid `deepseek-*` model ID is accepted (including future releases). Model IDs can include context-window suffix hints (`-32k`, `-256k`). To see live IDs from your configured endpoint:

```bash
deepseek models
```

## Configuration

Everything lives in `~/.deepseek/config.toml`. See [config.example.toml](config.example.toml) for the full set of options.

Common environment overrides: `DEEPSEEK_API_KEY`, `DEEPSEEK_BASE_URL`, `DEEPSEEK_CONFIG_PATH`, `DEEPSEEK_PROFILE`, `DEEPSEEK_ALLOW_SHELL`, `DEEPSEEK_TRUST_MODE`, and `DEEPSEEK_CAPACITY_*`.

For the full config/env matrix (profiles, feature flags, capacity tuning, sandbox controls), see [docs/CONFIGURATION.md](docs/CONFIGURATION.md).

## Docs

Detailed docs are in the [docs/](docs/) folder — architecture, modes, MCP integration, runtime API, etc.

## License

MIT
