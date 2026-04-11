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

Start the TUI:

```bash
deepseek-tui
```

On first launch, it will prompt for your API key if one is not already configured.

You can also set auth ahead of time with either of these:

```bash
deepseek-tui login
DEEPSEEK_API_KEY="YOUR_DEEPSEEK_API_KEY" deepseek-tui
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
project. crates.io publication can lag the repository workspace version and the
npm wrapper, so use npm or install from source if you need the newest release
surface immediately.

</details>

## What it does

A terminal coding agent for DeepSeek models with file editing, shell execution, `web.run` browsing, git operations, session resume, and [MCP](https://modelcontextprotocol.io) server integration.

Three visible modes (**Tab** / **Shift+Tab** to cycle):

| Mode | Behavior |
|------|----------|
| **Plan** | Review a plan before the agent starts making changes |
| **Agent** | Default interactive mode with multi-step tool use |
| **YOLO** | Auto-approve tools in a trusted workspace |

## Usage

```bash
deepseek-tui                                  # interactive TUI
deepseek-tui -p "explain this in 2 sentences" # one-shot prompt
deepseek-tui --yolo                           # YOLO mode
deepseek-tui login                            # save API key to config
deepseek-tui doctor                           # check setup
deepseek-tui models                           # list available models
deepseek-tui serve --http                     # HTTP/SSE API server
```

Controls: `F1` help, `Esc` backs out of the current action, `Ctrl+K` command palette.

## Configuration

`~/.deepseek/config.toml` — see [config.example.toml](config.example.toml) for all options.

Key environment overrides: `DEEPSEEK_API_KEY`, `DEEPSEEK_BASE_URL`, `DEEPSEEK_PROFILE`.

Full reference: [docs/CONFIGURATION.md](docs/CONFIGURATION.md).

## Docs

[docs/](docs/) — configuration, modes, MCP integration, runtime API, and release runbooks.

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md). Not affiliated with DeepSeek Inc.

## License

[MIT](LICENSE)
