# DeepSeek TUI

`npm i -g deepseek-tui`

A coding agent for [DeepSeek](https://platform.deepseek.com) models that runs in your terminal.

[![CI](https://github.com/Hmbown/DeepSeek-TUI/actions/workflows/ci.yml/badge.svg)](https://github.com/Hmbown/DeepSeek-TUI/actions/workflows/ci.yml)
[![crates.io](https://img.shields.io/crates/v/deepseek-tui)](https://crates.io/crates/deepseek-tui)
[![npm](https://img.shields.io/npm/v/deepseek-tui)](https://www.npmjs.com/package/deepseek-tui)

<p align="center">
  <img src="assets/hero.png" alt="DeepSeek TUI" width="800">
</p>

## Quickstart

```bash
npm install -g deepseek-tui
```

Set your API key:

```bash
mkdir -p ~/.deepseek && echo 'api_key = "YOUR_DEEPSEEK_API_KEY"' > ~/.deepseek/config.toml
```

Then run:

```bash
deepseek-tui
```

<details>
<summary>Other install methods</summary>

```bash
# From crates.io (requires Rust 1.85+)
cargo install deepseek-tui --locked

# From source
git clone https://github.com/Hmbown/DeepSeek-TUI.git
cd DeepSeek-TUI
cargo install --path crates/tui --locked
```

</details>

## What it does

An agent loop with file editing, shell execution, web search, git operations, task tracking, and [MCP](https://modelcontextprotocol.io) server integration. Context-aware memory compaction keeps long sessions on track.

Three modes (**Tab** to switch):

| Mode | Behavior |
|------|----------|
| **Plan** | Design-first — proposes before acting |
| **Agent** | Multi-step autonomous tool use |
| **YOLO** | Full auto-approve, no guardrails |

## Usage

```bash
deepseek-tui                                  # interactive TUI
deepseek-tui -p "explain this in 2 sentences" # one-shot prompt
deepseek-tui --yolo                           # YOLO mode
deepseek doctor                               # check setup
deepseek models                               # list available models
deepseek serve --http                         # HTTP/SSE API server
```

**F1** opens help. **Esc** cancels a running request. **Ctrl+K** opens command palette.

## Configuration

`~/.deepseek/config.toml` — see [config.example.toml](config.example.toml) for all options.

Key environment overrides: `DEEPSEEK_API_KEY`, `DEEPSEEK_BASE_URL`, `DEEPSEEK_PROFILE`.

Full reference: [docs/CONFIGURATION.md](docs/CONFIGURATION.md).

## Docs

[docs/](docs/) — architecture, modes, MCP integration, runtime API.

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md). Not affiliated with DeepSeek Inc.

## License

[MIT](LICENSE)
