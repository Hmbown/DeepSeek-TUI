# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Build and Development Commands

**Requirements:** Rust 1.85+ (edition 2024)

```bash
# Build
cargo build              # Debug build
cargo build --release    # Release build

# Run
cargo run                # Run debug build (opens TUI)
cargo run -- --yolo      # YOLO mode: agent tools + shell execution
cargo run -- -p "prompt" # One-shot prompt mode

# Lint and Format
cargo fmt                # Format code
cargo fmt --check        # Check formatting (CI uses this)
cargo clippy             # Run linter
cargo clippy --all-targets --all-features  # Full lint check (CI uses this)

# Test
cargo test               # Run all tests
cargo test --all-features  # Run with all features (CI uses this)
cargo test test_name     # Run a single test by name
cargo test -- --nocapture  # Run tests with stdout visible

# Documentation
cargo doc --no-deps      # Build docs

# Debug Logging
RUST_LOG=debug cargo run            # Enable debug logging
RUST_LOG=wagmii_cli=trace cargo run  # Trace-level for this crate only


```

## Architecture Overview

This is a Rust CLI application for chatting with the Wagmii API. The architecture follows an event-driven model with clear separation between UI and core logic.

### Core Flow

```
User Input -> TUI (tui/app.rs) -> Op messages -> Engine (core/engine.rs) -> LLM Client -> Events -> TUI
                                                        |
                                                        v
                                                  Tool Execution
                                                  (tools/*.rs)
```

### Key Components

**Entry & CLI** (`main.rs`)
- Clap-based CLI with subcommands: `doctor`, `completions`, `sessions`, `init`
- Routes to TUI for interactive mode or one-shot for `-p` prompts

**Engine** (`core/engine.rs`)
- Async agent loop running in background task
- Communicates via `Op` (operations in) and `Event` (events out) channels
- Handles streaming responses, tool execution, cancellation

**LLM Layer** (`client.rs`, `llm_client.rs`)
- `WagmiiClient`: HTTP client for Wagmii's OpenAI-compatible Responses API (with chat fallback)
- Streaming handler (Responses -> internal events), retry logic with exponential backoff

**Tool System** (`tools/`)
- `ToolRegistry` + `ToolRegistryBuilder` pattern for tool registration
- Built-in tools: shell, file ops, search, todo, plan, subagent
- Tools receive `ToolContext` with workspace path, approval settings

**Extension Systems**
- **MCP** (`mcp.rs`): Model Context Protocol for external tool servers, configured in `~/.wagmii/mcp.json`
- **Skills** (`skills.rs`): Plugin system, skills live in `~/.wagmii/skills/` with `SKILL.md` files
- **Hooks** (`hooks.rs`): Lifecycle hooks (session_start, tool_call_before, etc.) configured in `config.toml`

**TUI** (`tui/`)
- Ratatui-based terminal UI with streaming support
- `app.rs`: Application state, message handling
- `approval.rs`: Tool approval dialogs (non-YOLO mode)

**Sandbox** (`sandbox/`)
- macOS: Seatbelt profiles (`seatbelt.rs` generates profiles, `policy.rs` defines policies)
- Linux: Landlock support (`landlock.rs`)

### Application Modes

The CLI has several modes, each with different tool availability and approval behavior. Mode is tracked in `AppMode` enum and affects tool registration in `engine.rs`:

- **Normal**: Basic chat, asks for approval on file writes and shell
- **Plan**: Design-first prompting, same approvals as Normal
- **Agent**: Multi-step tool use, asks for shell only
- **YOLO**: All tools auto-approved, shell enabled (dangerous)
- **RLM**: External context mode for large files, auto-approves tools, adds RLM-specific tools (`rlm_load`, `rlm_exec`, `rlm_query`, `rlm_status`)
- **Duo**: Player-coach autocoding paradigm, adds Duo-specific tools (`duo_init`, `duo_player`, `duo_coach`, `duo_advance`, `duo_status`)

Tool registration is mode-gated in `engine.rs` via builder methods like `.with_rlm_tools()` and `.with_duo_tools()`.

### Configuration Files

- `~/.wagmii/config.toml` - API keys, default model, hooks
- `~/.wagmii/mcp.json` - MCP server definitions
- `~/.wagmii/skills/` - User-defined skills
- `AGENTS.md` (per-project) - Project-specific agent instructions

### Adding a New Tool

1. Create a struct implementing `ToolSpec` trait in `tools/` (see `tools/spec.rs` for trait definition)
2. Implement: `name()`, `description()`, `input_schema()`, `capabilities()`, `execute()`
3. Add a builder method in `ToolRegistryBuilder` (in `tools/registry.rs`) like `.with_*_tools()`
4. Call the builder method in `engine.rs` where tools are registered for each mode

### Wagmii API Integration

Wagmii exposes OpenAI-compatible endpoints. The CLI prefers the Responses API:

- `https://api.wagmii.com/v1/responses` - preferred
- `https://api.wagmii.com/v1/chat/completions` - fallback if Responses is unavailable

**Base URLs:**
- Global: `https://api.wagmii.com` (default)
- China: `https://api.wagmiii.com` (set via `WAGMII_BASE_URL`)

**Implementation:**
- `src/client.rs` `WagmiiClient` uses Responses with automatic chat fallback
- `core/engine.rs` uses `handle_wagmii_turn()` for chat turns
- Tool calls are expressed as structured tool items in Responses

**Debug Logging:**
- `RUST_LOG=wagmii_cli::client=debug` - HTTP requests/responses
- `RUST_LOG=wagmii_cli::core::engine=debug` - Agent loop events
- `RUST_LOG=wagmii_cli::tools=debug` - Tool execution

### Commit Messages

Use conventional commits: `feat:`, `fix:`, `docs:`, `refactor:`, `test:`, `chore:`
