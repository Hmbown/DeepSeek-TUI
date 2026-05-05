# MCP Integration: context-mode + claude-mem — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Enable DeepSeek TUI to connect to existing context-mode and claude-mem MCP servers by adding global config support, workspace-aware env injection, and a version-independent plugin path resolver.

**Architecture:** Add a `McpPool::from_workspace_config` constructor that merges `~/.deepseek/mcp.json` (global) with workspace-level config. Auto-inject `CONTEXT_MODE_PROJECT_DIR` for context-mode connections. Bump MCP protocol version to 2025-03-26. Ship a shell script that resolves latest plugin versions so config survives upgrades.

**Tech Stack:** Rust (tokio async, serde_json), Bash (plugin path resolver)

---

### Task 1: Add `McpPool::from_workspace_config` and workspace injection

**Files:**
- Modify: `crates/tui/src/mcp.rs` (McpPool impl, ~line 976-1015)

- [ ] **Step 1: Add `workspace_dir` field to `McpPool`**

```rust
// In McpPool struct, after `network_policy` field (~line 973):
pub struct McpPool {
    connections: HashMap<String, McpConnection>,
    config: McpConfig,
    network_policy: Option<NetworkPolicyDecider>,
    workspace_dir: Option<PathBuf>,
}
```

- [ ] **Step 2: Update `McpPool::new` to initialize the new field**

```rust
// In McpPool::new(), replace the constructor body:
pub fn new(config: McpConfig) -> Self {
    Self {
        connections: HashMap::new(),
        config,
        network_policy: None,
        workspace_dir: None,
    }
}
```

- [ ] **Step 3: Add `McpPool::from_workspace_config` constructor**

```rust
/// Merge global config (~/.deepseek/mcp.json) with workspace config.
/// Workspace entries override global entries with the same server name.
pub fn from_workspace_config(workspace_config_path: &std::path::Path) -> Result<Self> {
    let global_path = dirs::home_dir()
        .unwrap_or_default()
        .join(".deepseek")
        .join("mcp.json");
    let mut merged = load_config(&global_path).unwrap_or_default();
    let workspace_cfg = load_config(workspace_config_path)?;
    // workspace overrides global for same-named servers
    for (name, server) in workspace_cfg.servers {
        merged.servers.insert(name, server);
    }
    Ok(Self::new(merged))
}
```

Add `use std::path::PathBuf;` if not already imported.

- [ ] **Step 4: Add `McpPool::with_workspace` setter**

```rust
/// Set the workspace directory, used to inject CONTEXT_MODE_PROJECT_DIR
/// for context-mode MCP connections.
pub fn with_workspace(mut self, workspace: PathBuf) -> Self {
    self.workspace_dir = Some(workspace);
    self
}
```

- [ ] **Step 5: Inject `CONTEXT_MODE_PROJECT_DIR` in `get_or_connect`**

After `server_config` is cloned and before calling `McpConnection::connect_with_policy`, inject the env var if the server is context-mode and a workspace is set:

```rust
// In get_or_connect(), after the line:
//   let server_config = self.config.servers.get(server_name)...clone();
// Add:

let mut server_config = server_config;
if server_name == "context-mode" {
    if let Some(ref workspace) = self.workspace_dir {
        server_config
            .env
            .entry("CONTEXT_MODE_PROJECT_DIR".to_string())
            .or_insert_with(|| workspace.display().to_string());
    }
}
```

- [ ] **Step 6: Commit**

```bash
git add crates/tui/src/mcp.rs
git commit -m "feat(mcp): add from_workspace_config and workspace injection for context-mode"
```

---

### Task 2: Bump MCP protocol version

**Files:**
- Modify: `crates/tui/src/mcp.rs:660`

- [ ] **Step 1: Change protocol version string**

```rust
// Line 660, in McpConnection::initialize():
// Before:
"protocolVersion": "2024-11-05",
// After:
"protocolVersion": "2025-03-26",
```

- [ ] **Step 2: Commit**

```bash
git add crates/tui/src/mcp.rs
git commit -m "fix(mcp): bump protocol version to 2025-03-26"
```

---

### Task 3: Update engine call site

**Files:**
- Modify: `crates/tui/src/core/engine.rs` (ensure_mcp_pool, ~line 1392)

- [ ] **Step 1: Replace `from_config_path` with `from_workspace_config`**

```rust
// Line 1392, in Engine::ensure_mcp_pool():
// Before:
let mut pool = McpPool::from_config_path(&self.session.mcp_config_path)
    .map_err(|e| ToolError::execution_failed(format!("Failed to load MCP config: {e}")))?;
// After:
let mut pool = McpPool::from_workspace_config(&self.session.mcp_config_path)
    .map_err(|e| ToolError::execution_failed(format!("Failed to load MCP config: {e}")))?;
pool = pool.with_workspace(self.session.workspace.clone());
```

- [ ] **Step 2: Commit**

```bash
git add crates/tui/src/core/engine.rs
git commit -m "feat(engine): use from_workspace_config for MCP pool init"
```

---

### Task 4: Create plugin path resolver script

**Files:**
- Create: `scripts/mcp-resolve.sh`

- [ ] **Step 1: Create `scripts/mcp-resolve.sh`**

