# DeepSeek CLI Architecture

This document provides an overview of the DeepSeek CLI architecture for developers and contributors.

## High-Level Overview

```
┌─────────────────────────────────────────────────────────────────┐
│                         User Interface                          │
│  ┌─────────────────┐  ┌─────────────────┐  ┌────────────────┐  │
│  │   TUI (ratatui) │  │  One-shot Mode  │  │  Config/CLI    │  │
│  └────────┬────────┘  └────────┬────────┘  └────────┬───────┘  │
└───────────┼─────────────────────┼────────────────────┼──────────┘
            │                     │                    │
            ▼                     ▼                    ▼
┌─────────────────────────────────────────────────────────────────┐
│                        Core Engine                              │
│  ┌─────────────────────────────────────────────────────────┐   │
│  │                    Agent Loop (core/engine.rs)           │   │
│  │  ┌─────────┐  ┌─────────────┐  ┌──────────────────────┐ │   │
│  │  │ Session │  │ Turn Mgmt   │  │ Tool Orchestration   │ │   │
│  │  └─────────┘  └─────────────┘  └──────────────────────┘ │   │
│  └─────────────────────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────────────────┘
            │                     │                    │
            ▼                     ▼                    ▼
┌─────────────────────────────────────────────────────────────────┐
│                     Tool & Extension Layer                      │
│  ┌──────────┐  ┌──────────┐  ┌─────────┐  ┌────────────────┐   │
│  │  Tools   │  │  Skills  │  │  Hooks  │  │  MCP Servers   │   │
│  │ (shell,  │  │ (plugins)│  │ (pre/   │  │  (external)    │   │
│  │  file)   │  │          │  │  post)  │  │                │   │
│  └──────────┘  └──────────┘  └─────────┘  └────────────────┘   │
└─────────────────────────────────────────────────────────────────┘
            │                     │                    │
            ▼                     ▼                    ▼
┌─────────────────────────────────────────────────────────────────┐
│                  Runtime API + Task Management                  │
│  ┌─────────────────────────────┐  ┌──────────────────────────┐  │
│  │ HTTP/SSE Runtime API        │  │ Persistent Task Manager  │  │
│  │ (runtime_api.rs)            │  │ (task_manager.rs)        │  │
│  └─────────────────────────────┘  └──────────────────────────┘  │
└─────────────────────────────────────────────────────────────────┘
            │                     │
            ▼                     ▼
┌─────────────────────────────────────────────────────────────────┐
│                        LLM Layer                                │
│  ┌──────────────────────────────────────────────────────────┐  │
│  │              LLM Client Abstraction (llm_client.rs)       │  │
│  │  ┌─────────────────┐  ┌─────────────────────────────┐    │  │
│  │  │  DeepSeek Client │  │  Compatible Client (DeepSeek)│    │  │
│  │  │   (client.rs)   │  │       (client.rs)           │    │  │
│  │  └─────────────────┘  └─────────────────────────────┘    │  │
│  └──────────────────────────────────────────────────────────┘  │
└─────────────────────────────────────────────────────────────────┘
```

## Module Organization

### Entry Point

- **`main.rs`** - CLI argument parsing (clap), configuration loading, entry point routing

### Core Components

- **`core/`** - Main engine components
  - `engine.rs` - Agent loop, message processing, tool execution orchestration
  - `session.rs` - Session state management
  - `turn.rs` - Turn-based conversation handling
  - `events.rs` - Event system for UI updates
  - `ops.rs` - Core operations

### Configuration

- **`config.rs`** - Configuration loading, profiles, environment variables
- **`settings.rs`** - Runtime settings management

### LLM Integration

- **`client.rs`** - HTTP client for DeepSeek's OpenAI-compatible Responses API (with chat fallback)
- **`llm_client.rs`** - Abstract LLM client trait with retry logic
- **`models.rs`** - Data structures for API requests/responses

#### DeepSeek API Endpoints

DeepSeek exposes OpenAI-compatible endpoints. The CLI uses:
- `https://api.deepseek.com/v1/responses` - preferred Responses API
- `https://api.deepseek.com/v1/chat/completions` - fallback if Responses is unavailable

The engine uses `handle_deepseek_turn()` to drive the agent loop against the
Responses API (with automatic fallback if needed).

### Tool System

- **`tools/`** - Built-in tool implementations
  - `mod.rs` - Tool registry and common types
  - `shell.rs` - Shell command execution
  - `file.rs` - File read/write operations
  - `todo.rs` - Todo list management
  - `plan.rs` - Planning tools
  - `subagent.rs` - Sub-agent spawning
  - `spec.rs` - Tool specifications

### Extension Systems

- **`mcp.rs`** - Model Context Protocol client for external tool servers
- **`skills.rs`** - Plugin/skill loading and execution
- **`hooks.rs`** - Pre/post execution hooks with conditions

### User Interface

- **`tui/`** - Terminal UI components (ratatui-based)
  - `app.rs` - Application state and message handling
  - `ui.rs` - Rendering logic
  - `approval.rs` - Tool approval dialog
  - `clipboard.rs` - Clipboard handling
  - `streaming.rs` - Streaming text collector

- **`ui.rs`** - Legacy/simple UI utilities

### Security

