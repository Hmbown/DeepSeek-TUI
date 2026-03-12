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
cargo install deepseek-tui --locked       # TUI
cargo install deepseek-tui-cli --locked   # deepseek CLI facade

# From source
git clone https://github.com/Hmbown/DeepSeek-TUI.git
cd DeepSeek-TUI
cargo install --path crates/tui --locked
```

The canonical crates.io packages for this repository are `deepseek-tui` and
`deepseek-tui-cli`. The unrelated `deepseek-cli` crate is not part of this
project.

</details>

## What it does

An agent loop with file editing, shell execution, `web.run` browsing, git operations, task tracking, and [MCP](https://modelcontextprotocol.io) server integration. Context-aware memory compaction keeps long sessions on track. `crates/tui` remains the live shipped runtime while the workspace extraction continues.

Four modes (**Tab** / **Shift+Tab** to cycle):

| Mode | Behavior |
|------|----------|
| **Normal** | Chat-first mode for questions, explanation, and low-friction steering |
| **Plan** | Design-first — proposes before acting |
| **Agent** | Multi-step autonomous tool use |
| **YOLO** | Full auto-approve, no guardrails |

## First Run Workflow

1. Paste your API key in onboarding.
2. Choose a mode for the task in front of you:
   `Normal` to ask questions, `Plan` to review a plan first, `Agent` to let the model use tools, `YOLO` only inside a trusted workspace.
3. Watch the status area while work is running:
   approvals, queued work, and active sub-agents stay there while the turn is live.
4. Recover work with `Ctrl+R` or `/sessions` if you need to resume an interrupted thread.

## Everyday Workflows

- Use `Ctrl+K` for the command palette when you want to switch modes, open config, resume sessions, or inspect a tool quickly.
- Use `/queue` to review or edit queued prompts before sending them.
- Use `/subagents` to inspect background agent state when autonomous work fans out.
- Use `/config` to adjust approval mode, theme, sidebar focus, and other runtime preferences.

## Usage

```bash
deepseek-tui                                  # interactive TUI
deepseek-tui -p "explain this in 2 sentences" # one-shot prompt
deepseek-tui --yolo                           # YOLO mode
deepseek doctor                               # check setup
deepseek models                               # list available models
deepseek serve --http                         # HTTP/SSE API server
```

Controls: `F1` help, `Esc` walks the cancel stack, `Ctrl+K` command palette.

## Configuration

`~/.deepseek/config.toml` — see [config.example.toml](config.example.toml) for all options.

Key environment overrides: `DEEPSEEK_API_KEY`, `DEEPSEEK_BASE_URL`, `DEEPSEEK_PROFILE`.

Full reference: [docs/CONFIGURATION.md](docs/CONFIGURATION.md).

## Docs

[docs/](docs/) — architecture, modes, MCP integration, runtime API, and release runbooks. The live runtime still ships from `crates/tui`; the newer workspace crates are incremental extraction targets.

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md). Not affiliated with DeepSeek Inc.

## License

[MIT](LICENSE)
