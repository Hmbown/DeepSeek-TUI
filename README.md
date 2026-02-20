# Wagmii CLI

An agentic coding harness for [Wagmii](https://platform.wagmii.com) models, built in Rust.

[![CI](https://github.com/Hmbown/wagmii/actions/workflows/ci.yml/badge.svg)](https://github.com/Hmbown/wagmii/actions/workflows/ci.yml)
[![crates.io](https://img.shields.io/crates/v/wagmii-tui)](https://crates.io/crates/wagmii-tui)

<p align="center">
  <img src="assets/hero.png" alt="Wagmii CLI" width="800">
</p>

Works with Wagmii v3.2 (chat + reasoner). Ready for v4. Not affiliated with Wagmii Inc.

## What is this

A terminal-native agent loop that gives Wagmii the tools it needs to actually write code: file editing, shell execution, web search, git operations, task tracking, and MCP server integration. Coherence-aware memory compaction keeps long sessions on track without blowing up the context window.

Three modes:

- **Plan** — design-first, proposes before acting
- **Agent** — multi-step autonomous tool use
- **YOLO** — full auto-approve, no guardrails

Sub-agent orchestration is in there too (background workers, parallel tool calls). Still shaking out the rough edges.

## Install

```bash
# From crates.io (requires Rust 1.85+)
cargo install wagmii-tui --locked

# Or from source
git clone https://github.com/Hmbown/wagmii.git
cd wagmii && cargo install --path . --locked
```

## Setup

Create `~/.wagmii/config.toml`:

```toml
api_key = "YOUR_WAGMII_API_KEY"
```

Then run:

```bash
wagmii
```

**Tab** switches modes, **F1** opens help, **Esc** cancels a running request.

## Usage

```bash
wagmii                                  # interactive TUI
wagmii -p "explain this in 2 sentences" # one-shot prompt
wagmii --yolo                           # agent mode, all tools auto-approved
wagmii doctor                           # check your setup
```

## Configuration

Everything lives in `~/.wagmii/config.toml`. See [config.example.toml](config.example.toml) for the full set of options.

Environment overrides: `WAGMII_API_KEY`, `WAGMII_BASE_URL`.

## Docs

Detailed docs are in the [docs/](docs/) folder — architecture, modes, MCP integration, runtime API, etc.

## License

MIT
