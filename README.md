# DeepSeek TUI

> Terminal coding agent for DeepSeek V4. It runs from the `deepseek` command, streams reasoning blocks, edits local workspaces with approval gates, and can route each turn between Flash/Pro and thinking levels.

[简体中文 README](README.zh-CN.md)

## Install

`deepseek` is distributed as Rust binaries: the dispatcher command
(`deepseek`) and the companion TUI runtime (`deepseek-tui`). Pick whichever
install path you already use; they all put the same commands on your `PATH`.
The npm package is an installer/wrapper for the release binaries, not the
agent runtime itself.

```bash
# 1. npm — easiest if you already use Node. The package downloads the
#    matching prebuilt Rust binaries from GitHub Releases.
npm install -g deepseek-tui

# 2. Cargo — no Node needed.
cargo install deepseek-tui-cli --locked   # `deepseek` (entry point)
cargo install deepseek-tui     --locked   # `deepseek-tui` (TUI binary)

# 3. Homebrew — macOS package manager.
brew tap Hmbown/deepseek-tui
brew install deepseek-tui

# 4. Direct download — no package manager or toolchain.
#    https://github.com/Hmbown/DeepSeek-TUI/releases
#    Prebuilt for Linux x64/ARM64, macOS x64/ARM64, Windows x64.
```

