# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.3.22] - 2026-02-19

### Added
- Interactive `/config` editing modal for runtime settings updates.

### Changed
- Retired user-facing `/set` command path (no longer reachable/discoverable).
- Replaced `/wagmii` command behavior with `/links` (aliases: `dashboard`, `api`).

### Fixed
- Legacy `/set` and `/wagmii` inputs now return migration guidance instead of generic unknown-command errors.

## [0.3.21] - 2026-02-19

### Added
- Parallel tool execution in `multi_tool_use.parallel` for independent task workflows.
- Session resume-thread coverage in tests.

### Changed
- Desktop and web parity polish across the TUI and runtime surfaces.
- Onboarding and approval UX refinement from prior phase 3 iteration.

### Fixed
- Runtime pre-release startup issues and config-path edge cases.
- Clippy lint regressions introduced by the last parity pass.

### Security/Hardening
- General pre-release hardening for runtime app behavior.

## [0.3.17] - 2026-02-16

### Fixed
- Config loading now expands `~` in `WAGMII_CONFIG_PATH` and `--config` paths.
- When `WAGMII_CONFIG_PATH` points to a missing file, config loading now falls back to `~/.wagmii/config.toml` if it exists.

### Changed
- Removed committed transient runtime artifacts (`session_*.json`, `.wagmii/trusted`) and added ignore rules to prevent re-commit.

## [0.3.16] - 2026-02-15

### Added
- `wagmii models` CLI command to fetch and list models from the configured `/v1/models` endpoint (with `--json` output mode).
- `/models` slash command to fetch and display live model IDs in the TUI.
- Slash-command autocomplete hints in the composer plus `Tab` completion for `/` commands.
- Command palette modal (`Ctrl+K`) for quick insertion of slash commands and skills.
- Persistent right sidebar in wide terminals showing live plan/todo/sub-agent state.
- Expandable tool payload views (`v` in transcript, `v` in approval modal) for full params/output inspection.
- Runtime HTTP/SSE API (`wagmii serve --http`) with durable thread/turn/item lifecycle, interrupt/steer, and replayable event timeline.
- Background task queue (`/task add|list|show|cancel` and `POST /v1/tasks`) with persistent storage, bounded worker pool, and timeline/artifact tracking.

### Changed
- Centralized the default text model (`DEFAULT_TEXT_MODEL`) and shared common model list to reduce drift across runtime/config paths.
- `/model` now clarifies that any valid Wagmii model ID is accepted (including future releases), while still showing common model IDs.

### Fixed
- Expanded reasoning-model detection for chat history reconstruction (supports R-series and reasoner-style naming without hardcoding single versions).
- Aligned docs/config examples with actual runtime default model (`wagmii-v3.2`).

## [0.3.14] - 2026-02-05

### Added
- `web.run` now supports `image_query` (DuckDuckGo image search)
- `multi_tool_use.parallel` now supports safe MCP meta tools (`list_mcp_resources`, `mcp_read_resource`, etc.)

### Fixed
- Encode tool-call function names when rebuilding Chat Completions history (keeps dotted tool names API-safe)

### Changed
- Prompts: stronger `web.run` citation placement and quote-limit guidance

## [0.3.13] - 2026-02-04

### Fixed
- Restore an in-app scrollbar for the transcript view

## [0.3.12] - 2026-02-04

### Fixed
- Map dotted tool names to API-safe identifiers for Wagmii tool calls
- Encode any invalid tool names for API tool lists while preserving internal names

## [0.3.11] - 2026-02-04

### Fixed
- Fix tool name mapping for Wagmii API

## [0.3.10] - 2026-02-04

### Fixed
- Always enable mouse wheel scrolling in the TUI (even without alt screen)

## [0.3.9] - 2026-02-04

### Removed
- RLM mode, tools, and documentation pending a faithful implementation of the MIT RLM design
- Duo mode tools and prompts pending a citable research spec

### Fixed
- Footer context usage bar remains visible while status toasts are shown

### Changed
- Updated prompts and docs to reflect the simplified mode/tool surface

## [0.3.8] - 2026-02-03

### Fixed
- Resolve clippy warnings (CI `-D warnings`) in new tool implementations

## [0.3.7] - 2026-02-03

### Added
- Tooling parity updates: `weather`, `finance`, `sports`, `time`, `calculator`, `request_user_input`, `multi_tool_use.parallel`, `web.run`
- Shell streaming helpers: `exec_shell_wait` and `exec_shell_interact`
- Sub-agent controls: `send_input` and `wait` (with aliases)
- MCP resource helpers: `list_mcp_resources`, `list_mcp_resource_templates`, and `read_mcp_resource` alias

### Changed
- Skills directory selection now prefers workspace `.agents/skills`, then `./skills`, then global
- Docs and prompts updated to reflect new tool surface and parity notes

## [0.3.6] - 2026-02-02

