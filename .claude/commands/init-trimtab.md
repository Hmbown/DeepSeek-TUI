# /init-trimtab

Bootstrap or retune the Trimtab closed-loop workflow for this repository.

**Canonical protocol:** `.trimtab/init-trimtab-protocol.md`

## What this command does

1. Reads the canonical protocol from `.trimtab/init-trimtab-protocol.md`
2. Inspects the repo's current state (docs, CI, task surfaces, dependency graph)
3. Decides whether to bootstrap, upgrade, or retune
4. Aligns all workflow surfaces to the protocol

## Invocation

Run `/init-trimtab` in Claude Code to initialize or retune the workflow.

## Surfaces managed

| File | Purpose |
|------|---------|
| `.trimtab/init-trimtab-protocol.md` | Canonical shared protocol |
| `.claude/commands/init-trimtab.md` | This file (Claude Code entrypoint) |
| `.codex/skills/init-trimtab/SKILL.md` | Codex skill registration |
| `DEPENDENCY_GRAPH.md` | Crate + task dependency graph |
| `AI_HANDOFF.md` | Operative task queue and context for next agent |
| `CLAUDE.md` | Build/dev instructions (read-only — do not overwrite) |
| `AGENTS.md` | Project instructions for AI assistants |

## Rules

- Do not overwrite CLAUDE.md — it is the source of truth for build commands
- Do not flatten existing strong instructions into boilerplate
- The no-self-verdict rule is non-negotiable
- Keep Claude and Codex entrypoints thin; logic lives in the shared protocol
