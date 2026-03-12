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
[High] SHA-2794  UI Footer Redesign (Kimi CLI Style)
  -> no blockers                                          ← READY
  -> files: crates/tui/src/tui/ui.rs, crates/tui/src/palette.rs

[High] SHA-2795  Thinking vs Normal Chat Delineation
  -> no blockers                                          ← READY
  -> files: crates/tui/src/tui/ui.rs, history.rs, streaming.rs

[High] SHA-2798  Finance Tool Replacement
  -> no blockers                                          ← READY
  -> files: crates/tui/src/tools/

[Med]  SHA-2796  Intelligent Compaction UX
  -> no blockers                                          ← READY
  -> files: crates/tui/src/compaction.rs, core/engine.rs

[Med]  SHA-2797  Escape Key After Plan Mode
  -> no blockers (investigation)                          ← READY
  -> files: crates/tui/src/tui/ui.rs, app.rs

[Med]  SHA-2799  "Alive and Animated" Feel
  -> blocked by SHA-2794, SHA-2795
  -> files: crates/tui/src/tui/ (various)

[Med]  SHA-2801  Docs and Workflow Update
  -> blocked by SHA-2798
  -> files: AGENTS.md, README.md, CHANGELOG.md

[Med]  SHA-2802  Release Prep
  -> blocked by SHA-2794, SHA-2795, SHA-2798
  -> files: Cargo.toml, CHANGELOG.md, npm/

[Low]  SHA-2800  Header Redesign
  -> blocked by SHA-2794
  -> files: crates/tui/src/tui/widgets/header.rs

[Low]  SHA-2803  Context Window Visualization
  -> blocked by SHA-2800
  -> files: crates/tui/src/tui/ui.rs
```

## Ready Queue (unblocked, by priority)

1. **SHA-2794** UI Footer Redesign (High)
2. **SHA-2795** Thinking vs Chat Delineation (High)
3. **SHA-2798** Finance Tool Replacement (High)
4. **SHA-2796** Intelligent Compaction UX (Medium)
5. **SHA-2797** Escape Key After Plan Mode (Medium)