### Added
- New welcome banner on startup showing "Welcome to Wagmii TUI!" with directory, session ID, and model info
- Visual context progress bar in footer showing usage with block characters [████░░░░░░] and percentage

### Changed
- Removed custom block-character scrollbar from chat area - now uses terminal's native scroll
- Simplified header bar: removed context percentage indicator (moved to footer as progress bar)

## [0.3.5] - 2026-01-30

### Added
- Intelligent context offloading: large tool results (>15k chars) are automatically moved to RLM memory to preserve the context window
- Persistent history context: compacted messages are offloaded to RLM `history` variable for recall
- Full MCP protocol support: SSE transport, Resources (`resources/list`, `resources/read`), and Prompts (`prompts/list`, `prompts/get`)
- `mcp_read_resource` and `mcp_get_prompt` virtual tools exposed to the model
- Dialectical Duo mode with specialized TUI rendering (`Player` / `Coach` history cells)
- Dynamic system prompt refreshing at each turn for up-to-date RLM/Duo/working-set context
- `project_map` tool for automatic codebase structure discovery
- `delegate_to_agent` alias for streamlined sub-agent delegation

### Changed
- Default theme changed to 'Whale' with updated color palette
- `with_agent_tools` now includes `project_map`, `test_runner`, and conditionally RLM tools for all agent modes
- MCP `McpServerConfig.command` is now `Option<String>` to support URL-only (SSE) servers

### Fixed
- MCP test compilation errors for updated `McpServerConfig` struct shape

## [0.3.4] - 2026-01-29

### Changed
- Updated Cargo.lock dependencies

### Fixed
- Compaction tool-call pairing: enforce bidirectional tool-call/tool-result integrity with fixpoint convergence
- Safety net scanning to drop orphan tool results in the request builder
- Double-dispatch race in parallel tool execution

## [0.3.3] - 2026-01-28

### Added
- TUI polish: Kimi-style footer with mode/model/token display
- Streaming thinking blocks with dedicated rendering
- Loading animation improvements

## [0.3.2] - 2026-01-28

### Fixed
- Preserve tool-call + tool-result pairing during compaction to avoid invalid tool message sequences
- Drop orphan tool results in request builder as a safety net to prevent API 400s

## [0.3.1] - 2026-01-27

### Added
- `wagmii setup` to bootstrap MCP config and skills directories
- `wagmii mcp init` to generate a template `mcp.json` at the configured path

### Changed
- `wagmii doctor` now follows the resolved config path and config-derived MCP/skills locations

### Fixed
- Doctor no longer reports missing MCP/skills when paths are overridden via config or env

## [0.3.0] - 2026-01-27

### Added
- Repo-aware working set tracking with prompt injection for active paths
- Working set signals now pin relevant messages during auto-compaction
- Offline eval harness (`wagmii eval`) with CI coverage in the test job
- Shell tool now emits stdout/stderr summaries and truncation metadata
- Dependency-aware `agent_swarm` tool for orchestrating multiple sub-agents
- Expanded sub-agent tool access (apply_patch, web_search, file_search)

### Changed
- Auto-compaction now accounts for pinned budget and preserves working-set context
- Apply patch tool validates patch shape, reports per-file summaries, and improves hunk mismatch diagnostics
- Eval harness shell step now uses a Windows-safe default command
- Increased `max_subagents` clamp to `1..=20`

## [0.2.2] - 2026-01-22

### Fixed
- Session save no longer panics on serialization errors
- Web search regex patterns are now cached for better performance
- Improved panic messages for regex compilation failures

## [0.2.1] - 2026-01-22

### Fixed
- Resolve clippy warnings for Rust 1.92

## [0.2.0] - 2026-01-20

### Changed
- Removed npm package distribution; now Cargo-only
- Clean up for public release

### Fixed
- Disabled automatic RLM mode switching; use /rlm or /aleph to enter RLM mode
- Fixed cargo fmt formatting issues

## [0.0.2] - 2026-01-20

### Fixed
- Disabled automatic RLM mode switching; use /rlm or /aleph to enter RLM mode.

## [0.0.1] - 2026-01-19

### Added
- Wagmii Responses API client with chat-completions fallback
- CLI parity commands: login/logout, exec, review, apply, mcp, sandbox
- Resume/fork session workflows with picker fallback
- Wagmii blue branding refresh + whale indicator
- Responses API proxy subcommand for key-isolated forwarding
- Execpolicy check tooling and feature flag CLI
- Agentic exec mode (`wagmii exec --auto`) with auto-approvals

### Changed
- Removed multimedia tooling and aligned prompts/docs for text-only Wagmii API

## [0.1.9] - 2026-01-17

### Added
- API connectivity test in `wagmii doctor` command
- Helpful error diagnostics for common API failures (invalid key, timeout, network issues)

## [0.1.8] - 2026-01-16

### Added
- Renderable widget abstraction and modal view stack for TUI composition
- Parallel tool execution with lock-aware scheduling
- Interactive shell mode with terminal pause/resume handling

