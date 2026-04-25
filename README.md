# DeepSeek TUI

`npm i -g deepseek-tui`

A coding agent for [DeepSeek](https://platform.deepseek.com) models that runs in your terminal.

[![CI](https://github.com/Hmbown/DeepSeek-TUI/actions/workflows/ci.yml/badge.svg)](https://github.com/Hmbown/DeepSeek-TUI/actions/workflows/ci.yml)
[![crates.io](https://img.shields.io/crates/v/deepseek-tui)](https://crates.io/crates/deepseek-tui)
[![npm](https://img.shields.io/npm/v/deepseek-tui)](https://www.npmjs.com/package/deepseek-tui)

## Quickstart

```bash
npm install -g deepseek-tui
```

Start the TUI:

```bash
deepseek
```

On first launch, it will prompt for your API key if one is not already configured.
The package also installs `deepseek-tui`; both commands share the same
`~/.deepseek/config.toml` for DeepSeek auth and default model settings.

You can also set auth ahead of time with either of these:

```bash
deepseek login --api-key "YOUR_DEEPSEEK_API_KEY"
deepseek-tui login --api-key "YOUR_DEEPSEEK_API_KEY"
DEEPSEEK_API_KEY="YOUR_DEEPSEEK_API_KEY" deepseek-tui
```

To use NVIDIA NIM-hosted DeepSeek V4 Pro instead:

```bash
deepseek auth set --provider nvidia-nim --api-key "YOUR_NVIDIA_API_KEY"
deepseek --provider nvidia-nim

# or for one process:
DEEPSEEK_PROVIDER=nvidia-nim NVIDIA_API_KEY="YOUR_NVIDIA_API_KEY" deepseek
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

Three visible modes (**Tab** to cycle):

| Mode | Behavior |
|------|----------|
| **Plan** | Review a plan before the agent starts making changes |
| **Agent** | Default interactive mode with multi-step tool use |
| **YOLO** | Auto-approve tools in a trusted workspace |

**Shift+Tab** cycles the reasoning-effort tier for DeepSeek thinking mode:
`off` → `high` → `max`. The current tier is shown as a ⚡ chip in the header.
Set a default in config with `reasoning_effort = "max"` (or `off` / `low` /
`medium` / `high`).

## Models & pricing

| Model | Thinking | Context | Input cache hit | Input cache miss | Output |
|---|---|---|---|---|---|
| `deepseek-v4-pro` | default | 1M | $0.145 / 1M | $1.74 / 1M | $3.48 / 1M |
| `deepseek-v4-flash` | default | 1M | $0.028 / 1M | $0.14 / 1M | $0.28 / 1M |
| `deepseek-ai/deepseek-v4-pro` via NVIDIA NIM | default | 1M | NVIDIA account terms | NVIDIA account terms | NVIDIA account terms |
| `deepseek-ai/deepseek-v4-flash` via NVIDIA NIM | default | 1M | NVIDIA account terms | NVIDIA account terms | NVIDIA account terms |

Legacy `deepseek-chat` and `deepseek-reasoner` remain as silent aliases for
`deepseek-v4-flash` (priced identically). Pricing is per 1M tokens as published
by DeepSeek and is subject to change.

## Usage

```bash
deepseek                                      # interactive TUI
deepseek "explain this in 2 sentences"        # one-shot prompt
deepseek --model deepseek-v4-flash "summarize" # one-shot with model override
deepseek --yolo                               # YOLO mode
deepseek login --api-key "..."                # save API key to shared config
deepseek doctor                               # check setup
deepseek models                               # list live DeepSeek API models
deepseek sessions                             # list saved sessions
deepseek resume --last                        # resume the latest session
deepseek serve --http                         # HTTP/SSE API server
```

Controls: `F1` help, `Esc` backs out of the current action, `Ctrl+K` command palette.
In the composer, `@path/to/file` adds local text file or directory context to
the next message. Use `/attach <path>` for local image/video media references.

## Configuration

`~/.deepseek/config.toml` — see [config.example.toml](config.example.toml) for all options.

Key environment overrides: `DEEPSEEK_API_KEY`, `DEEPSEEK_BASE_URL`,
`DEEPSEEK_MODEL`, `DEEPSEEK_PROFILE`, `DEEPSEEK_PROVIDER`.
For NVIDIA NIM, use `DEEPSEEK_PROVIDER=nvidia-nim` plus `NVIDIA_API_KEY`
or `NVIDIA_NIM_API_KEY` (with `DEEPSEEK_API_KEY` as a compatibility fallback);
the default model is `deepseek-ai/deepseek-v4-pro` and the default base URL is
`https://integrate.api.nvidia.com/v1`. With `--provider nvidia-nim`,
`--model deepseek-v4-flash` maps to `deepseek-ai/deepseek-v4-flash`.

Quick checks and scaffolding:

- `deepseek-tui setup --status` — read-only, network-free status of API key,
  MCP/skills/tools/plugins, sandbox, and `.env`.
- `deepseek-tui setup --tools --plugins` — scaffold `~/.deepseek/tools/` and
  `~/.deepseek/plugins/` with self-describing example templates.
- `deepseek-tui doctor --json` — machine-readable doctor output for CI.

The client targets DeepSeek's documented OpenAI-compatible Chat Completions API
(`/chat/completions`). DeepSeek context caching is automatic; when the API
returns cache hit/miss token fields, the TUI includes them in usage and cost
tracking.

Full reference: [docs/CONFIGURATION.md](docs/CONFIGURATION.md).

## Docs

[docs/](docs/) — configuration, modes, MCP integration, runtime API, and release runbooks.

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md). Not affiliated with DeepSeek Inc.

## License

[MIT](LICENSE)
