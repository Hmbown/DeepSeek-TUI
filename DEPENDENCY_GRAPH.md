# Dependency Graph

## Crate Dependencies (from Cargo.toml)

```
deepseek-tui (binary: `deepseek-tui`)
  (no workspace deps — uses monolith src/ directly)

deepseek-tui-cli (binary: `deepseek`)
  <- deepseek-agent
  <- deepseek-app-server
  <- deepseek-config
  <- deepseek-execpolicy
  <- deepseek-mcp
  <- deepseek-state

deepseek-app-server
  <- deepseek-agent
  <- deepseek-config
  <- deepseek-core
  <- deepseek-execpolicy
  <- deepseek-hooks
  <- deepseek-mcp
  <- deepseek-protocol
  <- deepseek-state
  <- deepseek-tools

deepseek-core (agent loop)
  <- deepseek-agent
  <- deepseek-config
  <- deepseek-execpolicy
  <- deepseek-hooks
  <- deepseek-mcp
  <- deepseek-protocol
  <- deepseek-state
  <- deepseek-tools

deepseek-tools      <- deepseek-protocol
deepseek-mcp        <- deepseek-protocol
deepseek-hooks      <- deepseek-protocol
deepseek-execpolicy <- deepseek-protocol
deepseek-agent      <- deepseek-config

deepseek-config     (leaf — no internal deps)
deepseek-protocol   (leaf — no internal deps)
deepseek-state      (leaf — no internal deps)
deepseek-tui-core   (leaf — no internal deps)
```

Note: `deepseek-tui` has zero workspace deps because it still compiles the
monolith source tree (`src/main.rs`). The crate split is structural — actual
source migration into individual crates is incremental.

## Build Order (bottom-up)

```
Layer 0 (leaves):  deepseek-protocol, deepseek-config, deepseek-state, deepseek-tui-core
Layer 1:           deepseek-tools, deepseek-mcp, deepseek-hooks, deepseek-execpolicy
Layer 2:           deepseek-agent
Layer 3:           deepseek-core
Layer 4:           deepseek-app-server, deepseek-tui
Layer 5:           deepseek-tui-cli
```

## Task Dependencies (Linear: shannon-labs/deepseek-tui)

Canonical source: https://linear.app/shannon-labs/project/deepseek-tui-6213bbbeaa26

```
[High] SHA-2794  UI Footer Redesign (Kimi CLI Style)                ← DONE (v0.3.31)
  -> landed: mode/model/token/cost layout, quadrant separators, context bar
  -> remaining polish tracked in AI_HANDOFF.md

[High] SHA-2795  Thinking vs Normal Chat Delineation                ← DONE (v0.3.31)
  -> landed: labeled delimiters, separate transcript cell, show_thinking

[High] SHA-2798  Finance Tool Replacement                           ← DONE (v0.3.31)
  -> landed: Yahoo Finance v8 + CoinGecko fallback

[Med]  SHA-2796  Intelligent Compaction UX                          ← DONE (v0.3.31)
  -> landed: auto-compaction, /compact, status strip, CompactionCompleted stats

[Med]  SHA-2797  Escape Key After Plan Mode
  -> base fix landed (v0.3.31); remaining: regression test coverage  ← READY
  -> files: crates/tui/src/tui/ui.rs, app.rs

[Med]  SHA-2799  "Alive and Animated" Feel
  -> was blocked by SHA-2794, SHA-2795 (now done)                   ← READY
  -> files: crates/tui/src/tui/ (various)

[Med]  SHA-2801  Docs and Workflow Update
  -> was blocked by SHA-2798 (now done)                             ← READY
  -> files: AGENTS.md, README.md, CHANGELOG.md

[Med]  SHA-2802  Release Prep
  -> was blocked by SHA-2794, SHA-2795, SHA-2798 (all done)        ← READY
  -> files: Cargo.toml, CHANGELOG.md, npm/

[Low]  SHA-2800  Header Redesign
  -> was blocked by SHA-2794 (now done)                             ← READY
  -> files: crates/tui/src/tui/widgets/header.rs

[Low]  SHA-2803  Context Window Visualization
  -> blocked by SHA-2800
  -> files: crates/tui/src/tui/ui.rs
```

## Ready Queue (unblocked, by priority)

1. **SHA-2797** Escape Key regression test (Medium)
2. **SHA-2799** "Alive and Animated" Feel (Medium)
3. **SHA-2801** Docs and Workflow Update (Medium)
4. **SHA-2802** Release Prep (Medium)
5. **SHA-2800** Header Redesign (Low)