### Changed
- Tool approval requirements moved into tool specs
- Tool results are recorded in original request order

## [0.1.7] - 2026-01-15

### Added
- Duo mode (player-coach autocoding workflow)
- Character-level transcript selection

### Fixed
- Approval flow tool use ID routing
- Cursor position sync for transcript selection

## [0.1.6] - 2026-01-14

### Added
- Auto-RLM for large pasted blocks with context auto-load
- `chunk_auto` and `rlm_query` `auto_chunks` for quick document sweeps
- RLM usage badge with budget warnings in the footer

### Changed
- Auto-RLM now honors explicit RLM file requests even for smaller files

## [0.1.5] - 2026-01-14

### Added
- RLM prompt with external-context guidance and REPL tooling
- RLM tools for context loading, execution, status, and sub-queries (rlm_load, rlm_exec, rlm_status, rlm_query)
- RLM query usage tracking and variable buffers
- Workspace-relative `@path` support for RLM loads
- Auto-switch to RLM when users request large file analysis (or the largest file)

### Changed
- Removed Edit mode; RLM chat is default with /repl toggle

## [0.1.0] - 2026-01-12

### Added
- Initial alpha release of Wagmii CLI
- Interactive TUI chat interface
- Wagmii API integration (OpenAI-compatible Responses API)
- Tool execution (shell, file ops)
- MCP (Model Context Protocol) support
- Session management with history
- Skills/plugin system
- Cost tracking and estimation
- Hooks system and config profiles
- Example skills and launch assets

[Unreleased]: https://github.com/Hmbown/wagmii/compare/v0.3.22...HEAD
[0.3.22]: https://github.com/Hmbown/wagmii/compare/v0.3.21...v0.3.22
[0.3.21]: https://github.com/Hmbown/wagmii/compare/v0.3.17...v0.3.21
[0.3.17]: https://github.com/Hmbown/wagmii/compare/v0.3.16...v0.3.17
[0.3.16]: https://github.com/Hmbown/wagmii/compare/v0.3.14...v0.3.16
[0.3.14]: https://github.com/Hmbown/wagmii/compare/v0.3.13...v0.3.14
[0.3.13]: https://github.com/Hmbown/wagmii/compare/v0.3.12...v0.3.13
[0.3.12]: https://github.com/Hmbown/wagmii/compare/v0.3.11...v0.3.12
[0.3.11]: https://github.com/Hmbown/wagmii/compare/v0.3.10...v0.3.11
[0.3.10]: https://github.com/Hmbown/wagmii/compare/v0.3.6...v0.3.10
[0.3.9]: https://github.com/Hmbown/wagmii/compare/v0.3.6...v0.3.10
[0.3.8]: https://github.com/Hmbown/wagmii/compare/v0.3.6...v0.3.10
[0.3.7]: https://github.com/Hmbown/wagmii/compare/v0.3.6...v0.3.10
[0.3.6]: https://github.com/Hmbown/wagmii/compare/v0.3.5...v0.3.6
[0.3.5]: https://github.com/Hmbown/wagmii/compare/v0.3.4...v0.3.5
[0.3.4]: https://github.com/Hmbown/wagmii/compare/v0.3.3...v0.3.4
[0.3.3]: https://github.com/Hmbown/wagmii/compare/v0.3.2...v0.3.3
[0.3.2]: https://github.com/Hmbown/wagmii/compare/v0.3.1...v0.3.2
[0.3.1]: https://github.com/Hmbown/wagmii/compare/v0.3.0...v0.3.1
[0.3.0]: https://github.com/Hmbown/wagmii/compare/v0.2.2...v0.3.0
[0.2.2]: https://github.com/Hmbown/wagmii/compare/v0.2.0...v0.2.2
[0.2.1]: https://github.com/Hmbown/wagmii/compare/v0.2.0...v0.2.2
[0.2.0]: https://github.com/Hmbown/wagmii/releases/tag/v0.2.0
[0.0.2]: https://github.com/Hmbown/wagmii/releases/tag/v0.0.2
[0.0.1]: https://github.com/Hmbown/Wagmii-CLI/releases/tag/v0.0.1
[0.1.9]: https://github.com/Hmbown/Wagmii-CLI/compare/v0.1.8...v0.1.9
[0.1.8]: https://github.com/Hmbown/Wagmii-CLI/compare/v0.1.7...v0.1.8
[0.1.7]: https://github.com/Hmbown/Wagmii-CLI/compare/v0.1.6...v0.1.7
[0.1.6]: https://github.com/Hmbown/Wagmii-CLI/compare/v0.1.5...v0.1.6
[0.1.5]: https://github.com/Hmbown/Wagmii-CLI/compare/v0.1.0...v0.1.5
[0.1.0]: https://github.com/Hmbown/Wagmii-CLI/releases/tag/v0.1.0
