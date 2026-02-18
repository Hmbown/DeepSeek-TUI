# Runtime API (HTTP/SSE)

DeepSeek CLI can expose a local runtime API for external clients:

```bash
deepseek serve --http --host 127.0.0.1 --port 7878 --workers 2
```

Defaults:
- bind: `127.0.0.1:7878`
- workers: `2` (clamped to `1..8`)

## Security Model (Local-First)

- The server is designed for trusted local use.
- There is no built-in auth, user isolation, or TLS termination.
- Do not expose this API directly to untrusted networks.
- If remote access is required, place it behind your own authenticated reverse proxy/VPN.

## Runtime Data Model

The runtime uses a durable Thread/Turn/Item lifecycle.

- `ThreadRecord`
  - `id`, `created_at`, `updated_at`
  - `model`, `workspace`, `mode`
  - `latest_turn_id`, `latest_response_bookmark`, `archived`
- `TurnRecord`
  - `id`, `thread_id`
  - `status`: `queued|in_progress|completed|failed|interrupted|canceled`
  - timestamps, duration, usage, error summary
- `TurnItemRecord`
  - `id`, `turn_id`
  - `kind`: `user_message|agent_message|tool_call|file_change|command_execution|context_compaction|status|error`
  - lifecycle `status`: `queued|in_progress|completed|failed|interrupted|canceled`

The event log is append-only with global monotonic `seq` for replay/resume.

## Endpoints

### Health and Session

- `GET /health`
- `GET /v1/sessions?limit=50&search=<substring>`
- `GET /v1/workspace/status`
- `GET /v1/skills`
- `GET /v1/apps/mcp/servers`
- `GET /v1/apps/mcp/tools?server=<optional>`

### Compatibility Stream (Single Turn)

- `POST /v1/stream`

Backwards-compatible one-shot SSE wrapper. Internally creates an archived runtime thread+turn.

Request body:

```json
{
  "prompt": "Summarize recent commits",
  "model": "deepseek-reasoner",
  "mode": "agent",
  "workspace": ".",
  "allow_shell": false,
  "trust_mode": false,
  "auto_approve": true
}
```

Typical SSE events:
- `turn.started`
- `message.delta`
- `tool.started`
- `tool.progress`
- `tool.completed`
- `approval.required`
- `sandbox.denied`
- `status`
- `error`
- `turn.completed`
- `done`

### Thread Lifecycle

- `POST /v1/threads`
- `GET /v1/threads?limit=50&include_archived=false`
- `GET /v1/threads/summary?limit=50&search=<optional>&include_archived=false`
- `GET /v1/threads/{id}`
- `PATCH /v1/threads/{id}` (currently supports `{ "archived": true|false }`)
- `POST /v1/threads/{id}/resume`
- `POST /v1/threads/{id}/fork`

Create thread request example:

```json
{
  "model": "deepseek-reasoner",
  "workspace": ".",
  "mode": "agent",
  "allow_shell": false,
  "trust_mode": false,
  "auto_approve": true,
  "archived": false
}
```

### Turn Lifecycle

- `POST /v1/threads/{id}/turns`
- `POST /v1/threads/{id}/turns/{turn_id}/steer`
- `POST /v1/threads/{id}/turns/{turn_id}/interrupt`
- `POST /v1/threads/{id}/compact`

Notes:
- Only one active turn is allowed per thread (`409 Conflict` on overlap).
- `interrupt` returns quickly and marks `turn.interrupt_requested`.
- Terminal turn status becomes `interrupted` only after cleanup completes.
- Manual compaction is exposed as a turn with `context_compaction` item lifecycle events.
- Archiving/unarchiving threads updates persisted thread state and emits `thread.updated`.

### Replayable Events

- `GET /v1/threads/{id}/events?since_seq=<u64>`

Returns SSE replay backlog, then live events for that thread.

SSE payload shape:

```json
{
  "seq": 42,
  "timestamp": "2026-02-11T20:18:49.123Z",
  "thread_id": "thr_1234abcd",
  "turn_id": "turn_5678efgh",
  "item_id": "item_90ab12cd",
  "event": "item.delta",
  "payload": {
    "delta": "partial output",
    "kind": "agent_message"
  }
}
```

Common event names:
- `thread.started`
- `thread.forked`
- `turn.started`
- `turn.lifecycle`
- `turn.steered`
- `turn.interrupt_requested`
- `turn.completed`
- `item.started`
- `item.delta`
- `item.completed`
- `item.failed`
- `item.interrupted`
- `approval.required`
- `sandbox.denied`

Compaction visibility:
- auto compaction emits `item.started`/`item.completed` with item kind `context_compaction` and `auto=true`
- manual compaction emits the same with `auto=false`

### Background Tasks

- `GET /v1/tasks`
- `POST /v1/tasks`
- `GET /v1/tasks/{id}`
- `POST /v1/tasks/{id}/cancel`

Tasks execute through the same runtime thread/turn pipeline and include:
- linked `thread_id` / `turn_id`
- runtime event count
- timeline + tool summaries + artifact references

### Automations

- `GET /v1/automations`
- `POST /v1/automations`
- `GET /v1/automations/{id}`
- `PATCH /v1/automations/{id}`
- `DELETE /v1/automations/{id}`
- `POST /v1/automations/{id}/run`
- `POST /v1/automations/{id}/pause`
- `POST /v1/automations/{id}/resume`
- `GET /v1/automations/{id}/runs?limit=20`

RRULE support is intentionally constrained to:
- hourly: `FREQ=HOURLY;INTERVAL=<hours>[;BYDAY=MO,TU,...]`
- weekly: `FREQ=WEEKLY;BYDAY=...;BYHOUR=<0-23>;BYMINUTE=<0-59>`

Automations are persisted under `~/.deepseek/automations` (override with `DEEPSEEK_AUTOMATIONS_DIR`).
Each run is executed as a normal background task and links to task/thread/turn ids.

## Persistence

Runtime store (default under task data root):
- `runtime/threads/*.json`
- `runtime/turns/*.json`
- `runtime/items/*.json`
- `runtime/events/{thread_id}.jsonl`
- `runtime/state.json` (monotonic sequence)

Task store:
- default `~/.deepseek/tasks` (override with `DEEPSEEK_TASKS_DIR`)

Both runtime and task state are restart-safe.
