# AGENTS.md

This directory is a remote travel workspace, not a single project.

Expected layout:

- `deepseek-tui/` - canonical runtime/bridge checkout. The supported CLI is
  `deepseek`; install both `crates/cli` and `crates/tui`.
- `whalescale/` - product repo. Active surface is `whalescale-desktop/`.
- `worktrees/` - remote worktrees created on this VPS.

Operational rules:

- Treat `/opt/whalebro` as the workspace root for phone-controlled work.
- Keep `deepseek serve --http` bound to `127.0.0.1`.
- Use SSH keys for Git remotes and never paste secrets into prompts, logs, or
  committed files.
- Mac-only release tasks such as iOS simulator runs, `.app` packaging, DMG
  verification, notarization, and Apple signing still need the local Mac.
- If a project has its own `AGENTS.md`, read it before editing inside that
  project.
