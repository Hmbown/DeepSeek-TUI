# Configuration

DeepSeek CLI reads configuration from a TOML file plus environment variables.

## Where It Looks

Default config path:

- `~/.deepseek/config.toml`

Overrides:

- CLI: `deepseek --config /path/to/config.toml`
- Env: `DEEPSEEK_CONFIG_PATH=/path/to/config.toml`

If both are set, `--config` wins. Environment variable overrides are applied after the file is loaded.

## Profiles

You can define multiple profiles in the same file:

```toml
api_key = "PERSONAL_KEY"
default_text_model = "deepseek-reasoner"

[profiles.work]
api_key = "WORK_KEY"
base_url = "https://api.deepseek.com"
```

Select a profile with:

- CLI: `deepseek --profile work`
- Env: `DEEPSEEK_PROFILE=work`

If a profile is selected but missing, DeepSeek CLI exits with an error listing available profiles.

## Environment Variables

These override config values:

- `DEEPSEEK_API_KEY`
- `DEEPSEEK_BASE_URL`
- `DEEPSEEK_SKILLS_DIR`
- `DEEPSEEK_MCP_CONFIG`
- `DEEPSEEK_NOTES_PATH`
- `DEEPSEEK_MEMORY_PATH`
- `DEEPSEEK_ALLOW_SHELL` (`1`/`true` enables)
- `DEEPSEEK_MAX_SUBAGENTS` (clamped to `1..=20`)

## Settings File (Persistent UI Preferences)

DeepSeek CLI also stores user preferences in:

- `~/.config/deepseek/settings.toml`

Notable settings include `auto_compact` (default `true`), which automatically summarizes
earlier turns once the conversation grows large. You can inspect or update these from the
TUI with `/settings` and `/set <key> <value>`.

Common settings keys:

- `theme` (default, dark, light)
- `auto_compact` (on/off)
- `show_thinking` (on/off)
- `show_tool_details` (on/off)
- `default_mode` (normal, agent, plan, yolo, rlm, duo)
- `max_history` (number of input history entries)
- `default_model` (model name override)

## Key Reference

### Core keys (used by the TUI/engine)

- `api_key` (string, required): must be non-empty (or set `DEEPSEEK_API_KEY`).
- `base_url` (string, optional): defaults to `https://api.deepseek.com` (OpenAI-compatible Responses API).
- `default_text_model` (string, optional): defaults to `deepseek-reasoner`. Other available models include `deepseek-chat`, `deepseek-r1`, `deepseek-v3`, `deepseek-v3.2`. Check the DeepSeek API for the latest model list.
- `allow_shell` (bool, optional): defaults to `false`.
- `max_subagents` (int, optional): defaults to `5` and is clamped to `1..=20`.
- `skills_dir` (string, optional): defaults to `~/.deepseek/skills` (each skill is a directory containing `SKILL.md`).
- `mcp_config_path` (string, optional): defaults to `~/.deepseek/mcp.json`.
- `notes_path` (string, optional): defaults to `~/.deepseek/notes.txt` and is used by the `note` tool.
- `memory_path` (string, optional): defaults to `~/.deepseek/memory.md`.
- `retry.*` (optional): retry/backoff settings for API requests:
  - `[retry].enabled` (bool, default `true`)
  - `[retry].max_retries` (int, default `3`)
  - `[retry].initial_delay` (float seconds, default `1.0`)
  - `[retry].max_delay` (float seconds, default `60.0`)
  - `[retry].exponential_base` (float, default `2.0`)
- `tui.alternate_screen` (string, optional): `auto`, `always`, or `never`. `auto` disables the alternate screen in Zellij; `--no-alt-screen` forces inline mode.
- `hooks` (optional): lifecycle hooks configuration (see `config.example.toml`).
- `features.*` (optional): feature flag overrides (see below).

### Parsed but currently unused (reserved for future versions)

These keys are accepted by the config loader but not currently used by the interactive TUI or built-in tools:

- `tools_file`

## Feature Flags

Feature flags live under the `[features]` table and are merged across profiles.
Defaults are enabled for built-in tooling, so you only need to set entries you
want to force on or off.

```toml
[features]
shell_tool = true
subagents = true
web_search = true
apply_patch = true
mcp = true
rlm = true
duo = true
exec_policy = true
```

You can also override features for a single run:

- `deepseek --enable web_search`
- `deepseek --disable subagents`

Use `deepseek features list` to inspect known flags and their effective state.

## Notes On `deepseek doctor`

`deepseek doctor` checks default locations under `~/.deepseek/` (including `config.toml` and `mcp.json`). If you override paths via `--config` or `DEEPSEEK_MCP_CONFIG`, the doctor output may not reflect those overrides.
