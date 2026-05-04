# DeepSeek TUI

> **A terminal-native coding agent built around DeepSeek V4's 1M-token context and prefix cache. Single binary, no Node/Python runtime required — ships an MCP client, sandbox, and durable task queue out of the box.**

[简体中文 README](README.zh-CN.md)

```bash
npm i -g deepseek-tui
```

[![CI](https://github.com/Hmbown/DeepSeek-TUI/actions/workflows/ci.yml/badge.svg)](https://github.com/Hmbown/DeepSeek-TUI/actions/workflows/ci.yml)
[![npm](https://img.shields.io/npm/v/deepseek-tui)](https://www.npmjs.com/package/deepseek-tui)
[![crates.io](https://img.shields.io/crates/v/deepseek-tui-cli?label=crates.io)](https://crates.io/crates/deepseek-tui-cli)

<a href="https://www.buymeacoffee.com/hmbown" target="_blank"><img src="https://img.shields.io/badge/Buy%20me%20a%20coffee-5F7FFF?style=for-the-badge&logo=buymeacoffee&logoColor=white" alt="Buy me a coffee" /></a>

![DeepSeek TUI screenshot](assets/screenshot.png)

---

## What is it?

DeepSeek TUI is a coding agent that runs entirely in your terminal. It gives DeepSeek's frontier models direct access to your workspace — reading and editing files, running shell commands, searching the web, managing git, and orchestrating sub-agents — all through a fast, keyboard-driven TUI.

**Built for DeepSeek V4** (`deepseek-v4-pro` / `deepseek-v4-flash`) with 1M-token context windows and native thinking-mode (chain-of-thought) streaming.

### Key Features

- **Native RLM** (`rlm_query`) — fans out 1–16 cheap `deepseek-v4-flash` children in parallel for batched analysis and parallel reasoning, all against the existing API client
- **Thinking-mode streaming** — watch the model's chain-of-thought unfold in real time as it works through your tasks
- **Full tool suite** — file ops, shell execution, git, web search/browse, apply-patch, sub-agents, MCP servers
- **1M-token context** — automatic intelligent compaction when context fills up; prefix-cache aware for cost efficiency
- **Three modes** — Plan (read-only explore), Agent (interactive with approval), YOLO (auto-approved)
- **Reasoning-effort tiers** — cycle through `off → high → max` with `Shift+Tab`
- **Session save/resume** — checkpoint and resume long-running sessions
- **Workspace rollback** — side-git pre/post-turn snapshots with `/restore` and `revert_turn`, without touching your repo's `.git`
- **Durable task queue** — background tasks survive restarts; think scheduled automation, long-running reviews
- **HTTP/SSE runtime API** — `deepseek serve --http` for headless agent workflows
- **MCP protocol** — connect to Model Context Protocol servers for extended tooling; see [docs/MCP.md](docs/MCP.md)
- **LSP diagnostics** — inline error/warning surfacing after every edit via rust-analyzer, pyright, typescript-language-server, gopls, clangd
- **User memory** — optional persistent note file injected into the system prompt for cross-session preferences
- **Localized UI** — `en`, `ja`, `zh-Hans`, `pt-BR` with auto-detection
- **Live cost tracking** — per-turn and session-level token usage and cost estimates; cache hit/miss breakdown
- **Skills system** — composable, installable instruction packs from GitHub with no backend service required

---

## How it's wired

`deepseek` (dispatcher CLI) → `deepseek-tui` (companion binary) → ratatui interface ↔ async engine ↔ OpenAI-compatible streaming client. Tool calls route through a typed registry (shell, file ops, git, web, sub-agents, MCP, RLM) and results stream back into the transcript. The engine manages session state, turn tracking, the durable task queue, and an LSP subsystem that feeds post-edit diagnostics into the model's context before the next reasoning step.

See [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) for the full walkthrough.

---

## Quickstart

```bash
npm install -g deepseek-tui
deepseek
```

Prebuilt binaries are published for **Linux x64**, **Linux ARM64** (v0.8.8+), **macOS x64**, **macOS ARM64**, and **Windows x64**. For other targets (musl, riscv64, FreeBSD, etc.), see [Install from source](#install-from-source) or [docs/INSTALL.md](docs/INSTALL.md).

On first launch you'll be prompted for your [DeepSeek API key](https://platform.deepseek.com/api_keys). The key is saved to `~/.deepseek/config.toml` so it works from any directory without OS credential prompts.

You can also set it ahead of time:

```bash
deepseek auth set --provider deepseek   # saves to ~/.deepseek/config.toml

export DEEPSEEK_API_KEY="YOUR_KEY"      # env var alternative; use ~/.zshenv for non-interactive shells
deepseek

deepseek doctor                          # verify setup
```

> To rotate or remove a saved key: `deepseek auth clear --provider deepseek`.

### Linux ARM64 (Raspberry Pi, Asahi, Graviton, HarmonyOS PC)

`npm i -g deepseek-tui` works on glibc-based ARM64 Linux from v0.8.8 onward. You can also download prebuilt binaries from the [Releases page](https://github.com/Hmbown/DeepSeek-TUI/releases) and place them side by side on your `PATH`.

### China / mirror-friendly install

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

<details id="install-from-source">
<summary>Install from source</summary>

Works on any Tier-1 Rust target — including musl, riscv64, FreeBSD, and older ARM64 distros.

```bash
# Linux build deps (Debian/Ubuntu/RHEL):
#   sudo apt-get install -y build-essential pkg-config libdbus-1-dev
#   sudo dnf install -y gcc make pkgconf-pkg-config dbus-devel

git clone https://github.com/Hmbown/DeepSeek-TUI.git
cd DeepSeek-TUI

cargo install --path crates/cli --locked   # requires Rust 1.85+; provides `deepseek`
cargo install --path crates/tui --locked   # provides `deepseek-tui`
```

Both binaries are required. Cross-compilation and platform-specific notes: [docs/INSTALL.md](docs/INSTALL.md).

</details>

### Other API providers

```bash
# NVIDIA NIM
deepseek auth set --provider nvidia-nim --api-key "YOUR_NVIDIA_API_KEY"
deepseek --provider nvidia-nim

# Fireworks
deepseek auth set --provider fireworks --api-key "YOUR_FIREWORKS_API_KEY"
deepseek --provider fireworks --model deepseek-v4-pro

# Self-hosted SGLang
SGLANG_BASE_URL="http://localhost:30000/v1" deepseek --provider sglang --model deepseek-v4-flash
```

---

## What's new in v0.8.10

A patch release: hotfixes, UX polish, and runtime API additions for the whalescale desktop integration. No breaking changes. [Full changelog](CHANGELOG.md).

- **Stacked toast overlay** — status toasts queue and render together instead of overwriting each other
- **File @-mention frecency** — file mention suggestions learn from recent selections (`~/.deepseek/file-frecency.jsonl`)
- **Runtime API expansion** — CORS origins config, full thread editing (`PATCH /v1/threads/{id}`), `archived_only` query filter, aggregate usage endpoint (`GET /v1/usage?group_by=day|model|provider|thread`)
- **Language picker on first run** — new onboarding step selects locale before entering the API key
- **OPENCODE shell.env hook** — lifecycle hooks can inject shell environment into spawned commands
- **Cache-aware compaction** — compaction calls reuse cached prompt prefixes, cutting `/compact` costs significantly
- **glibc 2.28 baseline** — prebuilts now target glibc 2.28 (via `cargo zigbuild`), covering older distros; npm postinstall fails fast with a clear source-build message when incompatible
- **Better markdown rendering** — transcript now handles tables, bold/italic, and horizontal rules; no more infinite loops on unclosed markers
- **MCP SIGTERM on shutdown** — stdio servers receive SIGTERM with a 2-second grace period instead of SIGKILL
- **Shell-child PDEATHSIG on Linux** — children auto-SIGTERM when the parent exits, closing a leak window
- **Windows Terminal paste fix** — Ctrl/Cmd+V during onboarding now works correctly
- **Terminal startup repaint** — no more stale background rows above the first frame
- **Slash-prefix Enter activation** — typing `/mo` and pressing Enter activates the first match
- **Shell `cwd` boundary validation** — path escape returns `PathEscape` on out-of-workspace `cwd`, consistent with file tools

**6 first-time contributors:** [@staryxchen](https://github.com/staryxchen) (#556), [@shentoumengxin](https://github.com/shentoumengxin) (#524), [@Vishnu1837](https://github.com/Vishnu1837) (#565), [@20bytes](https://github.com/20bytes) (#569), [@loongmiaow-pixel](https://github.com/loongmiaow-pixel) (#578), [@WyxBUPT-22](https://github.com/WyxBUPT-22) (#579).
Thanks also to [@lloydzhou](https://github.com/lloydzhou), [@jeoor](https://github.com/jeoor), [@toi500](https://github.com/toi500), [@xsstomy](https://github.com/xsstomy), and [@melody0709](https://github.com/melody0709) for bug reports.

---

## Usage

```bash
deepseek                                       # interactive TUI
deepseek "explain this function"              # one-shot prompt
deepseek -p "agentic non-interactive mode"    # Claude Code CLI-compatible -p flag
deepseek --model deepseek-v4-flash "summarize" # model override
deepseek --yolo                                # auto-approve tools
deepseek auth set --provider deepseek         # save API key
deepseek doctor                                # check setup & connectivity
deepseek doctor --json                         # machine-readable diagnostics
deepseek setup --status                        # read-only setup status
deepseek setup --tools --plugins               # scaffold tool/plugin dirs
deepseek models                                # list live API models
deepseek sessions                              # list saved sessions
deepseek resume --last                         # resume latest session
deepseek serve --http                          # HTTP/SSE API server
deepseek pr <N>                                # fetch PR and pre-seed review prompt
deepseek mcp list                              # list configured MCP servers
deepseek mcp validate                          # validate MCP config/connectivity
deepseek mcp-server                            # run dispatcher MCP stdio server
```

### Keyboard shortcuts

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
| `Alt+↑` | Edit last queued message |

Full shortcut catalog: [docs/KEYBINDINGS.md](docs/KEYBINDINGS.md).

---

## Non-interactive / CI mode

Run `deepseek -p "prompt"` for Claude Code CLI-compatible non-interactive agent mode.
The model runs with tool access, auto-approves tool calls, prints progress to stderr,
and writes the final answer to stdout.

```bash
deepseek -p "explain this codebase"                     # basic usage
deepseek -p --model deepseek-v4-flash "summarize"       # model override
deepseek -p "find all unused dependencies" --add-dir ./src  # add dir context
deepseek -p "fix the bug" --dangerously-skip-permissions     # auto-approve all tools
deepseek -p --no-session-persistence "list files"       # stateless (no history saved)
echo "translate to french: hello" | deepseek -p         # read prompt from stdin
deepseek -p "list files" --output-format json           # JSON output envelope
deepseek -p "list files" --output-format stream-json    # streaming JSON events
```

### Output formats

- **text** (default): Final assistant message printed as plain text to stdout.
  Tool progress, errors, and debug info go to stderr.
- **json**: Print a single JSON envelope at the end:
  ```json
  {"type":"result","subtype":"success","result":"...","cost_usd":0.0}
  ```
- **stream-json**: Stream each event as newline-delimited JSON to stdout:
  ```
  {"type":"message","content":"Analyzing..."}
  {"type":"tool_start","name":"grep_files","input":"pattern"}
  {"type":"tool_end","name":"grep_files","success":true,"output":"..."}
  {"type":"turn_complete","status":"completed","error":null}
  {"type":"result","subtype":"success","result":"...","cost_usd":0.0}
  ```

### Exit codes

| Code | Meaning |
|---|---|
| 0 | Success — agent completed the task |
| 1 | Error — agent failed or API error |
| 2 | Interrupted — timeout or signal |

### Piped / stdin usage

When no prompt argument is given after `-p`, the prompt is read from stdin:

```bash
echo "Explain this Makefile" | deepseek -p
deepseek -p < /tmp/prompt.txt
```

### CI integration example

```bash
# GitHub Actions / CI pipeline
deepseek -p "Review the diff and report any bugs" \
  --output-format json \
  --dangerously-skip-permissions \
  --add-dir ./src \
  --no-session-persistence
```

---

## Modes

| Mode | Behavior |
|---|---|
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
| `DEEPSEEK_PROVIDER` | `deepseek` (default), `nvidia-nim`, `fireworks`, `sglang` |
| `DEEPSEEK_PROFILE` | Config profile name |
| `DEEPSEEK_MEMORY` | Set to `on` to enable user memory |
| `NVIDIA_API_KEY` / `FIREWORKS_API_KEY` / `SGLANG_API_KEY` | Provider auth |
| `SGLANG_BASE_URL` | Self-hosted SGLang endpoint |
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

*\*DeepSeek Pro rates are a limited-time 75% discount valid until 2026-05-05 15:59 UTC; the TUI cost estimator falls back to base Pro rates after that timestamp.*

---

## Publishing your own skill

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
| [OPERATIONS_RUNBOOK.md](docs/OPERATIONS_RUNBOOK.md) | Ops & recovery |

Full changelog: [CHANGELOG.md](CHANGELOG.md).

---

## Thanks

Earlier releases shipped with help from these contributors:

- **Hafeez Pizofreude** — SSRF protection in `fetch_url` and Star History chart
- **Unic (YuniqueUnic)** — Schema-driven config UI (TUI + web)
- **Jason** — SSRF security hardening

---

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md). Pull requests welcome — check the [open issues](https://github.com/Hmbown/DeepSeek-TUI/issues) for good first contributions.

*Not affiliated with DeepSeek Inc.*

## License

[MIT](LICENSE)

## Star History

[![Star History Chart](https://api.star-history.com/chart?repos=Hmbown/DeepSeek-TUI&type=date&legend=top-left)](https://www.star-history.com/?repos=Hmbown%2FDeepSeek-TUI&type=date&logscale=&legend=top-left)
