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

**Built for DeepSeek V4** (`deepseek-v4-pro` / `deepseek-v4-flash`) with 1M-token context windows and native thinking-mode (chain-of-thought) streaming. See the model's reasoning unfold in real time as it works through your tasks.

### Key Features

- **Native RLM** (`rlm_query` tool) — fans out 1–16 cheap `deepseek-v4-flash` children in parallel against the existing DeepSeek client for batched analysis, decomposition, or parallel reasoning
- **Thinking-mode streaming** — shows DeepSeek's chain-of-thought as it reasons about your code
- **Full tool suite** — file ops, shell execution, git, web search/browse, apply-patch, sub-agents, MCP servers
- **1M-token context** — automatic intelligent compaction when context fills up
- **Three interaction modes** — Plan (read-only explore), Agent (interactive with approval), YOLO (auto-approved). Decomposition-first system prompts teach the model to `checklist_write`, `update_plan`, and spawn sub-agents before acting
- **Reasoning-effort tiers** — cycle through `off → high → max` with Shift+Tab
- **Session save/resume** — checkpoint and resume long sessions
- **Workspace rollback** — side-git pre/post-turn snapshots with `/restore` and `revert_turn`, without touching your repo's `.git`
- **HTTP/SSE runtime API** — `deepseek serve --http` for headless agent workflows
- **MCP protocol** — connect to Model Context Protocol servers for extended tooling; see [docs/MCP.md](docs/MCP.md)
- **Live cost tracking** — per-turn and session-level token usage and cost estimates
- **Dark theme** — DeepSeek-blue palette

---

## How it's wired

DeepSeek TUI's architecture follows a **dispatcher → TUI → engine → tools** pattern.
The `deepseek` CLI binary is a lightweight dispatcher that parses subcommands and
delegates to the `deepseek-tui` companion binary for interactive sessions. The TUI
runs a **ratatui**-based interface that communicates with an async engine executing
an agent loop: user input flows to the LLM via a streaming client (OpenAI-compatible
Chat Completions), tool calls are extracted from the response and dispatched through
a typed tool registry (shell, file ops, git, web, sub-agents, MCP), and results
stream back into the transcript.

Behind the scenes, the engine manages session state, turn tracking, and a durable
task queue. The LSP subsystem (`crates/tui/src/lsp/`) provides post-edit diagnostics
by spawning language servers (rust-analyzer, pyright, etc.) and injecting errors
into the model's context before the next reasoning step. A recursive language model
(RLM) subsystem gives the agent a sandboxed Python REPL for batch classification
and sub-LLM orchestration. See [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) for
the full walkthrough.

---

## Quickstart

```bash
npm install -g deepseek-tui
deepseek
```

### China / mirror-friendly install

If GitHub or npm downloads are slow from mainland China, install the Rust
crates through a Cargo registry mirror:

```toml
# ~/.cargo/config.toml
[source.crates-io]
replace-with = "tuna"

[source.tuna]
registry = "sparse+https://mirrors.tuna.tsinghua.edu.cn/crates.io-index/"
```

Then install the canonical `deepseek` dispatcher and (optionally) the
companion TUI binary:

```bash
cargo install deepseek-tui-cli --locked   # provides `deepseek`
cargo install deepseek-tui     --locked   # provides `deepseek-tui` (optional)
deepseek --version
```

