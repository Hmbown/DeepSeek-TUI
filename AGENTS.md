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

### Project: deepseek-tui

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

- **Finance tool currently unavailable**: The finance tool relies on Stooq which frequently returns no data. As a workaround, use `web.run` to fetch financial data from web sources.
- **Token/cost tracking inaccuracies**: Token counting and cost estimation may be inflated due to thinking token accounting bugs. Use `/compact` to manage context, and treat cost estimates as approximate.
- **Web.run tool name**: Note that the tool is named `web.run` (single dot), not `web..run`. Some earlier versions of the CLI may have had this typo.

### DeepSeek-Specific Capabilities

This project is built specifically for DeepSeek models, leveraging their unique features:

**Thinking Tokens**: DeepSeek models can output thinking blocks (`ContentBlock::Thinking`) before providing final answers. The TUI supports streaming and displaying thinking tokens with visual distinction. You can use thinking tokens to reason step-by-step before committing to a response.

**Reasoning Models**: DeepSeek offers specialized reasoning models (e.g., `deepseek-reasoner`, `deepseek-r1`) that excel at step-by-step problem solving. Consider using these models for complex tasks.

**Large Context Window**: DeepSeek models have 128k context windows, allowing you to process large codebases. Use `project_map` and `file_search` to navigate efficiently.

**DeepSeek API**: The CLI uses DeepSeek's OpenAI‑compatible API with support for the Responses API endpoint. The base URL can be configured for global (`api.deepseek.com`) or China (`api.deepseeki.com`).

**Web Browsing**: For up‑to‑date information about DeepSeek models, documentation, or API changes, use `web.run` with citations. Example search: “DeepSeek API documentation”.

### Dogfooding Tips

As a DeepSeek model working on this project, you are “dogfooding” your own tool. Use this opportunity to:
- Test the toolset thoroughly and report any issues.
- Suggest improvements that would make DeepSeek models more effective.
- Keep changes small, focused, and well‑tested.

Remember to run `cargo test` and `cargo check` after any changes.