```bash
#!/usr/bin/env bash
# Resolves latest installed version of a claude-mem or context-mode plugin.
# Usage: mcp-resolve.sh <plugin-name> <relative-script-path>
#   plugin-name: "claude-mem" | "context-mode"
#   relative-script-path: path inside the version directory
#
# Output: absolute path to the script (trailing newline)
# Exit: 1 if plugin not found

set -euo pipefail

PLUGIN_CACHE="${HOME}/.claude/plugins/cache"
PLUGIN_NAME="$1"
SCRIPT_PATH="$2"

case "${PLUGIN_NAME}" in
  claude-mem)
    base="${PLUGIN_CACHE}/thedotmack/claude-mem"
    ;;
  context-mode)
    base="${PLUGIN_CACHE}/context-mode/context-mode"
    ;;
  *)
    echo "mcp-resolve.sh: unknown plugin '${PLUGIN_NAME}'" >&2
    exit 1
    ;;
esac

if [[ ! -d "${base}" ]]; then
  echo "mcp-resolve.sh: plugin directory not found: ${base}" >&2
  exit 1
fi

latest=$(ls -d "${base}"/*/ 2>/dev/null | sort -V | tail -1)
if [[ -z "${latest}" ]]; then
  echo "mcp-resolve.sh: no version directories found under ${base}" >&2
  exit 1
fi

echo "${latest}${SCRIPT_PATH}"
```

- [ ] **Step 2: Make it executable**

```bash
chmod +x scripts/mcp-resolve.sh
```

- [ ] **Step 3: Commit**

```bash
git add scripts/mcp-resolve.sh
git commit -m "feat: add mcp-resolve.sh for version-independent plugin paths"
```

---

### Task 5: Create example MCP config

**Files:**
- Create: `config.example.mcp.json`

- [ ] **Step 1: Create `config.example.mcp.json`**

```json
{
  "_comment": "Example MCP server configuration for DeepSeek TUI. Copy to ~/.deepseek/mcp.json for global use, or .deepseek/mcp.json for per-workspace servers.",
  "timeouts": {
    "connect_timeout": 10,
    "execute_timeout": 60,
    "read_timeout": 120
  },
  "mcpServers": {
    "claude-mem": {
      "_comment": "Cross-session memory and search from Claude Code sessions",
      "command": "bash",
      "args": [
        "-c",
        "exec bun \"$(~/.deepseek/mcp-resolve.sh claude-mem scripts/mcp-server.cjs)\""
      ]
    },
    "context-mode": {
      "_comment": "Sandboxed execution, FTS5 knowledge base, context savings",
      "command": "bash",
      "args": [
        "-c",
        "exec node \"$(~/.deepseek/mcp-resolve.sh context-mode start.mjs)\""
      ]
    }
  }
}
```

- [ ] **Step 2: Commit**

```bash
git add config.example.mcp.json
git commit -m "docs: add example MCP config for claude-mem and context-mode"
```

---

### Task 6: Write unit test for config merge

**Files:**
- Modify: `crates/tui/src/mcp.rs` (tests module, ~line 1859)

- [ ] **Step 1: Add test for `from_workspace_config` merge behavior**

In the `mod tests` block at the end of `mcp.rs`:

```rust
fn make_server(command: &str, args: &[&str]) -> McpServerConfig {
    McpServerConfig {
        command: Some(command.to_string()),
        args: args.iter().map(|s| s.to_string()).collect(),
        env: HashMap::new(),
        url: None,
        connect_timeout: None,
        execute_timeout: None,
        read_timeout: None,
        disabled: false,
        enabled: true,
        required: false,
        enabled_tools: Vec::new(),
        disabled_tools: Vec::new(),
    }
}

#[test]
fn test_workspace_overrides_global() {
    // from_workspace_config reads ~/.deepseek/mcp.json which varies per
    // machine, so this tests the merge logic in isolation.

    let mut global = McpConfig::default();
    global.servers.insert(
        "claude-mem".to_string(),
        make_server("bun", &["global-mem.cjs"]),
    );
    global.servers.insert(
        "shared".to_string(),
        make_server("node", &["global-shared.js"]),
    );

    let mut workspace = McpConfig::default();
    workspace.servers.insert(
        "context-mode".to_string(),
        make_server("node", &["workspace-ctx.mjs"]),
    );
    workspace.servers.insert(
        "shared".to_string(),
        make_server("node", &["workspace-shared.js"]),
    );

    // Merge (same logic as from_workspace_config)
    for (name, server) in workspace.servers {
        global.servers.insert(name, server);
    }

    assert_eq!(global.servers.len(), 3);
    assert!(global.servers.contains_key("claude-mem"));
    assert!(global.servers.contains_key("context-mode"));
    assert!(global.servers.contains_key("shared"));
    // workspace override wins
    assert_eq!(
        global.servers["shared"].args,
        vec!["workspace-shared.js"]
    );
}

#[test]
fn test_load_config_parses_minimal_server() {
    let dir = tempfile::tempdir().unwrap();
    let ws_config = dir.path().join("mcp.json");
    std::fs::write(
        &ws_config,
        r#"{"servers": {"test": {"command": "echo", "args": ["hi"]}}}"#,
    )
    .unwrap();

    let pool = McpPool::from_config_path(&ws_config).unwrap();
    assert!(pool.config.servers.contains_key("test"));
}
```

Add `use std::collections::HashMap;` at the top of the test module if not already imported.

- [ ] **Step 2: Run the tests**

```bash
cargo test -p deepseek-tui -- mcp
```

Expected: all existing MCP tests pass, plus the two new ones.

- [ ] **Step 3: Commit**

```bash
git add crates/tui/src/mcp.rs
git commit -m "test(mcp): add from_workspace_config merge behavior test"
```

---

### Task 7: Build and verify

- [ ] **Step 1: Build the project**

```bash
cargo build
```

Expected: compile cleanly, no warnings.

- [ ] **Step 2: Run the full MCP test suite**

```bash
cargo test -p deepseek-tui -- mcp
```

Expected: all tests pass.

- [ ] **Step 3: Run clippy**

```bash
cargo clippy -p deepseek-tui -- -D warnings
```

Expected: no warnings.

- [ ] **Step 4: Commit any remaining changes**

Only if needed for clippy fixes or build tweaks.
