# DeepSeek TUI

> **A terminal-native coding agent for [DeepSeek V4](https://platform.deepseek.com) models — with 1M-token context, thinking-mode reasoning, and full tool-use.**

```bash
npm i -g deepseek-tui
```

[![CI](https://github.com/Hmbown/DeepSeek-TUI/actions/workflows/ci.yml/badge.svg)](https://github.com/Hmbown/DeepSeek-TUI/actions/workflows/ci.yml)
[![npm](https://img.shields.io/npm/v/deepseek-tui)](https://www.npmjs.com/package/deepseek-tui)

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
- **Three interaction modes** — Plan (read-only explore), Agent (interactive with approval), YOLO (auto-approved). Decomposition-first system prompts teach the model to `todo_write`, `update_plan`, and spawn sub-agents before acting
- **Reasoning-effort tiers** — cycle through `off → high → max` with Shift+Tab
- **Session save/resume** — checkpoint and resume long sessions
- **Workspace rollback** — side-git pre/post-turn snapshots with `/restore` and `revert_turn`, without touching your repo's `.git`
- **HTTP/SSE runtime API** — `deepseek serve --http` for headless agent workflows
- **MCP protocol** — connect to Model Context Protocol servers for extended tooling
- **Live cost tracking** — per-turn and session-level token usage and cost estimates
- **Dark theme** — DeepSeek-blue palette

---

## Quickstart

```bash
npm install -g deepseek-tui
deepseek
```

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
cargo install --path crates/tui --bin deepseek-tui --locked   # requires Rust 1.85+
cargo install --path crates/cli --bin deepseek --locked
```

</details>

---

## What's new in v0.6.0

### 🌊 `rlm_query` — recursive language models as a first-class tool

The model now has direct access to a native recursive-LLM primitive. Inspired by [Alex Zhang's RLM work](https://github.com/alexzhang13/rlm) and Sakana AI's published research on novelty search, but trimmed to what an agent loop actually needs: one tool, structured args, no DSL.

```jsonc
// Single child:
rlm_query({ "prompt": "Summarise this 4k-line log: ..." })

// 8 parallel children, indexed result:
rlm_query({
  "prompts": [
    "Review src/foo.rs for race conditions: ...",
    "Review src/foo.rs for input validation: ...",
    "Review src/foo.rs for error-handling gaps: ...",
    "..."
  ]
})

// Promote one call to Pro:
rlm_query({ "prompt": "Hard reasoning here", "model": "deepseek-v4-pro" })
```

Children run concurrently against the existing DeepSeek client via `tokio` — no external binary, no Python sandbox, no fenced-block DSL. Returns a single string for one prompt or `[i] ...` indexed blocks for many. Available in Plan / Agent / YOLO. The cost is folded into the session's running total automatically.

### Other changes

- **Scroll position survives content rewrites** — anchor fallback now clamps to the nearest surviving cell instead of teleporting to the bottom (#56)
- **Looser command-safety chains** — `cargo build && cargo test` is no longer blocked outright; chains of known-safe commands escalate to RequiresApproval instead of Dangerous (#57)
- **Multi-turn tool calls no longer 400 on thinking-mode models** — `reasoning_content` is replayed across user-message boundaries with a safe placeholder when the round produced none

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
deepseek models                               # list live API models
deepseek sessions                             # list saved sessions
deepseek resume --last                        # resume latest session
deepseek serve --http                         # HTTP/SSE API server
```

### Keyboard shortcuts

| Key | Action |
|---|---|
| `Tab` | Cycle mode: Plan → Agent → YOLO |
| `Shift+Tab` | Cycle reasoning-effort: off → high → max |
| `F1` | Help |
| `Esc` | Back / dismiss |
| `Ctrl+K` | Command palette |
| `@path` | Attach file/directory context in composer |
| `/attach <path>` | Attach image/video media references |

---

## Modes

| Mode | Behavior |
|---|---|
| **Plan** 🔍 | Read-only investigation — model explores and proposes a decomposition plan (`update_plan` + `todo_write`) before making changes |
| **Agent** 🤖 | Default interactive mode — multi-step tool use with approval gates; model outlines work via `todo_write` before requesting writes |
| **YOLO** ⚡ | Auto-approve all tools in a trusted workspace; model still creates `todo_write`/`update_plan` to keep work visible and trackable |

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

Quick diagnostics:

```bash
deepseek-tui setup --status    # read-only status check (API key, MCP, sandbox, .env)
deepseek-tui doctor --json     # machine-readable doctor output for CI
deepseek-tui setup --tools --plugins  # scaffold tools/ and plugins/ directories
```

DeepSeek context caching is automatic — when the API returns cache hit/miss token fields, the TUI includes them in usage and cost tracking.

Full reference: [docs/CONFIGURATION.md](docs/CONFIGURATION.md)

---

## Publishing your own skill

DeepSeek-TUI can install community skills directly from a GitHub repo, with no
backend service in the loop:

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
