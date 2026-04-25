# Configuration

DeepSeek TUI reads configuration from a TOML file plus environment variables.
At process startup it also loads a workspace-local `.env` file when present.
Use the tracked `.env.example` as the template; copy it to `.env`, then edit
only the provider and safety knobs you need.

## Where It Looks

Default config path:

- `~/.deepseek/config.toml`

Overrides:

- CLI: `deepseek --config /path/to/config.toml`
- Env: `DEEPSEEK_CONFIG_PATH=/path/to/config.toml`

If both are set, `--config` wins. Environment variable overrides are applied after the file is loaded.

The `deepseek` facade and `deepseek-tui` binary share the same config file for
DeepSeek auth and model defaults. `deepseek login --api-key ...` writes the
root `api_key` field that `deepseek-tui` reads directly, and `deepseek --model
deepseek-v4-flash` is forwarded to the TUI as `DEEPSEEK_MODEL`.

For NVIDIA NIM-hosted DeepSeek V4 Pro, set `provider = "nvidia-nim"` or pass
`deepseek --provider nvidia-nim`. The facade stores NIM credentials under
`[providers.nvidia_nim]` and forwards the resolved key, base URL, provider, and
model to the TUI process. Use
`deepseek auth set --provider nvidia-nim --api-key "YOUR_NVIDIA_API_KEY"` to
save the NIM key through the facade. `DEEPSEEK_API_KEY` remains a compatibility
fallback when `DEEPSEEK_PROVIDER=nvidia-nim`, but `NVIDIA_API_KEY` or
`NVIDIA_NIM_API_KEY` is preferred.

To bootstrap MCP and skills directories at their resolved paths, run `deepseek-tui setup`.
To only scaffold MCP, run `deepseek-tui mcp init`.

Note: setup, doctor, mcp, features, sessions, resume/fork, exec, review, and eval
are subcommands of the `deepseek-tui` binary. The `deepseek` dispatcher exposes a
distinct set of commands (`auth`, `config`, `model`, `thread`, `sandbox`,
`app-server`, `mcp-server`, `completion`) and forwards plain prompts to
`deepseek-tui`.

## Profiles

You can define multiple profiles in the same file:

```toml
api_key = "PERSONAL_KEY"
default_text_model = "deepseek-v4-pro"

[profiles.work]
api_key = "WORK_KEY"
base_url = "https://api.deepseek.com"

[profiles.nvidia-nim]
provider = "nvidia-nim"
api_key = "NVIDIA_KEY"
base_url = "https://integrate.api.nvidia.com/v1"
default_text_model = "deepseek-ai/deepseek-v4-pro"
```

Select a profile with:

- CLI: `deepseek --profile work`
- Env: `DEEPSEEK_PROFILE=work`

If a profile is selected but missing, DeepSeek TUI exits with an error listing available profiles.

## Environment Variables

These override config values:

- `DEEPSEEK_API_KEY`
- `DEEPSEEK_BASE_URL`
- `DEEPSEEK_PROVIDER` (`deepseek|nvidia-nim`)
- `DEEPSEEK_MODEL` or `DEEPSEEK_DEFAULT_TEXT_MODEL`
- `NVIDIA_API_KEY` or `NVIDIA_NIM_API_KEY` (preferred when provider is `nvidia-nim`; falls back to `DEEPSEEK_API_KEY`)
- `NVIDIA_NIM_BASE_URL`, `NIM_BASE_URL`, or `NVIDIA_BASE_URL`
- `NVIDIA_NIM_MODEL`
- `DEEPSEEK_LOG_LEVEL` or `RUST_LOG` (`info`/`debug`/`trace` enables lightweight verbose logs)
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
- `DEEPSEEK_CAPACITY_ENABLED`
- `DEEPSEEK_CAPACITY_LOW_RISK_MAX`
- `DEEPSEEK_CAPACITY_MEDIUM_RISK_MAX`
- `DEEPSEEK_CAPACITY_SEVERE_MIN_SLACK`
- `DEEPSEEK_CAPACITY_SEVERE_VIOLATION_RATIO`
- `DEEPSEEK_CAPACITY_REFRESH_COOLDOWN_TURNS`
- `DEEPSEEK_CAPACITY_REPLAN_COOLDOWN_TURNS`
- `DEEPSEEK_CAPACITY_MAX_REPLAY_PER_TURN`
- `DEEPSEEK_CAPACITY_MIN_TURNS_BEFORE_GUARDRAIL`
- `DEEPSEEK_CAPACITY_PROFILE_WINDOW`
- `DEEPSEEK_CAPACITY_PRIOR_CHAT`
- `DEEPSEEK_CAPACITY_PRIOR_REASONER`
- `DEEPSEEK_CAPACITY_PRIOR_FALLBACK`