You can also download prebuilt binaries directly from the
[GitHub Releases](https://github.com/Hmbown/DeepSeek-TUI/releases) page when
GitHub release assets are reachable. TUNA, rsproxy, Tencent COS, or Aliyun OSS
mirrors can also be used with `DEEPSEEK_TUI_RELEASE_BASE_URL` when a mirrored
release-asset directory is available.

On first launch you'll be prompted for your [DeepSeek API key](https://platform.deepseek.com/api_keys). You can also set it ahead of time:

```bash
# via CLI
deepseek login --api-key "YOUR_DEEPSEEK_API_KEY"

# via env var
export DEEPSEEK_API_KEY="YOUR_DEEPSEEK_API_KEY"
deepseek
```

### Using NVIDIA NIM

```bash
deepseek auth set --provider nvidia-nim --api-key "YOUR_NVIDIA_API_KEY"
deepseek --provider nvidia-nim

# or per-process:
DEEPSEEK_PROVIDER=nvidia-nim NVIDIA_API_KEY="..." deepseek
```

### Other DeepSeek V4 providers

```bash
deepseek auth set --provider fireworks --api-key "YOUR_FIREWORKS_API_KEY"
deepseek --provider fireworks --model deepseek-v4-pro

# SGLang is self-hosted; auth is optional for localhost deployments.
SGLANG_BASE_URL="http://localhost:30000/v1" deepseek --provider sglang --model deepseek-v4-flash
```

<details>
<summary>Install from source</summary>

```bash
git clone https://github.com/Hmbown/DeepSeek-TUI.git
cd DeepSeek-TUI
cargo install --path crates/tui --locked   # requires Rust 1.85+
```

</details>

---

## What's new in v0.8.7

Quick patch on top of v0.8.6 to unblock copy/select.

### ✂️ Selection works across the whole transcript

The selection-tightening from v0.8.6 restricted copy/select to user and
assistant message bodies, which made it impossible to copy text out of
system notes, thinking blocks, or tool output. v0.8.7 drops that gate so
the rendered transcript block is selectable end-to-end again.

Full changelog: [CHANGELOG.md](CHANGELOG.md).

## What's new in v0.8.6

### 📝 AGENTS.md bootstrap (`/init`)

`/init` walks the workspace, auto-detects the project type (Cargo.toml,
package.json, pyproject.toml, etc.), and writes a starter `AGENTS.md` with
build/test commands, workspace layout, and conventions derived from `git log`.
Re-running shows a diff of the proposed update without overwriting changes.

### 🔍 Inline LSP diagnostics

After every `apply_patch`/`edit_file`/`write_file`, the engine sends a
`textDocument/didChange` to the LSP server and surfaces errors/warnings
inline in the tool result. Configurable via `/lsp on|off` and the
`[lsp]` config section. Currently supports rust-analyzer, pyright,
typescript-language-server, gopls, and clangd.

### 🔄 Self-update (`deepseek update`)

`deepseek update` fetches the latest GitHub release, downloads the
platform-correct binary with SHA256 verification, and atomically replaces
the running binary. No more remembering `cargo install` or `npm install -g`.

### 🌐 Session sharing (`/share`)

`/share` exports the current session as a static HTML page and uploads it
to a GitHub Gist via the `gh` CLI, producing a clickable URL you can paste
anywhere.

### 📖 Docs refresh

README hero updated with intent statement and architecture summary.
ARCHITECTURE.md cleaned up for v0.8.6 (swarm references removed, current
crate map). CONTRIBUTING.md now has a "shape of a PR" section.

Full changelog: [CHANGELOG.md](CHANGELOG.md).

---

## What's new in v0.8.5

### 🛡️ SSRF protection for fetch_url

`fetch_url` now validates target hostnames and IPs before connecting —
localhost-only HTTP for loopback, DNS pinning for remote hosts, and
blocked internal IP ranges. Contributed by Hafeez Pizofreude (#261)
and Jason.

### 🖥️ Schema-driven config editor

`/config tui` opens a forms-style config editor powered by schemaui.
Bare `/config` opens the legacy native modal; `/config web` launches a
browser surface (requires the `web` feature). Contributed by Unic
(YuniqueUnic) via #365.

### 🏷️ DeepseekCN provider

`ApiProvider::DeepseekCN` targets `api.deepseeki.com` for China-based
users. Auto-detects when `zh-*` is the system locale on first run.

### 🔐 Atomic file writes

All writes to `~/.deepseek/` now go through `write_atomic` (tempfile +
fsync + rename), preventing corruption from mid-write crashes.

### 🧵 Panic safety foundations

`spawn_supervised` catches and logs task panics with crash dumps instead
of silently dropping the task.

### ⌨️ `/config <key> <value>` wiring

`/config model deepseek-v4-flash`, `/config locale zh-Hans`, etc. change
settings live in-session without opening the editor.

Full changelog: [CHANGELOG.md](CHANGELOG.md).

---

## Thanks

v0.8.5 shipped with help from these contributors:

- **[Hafeez Pizofreude](https://github.com/pizofreude)** — SSRF protection in `fetch_url` and Star History chart
- **[Unic (YuniqueUnic)](https://github.com/YuniqueUnic)** — Schema-driven config UI (TUI + web)
- **[Jason](mailto:jason@aveoresearchlabs.com)** — SSRF security hardening

---

## What's new in v0.8.0

### ⚡ Shell stability and post-send responsiveness

Completed background shell jobs now release their live process and pipe
handles as soon as completion is observed, while keeping the job record
inspectable. This prevents long-running sessions from hitting `Too many
open files (os error 24)`, which could make checkpoint saves fail and
cause shell spawning, message send, close, and Esc/cancel paths to lag
or fail.

### 🪟 Windows REPL runtime CI hardening

Windows gets a longer Python bootstrap readiness timeout for the REPL
runtime tests, matching GitHub runner startup contention without
weakening bootstrap failures on other platforms.

### 🌏 Cargo mirror install docs

The README now includes a TUNA Cargo mirror setup and direct release
asset guidance for users with slow GitHub/npm access.

### 🧪 Test hardening

New regression coverage proves completed background shell jobs drop
their live process handles after `exec_shell_wait`.

Full changelog: [CHANGELOG.md](CHANGELOG.md).

---

## What's new in v0.7.8

### ⚡ Shell controls: foreground-to-background detach + `exec_shell_cancel`

A running foreground command can now be moved to the background interactive
session — press **`Ctrl+B`** while a command is executing to open shell
controls, then either detach it (it continues running and can be polled
with `exec_shell_wait`) or cancel the current turn.

**New tool: `exec_shell_cancel`** — cancel a specific background shell
task by `task_id`, or cancel all running background tasks with `all: true`.

**Cancel-aware `exec_shell_wait`** — canceling a turn while
`exec_shell_wait` is blocking now stops the wait but leaves the background
task running.

### 🐛 Unicode glob search fix

Filenames containing multi-byte characters (e.g., `dialogue_line__冰糖.mp3`)
no longer panic the `matches_glob` function — byte-index slicing was replaced
with `char_indices()` boundary-safe iteration.

### 🔄 Swarm UI reconciliation

The fanout card no longer pre-seeds with zero-state workers, eliminating the
"0 done · 0 running · 0 failed · N pending" vs sidebar "N running"
contradiction. The sidebar now shows "dispatching N" before the first progress
event arrives from a `agent_swarm` invocation.

Full changelog: [CHANGELOG.md](CHANGELOG.md).

---

## What's new in v0.7.6

### 🌐 UI Localization

DeepSeek TUI now speaks your language. The new `locale` setting
in `settings.toml` controls UI chrome — composer, history search,
`/config`, help overlay, and status hints — without changing model
output language.

| Setting | Display |
|---|---|
| `locale = \"auto\"` | Checks `LC_ALL` → `LC_MESSAGES` → `LANG` (default) |
| `locale = \"ja\"` | Japanese |
| `locale = \"zh-Hans\"` | Chinese Simplified |
| `locale = \"pt-BR\"` | Portuguese (Brazil) |
| `locale = \"en\"` | English fallback |

Unsure what to pick? Run `locale` in your terminal; the first matching
tag is used automatically.

### 📋 Smarter paste handling

Paste-burst detection catches rapid-key pastes in terminals that don't
send bracketed-paste events — CRLF is normalized, and multiline pastes
stay buffered until you stop typing. Configurable via `paste_burst_detection`.

### 🔍 Composer history search

Forgot that prompt you wrote an hour ago? `Alt+R` opens a live search
across input history and recovered drafts. Type to filter, `Enter` to
accept, `Esc` to restore what you were typing.

### 👁️ Pending input preview

During a running turn, queued messages, pending steers, and context chips
appear above the composer so you can see what will be sent next.
`Alt+↑` pops the last queued message back for editing.

### ⚙️ Grouped `/config` editor

`/config` now groups settings by section (Model, Permissions, Display,
...) with a live filter. `↑/↓` (or `j`/`k` when the filter is empty)
navigate; `Enter`/`e` edit the selected row; `Esc` clears the filter
or closes.

### ⌨️ Searchable help overlay

`?` (with empty input), `F1`, or `Ctrl+/` opens a searchable help
overlay. Type to filter commands and keybindings; multi-term searches
act as AND.

Full history: [CHANGELOG.md](CHANGELOG.md).

---

## Models & Pricing

DeepSeek TUI targets **DeepSeek V4** models with 1M-token context windows by default.

| Model | Context | Input (cache hit) | Input (cache miss) | Output |
|---|---|---|---|---|
| `deepseek-v4-pro` | 1M | $0.003625 / 1M* | $0.435 / 1M* | $0.87 / 1M* |
| `deepseek-v4-flash` | 1M | $0.0028 / 1M | $0.14 / 1M | $0.28 / 1M |

Legacy aliases `deepseek-chat` and `deepseek-reasoner` silently map to `deepseek-v4-flash`.

**NVIDIA NIM** hosted variants (`deepseek-ai/deepseek-v4-pro`, `deepseek-ai/deepseek-v4-flash`) use your NVIDIA account terms — no DeepSeek platform billing.

*\*DeepSeek lists the Pro rates above as a limited-time 75% discount valid until 2026-05-05 15:59 UTC; the TUI cost estimator falls back to base Pro rates after that timestamp.*

---

## Usage

```bash
deepseek                                      # interactive TUI
deepseek "explain this function"              # one-shot prompt
deepseek --model deepseek-v4-flash "summarize" # model override
deepseek --yolo                               # YOLO mode (auto-approve tools)
deepseek login --api-key "..."                # save API key
deepseek doctor                               # check setup & connectivity
deepseek doctor --json                        # machine-readable diagnostics
deepseek setup --status                       # read-only setup status
deepseek setup --tools --plugins              # scaffold local tool/plugin dirs
deepseek models                               # list live API models
deepseek sessions                             # list saved sessions
deepseek resume --last                        # resume latest session
deepseek serve --http                         # HTTP/SSE API server
deepseek mcp list                             # list configured MCP servers
deepseek mcp validate                         # validate MCP config/connectivity
deepseek mcp-server                           # run dispatcher MCP stdio server
```

### Keyboard shortcuts

| Key | Action |
|---|---|
| `Tab` | Complete `/` or `@` entries; while a turn is running, queue the draft as a follow-up; otherwise cycle mode |
| `Shift+Tab` | Cycle reasoning-effort: off → high → max |
| `F1` | Help |
| `Esc` | Back / dismiss |
| `Ctrl+K` | Command palette |
| `Ctrl+R` | Resume an earlier session |
| `Alt+R` | Search prompt history and recover cleared drafts |
| `@path` | Attach file/directory context in composer |
| `↑` (at composer start) | Select attachment row for removal |
| `Alt+↑` | Edit last queued message |
| `/attach <path>` | Attach image/video media references; select the row with `↑` at composer start and remove with `Backspace`/`Delete` |

---

## Modes

| Mode | Behavior |
|---|---|
| **Plan** 🔍 | Read-only investigation — model explores and proposes a decomposition plan (`update_plan` + `checklist_write`) before making changes |
| **Agent** 🤖 | Default interactive mode — multi-step tool use with approval gates; model outlines work via `checklist_write` before requesting writes |
| **YOLO** ⚡ | Auto-approve all tools in a trusted workspace; model still creates `checklist_write`/`update_plan` to keep work visible and trackable |

---

## Configuration

`~/.deepseek/config.toml` — see [config.example.toml](config.example.toml) for every option.

Key environment overrides:

| Variable | Purpose |
|---|---|
| `DEEPSEEK_API_KEY` | API key |
| `DEEPSEEK_BASE_URL` | API base URL |
| `DEEPSEEK_MODEL` | Default model |
| `DEEPSEEK_PROVIDER` | Provider: `deepseek` (default), `nvidia-nim`, `fireworks`, or `sglang` |
| `DEEPSEEK_PROFILE` | Config profile name |
| `NVIDIA_API_KEY` | NVIDIA NIM API key |
| `FIREWORKS_API_KEY` | Fireworks AI API key |
| `SGLANG_BASE_URL` | Self-hosted SGLang endpoint |
| `SGLANG_API_KEY` | Optional SGLang bearer token |

Quick diagnostics: `deepseek setup --status` checks API key, MCP, sandbox, and
`.env` state without network calls; `deepseek doctor --json` is suitable for CI;
`deepseek setup --tools --plugins` scaffolds local tool and plugin directories.

DeepSeek context caching is automatic — when the API returns cache hit/miss token fields, the TUI includes them in usage and cost tracking.

Full reference: [docs/CONFIGURATION.md](docs/CONFIGURATION.md) and [docs/MCP.md](docs/MCP.md).

UI locale is separate from model language — set `locale` in `settings.toml`
or via the `LC_ALL`/`LANG` environment variables. See [docs/CONFIGURATION.md](docs/CONFIGURATION.md).

---

## Publishing your own skill

DeepSeek-TUI discovers skills from the active skills directory. Workspace-local
`.agents/skills` wins when present, then `./skills`, then the configured global
directory (`~/.deepseek/skills` by default). Each skill is a directory with a
`SKILL.md` file:

```text
~/.deepseek/skills/my-skill/
└── SKILL.md
```

`SKILL.md` must start with YAML frontmatter:

```markdown
---
name: my-skill
description: Use this when DeepSeek should follow my custom workflow.
---

# My Skill

Instructions for the agent go here.
```

Run `/skills` to list discovered skills, `/skill <name>` to activate one for
the next message, or `/skill new` to use the bundled skill-creator helper.
Installed skills are also listed in the model-visible session context so the
agent can choose relevant skills when the user names them or when the task
matches their descriptions.

DeepSeek-TUI can also install community skills directly from a GitHub repo,
with no backend service in the loop:

1. Create a public GitHub repo with a `SKILL.md` at the root containing the
   usual `---` frontmatter (`name`, `description`).
2. Multi-skill bundles use `skills/<name>/SKILL.md` instead — the installer
   picks the first match and names the install after the frontmatter `name`.
3. Push to `main` (or `master`); the installer fetches
   `archive/refs/heads/main.tar.gz` and falls back to `master.tar.gz`.
4. Users install via `/skill install github:<owner>/<repo>` — installs are
   gated by the `[network]` policy, validated for path traversal and size, and
   placed under `~/.deepseek/skills/<name>/`.
5. Submit a PR to the curated `index.json` (default registry) to make the skill
   installable by name (`/skill install <name>`) instead of the GitHub spec.
6. Use `/skill update <name>`, `/skill uninstall <name>`, or
   `/skill trust <name>` for installed community skills. Trust is only needed
   when you want scripts bundled with a skill to be eligible for execution.

## Documentation

| Doc | Topic |
|---|---|
| [ARCHITECTURE.md](docs/ARCHITECTURE.md) | Codebase internals |
| [CONFIGURATION.md](docs/CONFIGURATION.md) | Full config reference |
| [MODES.md](docs/MODES.md) | Plan / Agent / YOLO modes |
| [MCP.md](docs/MCP.md) | Model Context Protocol integration |
| [RUNTIME_API.md](docs/RUNTIME_API.md) | HTTP/SSE API server |
| [RELEASE_RUNBOOK.md](docs/RELEASE_RUNBOOK.md) | Release process |
| [OPERATIONS_RUNBOOK.md](docs/OPERATIONS_RUNBOOK.md) | Ops & recovery |

---

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md). Pull requests welcome!

*Not affiliated with DeepSeek Inc.*

## License

[MIT](LICENSE)

## Star History

[![Star History Chart](https://api.star-history.com/chart?repos=Hmbown/DeepSeek-TUI&type=date&legend=top-left)](https://www.star-history.com/?repos=Hmbown%2FDeepSeek-TUI&type=date&logscale=&legend=top-left)
