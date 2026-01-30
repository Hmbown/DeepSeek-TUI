# Project Instructions

This file provides context for AI assistants working on this project.

## Project Type: Rust

### Commands
- Build: `cargo build`
- Test: `cargo test`
- Run: `cargo run`
- Check: `cargo check`
- Format: `cargo fmt`
- Lint: `cargo clippy`

### Project: deepseek-cli

### Documentation
See README.md for project overview.

### Version Control
This project uses Git. See .gitignore for excluded files.


## Advanced Capabilities

### Model Context Protocol (MCP)
This CLI supports MCP for extending tool access. 
- Use `mcp_read_resource` to read context from external servers.
- Use `mcp_get_prompt` to leverage pre-defined expert prompts from servers.
- You can connect to HTTP/SSE servers by adding their URL to `mcp.json`.

### Multi-Agent Orchestration
For complex, multi-step tasks, you should delegate work:
- **Sub-agents**: Use `agent_spawn` (or its alias `delegate_to_agent`) to launch a background assistant for a specific sub-task. Use `agent_result` to get their output.
- **Swarms**: Use `agent_swarm` to orchestrate multiple sub-agents with dependencies. This is ideal for parallel exploration or complex refactoring where different parts of the project can be analyzed concurrently.

### Project Mapping
- Use `project_map` to get a comprehensive view of the codebase structure. This tool respects `.gitignore` and provides a summary of key files.

## Guidelines

- **Proactive Investigation**: Always start by exploring the codebase using `project_map` and `file_search`.
- **Parallelism**: When you need to read multiple files or search across different areas, use parallel tool calls if possible.
- **Delegation**: If a task is large, break it down into sub-tasks and use `agent_swarm` or `agent_spawn`.
- **Testing**: Rigorously verify changes using `cargo test` and `cargo check`.

## Important Notes

<!-- Add project-specific notes here -->
