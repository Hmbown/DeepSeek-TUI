# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.0.2] - 2026-01-20

### Fixed
- Disabled automatic RLM mode switching; use /rlm or /aleph to enter RLM mode.

## [0.0.1] - 2026-01-19

### Added
- DeepSeek Responses API client with chat-completions fallback
- CLI parity commands: login/logout, exec, review, apply, mcp, sandbox
- Resume/fork session workflows with picker fallback
- DeepSeek blue branding refresh + whale indicator
- Responses API proxy subcommand for key-isolated forwarding
- Execpolicy check tooling and feature flag CLI
- Agentic exec mode (`deepseek exec --auto`) with auto-approvals

### Changed
- Removed multimedia tooling and aligned prompts/docs for text-only DeepSeek API

## [0.1.9] - 2026-01-17

### Added
- API connectivity test in `deepseek doctor` command
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
- Initial alpha release of DeepSeek CLI
- Interactive TUI chat interface
- DeepSeek API integration (OpenAI-compatible Responses API)
- Tool execution (shell, file ops)
- MCP (Model Context Protocol) support
- Session management with history
- Skills/plugin system
- Cost tracking and estimation
- Hooks system and config profiles
- Example skills and launch assets

[Unreleased]: https://github.com/Hmbown/DeepSeek-CLI/compare/v0.0.2...HEAD
[0.0.2]: https://github.com/Hmbown/DeepSeek-CLI/releases/tag/v0.0.2
[0.0.1]: https://github.com/Hmbown/DeepSeek-CLI/releases/tag/v0.0.1
[0.1.9]: https://github.com/Hmbown/DeepSeek-CLI/compare/v0.1.8...v0.1.9
[0.1.8]: https://github.com/Hmbown/DeepSeek-CLI/compare/v0.1.7...v0.1.8
[0.1.7]: https://github.com/Hmbown/DeepSeek-CLI/compare/v0.1.6...v0.1.7
[0.1.6]: https://github.com/Hmbown/DeepSeek-CLI/compare/v0.1.5...v0.1.6
[0.1.5]: https://github.com/Hmbown/DeepSeek-CLI/compare/v0.1.0...v0.1.5
[0.1.0]: https://github.com/Hmbown/DeepSeek-CLI/releases/tag/v0.1.0
