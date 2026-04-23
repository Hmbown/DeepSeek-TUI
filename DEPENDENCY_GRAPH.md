# Dependency Graph

## Crate Dependencies (from Cargo.toml)

```
deepseek-tui (binary: `deepseek-tui`)
  (no workspace deps — monolith source under crates/tui/src/)

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
monolith source tree (`crates/tui/src/main.rs`). The crate split is
structural — source migration into individual workspace crates is
incremental.

## Build Order (bottom-up)

```
Layer 0 (leaves):  deepseek-protocol, deepseek-config, deepseek-state, deepseek-tui-core
Layer 1:           deepseek-tools, deepseek-mcp, deepseek-hooks, deepseek-execpolicy
Layer 2:           deepseek-agent
Layer 3:           deepseek-core
Layer 4:           deepseek-app-server, deepseek-tui
Layer 5:           deepseek-tui-cli
```