- **`sandbox/`** - macOS sandboxing support
  - `mod.rs` - Sandbox type definitions
  - `policy.rs` - Sandbox policy configuration
  - `seatbelt.rs` - macOS Seatbelt profile generation

### Utilities

- **`utils.rs`** - Common utilities
- **`logging.rs`** - Logging infrastructure
- **`compaction.rs`** - Context compaction for long conversations
- **`pricing.rs`** - Cost estimation
- **`prompts.rs`** - System prompt templates
- **`project_doc.rs`** - Project documentation handling
- **`session.rs`** - Session serialization
- **`runtime_api.rs`** - HTTP/SSE runtime API (`deepseek serve --http`)
- **`runtime_threads.rs`** - Durable thread/turn/item store + replayable event timeline
- **`task_manager.rs`** - Durable queue, worker pool, task timelines and artifacts

## Data Flow

### Interactive Session

1. User input received in TUI
2. Input processed by `core/engine.rs`
3. Message sent to LLM via `llm_client.rs`
4. Response streamed back, parsed in `client.rs`
5. Tool calls extracted and executed via `tools/`
6. Hooks triggered before/after tool execution
7. Results aggregated and sent back to LLM
8. Final response rendered in TUI

### Crash Recovery + Offline Queue

1. Before sending user input, the TUI writes a checkpoint snapshot to `~/.deepseek/sessions/checkpoints/latest.json`
2. Startup remains fresh by default; prior sessions are resumed explicitly via `--resume`/`--continue` (or `Ctrl+R` in TUI)
3. While degraded/offline, new prompts are queued in-memory and mirrored to `~/.deepseek/sessions/checkpoints/offline_queue.json`
4. Queue edits (`/queue ...`) are persisted continuously so drafts and queued prompts survive restarts
5. Successful turn completion clears the active checkpoint and writes a durable session snapshot

### Tool Execution

1. LLM requests tool via `tool_use` content block
2. Tool registry looks up handler
3. Pre-execution hooks run
4. Approval requested if needed (non-yolo mode)
5. Tool executed (possibly sandboxed on macOS)
6. Post-execution hooks run
7. Result returned to agent loop

### Background Tasks

1. Client enqueues task (`/task add ...` or `POST /v1/tasks`)
2. `task_manager.rs` persists task + queue entry under `~/.deepseek/tasks`
3. Worker picks queued task (bounded pool), transitions to `running`
4. Task creates/uses a runtime thread and starts a runtime turn
5. `runtime_threads.rs` persists thread/turn/item records + monotonic event sequence
6. Timeline/tool summaries/artifact references are persisted incrementally
7. Final state (`completed|failed|canceled`) is durable and queryable via TUI/API

### Runtime Thread/Turn Timeline

1. API/TUI creates or resumes a thread (`/v1/threads*`)
2. Turn starts on the thread (`/v1/threads/{id}/turns`)
3. Engine events are mapped to item lifecycle events (`item.started|item.delta|item.completed`)
4. Interrupt/steer operations apply to the active turn only
5. Compaction (auto/manual) is emitted as `context_compaction` item lifecycle
6. Clients replay history and resume with `/v1/threads/{id}/events?since_seq=<n>`

### Durable Schema Gates

- `session_manager.rs`, `runtime_threads.rs`, and `task_manager.rs` embed `schema_version` on persisted records.
- On load, newer schema versions are rejected with explicit errors instead of silently truncating/overwriting data.
- This allows safe forward migrations and prevents corruption when binaries and stored state are out of sync.

## Extension Points

### Adding a New Tool

1. Create handler in `tools/`
2. Register in `tools/registry.rs`
3. Add tool specification (name, description, input schema)

### Adding an MCP Server

1. Configure in `~/.deepseek/mcp.json`
2. Server auto-discovered at startup
3. Tools exposed to LLM automatically

### Creating a Skill

1. Create skill directory with `SKILL.md`
2. Define skill prompt and optional scripts
3. Place in `~/.deepseek/skills/`

### Adding Hooks

Configure in `~/.deepseek/config.toml`:

```toml
[[hooks]]
event = "tool_call_before"
command = "echo 'Running tool: $TOOL_NAME'"
```

## Key Design Decisions

1. **Streaming-first**: All LLM responses stream for responsiveness
2. **Tool safety**: Non-yolo mode requires approval for destructive operations, including side-effectful MCP tools
3. **Extensibility**: MCP, skills, and hooks allow customization without code changes
4. **Cross-platform**: Core works on Linux/macOS/Windows, sandboxing macOS-only
5. **Minimal dependencies**: Careful dependency selection for build speed
6. **Local-first runtime API**: HTTP/SSE endpoints are intended for trusted localhost access

## Configuration Files

- `~/.deepseek/config.toml` - Main configuration
- `/etc/deepseek/managed_config.toml` - Optional managed defaults layer (Unix)
- `/etc/deepseek/requirements.toml` - Optional allowed-policy constraints (Unix)
- `~/.deepseek/mcp.json` - MCP server configuration
- `~/.deepseek/skills/` - User skills directory
- `~/.deepseek/sessions/` - Session history
- `~/.deepseek/sessions/checkpoints/` - Crash checkpoint + offline queue persistence
- `~/.deepseek/tasks/` - Background task records, queue, timelines, artifacts
- `~/.deepseek/audit.log` - Append-only audit events for credential + approval/elevation actions