## Settings File (Persistent UI Preferences)

DeepSeek TUI also stores user preferences in:

- `~/.config/deepseek/settings.toml`

Notable settings include `auto_compact` (default `true`), which automatically summarizes
earlier turns once the conversation grows large. You can inspect or update these from the
TUI with `/settings` and `/config` (interactive editor).

Common settings keys:

- `theme` (default, dark, light, whale)
- `auto_compact` (on/off)
- `show_thinking` (on/off)
- `show_tool_details` (on/off)
- `default_mode` (agent, plan, yolo; legacy `normal` is accepted and normalized to `agent`)
- `max_history` (number of input history entries)
- `default_model` (model name override)

Only `agent`, `plan`, and `yolo` are visible modes in the UI. For compatibility,
older settings files with `default_mode = "normal"` still load as `agent`, and
the hidden `/normal` slash command switches to `Agent`.

Readability semantics:

- Selection uses a unified style across transcript, composer menus, and modals.
- Footer hints use a dedicated semantic role (`FOOTER_HINT`) so hint text stays readable across themes.
- The footer includes a compact `coherence` chip that describes how stable and
  focused the current session is right now. Possible states are `healthy`,
  `crowded`, `refreshing`, `verifying`, and `resetting`; these are derived from
  capacity and compaction events without exposing internal formulas in normal UI.

### Command Migration Notes

If you are upgrading from older releases:

- Old: `/deepseek`
  New: `/links` (aliases: `/dashboard`, `/api`)
- Old: `/set model deepseek-reasoner`
  New: `/config` and edit the `model` row to `deepseek-v4-pro` or `deepseek-v4-flash`
- Old: visible `Normal` mode or `default_mode = "normal"`
  New: use `Agent` / `default_mode = "agent"`; legacy `normal` still maps to `agent`
- Old: discover `/set` in slash UX/help
  New: use `/config` for editing and `/settings` for read-only inspection

## Key Reference

### Core keys (used by the TUI/engine)

- `provider` (string, optional): `deepseek` (default) or `nvidia-nim`. `nvidia-nim` targets NVIDIA's NIM-hosted DeepSeek endpoints through `https://integrate.api.nvidia.com/v1`.
- `api_key` (string, required): must be non-empty (or set `DEEPSEEK_API_KEY`).
- `base_url` (string, optional): defaults to `https://api.deepseek.com` for DeepSeek's OpenAI-compatible Chat Completions API, or `https://integrate.api.nvidia.com/v1` for `provider = "nvidia-nim"`. `https://api.deepseek.com/v1` is also accepted for SDK compatibility; use `https://api.deepseek.com/beta` only for DeepSeek beta features such as strict tool mode, chat prefix completion, and FIM completion.
- `default_text_model` (string, optional): defaults to `deepseek-v4-pro` for DeepSeek or `deepseek-ai/deepseek-v4-pro` for NVIDIA NIM. Current public DeepSeek IDs are `deepseek-v4-pro` and `deepseek-v4-flash`, both with 1M context windows and thinking mode enabled by default. Legacy `deepseek-chat` and `deepseek-reasoner` remain compatibility aliases for `deepseek-v4-flash`. With `provider = "nvidia-nim"`, `deepseek-v4-pro` maps to `deepseek-ai/deepseek-v4-pro` and `deepseek-v4-flash` maps to `deepseek-ai/deepseek-v4-flash`. Use `/models` or `deepseek models` to discover live IDs from your configured endpoint. `DEEPSEEK_MODEL` overrides this for a single process.
- `reasoning_effort` (string, optional): `off`, `low`, `medium`, `high`, or `max`; defaults to the configured UI tier. DeepSeek Platform receives top-level `thinking` / `reasoning_effort` fields. NVIDIA NIM receives equivalent settings through `chat_template_kwargs`.
- `allow_shell` (bool, optional): defaults to `true` (sandboxed).
- `approval_policy` (string, optional): `on-request`, `untrusted`, or `never`. Runtime `approval_mode` editing in `/config` also accepts `on-request` and `untrusted` aliases.
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
- `capacity.*` (optional): runtime context-capacity controller:
  - `[capacity].enabled` (bool, default `true`)
  - `[capacity].low_risk_max` (float, default `0.34`)
  - `[capacity].medium_risk_max` (float, default `0.62`)
  - `[capacity].severe_min_slack` (float, default `-0.25`)
  - `[capacity].severe_violation_ratio` (float, default `0.40`)
  - `[capacity].refresh_cooldown_turns` (int, default `2`)
  - `[capacity].replan_cooldown_turns` (int, default `5`)
  - `[capacity].max_replay_per_turn` (int, default `1`)
  - `[capacity].min_turns_before_guardrail` (int, default `2`)
  - `[capacity].profile_window` (int, default `8`)
  - `[capacity].deepseek_v3_2_chat_prior` (float, default `3.9`)
  - `[capacity].deepseek_v3_2_reasoner_prior` (float, default `4.1`)
  - `[capacity].fallback_default_prior` (float, default `3.8`)
