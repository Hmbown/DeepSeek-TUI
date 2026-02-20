# Contributing to Wagmii CLI

Thank you for your interest in contributing to Wagmii CLI! This document provides guidelines and instructions for contributing.

## Getting Started

### Prerequisites

- Rust 1.85 or later (edition 2024)
- Cargo package manager
- Git

### Setting Up Development Environment

1. Fork and clone the repository:
   ```bash
   git clone https://github.com/YOUR_USERNAME/Wagmii-CLI.git
   cd Wagmii-CLI
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
- Ensure all existing tests pass: `cargo test`
- For integration tests, use the `tests/` directory

### Commit Messages

Use clear, descriptive commit messages following conventional commits:

- `feat:` New feature
- `fix:` Bug fix
- `docs:` Documentation changes
- `refactor:` Code refactoring
- `test:` Adding or updating tests
- `chore:` Maintenance tasks

Example: `feat: add --doctor command for system diagnostics`

## Project Structure

```
src/
├── main.rs           # Entry point and CLI definition
├── config.rs         # Configuration management
├── client.rs         # HTTP client for Wagmii API
├── llm_client.rs     # LLM abstraction layer
├── models.rs         # Data structures
├── mcp.rs            # Model Context Protocol support
├── hooks.rs          # Hook system for extensibility
├── skills.rs         # Skills/plugin system
├── core/             # Core engine components
│   ├── engine.rs     # Main agent loop
│   ├── session.rs    # Session management
│   └── ...
├── tools/            # Built-in tools
│   ├── shell.rs      # Shell execution
│   ├── file.rs       # File operations
│   └── ...
├── tui/              # Terminal UI
│   ├── app.rs        # Application state
│   ├── ui.rs         # Rendering logic
│   └── ...
└── sandbox/          # Sandbox execution (macOS)
```

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
- Wagmii CLI version (`wagmii --version`)
- Steps to reproduce the issue
- Expected vs actual behavior
- Relevant error messages or logs

## Code of Conduct

Be respectful and inclusive. We welcome contributors of all backgrounds and experience levels.

## License

By contributing to Wagmii CLI, you agree that your contributions will be licensed under the MIT License.

## Questions?

Feel free to open an issue for any questions about contributing.
