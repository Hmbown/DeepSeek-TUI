# Configuration

DeepSeek CLI reads configuration from a TOML file plus environment variables.

## Where It Looks

Default config path:

- `~/.deepseek/config.toml`

Overrides:

- CLI: `deepseek --config /path/to/config.toml`
- Env: `DEEPSEEK_CONFIG_PATH=/path/to/config.toml`

If both are set, `--config` wins. Environment variable overrides are applied after the file is loaded.

To bootstrap MCP and skills directories at their resolved paths, run `deepseek setup`.
To only scaffold MCP, run `deepseek mcp init`.

## Profiles

You can define multiple profiles in the same file:

```toml
api_key = "PERSONAL_KEY"
default_text_model = "deepseek-v3.2"

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
- `DEEPSEEK_APPROVAL_POLICY` (`on-request|untrusted|never`)
- `DEEPSEEK_SANDBOX_MODE` (`read-only|workspace-write|danger-full-access|external-sandbox`)
- `DEEPSEEK_MANAGED_CONFIG_PATH`
- `DEEPSEEK_REQUIREMENTS_PATH`
- `DEEPSEEK_MAX_SUBAGENTS` (clamped to `1..=20`)
- `DEEPSEEK_TASKS_DIR` (runtime task queue/artifact storage, default `~/.deepseek/tasks`)
- `DEEPSEEK_ALLOW_INSECURE_HTTP` (`1`/`true` allows non-local `http://` base URLs; default is reject)

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
- `default_mode` (normal, agent, plan, yolo)
- `max_history` (number of input history entries)
- `default_model` (model name override)

## Key Reference

### Core keys (used by the TUI/engine)

- `api_key` (string, required): must be non-empty (or set `DEEPSEEK_API_KEY`).
- `base_url` (string, optional): defaults to `https://api.deepseek.com` (OpenAI-compatible Responses API).
- `default_text_model` (string, optional): defaults to `deepseek-v3.2`. Any valid DeepSeek model ID is accepted; common IDs include `deepseek-chat`, `deepseek-reasoner`, `deepseek-r1`, `deepseek-v3`, and `deepseek-v3.2`. Check the DeepSeek API for the latest model list.
- `allow_shell` (bool, optional): defaults to `false`.
- `approval_policy` (string, optional): `on-request`, `untrusted`, or `never`. Runtime `/set approval_mode` also accepts `on-request` and `untrusted` aliases.
- `sandbox_mode` (string, optional): `read-only`, `workspace-write`, `danger-full-access`, `external-sandbox`.
- `managed_config_path` (string, optional): managed config file loaded after user/env config.
- `requirements_path` (string, optional): requirements file used to enforce allowed approval/sandbox values.
- `max_subagents` (int, optional): defaults to `5` and is clamped to `1..=20`.
- `skills_dir` (string, optional): defaults to `~/.deepseek/skills` (each skill is a directory containing `SKILL.md`). Workspace-local `.agents/skills` or `./skills` are preferred when present.
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
web_search = true # enables web.run and web_search
apply_patch = true
mcp = true
exec_policy = true
```

You can also override features for a single run:

- `deepseek --enable web_search`
- `deepseek --disable subagents`

Use `deepseek features list` to inspect known flags and their effective state.

## Managed Configuration and Requirements

DeepSeek CLI supports a policy layering model:

1. user config + profile + env overrides
2. managed config (if present)
3. requirements validation (if present)

By default on Unix:
- managed config: `/etc/deepseek/managed_config.toml`
- requirements: `/etc/deepseek/requirements.toml`

Requirements file shape:

```toml
allowed_approval_policies = ["on-request", "untrusted", "never"]
allowed_sandbox_modes = ["read-only", "workspace-write"]
```

If configured values violate requirements, startup fails with a descriptive error.

## Notes On `deepseek doctor`

`deepseek doctor` now follows the same config resolution rules as the rest of the CLI.
That means `--config` / `DEEPSEEK_CONFIG_PATH` are respected, and MCP/skills checks
use the resolved `mcp_config_path` / `skills_dir` (including env overrides).

To bootstrap missing MCP/skills paths, run `deepseek setup --all`. You can also
run `deepseek setup --skills --local` to create a workspace-local `./skills` dir.