> In mainland China, speed up the npm path with
> `--registry=https://registry.npmmirror.com`, or use the
> [Cargo mirror](#china--mirror-friendly-installation) below.

[![CI](https://github.com/Hmbown/DeepSeek-TUI/actions/workflows/ci.yml/badge.svg)](https://github.com/Hmbown/DeepSeek-TUI/actions/workflows/ci.yml)
[![npm](https://img.shields.io/npm/v/deepseek-tui)](https://www.npmjs.com/package/deepseek-tui)
[![crates.io](https://img.shields.io/crates/v/deepseek-tui-cli?label=crates.io)](https://crates.io/crates/deepseek-tui-cli)
[DeepWiki project index](https://deepwiki.com/Hmbown/DeepSeek-TUI)

![DeepSeek TUI screenshot](assets/screenshot.png)

---

## What Is It?

DeepSeek TUI is a coding agent that runs in your terminal. It can read and edit files, run shell commands, search the web, manage git, and coordinate sub-agents from a keyboard-driven TUI.

It is built around DeepSeek V4 (`deepseek-v4-pro` / `deepseek-v4-flash`), including 1M-token context windows, streaming reasoning blocks, and prefix-cache-aware cost reporting.

### Key Features

- **Auto model router** — `--model auto` / `/model auto` chooses Flash or Pro plus thinking `off` / `high` / `max` per turn
- **Thinking-mode streaming** — see DeepSeek reasoning blocks as the model works
- **Full tool suite** — file ops, shell execution, git, web search/browse, apply-patch, sub-agents, MCP servers
- **1M-token context** — context tracking, manual or configured compaction, and prefix-cache telemetry
- **Three modes** — Plan (read-only explore), Agent (interactive with approval), YOLO (auto-approved)
- **Reasoning-effort tiers** — cycle through `off → high → max` with `Shift + Tab`
- **Session save/resume** — checkpoint and resume long-running sessions
- **Workspace rollback** — side-git pre/post-turn snapshots with `/restore` and `revert_turn`, without touching your repo's `.git`
- **Durable task queue** — background tasks can survive restarts
- **HTTP/SSE runtime API** — `deepseek serve --http` for headless agent workflows
- **MCP protocol** — connect to Model Context Protocol servers for extended tooling; please see [docs/MCP.md](docs/MCP.md)
- **Native RLM** (`rlm_query`) — run batched analysis through cheap `deepseek-v4-flash` children using the same API client
- **LSP diagnostics** — inline error/warning surfacing after every edit via rust-analyzer, pyright, typescript-language-server, gopls, clangd
- **User memory** — optional persistent note file injected into the system prompt for cross-session preferences
- **Localized UI** — `en`, `ja`, `zh-Hans`, `pt-BR` with auto-detection
- **Live cost tracking** — per-turn and session-level token usage and cost estimates; cache hit/miss breakdown
- **Skills system** — composable, installable instruction packs from GitHub with no backend service required

---

## How It's Wired

`deepseek` (dispatcher CLI) → `deepseek-tui` (companion binary) → ratatui interface ↔ async engine ↔ OpenAI-compatible streaming client. Tool calls route through a typed registry (shell, file ops, git, web, sub-agents, MCP, RLM) and results stream back into the transcript. The engine manages session state, turn tracking, the durable task queue, and an LSP subsystem that feeds post-edit diagnostics into the model's context before the next reasoning step.

See [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) for the full walkthrough.

---

## Quickstart

```bash
npm install -g deepseek-tui
deepseek --version
deepseek --model auto
```

Prebuilt binaries are published for **Linux x64**, **Linux ARM64** (v0.8.8+), **macOS x64**, **macOS ARM64**, and **Windows x64**. For other targets (musl, riscv64, FreeBSD, etc.), see [Install from source](#install-from-source) or [docs/INSTALL.md](docs/INSTALL.md).

On first launch you'll be prompted for your [DeepSeek API key](https://platform.deepseek.com/api_keys). The key is saved to `~/.deepseek/config.toml` so it works from any directory without OS credential prompts.

You can also set it ahead of time:

```bash
deepseek auth set --provider deepseek   # saves to ~/.deepseek/config.toml

export DEEPSEEK_API_KEY="YOUR_KEY"      # env var alternative; use ~/.zshenv for non-interactive shells
deepseek

deepseek doctor                         # verify setup
```

> To rotate or remove a saved key: `deepseek auth clear --provider deepseek`.

### Auto Model Router

Use `deepseek --model auto` or `/model auto` when you want DeepSeek TUI to choose both the model and thinking level for each turn.

`auto` is local routing state. Before the real turn is sent, DeepSeek TUI makes a small `deepseek-v4-flash` request with thinking off and asks it to choose one concrete route:

| Route | Typical use |
|---|---|
| `deepseek-v4-flash` + `off` | Short answers, simple edits, quick summaries |
| `deepseek-v4-flash` + `high` | Normal coding and debugging |
| `deepseek-v4-flash` + `max` | Harder work where Flash is likely enough |
| `deepseek-v4-pro` + `off` | Pro model without reasoning overhead |
| `deepseek-v4-pro` + `high` | Complex multi-file work |
| `deepseek-v4-pro` + `max` | Ambiguous bugs, architecture, release work, security review |

The upstream API never receives `model: "auto"`. It receives the concrete model selected for that turn. If the router call fails or returns an invalid route, the app falls back to a local heuristic. Sub-agents inherit auto routing unless you assign them an explicit model. Cost tracking is attributed to the concrete model that actually ran.

### Linux ARM64 (Raspberry Pi, Asahi, Graviton, HarmonyOS PC)

`npm i -g deepseek-tui` works on glibc-based ARM64 Linux from v0.8.8 onward. You can also download prebuilt binaries from the [Releases page](https://github.com/Hmbown/DeepSeek-TUI/releases) and place them side by side on your `PATH`.

### China / Mirror-friendly Installation

If GitHub or npm downloads are slow from mainland China, use a Cargo registry mirror:

```toml
# ~/.cargo/config.toml
[source.crates-io]
replace-with = "tuna"

[source.tuna]
registry = "sparse+https://mirrors.tuna.tsinghua.edu.cn/crates.io-index/"
```

Then install both binaries (the dispatcher delegates to the TUI at runtime):

```bash
cargo install deepseek-tui-cli --locked   # provides `deepseek`
cargo install deepseek-tui     --locked   # provides `deepseek-tui`
deepseek --version
```

Prebuilt binaries can also be downloaded from [GitHub Releases](https://github.com/Hmbown/DeepSeek-TUI/releases). Use `DEEPSEEK_TUI_RELEASE_BASE_URL` for mirrored release assets.

### Windows (Scoop)

[Scoop](https://scoop.sh) is a Windows package manager. Once installed, run:

```bash
scoop install deepseek-tui
```


<details id="install-from-source">
<summary>Install from source</summary>

Works on any Tier-1 Rust target — including musl, riscv64, FreeBSD, and older ARM64 distros.

```bash
# Linux build deps (Debian/Ubuntu/RHEL):
#   sudo apt-get install -y build-essential pkg-config libdbus-1-dev
#   sudo dnf install -y gcc make pkgconf-pkg-config dbus-devel

git clone https://github.com/Hmbown/DeepSeek-TUI.git
cd DeepSeek-TUI

cargo install --path crates/cli --locked   # requires Rust 1.88+; provides `deepseek`
cargo install --path crates/tui --locked   # provides `deepseek-tui`
```

Both binaries are required. Cross-compilation and platform-specific notes: [docs/INSTALL.md](docs/INSTALL.md).

</details>

### Other API Providers

```bash
# NVIDIA NIM
deepseek auth set --provider nvidia-nim --api-key "YOUR_NVIDIA_API_KEY"
deepseek --provider nvidia-nim

# Fireworks
deepseek auth set --provider fireworks --api-key "YOUR_FIREWORKS_API_KEY"
deepseek --provider fireworks --model deepseek-v4-pro

# Self-hosted SGLang
SGLANG_BASE_URL="http://localhost:30000/v1" deepseek --provider sglang --model deepseek-v4-flash

# Self-hosted vLLM
VLLM_BASE_URL="http://localhost:8000/v1" deepseek --provider vllm --model deepseek-v4-flash
```

---

## What's New In v0.8.14

A stabilization release focused on first-run setup, auto model routing, cost accounting, and provider support. [Full changelog](CHANGELOG.md).

- **Auto model routing restored** — `--model auto`, `/model auto`, config `default_model = "auto"`, one-shot prompts, and sub-agents resolve to concrete Flash/Pro routes before calling the API
- **Per-turn cost accounting fix** — V4 reasoning tokens are counted as billable output when providers report them separately from completion tokens
- **First-run setup repair** — missing config files now lead users through API key setup and create `~/.deepseek/config.toml`
- **Settings navigation fix** — arrow-key selection and click highlighting in the config UI work reliably on Windows terminals
- **vLLM provider support** — self-hosted vLLM endpoints can be used with `--provider vllm` and `VLLM_BASE_URL`

---

## Usage

```bash
deepseek                                         # interactive TUI
deepseek "explain this function"                 # one-shot prompt
deepseek --model deepseek-v4-flash "summarize"   # model override
deepseek --model auto "fix this bug"             # route model + thinking per turn
deepseek --yolo                                  # auto-approve tools
deepseek auth set --provider deepseek            # save API key
deepseek doctor                                  # check setup & connectivity
deepseek doctor --json                           # machine-readable diagnostics
deepseek setup --status                          # read-only setup status
deepseek setup --tools --plugins                 # scaffold tool/plugin dirs
deepseek models                                  # list live API models
deepseek sessions                                # list saved sessions
deepseek resume --last                           # resume the most recent session
deepseek resume <SESSION_ID>                     # resume a specific session by UUID
deepseek fork <SESSION_ID>                       # fork a session at a chosen turn
deepseek serve --http                            # HTTP/SSE API server
deepseek pr <N>                                  # fetch PR and pre-seed review prompt
deepseek mcp list                                # list configured MCP servers
deepseek mcp validate                            # validate MCP config/connectivity
deepseek mcp-server                              # run dispatcher MCP stdio server
```

### Keyboard Shortcuts

| Key | Action |
|---|---|
| `Tab` | Complete `/` or `@` entries; while running, queue draft as follow-up; otherwise cycle mode |
| `Shift+Tab` | Cycle reasoning-effort: off → high → max |
| `F1` | Searchable help overlay |
| `Esc` | Back / dismiss |
| `Ctrl+K` | Command palette |
| `Ctrl+R` | Resume an earlier session |
| `Alt+R` | Search prompt history and recover cleared drafts |
| `Ctrl+S` | Stash current draft (`/stash list`, `/stash pop` to recover) |
| `@path` | Attach file/directory context in composer |
| `↑` (at composer start) | Select attachment row for removal |

Full shortcut catalog: [docs/KEYBINDINGS.md](docs/KEYBINDINGS.md).

---

## Modes

| Mode | Behavior |
| --- | --- |
| **Plan** 🔍 | Read-only investigation — model explores and proposes a plan (`update_plan` + `checklist_write`) before making changes |
| **Agent** 🤖 | Default interactive mode — multi-step tool use with approval gates; model outlines work via `checklist_write` |
| **YOLO** ⚡ | Auto-approve all tools in a trusted workspace; still maintains plan and checklist for visibility |

---

## Configuration

User config: `~/.deepseek/config.toml`. Project overlay: `<workspace>/.deepseek/config.toml` (denied: `api_key`, `base_url`, `provider`, `mcp_config_path`). [config.example.toml](config.example.toml) has every option.

Key environment variables:

| Variable | Purpose |
|---|---|
| `DEEPSEEK_API_KEY` | API key |
| `DEEPSEEK_BASE_URL` | API base URL |
| `DEEPSEEK_MODEL` | Default model |
| `DEEPSEEK_PROVIDER` | `deepseek` (default), `nvidia-nim`, `fireworks`, `sglang`, `vllm` |
| `DEEPSEEK_PROFILE` | Config profile name |
| `DEEPSEEK_MEMORY` | Set to `on` to enable user memory |
| `NVIDIA_API_KEY` / `FIREWORKS_API_KEY` / `SGLANG_API_KEY` / `VLLM_API_KEY` | Provider auth |
| `SGLANG_BASE_URL` | Self-hosted SGLang endpoint |
| `VLLM_BASE_URL` | Self-hosted vLLM endpoint |
| `NO_ANIMATIONS=1` | Force accessibility mode at startup |
| `SSL_CERT_FILE` | Custom CA bundle for corporate proxies |

UI locale is separate from model language — set `locale` in `settings.toml`, use `/config locale zh-Hans`, or rely on `LC_ALL`/`LANG`. See [docs/CONFIGURATION.md](docs/CONFIGURATION.md) and [docs/MCP.md](docs/MCP.md).

---

## Models & Pricing

| Model | Context | Input (cache hit) | Input (cache miss) | Output |
|---|---|---|---|---|
| `deepseek-v4-pro` | 1M | $0.003625 / 1M* | $0.435 / 1M* | $0.87 / 1M* |
| `deepseek-v4-flash` | 1M | $0.0028 / 1M | $0.14 / 1M | $0.28 / 1M |

Legacy aliases `deepseek-chat` / `deepseek-reasoner` map to `deepseek-v4-flash`. NVIDIA NIM variants use your NVIDIA account terms.

*DeepSeek Pro rates currently reflect a limited-time 75% discount, which remains valid until 15:59 UTC on 31 May 2026. After that time, the TUI cost estimator will revert to the base Pro rates.*

---

## Publishing Your Own Skill

DeepSeek TUI discovers skills from workspace directories (`.agents/skills` → `skills` → `.opencode/skills` → `.claude/skills`) and the global `~/.deepseek/skills`. Each skill is a directory with a `SKILL.md` file:

```text
~/.deepseek/skills/my-skill/
└── SKILL.md
```

Frontmatter required:

```markdown
---
name: my-skill
description: Use this when DeepSeek should follow my custom workflow.
---

# My Skill
Instructions for the agent go here.
```

Commands: `/skills` (list), `/skill <name>` (activate), `/skill new` (scaffold), `/skill install github:<owner>/<repo>` (community), `/skill update` / `uninstall` / `trust`. Community installs from GitHub require no backend service. Installed skills appear in the model-visible session context; the agent can auto-select relevant skills via the `load_skill` tool when your task matches their descriptions.

---

## Documentation

| Doc | Topic |
|---|---|
| [ARCHITECTURE.md](docs/ARCHITECTURE.md) | Codebase internals |
| [CONFIGURATION.md](docs/CONFIGURATION.md) | Full config reference |
| [MODES.md](docs/MODES.md) | Plan / Agent / YOLO modes |
| [MCP.md](docs/MCP.md) | Model Context Protocol integration |
| [RUNTIME_API.md](docs/RUNTIME_API.md) | HTTP/SSE API server |
| [INSTALL.md](docs/INSTALL.md) | Platform-specific install guide |
| [MEMORY.md](docs/MEMORY.md) | User memory feature guide |
| [SUBAGENTS.md](docs/SUBAGENTS.md) | Sub-agent role taxonomy and lifecycle |
| [KEYBINDINGS.md](docs/KEYBINDINGS.md) | Full shortcut catalog |
| [RELEASE_RUNBOOK.md](docs/RELEASE_RUNBOOK.md) | Release process |
| [LOCALIZATION.md](docs/LOCALIZATION.md) | UI locale matrix & switching |
| [OPERATIONS_RUNBOOK.md](docs/OPERATIONS_RUNBOOK.md) | Ops & recovery |

Full Changelog: [CHANGELOG.md](CHANGELOG.md).

---

## Thanks

This project ships with help from a growing community of contributors:

- **[merchloubna70-dot](https://github.com/merchloubna70-dot)** — 28 PRs spanning features, fixes, and VS Code extension scaffolding (#645–#681)
- **[WyxBUPT-22](https://github.com/WyxBUPT-22)** — Markdown rendering for tables, bold/italic, and horizontal rules (#579)
- **[loongmiaow-pixel](https://github.com/loongmiaow-pixel)** — Windows + China install documentation (#578)
- **[20bytes](https://github.com/20bytes)** — User memory docs and help polish (#569)
- **[staryxchen](https://github.com/staryxchen)** — glibc compatibility preflight (#556)
- **[Vishnu1837](https://github.com/Vishnu1837)** — glibc compatibility improvements (#565)
- **[shentoumengxin](https://github.com/shentoumengxin)** — Shell `cwd` boundary validation (#524)
- **[toi500](https://github.com/toi500)** — Windows paste fix report
- **[xsstomy](https://github.com/xsstomy)** — Terminal startup repaint report
- **[melody0709](https://github.com/melody0709)** — Slash-prefix Enter activation report
- **[lloydzhou](https://github.com/lloydzhou)** and **[jeoor](https://github.com/jeoor)** — Compaction cost reports
- **[Agent-Skill-007](https://github.com/Agent-Skill-007)** — README clarity pass (#685)
- **[woyxiang](https://github.com/woyxiang)** — Windows Scoop install docs (#696)
- **[wangfeng](mailto:wangfengcsu@qq.com)** — Pricing/discount info update (#692)
- **[zichen0116](https://github.com/zichen0116)** — CODE_OF_CONDUCT.md (#686)
- **[dfwqdyl-ui](https://github.com/dfwqdyl-ui)** — model ID case-sensitivity compatibility report (#729)
- **[Oliver-ZPLiu](https://github.com/Oliver-ZPLiu)** — stale `working...` state bug report with detailed reproduction and fix (#738)
- **Hafeez Pizofreude** — SSRF protection in `fetch_url` and Star History chart
- **Unic (YuniqueUnic)** — Schema-driven config UI (TUI + web)
- **Jason** — SSRF security hardening

---

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md). Pull requests welcome — check the [open issues](https://github.com/Hmbown/DeepSeek-TUI/issues) for good first contributions.

Support: [Buy me a coffee](https://www.buymeacoffee.com/hmbown).

> [!Note]
> *Not affiliated with DeepSeek Inc.*

## License

[MIT](LICENSE)

## Star History

[![Star History Chart](https://api.star-history.com/chart?repos=Hmbown/DeepSeek-TUI&type=date&legend=top-left)](https://www.star-history.com/?repos=Hmbown%2FDeepSeek-TUI&type=date&logscale=&legend=top-left)
