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

Prebuilt binaries are published for **Linux x64**, **Linux ARM64** (v0.8.8+),
**macOS x64**, **macOS ARM64**, and **Windows x64**. For everything else —
musl, riscv64, FreeBSD, etc. — see [Build from source](#install-from-source)
below or the full [docs/INSTALL.md](docs/INSTALL.md) walkthrough.

### Linux ARM64 (Raspberry Pi, Asahi, Graviton, HarmonyOS PC)

`npm i -g deepseek-tui` works on glibc-based ARM64 Linux from **v0.8.8**
onward. If you're stuck on v0.8.7 or earlier (where you'll see
`Unsupported architecture: arm64`), upgrade or use `cargo install`:

```bash
# requires Rust 1.85+ (https://rustup.rs)
cargo install deepseek-tui-cli --locked   # provides `deepseek`
cargo install deepseek-tui     --locked   # provides `deepseek-tui`
```

You can also download `deepseek-linux-arm64` and `deepseek-tui-linux-arm64`
directly from the [Releases page](https://github.com/Hmbown/DeepSeek-TUI/releases)
and drop both side by side into a directory on your `PATH`. Cross-compiling
from x64 to ARM64 is documented in
[docs/INSTALL.md](docs/INSTALL.md#cross-compiling-from-x64-to-arm64-linux).

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

Then install the canonical `deepseek` dispatcher and the companion TUI binary
(both are required — the dispatcher delegates to the TUI runtime):

```bash
cargo install deepseek-tui-cli --locked   # provides `deepseek`
cargo install deepseek-tui     --locked   # provides `deepseek-tui`
deepseek --version
```

You can also download prebuilt binaries directly from the
[GitHub Releases](https://github.com/Hmbown/DeepSeek-TUI/releases) page when
GitHub release assets are reachable. TUNA, rsproxy, Tencent COS, or Aliyun OSS
mirrors can also be used with `DEEPSEEK_TUI_RELEASE_BASE_URL` when a mirrored
release-asset directory is available.

On first launch you'll be prompted for your [DeepSeek API key](https://platform.deepseek.com/api_keys). The TUI saves it to your user config at `~/.deepseek/config.toml` so it works from every folder without OS credential prompts.

You can also set it ahead of time:

```bash
# Recommended — saves to ~/.deepseek/config.toml; works everywhere
# (interactive shells, IDE terminals, scripts, cron):
deepseek auth set --provider deepseek

# Env var alternative — note that on zsh, exports in ~/.zshrc only
# reach interactive shells. Put it in ~/.zshenv if you want it in
# every context (login shells, IDEs, scripts):
export DEEPSEEK_API_KEY="YOUR_DEEPSEEK_API_KEY"
deepseek

# Verify which source the binary is reading:
deepseek doctor
```

> To rotate or remove a saved key, run
> `deepseek auth clear --provider deepseek` (or `deepseek logout` for
> the legacy alias), then run `deepseek auth set --provider deepseek`
> again.

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

<details id="install-from-source">
<summary>Install from source</summary>

Works on any Tier-1 Rust target — including Linux musl/riscv64, FreeBSD, and
ARM64 distros that pre-date our prebuilt binaries.

```bash
# Linux build deps (Debian/Ubuntu/openEuler/Kylin):
#   sudo apt-get install -y build-essential pkg-config libdbus-1-dev
#   # RHEL family: sudo dnf install -y gcc make pkgconf-pkg-config dbus-devel

git clone https://github.com/Hmbown/DeepSeek-TUI.git
cd DeepSeek-TUI

cargo install --path crates/cli --locked   # requires Rust 1.85+; provides `deepseek`
cargo install --path crates/tui --locked   # provides `deepseek-tui`
```

Both binaries are required — the `deepseek` dispatcher delegates to
`deepseek-tui` at runtime. Cross-compilation, mirror, and platform-specific
notes live in [docs/INSTALL.md](docs/INSTALL.md).

</details>

---

## What's new in v0.8.8

A stabilization-focused release: a thick band of UX polish on top of the v0.8.6 / v0.8.7 base, plus runtime fixes for the rough edges that surfaced in production sessions. No model or API changes; every existing config and session keeps working.

### 🪟 TUI polish

- **Visual retry / backoff banner** when the upstream rate-limits or 5xxs, with a per-second countdown so a stalled session is obviously stalled instead of silently frozen ([#499](https://github.com/Hmbown/DeepSeek-TUI/issues/499)).
- **MCP health chip** in the footer — a coloured `MCP n/n` glyph reflects how many configured servers are actually reachable, hidden when no servers are configured ([#502](https://github.com/Hmbown/DeepSeek-TUI/issues/502)).
- **Tool-output spillover** routes full bodies to `~/.deepseek/tool_outputs/<id>.txt` with a 32 KiB head visible in the cell; the existing details pager appends the full output so nothing is hidden, just paged ([#500](https://github.com/Hmbown/DeepSeek-TUI/issues/500)).
- **Multi-day duration formatting** — `humanize_duration` walks `s → m → h → d → w` and caps at two units, so a long-running session reads `2d 3h` instead of `188415s` ([#447](https://github.com/Hmbown/DeepSeek-TUI/issues/447)).
- **Cumulative `worked Nh Mm` footer chip** appears once a session crosses 60s, dropping first under narrow widths so it never shoves more important chips off-screen ([#448](https://github.com/Hmbown/DeepSeek-TUI/issues/448)).
- **OSC 8 hyperlinks** — URLs in the transcript are Cmd+click-openable on iTerm2, Terminal.app, Ghostty, Kitty, WezTerm, Alacritty, and modern gnome-terminal/konsole; legacy terminals just show the visible text ([#498](https://github.com/Hmbown/DeepSeek-TUI/issues/498)).
- **Inline diff rendering** for `edit_file` and `write_file` — tool results emit a unified diff at the head of the body, picked up by the diff-aware renderer with line numbers and coloured `+`/`-` gutters ([#505](https://github.com/Hmbown/DeepSeek-TUI/issues/505)).
- **Composer prompt stash** — Ctrl+S parks the current draft to `~/.deepseek/composer_stash.jsonl`, `/stash list` shows parked drafts, `/stash pop` restores LIFO, `/stash clear` wipes the file. Self-healing JSONL parser, 200-entry cap, multi-line drafts preserved ([#440](https://github.com/Hmbown/DeepSeek-TUI/issues/440)).
- **Slash-menu layout no longer jitters** the chat area as the matched-entry count changes mid-typing — reported on Windows 10 PowerShell + WSL where the per-cell write cost made the redraw visibly laggy. The composer now reserves its panel-max envelope for the whole slash/mention session.

### ♿ Accessibility

- **`NO_ANIMATIONS=1`** env var (also `1` / `true` / `yes` / `on`) forces `low_motion = true` and `fancy_animations = false` at startup regardless of saved settings; new `docs/ACCESSIBILITY.md` documents every motion / output knob ([#450](https://github.com/Hmbown/DeepSeek-TUI/issues/450)).
- **Keyboard-enhancement flags** pop on every shutdown path including panic, Ctrl+Z suspend, and external-editor invocation, so a crashed TUI never leaves your terminal in raw mode ([#443](https://github.com/Hmbown/DeepSeek-TUI/issues/443)/[#444](https://github.com/Hmbown/DeepSeek-TUI/issues/444)).
- **Kitty keyboard protocol** (`DISAMBIGUATE_ESCAPE_CODES`) pushed at startup so kitty-protocol terminals report unambiguous events for Option/Alt-modified keys; legacy terminals are unaffected ([#442](https://github.com/Hmbown/DeepSeek-TUI/issues/442)).

### 🤖 Agents / sub-agents

- **Sub-agent cap raised 5 → 10** (configurable via `[subagents].max_concurrent`, hard ceiling 20). Completed agents no longer count against the running cap ([#509](https://github.com/Hmbown/DeepSeek-TUI/issues/509)).
- **Multi-agent fan-out UI freeze fixed** — `SharedSubAgentManager` is now `Arc<RwLock<…>>`; read paths take read locks instead of contending on a `Mutex` ([#510](https://github.com/Hmbown/DeepSeek-TUI/issues/510)).
- **Sub-agent output summarized** before being folded into the parent's context, so a child returning 100KB of evidence doesn't wreck the parent's window ([#511](https://github.com/Hmbown/DeepSeek-TUI/issues/511)).
- **`Implementer` + `Verifier` sub-agent roles** wired into `agent_spawn` / `agent_assign` schemas so the model surfaces them by name ([#404](https://github.com/Hmbown/DeepSeek-TUI/issues/404)).
- **`agent_list` defaults to current-session view** — prior sessions filtered out unless `include_archived=true` ([#405](https://github.com/Hmbown/DeepSeek-TUI/issues/405)).
- **Compact `agent_spawn` rendering** in live mode collapses to a single header line; transcript replay keeps the full block ([#409](https://github.com/Hmbown/DeepSeek-TUI/issues/409)).
- **`agent_swarm` / `spawn_agents_on_csv` / `/swarm`** removed in v0.8.5 — confirmed gone in this release; multi-child fanout is no longer a model-callable tool.

### 🛠️ Workflows / extensibility

- **`load_skill` model-callable tool** — takes a skill id, returns the SKILL.md body plus sibling companion-file list in one call. Available in Plan and Agent / Yolo modes ([#434](https://github.com/Hmbown/DeepSeek-TUI/issues/434)).
- **Cross-tool skill discovery** — skills catalogue and `load_skill` walk `.agents/skills`, `skills`, `.opencode/skills`, `.claude/skills`, and `~/.deepseek/skills` with first-wins precedence ([#432](https://github.com/Hmbown/DeepSeek-TUI/issues/432)).
- **`/hooks` read-only lifecycle hook listing** groups configured hooks by event with name / command preview / timeout / condition. Notes the global `[hooks].enabled` state. `/hooks events` lists every supported `HookEvent` value ([#460](https://github.com/Hmbown/DeepSeek-TUI/issues/460)).
- **Every `HookEvent` now has a live producer** — `tool_call_before` / `tool_call_after` / `message_submit` / `on_error` fire from the runtime in addition to the existing session-lifecycle and mode-change events. Hooks remain read-only observers in v0.8.8 ([#455](https://github.com/Hmbown/DeepSeek-TUI/issues/455)).
- **`instructions = [...]` config array** lets you stack additional system-prompt files; paths capped at 100 KiB each, project array replaces user array wholesale ([#454](https://github.com/Hmbown/DeepSeek-TUI/issues/454)).
- **`deepseek pr <N>` subcommand** fetches a PR's title / body / diff via `gh` and launches the TUI with a review prompt already in the composer. Codepoint-safe diff cap at 200 KiB; optional `--repo` / `--checkout` ([#451](https://github.com/Hmbown/DeepSeek-TUI/issues/451)).
- **User-memory MVP** (opt-in) — `~/.deepseek/memory.md` injected into the system prompt as a `<user_memory>` block; `# foo` typed in the composer appends a timestamped bullet without firing a turn; `/memory [show|path|clear|edit|help]` for inspection. Default off; enable with `[memory] enabled = true` or `DEEPSEEK_MEMORY=on`. See [docs/MEMORY.md](docs/MEMORY.md) for the full guide and examples ([#489](https://github.com/Hmbown/DeepSeek-TUI/issues/489)–[#493](https://github.com/Hmbown/DeepSeek-TUI/issues/493)).

### 🔒 Security

- **Project-config keys denied at workspace scope** — a malicious `./.deepseek/config.toml` can no longer override `api_key`, `base_url`, `provider`, or `mcp_config_path`. The loosest values (`approval_policy = "auto"`, `sandbox_mode = "danger-full-access"`) are also denied at project scope ([#417](https://github.com/Hmbown/DeepSeek-TUI/issues/417)).
- **`SSL_CERT_FILE` honoured** in the HTTPS client so corporate-CA / MITM-proxy users can connect — PEM bundle and DER fallback; failures log a warning and continue ([#418](https://github.com/Hmbown/DeepSeek-TUI/issues/418)).
- **Execpolicy heredoc parsing** — `normalize_command` strips heredoc bodies before shlex tokenization so `auto_allow = ["cat > file.txt"]` matches the heredoc form `cat <<EOF > file.txt\nbody\nEOF`. Recognises `<<DELIM` / `<<-DELIM` / `<<'DELIM'` / `<<"DELIM"`; leaves `<<<` (here-string) untouched ([#419](https://github.com/Hmbown/DeepSeek-TUI/issues/419)).
- **`Don't auto-approve git -C ...`** runtime fix shipped on v0.8.7 main ([#416](https://github.com/Hmbown/DeepSeek-TUI/issues/416)) — included for completeness.

### 📦 Packaging

- **Linux ARM64 prebuilts** added to the release matrix; npm wrapper picks the right binary on `aarch64-linux` automatically. New `docs/INSTALL.md` covers every install path (npm, cargo, prebuilt, source).
- **`deepseek update` fixed** — the v0.8.7 self-updater used Rust ARCH constants (`aarch64`/`x86_64`) instead of release-asset naming (`arm64`/`x64`), so the command failed on every platform. Now maps correctly and rejects `.sha256` siblings as primary binaries ([#503](https://github.com/Hmbown/DeepSeek-TUI/issues/503)).
- **CI workflow cleanup** — pruned three duplicated/dead workflows; `release.yml` `build` job now allows the `parity` gate to be skipped on manual `workflow_dispatch` ([#507](https://github.com/Hmbown/DeepSeek-TUI/issues/507)).

### 🐛 Bug fixes

- **Composer Option+Backspace** deletes by word now ([#488](https://github.com/Hmbown/DeepSeek-TUI/issues/488)).
- **Offline composer queue is session-scoped** — legacy unscoped queues fail closed instead of leaking content into unrelated chats ([#487](https://github.com/Hmbown/DeepSeek-TUI/issues/487)).
- **`display_path` test race + Windows separator** — tests no longer mutate `$HOME`; home-relative suffix joins with `MAIN_SEPARATOR_STR` so Windows shows `~\projects\foo` ([#506](https://github.com/Hmbown/DeepSeek-TUI/issues/506)).
- **Footer reads statusline colours from `app.ui_theme`** ([#449](https://github.com/Hmbown/DeepSeek-TUI/issues/449)).

### 🔑 Auth & onboarding

- **No automatic OS credential prompts** — startup, `doctor`, `doctor --json`, and normal dispatcher setup now use CLI flag → `~/.deepseek/config.toml` → env.
- **One setup command works everywhere** — `deepseek auth set --provider deepseek` and the in-TUI onboarding screen both write the shared user config file, so the key is available from any folder without relying on `~/.zshrc` propagation.
- **Onboarding screen wording rewritten** — "Step 1: open https://platform.deepseek.com/api_keys" / "Step 2: paste below and press Enter" with an explicit note showing where the key is saved.
- **Missing-key error is now actionable** — the `DeepSeek API key not found` bail message lists the config-backed CLI command first and the env-var alternative second, with a `~/.zshrc` vs `~/.zshenv` note for zsh users whose env var only reaches interactive shells.
- **Dispatcher provider/auth parity** — the canonical `deepseek` entry point now accepts the same DeepSeek V4 providers advertised by the TUI (`fireworks`, `sglang` included), and legacy `deepseek login --api-key` / `deepseek logout` now share the same config-backed path.

Full changelog: [CHANGELOG.md](CHANGELOG.md).

---

## What's new in v0.8.7

Quick patch on top of v0.8.6 to unblock copy/select.

### ✂️ Selection works across the whole transcript

The selection-tightening from v0.8.6 restricted copy/select to user and
assistant message bodies, which made it impossible to copy text out of
system notes, thinking blocks, or tool output. v0.8.7 drops that gate so
the rendered transcript block is selectable end-to-end again.

> **Known issues in v0.8.7 (fixed in v0.8.8):**
> - `deepseek update` fails with `no asset found for platform …` because the
>   platform-string mapping in the self-updater uses `aarch64`/`x86_64`
>   instead of the release artifact's `arm64`/`x64`
>   ([#503](https://github.com/Hmbown/DeepSeek-TUI/issues/503)).
> - `npm i -g deepseek-tui` exits with `Unsupported architecture: arm64 on
>   platform linux` on ARM64 Linux because v0.8.7 didn't publish a
>   `deepseek-linux-arm64` asset.
>
> Until v0.8.8 ships, install via:
> ```bash
> # x64 Linux / macOS / Windows
> npm i -g deepseek-tui
>
> # ARM64 Linux (HarmonyOS, openEuler, Asahi, Raspberry Pi, Graviton, …) —
> # build from source with Cargo (Rust 1.85+):
> cargo install deepseek-tui-cli --locked   # provides `deepseek`
> cargo install deepseek-tui     --locked   # provides `deepseek-tui`
> ```

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
ARCHITECTURE.md cleaned up for v0.8.6 (removed swarm tool surface, current
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

### 🔄 Fanout UI reconciliation

The fanout card no longer pre-seeds with zero-state workers, eliminating the
"0 done · 0 running · 0 failed · N pending" vs sidebar "N running"
contradiction. The sidebar now shows "dispatching N" before the first progress
event arrives from a legacy fanout invocation.

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
deepseek auth set --provider deepseek         # save API key to ~/.deepseek/config.toml
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
