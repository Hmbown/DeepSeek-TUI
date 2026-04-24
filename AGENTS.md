# Project Instructions

This file provides context for AI assistants working on this project.

## Project Type: Rust

### Commands
- Build: `cargo build`
- Test: `cargo test --workspace --all-features`
- Lint: `cargo clippy --workspace --all-targets --all-features`
- Format: `cargo fmt --all`
- Run: `cargo run -p deepseek-tui`

### Documentation
See README.md for project overview, docs/ARCHITECTURE.md for internals.

## DeepSeek-Specific Notes

- **Thinking Tokens**: DeepSeek models output thinking blocks (`ContentBlock::Thinking`) before final answers. The TUI streams and displays these with visual distinction.
- **Reasoning Models**: `deepseek-v4-pro` and `deepseek-v4-flash` are the documented V4 model IDs. Legacy `deepseek-chat` and `deepseek-reasoner` are compatibility aliases for `deepseek-v4-flash`.
- **Large Context Window**: DeepSeek V4 models have 1M-token context windows. Use search tools to navigate efficiently.
- **API**: OpenAI-compatible Chat Completions (`/chat/completions`) is the documented DeepSeek API path. Base URL configurable for global (`api.deepseek.com`) or China (`api.deepseeki.com`); `/v1` is accepted for OpenAI SDK compatibility, and `/beta` is only needed for beta features such as strict tool mode, chat prefix completion, and FIM completion.
- **Thinking + Tool Calls**: In V4 thinking mode, assistant messages that contain tool calls must replay their `reasoning_content` in all subsequent requests or the API returns HTTP 400.

## Important Notes

- **Token/cost tracking inaccuracies**: Token counting and cost estimation may be inflated due to thinking token accounting bugs. Use `/compact` to manage context, and treat cost estimates as approximate.
- **Modes**: Three modes — Plan (read-only investigation), Agent (tool use with approval), YOLO (auto-approved). See `docs/MODES.md` for details.
