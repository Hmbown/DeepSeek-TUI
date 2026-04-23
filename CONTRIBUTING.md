# Contributing to DeepSeek TUI

Thank you for your interest in contributing to DeepSeek TUI! This document provides guidelines and instructions for contributing.

## Getting Started

### Prerequisites

- Rust 1.85 or later (edition 2024)
- Cargo package manager
- Git

### Setting Up Development Environment

1. Fork and clone the repository:
   ```bash
   git clone https://github.com/YOUR_USERNAME/DeepSeek-TUI.git
   cd DeepSeek-TUI
   ```

2. Build the project:
   ```bash
   cargo build
   ```

3. Run tests:
   ```bash
   cargo test
   ```

4. Run with development settings:
   ```bash
   cargo run
   ```

## Development Workflow

### Code Style

- Run `cargo fmt` before committing to ensure consistent formatting
- Run `cargo clippy` and address all warnings
- Follow Rust naming conventions (snake_case for functions/variables, CamelCase for types)
- Add documentation comments for public APIs

### Testing

- Write tests for new functionality
- Ensure all existing tests pass: `cargo test --workspace --all-features`
- Colocate unit tests beside the code they cover (standard Rust `#[cfg(test)]`
  modules), and add integration tests under the owning crate's `tests/`
  directory (for example `crates/tui/tests/` or `crates/state/tests/`). The
  repository root `tests/` directory is not used

### Commit Messages

Use clear, descriptive commit messages following conventional commits:

- `feat:` New feature
- `fix:` Bug fix
- `docs:` Documentation changes
- `refactor:` Code refactoring
- `test:` Adding or updating tests
- `chore:` Maintenance tasks

Example: `feat: add doctor subcommand for system diagnostics`

## Project Structure

DeepSeek TUI is a Cargo workspace. The live runtime and the majority of TUI,
engine, and tool code currently live in `crates/tui/src/`. Smaller workspace
crates provide shared abstractions that are being extracted incrementally.

```
crates/
├── tui/           deepseek-tui binary (interactive TUI + runtime API)
├── cli/           deepseek binary (dispatcher facade)
├── app-server/    HTTP/SSE + JSON-RPC transport
├── core/          Agent loop / session / turn management
├── protocol/      Request/response framing
├── config/        Config loading, profiles, env precedence
├── state/         SQLite thread/session persistence
├── tools/         Typed tool specs and lifecycle
├── mcp/           MCP client + stdio server
├── hooks/         Lifecycle hooks (stdout/jsonl/webhook)
├── execpolicy/    Approval/sandbox policy engine
├── agent/         Model/provider registry
└── tui-core/      Event-driven TUI state machine scaffold
```

See [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) for the live data flow across
these crates and [DEPENDENCY_GRAPH.md](DEPENDENCY_GRAPH.md) for build ordering.

## Submitting Changes

1. Create a feature branch from `main`:
   ```bash
   git checkout -b feat/your-feature
   ```

2. Make your changes and commit them

3. Ensure CI passes:
   ```bash
   cargo fmt --check
   cargo clippy
   cargo test
   ```

4. Push your branch and create a Pull Request

5. Describe your changes clearly in the PR description

## Pull Request Guidelines

- Keep PRs focused on a single change
- Update documentation if needed
- Add tests for new functionality
- Ensure CI passes before requesting review

## Reporting Issues

When reporting issues, please include:

- Operating system and version
- Rust version (`rustc --version`)
- DeepSeek TUI version (`deepseek --version`)
- Steps to reproduce the issue
- Expected vs actual behavior
- Relevant error messages or logs

## Code of Conduct

Be respectful and inclusive. We welcome contributors of all backgrounds and experience levels.

## License

By contributing to DeepSeek TUI, you agree that your contributions will be licensed under the MIT License.

## Questions?

Feel free to open an issue for any questions about contributing.
