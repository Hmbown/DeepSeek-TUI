# DeepSeek App (Web + Desktop)

Standalone DeepSeek-themed Codex-style app built with Next.js + Tauri.

## Features

- Three-pane layout: rail, threads, chat/composer
- Thread lifecycle: create, resume, fork, send turns, steer, interrupt
- Live SSE updates from runtime events
- Automations with RRULE subset (hourly/weekly)
- Skills + MCP servers/tools views
- Workspace status panel
- Runtime endpoint settings (default `http://127.0.0.1:7878`)

## Run (Web)

```bash
pnpm --filter deepseek-app dev
```

## Run (Desktop)

```bash
pnpm --filter deepseek-app tauri:dev
```

Tauri startup auto-checks runtime API health and spawns:

```bash
deepseek serve --http --host 127.0.0.1 --port 7878 --workers 2
```

if not already running.

If runtime bootstrap fails (binary missing, startup timeout, or port conflict), the desktop shell still launches and shows offline/disconnected state instead of crashing.

## Build

```bash
pnpm --filter deepseek-app build
pnpm --filter deepseek-app tauri:build
```

## Test and Typecheck

```bash
pnpm --filter deepseek-app typecheck
pnpm --filter deepseek-app test
pnpm --filter deepseek-app lint
```

## Runtime host/port overrides (desktop bootstrap)

Set before launching Tauri dev/build:

- `DEEPSEEK_APP_RUNTIME_HOST`
- `DEEPSEEK_APP_RUNTIME_PORT`
