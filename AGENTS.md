# Project Instructions

This file provides context for AI assistants working on this project.

## Project Type: Rust

### Commands
- Build: `cargo build` (default-members include the `deepseek` dispatcher)
- Test: `cargo test --workspace --all-features`
- Lint: `cargo clippy --workspace --all-targets --all-features`
- Format: `cargo fmt --all`
- Run (canonical): `deepseek` — use the **`deepseek` binary**, not `deepseek-tui`. The dispatcher delegates to the TUI for interactive use and is the supported entry point for every flow (`deepseek`, `deepseek -p "..."`, `deepseek doctor`, `deepseek mcp …`, etc.).
- Run from source: `cargo run --bin deepseek` (or `cargo run -p deepseek-tui-cli`).
- Local dev shorthand: after `cargo build --release`, run `./target/release/deepseek`.

### Build Dependencies
- **Rust** 1.85+ (for the workspace)

### Documentation
See README.md for project overview, docs/ARCHITECTURE.md for internals.

## DeepSeek-Specific Notes

- **Thinking Tokens**: DeepSeek models output thinking blocks (`ContentBlock::Thinking`) before final answers. The TUI streams and displays these with visual distinction.
- **Reasoning Models**: `deepseek-v4-pro` and `deepseek-v4-flash` are the documented V4 model IDs. Legacy `deepseek-chat` and `deepseek-reasoner` are compatibility aliases for `deepseek-v4-flash`.
- **Large Context Window**: DeepSeek V4 models have 1M-token context windows. Use search tools to navigate efficiently.
- **API**: OpenAI-compatible Chat Completions (`/chat/completions`) is the documented DeepSeek API path. Base URL configurable for global (`api.deepseek.com`) or China (`api.deepseeki.com`); `/v1` is accepted for OpenAI SDK compatibility, and `/beta` is only needed for beta features such as strict tool mode, chat prefix completion, and FIM completion.
- **Thinking + Tool Calls**: In V4 thinking mode, assistant messages that contain tool calls must replay their `reasoning_content` in all subsequent requests or the API returns HTTP 400.

## GitHub Operations

Use the **`gh` CLI** (`/opt/homebrew/bin/gh`) for all GitHub operations — issues, PRs, branches, labels. It's already authenticated as `Hmbown` (token scopes: `gist`, `read:org`, `repo`, `workflow`). Examples:

- List open issues: `gh issue list --state open --limit 20`
- View an issue: `gh issue view <number>`
- Create an issue branch: `gh issue develop <number> --branch-name feat/issue-<number>-<slug>`
- Close a verified issue: `gh issue close <number> --comment "..."`
- Create a PR: `gh pr create --base feat/v0.6.2 --title "..." --body "..."`
- Check PR status: `gh pr view <number>`

Prefer `gh` over `fetch_url` or `web_search` for GitHub data — it's faster, authenticated, and avoids rate limits.
Issues may be closed when the acceptance criteria have been verified or when the user explicitly asks for closure; avoid closing unrelated issues opportunistically.

## Important Notes

- **Token/cost tracking inaccuracies**: Token counting and cost estimation may be inflated due to thinking token accounting bugs. Use `/compact` to manage context, and treat cost estimates as approximate.
- **Modes**: Three modes — Plan (read-only investigation), Agent (tool use with approval), YOLO (auto-approved). See `docs/MODES.md` for details.
- **Sub-agents**: Single model-callable surface is `agent_spawn` (returns an `agent_id` immediately; parent keeps working) plus `agent_wait` / `agent_result` / `agent_cancel` / `agent_list` / `agent_send_input` / `agent_resume` / `agent_assign`. The old `agent_swarm` / `spawn_agents_on_csv` / `/swarm` surface was removed in v0.8.5 (#336).
- **`rlm` tool** (`crates/tui/src/tools/rlm.rs`): a sandboxed Python REPL where a sub-LLM can call in-REPL helpers (`llm_query()`, `llm_query_batched()`, `rlm_query()`, `rlm_query_batched()`) — those `*_query` names are **Python helpers inside the REPL**, not separately-registered model-visible tools. Always loaded across all modes.
