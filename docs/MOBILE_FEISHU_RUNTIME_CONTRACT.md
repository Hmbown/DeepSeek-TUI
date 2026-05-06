# Mobile / Feishu Runtime Control Contract

This document sketches a narrow contract for remote runtime control clients such
as a phone web page, Feishu/Lark bot, or other local operator surface.

It is a design note only. It does not change runtime behavior.

## Goals

- Let a user observe and steer an already-running DeepSeek runtime session from a
  small remote surface.
- Keep the runtime API local-first. There is no hosted relay in this contract.
- Make approval, interruption, and steering explicit actions rather than hidden
  automation.
- Preserve the existing thread/turn/event data model.
- Keep Feishu/Lark integration as a thin client over the same runtime contract,
  not a separate agent engine.

## Non-Goals

- No client memory injection, desktop automation, or OS-level remote control.
- No multi-tenant hosted service.
- No raw session database export.
- No provider API key exposure.
- No automatic tool approval by default.
- No claim that a phone or Feishu client is a replacement for the TUI.

## Actors

| Actor | Role |
|---|---|
| Runtime API | Owns threads, turns, events, tasks, tools, and approvals. |
| Operator client | Small UI or bot surface that lists threads, sends prompts, and handles approvals. |
| User | Human operator who decides when to send, steer, interrupt, approve, or deny. |
| LLM provider | Used only by the runtime process; tokens are never exposed to operator clients. |

## Transport

The contract should remain HTTP/SSE-compatible:

- HTTP for commands: create thread, start turn, steer, interrupt, approve, deny.
- SSE for replayable live events: turn lifecycle, item deltas, tool starts,
  approval requests, completion.

Feishu/Lark can use this contract through a bridge process:

```text
Feishu/Lark event -> bridge -> local runtime API -> SSE/event summary -> bridge -> Feishu/Lark message
```

The bridge is responsible for platform-specific message formatting and callback
verification. The runtime API should not become Feishu-specific.

## Authentication Boundary

Remote operator clients should not talk to `/v1/*` without a runtime token or a
stronger local network boundary.

Recommended minimum:

- `/health` remains public for local readiness checks.
- `/v1/*` accepts `Authorization: Bearer <token>`.
- Browser/EventSource-style clients may use a short-lived URL token if headers
  are not possible.
- Tokens are runtime-local secrets and must not be logged or sent to the LLM.

This is still a local/LAN convenience guard, not a replacement for TLS, VPN, or
reverse-proxy authentication on public networks.

## Core Resources

| Resource | Purpose |
|---|---|
| Thread | Durable conversation/work unit. |
| Turn | One active model/tool execution cycle. |
| Event | Replayable lifecycle record for clients. |
| Approval | Pending user decision for a tool or sandbox retry. |
| Task | Background or scheduled work. |

## Minimal Command Surface

| Capability | Example Endpoint Shape |
|---|---|
| List threads | `GET /v1/threads/summary` |
| Create thread | `POST /v1/threads` |
| Start turn | `POST /v1/threads/{id}/turns` |
| Stream events | `GET /v1/threads/{id}/events` |
| Steer active turn | `POST /v1/threads/{id}/turns/{turn_id}/steer` |
| Interrupt active turn | `POST /v1/threads/{id}/turns/{turn_id}/interrupt` |
| List pending approvals | `GET /v1/threads/{id}/turns/{turn_id}/approvals` |
| Approve pending approval | `POST /v1/threads/{id}/turns/{turn_id}/approvals/{approval_id}/approve` |
| Deny pending approval | `POST /v1/threads/{id}/turns/{turn_id}/approvals/{approval_id}/deny` |

These shapes are illustrative. They should be stabilized in smaller PRs before a
mobile page or Feishu bridge depends on them.

## Event Contract

Operator clients should be able to rebuild current UI state from event replay
plus live events.

Important events:

- `thread.started`
- `turn.started`
- `turn.lifecycle`
- `turn.steered`
- `turn.interrupt_requested`
- `turn.completed`
- `item.started`
- `item.delta`
- `item.completed`
- `item.failed`
- `approval.required`
- `approval.resolved`
- `sandbox.denied`

The event payload should remain machine-readable. Human text can be rendered by
clients, but clients should not have to parse prose to know what action is
available.

## Approval Semantics

Remote control should not require full auto-approve.

Recommended behavior:

- `auto_approve=false` keeps approval-required tools pending.
- Client receives an `approval.required` event.
- User approves or denies from the operator client.
- Approval decisions are persisted as events.
- Sandbox elevation should be explicit and visibly different from a normal
  approval.

This keeps remote operation useful without turning a phone or Feishu chat into a
silent tool-execution trigger.

## Feishu/Lark Bridge Notes

A Feishu/Lark bridge should be a separate adapter:

- Verify Feishu/Lark callback signatures outside the runtime core.
- Map user commands to runtime API calls.
- Format runtime events into short chat messages.
- Avoid sending raw session logs into chat by default.
- Avoid exposing provider API keys, config files, tool outputs, or local paths
  unless the runtime explicitly returns safe summaries.

The bridge should prefer short action buttons for common decisions:

- Approve
- Deny
- Interrupt
- Show latest event
- Open local dashboard link

## Suggested PR Slices

1. Optional runtime API token guard with tests.
2. Documented command/event/approval contract.
3. Approval endpoints without any UI.
4. Minimal mobile page or Feishu bridge prototype.
5. Hardening pass for token handling, URL token expiry, and audit events.

Each slice should be independently reviewable and avoid changing unrelated
runtime behavior.

