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
- **Reasoning Models**: `deepseek-reasoner` and `deepseek-r1` excel at step-by-step problem solving.
- **Large Context Window**: 128k tokens. Use search tools to navigate efficiently.
- **API**: OpenAI-compatible with Responses API preferred, chat completions as fallback. Base URL configurable for global (`api.deepseek.com`) or China (`api.deepseeki.com`).

## Important Notes

- **Token/cost tracking inaccuracies**: Token counting and cost estimation may be inflated due to thinking token accounting bugs. Use `/compact` to manage context, and treat cost estimates as approximate.
- **Modes**: Three modes — Plan (read-only investigation), Agent (tool use with approval), YOLO (auto-approved). See `docs/MODES.md` for details.