- `tui.alternate_screen` (string, optional): `auto`, `always`, or `never`. `auto` disables the alternate screen in Zellij; `--no-alt-screen` forces inline mode. Set `never` or run with `--no-alt-screen` when you want real terminal scrollback.
- `tui.mouse_capture` (bool, optional, default `true` when the alternate screen is active): enable internal mouse scrolling/transcript selection. Set this to `false` or run with `--no-mouse-capture` for terminal-native drag selection and highlight-to-copy.
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
web_search = true # enables canonical web.run plus the compatibility web_search alias
apply_patch = true
mcp = true
exec_policy = true
```

You can also override features for a single run:

- `deepseek-tui --enable web_search`
- `deepseek-tui --disable subagents`

Use `deepseek-tui features list` to inspect known flags and their effective state.

## Local Media Attachments

Use `@path/to/file` in the composer to add local text file or directory context
to the next message. Use `/attach <path>` for local image/video media paths, or
`Ctrl+V` to attach an image from the clipboard. DeepSeek's public Chat
Completions API currently accepts text message content, so media attachments are
sent as explicit local path references instead of native image/video payloads.

## Managed Configuration and Requirements

DeepSeek TUI supports a policy layering model:

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

See `docs/capacity_controller.md` for formulas, intervention behavior, and telemetry.

## Notes On `deepseek-tui doctor`

`deepseek-tui doctor` follows the same config resolution rules as the rest of the
TUI. That means `--config` / `DEEPSEEK_CONFIG_PATH` are respected, and MCP/skills
checks use the resolved `mcp_config_path` / `skills_dir` (including env overrides).

To bootstrap missing MCP/skills paths, run `deepseek-tui setup --all`. You can
also run `deepseek-tui setup --skills --local` to create a workspace-local
`./skills` dir.

`deepseek-tui doctor --json` prints a machine-readable report that skips the
live API connectivity probe. Top-level keys: `version`, `config_path`,
`config_present`, `workspace`, `api_key.source`, `base_url`,
`default_text_model`, `mcp`, `skills`, `tools`, `plugins`, `sandbox`,
`platform`, `api_connectivity`. CI consumers should rely on `api_key.source`
(`env`/`config`/`missing`) rather than parsing the human-readable `doctor`
text.

## Setup status, clean, and extension dirs

`deepseek-tui setup` accepts a few flags beyond the existing `--mcp`,
`--skills`, `--local`, `--all`, and `--force`:

- `--status` — print a compact one-screen status (api key, base URL, model,
  MCP/skills/tools/plugins counts, sandbox, `.env` presence). Read-only and
  network-free; safe to run in CI. If `.env` is missing and `.env.example` is
  present in the workspace, the status output points at `cp .env.example .env`.
- `--tools` — scaffold `~/.deepseek/tools/` with a `README.md` describing the
  self-describing frontmatter convention (`# name:` / `# description:` /
  `# usage:`) and an `example.sh` that follows it. The directory is
  intentionally not auto-loaded; wire individual scripts into the agent via
  MCP, hooks, or skills.
- `--plugins` — scaffold `~/.deepseek/plugins/` with a `README.md` and an
  `example/PLUGIN.md` placeholder using the same frontmatter shape as
  `SKILL.md`. Plugins are not loaded automatically either; reference them
  from a skill or MCP wrapper when you want them active.
- `--all` now scaffolds MCP + skills + tools + plugins together.
- `--clean` — list `~/.deepseek/sessions/checkpoints/latest.json` and
  `offline_queue.json` if they exist. Pass `--force` to actually remove them.
  This never touches real session history or the task queue.

`--status` and `--clean` are mutually exclusive with the scaffold flags.

## Why the engine strips XML/`[TOOL_CALL]` text

DeepSeek TUI sends and receives tool calls only over the API tool channel
(structured `tool_use` / `tool_call` items). The streaming loop in
`crates/tui/src/core/engine.rs` recognizes a fixed set of fake-wrapper start
markers — `[TOOL_CALL]`, `<deepseek:tool_call`, `<tool_call`, `<invoke `,
`<function_calls>` — and scrubs them from visible assistant text without ever
turning them into structured tool calls. When a wrapper is stripped, the loop
emits one compact `status` notice per turn so the user can see why their
visible text shrank. Treat any change that re-enables text-based tool
execution as a regression; the protocol-recovery tests in
`crates/tui/tests/protocol_recovery.rs` lock the contract.
