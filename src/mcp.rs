//! Async MCP (Model Context Protocol) Implementation
//!
//! This module provides full async support for MCP servers with:
//! - Connection pooling for server reuse
//! - Automatic tool discovery via `tools/list`
//! - Configurable timeouts per-server and globally
//! - Backward compatibility with existing sync API

#![allow(dead_code)]

use std::collections::HashMap;
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt};
use tokio::process::{Child, ChildStdin, ChildStdout};

// === Configuration Types ===

/// Full MCP configuration from mcp.json
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct McpConfig {
    #[serde(default)]
    pub timeouts: McpTimeouts,
    #[serde(default, alias = "mcpServers")]
    pub servers: HashMap<String, McpServerConfig>,
}

/// Global timeout configuration
#[derive(Debug, Clone, Copy, Deserialize, Serialize)]
#[allow(clippy::struct_field_names)]
pub struct McpTimeouts {
    #[serde(default = "default_connect_timeout")]
    pub connect_timeout: u64,
    #[serde(default = "default_execute_timeout")]
    pub execute_timeout: u64,
    #[serde(default = "default_read_timeout")]
    pub read_timeout: u64,
}

fn default_connect_timeout() -> u64 {
    10
}
fn default_execute_timeout() -> u64 {
    60
}
fn default_read_timeout() -> u64 {
    120
}

impl Default for McpTimeouts {
    fn default() -> Self {
        Self {
            connect_timeout: default_connect_timeout(),
            execute_timeout: default_execute_timeout(),
            read_timeout: default_read_timeout(),
        }
    }
}

/// Configuration for a single MCP server
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct McpServerConfig {
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: HashMap<String, String>,
    #[serde(default)]
    pub connect_timeout: Option<u64>,
    #[serde(default)]
    pub execute_timeout: Option<u64>,
    #[serde(default)]
    pub read_timeout: Option<u64>,
    #[serde(default)]
    pub disabled: bool,
}

impl McpServerConfig {
    pub fn effective_connect_timeout(&self, global: &McpTimeouts) -> u64 {
        self.connect_timeout.unwrap_or(global.connect_timeout)
    }

    pub fn effective_execute_timeout(&self, global: &McpTimeouts) -> u64 {
        self.execute_timeout.unwrap_or(global.execute_timeout)
    }

    pub fn effective_read_timeout(&self, global: &McpTimeouts) -> u64 {
        self.read_timeout.unwrap_or(global.read_timeout)
    }
}

// === MCP Tool Definition ===

/// Tool discovered from an MCP server
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct McpTool {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(rename = "inputSchema", default)]
    pub input_schema: serde_json::Value,
}

// === Connection State ===

/// State of an MCP connection
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionState {
    Connecting,
    Ready,
    Disconnected,
}

// === McpConnection - Async Connection Management ===

/// Manages a single async connection to an MCP server
pub struct McpConnection {
    name: String,
    _child: Child,
    stdin: ChildStdin,
    reader: tokio::io::BufReader<ChildStdout>,
    tools: Vec<McpTool>,
    request_id: AtomicU64,
    state: ConnectionState,
    config: McpServerConfig,
}

impl McpConnection {
    /// Connect to an MCP server and initialize it
    pub async fn connect(
        name: String,
        config: McpServerConfig,
        global_timeouts: &McpTimeouts,
    ) -> Result<Self> {
        let connect_timeout_secs = config.effective_connect_timeout(global_timeouts);

        let mut cmd = tokio::process::Command::new(&config.command);
        cmd.args(&config.args)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .kill_on_drop(true);

        for (key, value) in &config.env {
            cmd.env(key, value);
        }

        let mut child = cmd
            .spawn()
            .with_context(|| format!("Failed to spawn MCP server '{name}'"))?;

        let stdin = child.stdin.take().context("Failed to get MCP stdin")?;
        let stdout = child.stdout.take().context("Failed to get MCP stdout")?;

        let mut conn = Self {
            name: name.clone(),
            _child: child,
            stdin,
            reader: tokio::io::BufReader::new(stdout),
            tools: Vec::new(),
            request_id: AtomicU64::new(1),
            state: ConnectionState::Connecting,
            config,
        };

        // Initialize with timeout
        tokio::time::timeout(Duration::from_secs(connect_timeout_secs), conn.initialize())
            .await
            .with_context(|| format!("MCP server '{name}' initialization timed out"))??;

        // Discover tools with timeout
        tokio::time::timeout(
            Duration::from_secs(connect_timeout_secs),
            conn.discover_tools(),
        )
        .await
        .with_context(|| format!("MCP server '{name}' tool discovery timed out"))??;

        conn.state = ConnectionState::Ready;
        Ok(conn)
    }

    /// Send initialize request and wait for response
    async fn initialize(&mut self) -> Result<()> {
        let init_id = self.next_id();
        self.send(serde_json::json!({
            "jsonrpc": "2.0",
            "id": init_id,
            "method": "initialize",
            "params": {
                "protocolVersion": "2024-11-05",
                "clientInfo": {
                    "name": "deepseek-cli",
                    "version": env!("CARGO_PKG_VERSION")
                },
                "capabilities": { "tools": {} }
            }
        }))
        .await?;

        self.recv(init_id).await?;

        // Send initialized notification (no id, no response expected)
        self.send(serde_json::json!({
            "jsonrpc": "2.0",
            "method": "notifications/initialized"
        }))
        .await?;

        Ok(())
    }

    /// Discover available tools from the MCP server
    async fn discover_tools(&mut self) -> Result<()> {
        let list_id = self.next_id();
        self.send(serde_json::json!({
            "jsonrpc": "2.0",
            "id": list_id,
            "method": "tools/list",
            "params": {}
        }))
        .await?;

        let response = self.recv(list_id).await?;

        if let Some(result) = response.get("result")
            && let Some(tools) = result.get("tools")
        {
            self.tools = serde_json::from_value(tools.clone()).unwrap_or_default();
        }

        Ok(())
    }

    /// Call a tool on this MCP server
    pub async fn call_tool(
        &mut self,
        tool_name: &str,
        arguments: serde_json::Value,
        timeout_secs: u64,
    ) -> Result<serde_json::Value> {
        if self.state != ConnectionState::Ready {
            anyhow::bail!(
                "Failed to call MCP tool: connection '{}' is not ready",
                self.name
            );
        }

        let call_id = self.next_id();
        self.send(serde_json::json!({
            "jsonrpc": "2.0",
            "id": call_id,
            "method": "tools/call",
            "params": {
                "name": tool_name,
                "arguments": arguments
            }
        }))
        .await?;

        let response = tokio::time::timeout(Duration::from_secs(timeout_secs), self.recv(call_id))
            .await
            .with_context(|| {
                format!(
                    "MCP tool '{}' on server '{}' timed out after {}s",
                    tool_name, self.name, timeout_secs
                )
            })??;

        if let Some(error) = response.get("error") {
            return Err(anyhow::anyhow!(
                "MCP error: {}",
                serde_json::to_string_pretty(error)?
            ));
        }

        Ok(response
            .get("result")
            .cloned()
            .unwrap_or(serde_json::json!(null)))
    }

    /// Get discovered tools
    pub fn tools(&self) -> &[McpTool] {
        &self.tools
    }

    /// Get server name
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Check if connection is ready
    pub fn is_ready(&self) -> bool {
        self.state == ConnectionState::Ready
    }

    /// Get server config
    pub fn config(&self) -> &McpServerConfig {
        &self.config
    }

    /// Get connection state
    pub fn state(&self) -> ConnectionState {
        self.state
    }

    fn next_id(&self) -> u64 {
        self.request_id.fetch_add(1, Ordering::SeqCst)
    }

    async fn send(&mut self, msg: serde_json::Value) -> Result<()> {
        let line = serde_json::to_string(&msg)? + "\n";
        self.stdin.write_all(line.as_bytes()).await?;
        self.stdin.flush().await?;
        Ok(())
    }

    async fn recv(&mut self, expected_id: u64) -> Result<serde_json::Value> {
        let mut line = String::new();
        loop {
            line.clear();
            let bytes = self.reader.read_line(&mut line).await?;
            if bytes == 0 {
                self.state = ConnectionState::Disconnected;
                anyhow::bail!(
                    "Failed to read MCP response: server '{}' closed connection",
                    self.name
                );
            }

            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }

            if let Ok(value) = serde_json::from_str::<serde_json::Value>(trimmed) {
                // Check if this is a response with the expected id
                if value.get("id").and_then(serde_json::Value::as_u64) == Some(expected_id) {
                    return Ok(value);
                }
                // Skip notifications (no id) and responses with different ids
            }
        }
    }

    /// Gracefully close the connection
    pub fn close(&mut self) {
        self.state = ConnectionState::Disconnected;
        // Child process will be killed on drop due to kill_on_drop(true)
    }
}

impl Drop for McpConnection {
    fn drop(&mut self) {
        // Child is automatically killed due to kill_on_drop(true)
    }
}

// === McpPool - Connection Pool Management ===

/// Pool of MCP connections for reuse
pub struct McpPool {
    connections: HashMap<String, McpConnection>,
    config: McpConfig,
}

impl McpPool {
    /// Create a new pool with the given configuration
    pub fn new(config: McpConfig) -> Self {
        Self {
            connections: HashMap::new(),
            config,
        }
    }

    /// Create a pool from a configuration file path
    pub fn from_config_path(path: &std::path::Path) -> Result<Self> {
        let config = if path.exists() {
            let contents = fs::read_to_string(path)
                .with_context(|| format!("Failed to read MCP config: {}", path.display()))?;
            serde_json::from_str(&contents)
                .with_context(|| format!("Failed to parse MCP config: {}", path.display()))?
        } else {
            McpConfig::default()
        };
        Ok(Self::new(config))
    }

    /// Get or create a connection to a server
    pub async fn get_or_connect(&mut self, server_name: &str) -> Result<&mut McpConnection> {
        let is_ready = self
            .connections
            .get(server_name)
            .map(|conn| conn.is_ready())
            .unwrap_or(false);
        if is_ready {
            return self
                .connections
                .get_mut(server_name)
                .ok_or_else(|| anyhow::anyhow!("MCP connection disappeared for {server_name}"));
        }

        self.connections.remove(server_name);

        let server_config = self
            .config
            .servers
            .get(server_name)
            .ok_or_else(|| anyhow::anyhow!("Failed to find MCP server: {server_name}"))?
            .clone();

        if server_config.disabled {
            anyhow::bail!("Failed to connect MCP server '{server_name}': server is disabled");
        }

        let connection = McpConnection::connect(
            server_name.to_string(),
            server_config,
            &self.config.timeouts,
        )
        .await?;

        self.connections.insert(server_name.to_string(), connection);
        self.connections
            .get_mut(server_name)
            .ok_or_else(|| anyhow::anyhow!("Failed to store MCP connection for {server_name}"))
    }

    /// Connect to all enabled servers, returning errors for failed connections
    pub async fn connect_all(&mut self) -> Vec<(String, anyhow::Error)> {
        let mut errors = Vec::new();
        let names: Vec<String> = self
            .config
            .servers
            .keys()
            .filter(|n| !self.config.servers[*n].disabled)
            .cloned()
            .collect();

        for name in names {
            if let Err(e) = self.get_or_connect(&name).await {
                errors.push((name, e));
            }
        }

        errors
    }

    /// Get all discovered tools with server-prefixed names
    pub fn all_tools(&self) -> Vec<(String, &McpTool)> {
        let mut tools = Vec::new();
        for (server, conn) in &self.connections {
            for tool in conn.tools() {
                // Format: mcp_{server}_{tool}
                tools.push((format!("mcp_{}_{}", server, tool.name), tool));
            }
        }
        tools
    }

    /// Convert discovered tools to API Tool format
    pub fn to_api_tools(&self) -> Vec<crate::models::Tool> {
        self.all_tools()
            .into_iter()
            .map(|(name, tool)| crate::models::Tool {
                name,
                description: tool.description.clone().unwrap_or_default(),
                input_schema: tool.input_schema.clone(),
                cache_control: None,
            })
            .collect()
    }

    /// Call a tool by its prefixed name (mcp_{server}_{tool})
    pub async fn call_tool(
        &mut self,
        prefixed_name: &str,
        arguments: serde_json::Value,
    ) -> Result<serde_json::Value> {
        let parts: Vec<&str> = prefixed_name.splitn(3, '_').collect();
        if parts.len() != 3 || parts[0] != "mcp" {
            anyhow::bail!(
                "Failed to parse MCP tool name '{prefixed_name}': expected format mcp_{{server}}_{{tool}}"
            );
        }

        let (server_name, tool_name) = (parts[1], parts[2]);
        // Copy the global timeouts to avoid borrow conflict
        let global_timeouts = self.config.timeouts;
        let conn = self.get_or_connect(server_name).await?;
        let timeout = conn.config().effective_execute_timeout(&global_timeouts);
        conn.call_tool(tool_name, arguments, timeout).await
    }

    /// Get list of configured server names
    pub fn server_names(&self) -> Vec<&str> {
        self.config
            .servers
            .keys()
            .map(std::string::String::as_str)
            .collect()
    }

    /// Get list of connected server names
    pub fn connected_servers(&self) -> Vec<&str> {
        self.connections
            .iter()
            .filter(|(_, c)| c.is_ready())
            .map(|(n, _)| n.as_str())
            .collect()
    }

    /// Disconnect all connections
    pub fn disconnect_all(&mut self) {
        self.connections.clear();
    }

    /// Get the underlying configuration
    pub fn config(&self) -> &McpConfig {
        &self.config
    }

    /// Check if a tool name is an MCP tool
    pub fn is_mcp_tool(name: &str) -> bool {
        name.starts_with("mcp_")
    }
}

// === Helper Functions ===

/// Format MCP tool result for display
pub fn format_tool_result(result: &serde_json::Value) -> String {
    let is_error = result
        .get("isError")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false);

    let content = result
        .get("content")
        .and_then(|v| v.as_array())
        .map_or_else(
            || serde_json::to_string_pretty(result).unwrap_or_default(),
            |arr| {
                arr.iter()
                    .filter_map(|item| match item.get("type")?.as_str()? {
                        "text" => item.get("text")?.as_str().map(String::from),
                        other => Some(format!("[{other} content]")),
                    })
                    .collect::<Vec<_>>()
                    .join("\n")
            },
        );

    if is_error {
        format!("Error: {content}")
    } else {
        content
    }
}

// === Backward Compatibility - Sync API (Legacy) ===

/// Legacy input struct for adding MCP servers
#[derive(Debug, Clone)]
pub struct McpServerInput {
    pub name: String,
    pub command: String,
    pub args: Vec<String>,
    pub env: Vec<String>,
}

/// Legacy MCP server struct for internal use
#[derive(Debug, Serialize, Deserialize, Default)]
struct LegacyMcpServer {
    command: String,
    args: Vec<String>,
    env: HashMap<String, String>,
    #[serde(default)]
    connect_timeout: Option<u64>,
    #[serde(default)]
    execute_timeout: Option<u64>,
    #[serde(default)]
    read_timeout: Option<u64>,
}

/// Legacy config wrapper for backward compatibility
#[derive(Debug, Serialize, Deserialize, Default)]
struct LegacyMcpConfig {
    #[serde(default, alias = "mcpServers")]
    servers: HashMap<String, LegacyMcpServer>,
    #[serde(default)]
    timeouts: McpTimeouts,
}

/// List configured MCP servers (sync, for CLI)
pub fn list(path: &Path) -> Result<()> {
    let config = load_legacy(path)?;
    if config.servers.is_empty() {
        println!("No MCP servers configured.");
        return Ok(());
    }

    for (name, server) in config.servers {
        println!("{} -> {} {}", name, server.command, server.args.join(" "));
    }
    Ok(())
}

/// Add an MCP server to configuration (sync, for CLI)
pub fn add(path: &Path, input: McpServerInput) -> Result<()> {
    let mut config = load_legacy(path)?;
    let env = parse_env(&input.env)?;
    config.servers.insert(
        input.name.clone(),
        LegacyMcpServer {
            command: input.command,
            args: input.args,
            env,
            connect_timeout: None,
            execute_timeout: None,
            read_timeout: None,
        },
    );
    save_legacy(path, &config)?;
    println!("Added MCP server: {}", input.name);
    Ok(())
}

/// Remove an MCP server from configuration (sync, for CLI)
pub fn remove(path: &Path, name: &str) -> Result<()> {
    let mut config = load_legacy(path)?;
    if config.servers.remove(name).is_some() {
        save_legacy(path, &config)?;
        println!("Removed MCP server: {name}");
    } else {
        println!("No MCP server named {name}.");
    }
    Ok(())
}

/// Call an MCP tool (sync, for backward compatibility)
pub fn call_tool(
    path: &Path,
    server: &str,
    tool: &str,
    args: &serde_json::Value,
) -> Result<String> {
    let config = load_legacy(path)?;
    let Some(server_cfg) = config.servers.get(server) else {
        anyhow::bail!("Failed to find MCP server: {server}");
    };
    let timeouts = config.timeouts;
    let connect_timeout = server_cfg
        .connect_timeout
        .unwrap_or(timeouts.connect_timeout);
    let execute_timeout = server_cfg
        .execute_timeout
        .unwrap_or(timeouts.execute_timeout);
    let read_timeout = server_cfg.read_timeout.unwrap_or(timeouts.read_timeout);

    let mut cmd = Command::new(&server_cfg.command);
    cmd.args(&server_cfg.args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    for (key, value) in &server_cfg.env {
        cmd.env(key, value);
    }

    let mut child = cmd.spawn().with_context(|| "Failed to spawn MCP server")?;
    let mut stdin = child.stdin.take().context("Failed to open MCP stdin")?;
    let stdout = child.stdout.take().context("Failed to open MCP stdout")?;
    let reader = Arc::new(Mutex::new(BufReader::new(stdout)));
    let child = Arc::new(Mutex::new(child));

    let init_id = next_id();
    let init_payload = serde_json::json!({
        "jsonrpc": "2.0",
        "id": init_id,
        "method": "initialize",
        "params": {
            "protocolVersion": "2024-11-05",
            "clientInfo": { "name": "deepseek-cli", "version": env!("CARGO_PKG_VERSION") },
            "capabilities": {}
        }
    });
    send_request_sync(&mut stdin, &init_payload)?;
    if let Err(e) = read_response_with_timeout(
        &reader,
        &child,
        init_id,
        Duration::from_secs(connect_timeout),
        read_timeout,
    ) {
        if let Ok(mut child_guard) = child.lock() {
            let _ = child_guard.kill();
        }
        return Err(e);
    }
    let initialized_payload = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "initialized",
        "params": {}
    });
    send_request_sync(&mut stdin, &initialized_payload)?;

    let call_id = next_id();
    let call_payload = serde_json::json!({
        "jsonrpc": "2.0",
        "id": call_id,
        "method": "tools/call",
        "params": {
            "name": tool,
            "arguments": args
        }
    });
    send_request_sync(&mut stdin, &call_payload)?;
    let response = match read_response_with_timeout(
        &reader,
        &child,
        call_id,
        Duration::from_secs(execute_timeout),
        read_timeout,
    ) {
        Ok(result) => result,
        Err(e) => {
            if let Ok(mut child_guard) = child.lock() {
                let _ = child_guard.kill();
            }
            return Err(e);
        }
    };

    if let Ok(mut child_guard) = child.lock() {
        let _ = child_guard.kill();
    }

    if let Some(result) = response.get("result") {
        return Ok(serde_json::to_string_pretty(result)?);
    }
    if let Some(error) = response.get("error") {
        return Ok(serde_json::to_string_pretty(error)?);
    }
    Ok(serde_json::to_string_pretty(&response)?)
}

fn load_legacy(path: &Path) -> Result<LegacyMcpConfig> {
    if path.exists() {
        let contents = fs::read_to_string(path)
            .with_context(|| format!("Failed to read {}", path.display()))?;
        let config = serde_json::from_str(&contents)
            .with_context(|| format!("Failed to parse {}", path.display()))?;
        Ok(config)
    } else {
        Ok(LegacyMcpConfig::default())
    }
}

fn save_legacy(path: &Path, config: &LegacyMcpConfig) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let contents = serde_json::to_string_pretty(config)?;
    fs::write(path, contents)?;
    Ok(())
}

fn parse_env(items: &[String]) -> Result<HashMap<String, String>> {
    let mut env = HashMap::new();
    for item in items {
        let parts: Vec<&str> = item.splitn(2, '=').collect();
        if parts.len() != 2 {
            anyhow::bail!("Failed to parse MCP env var '{item}': expected KEY=VALUE");
        }
        env.insert(parts[0].to_string(), parts[1].to_string());
    }
    Ok(env)
}

fn send_request_sync(stdin: &mut impl Write, payload: &serde_json::Value) -> Result<()> {
    let line = serde_json::to_string(payload)?;
    stdin
        .write_all(format!("{line}\n").as_bytes())
        .with_context(|| "Failed to write MCP request")?;
    stdin.flush()?;
    Ok(())
}

fn read_response_with_timeout(
    reader: &Arc<Mutex<BufReader<std::process::ChildStdout>>>,
    child: &Arc<Mutex<std::process::Child>>,
    id: u64,
    timeout: Duration,
    read_timeout: u64,
) -> Result<serde_json::Value> {
    let effective_timeout = Duration::from_secs(timeout.as_secs().min(read_timeout));
    let (tx, rx) = std::sync::mpsc::channel();

    let reader_clone = Arc::clone(reader);
    std::thread::spawn(move || {
        let result = read_response_sync(&reader_clone, id);
        let _ = tx.send(result);
    });

    if let Ok(result) = rx.recv_timeout(effective_timeout) {
        result
    } else {
        if let Ok(mut child_guard) = child.lock() {
            let _ = child_guard.kill();
        }
        anyhow::bail!(
            "Failed to read MCP response: timed out after {}s",
            effective_timeout.as_secs()
        )
    }
}

fn read_response_sync(
    reader: &Arc<Mutex<BufReader<std::process::ChildStdout>>>,
    id: u64,
) -> Result<serde_json::Value> {
    let mut line = String::new();
    loop {
        line.clear();
        let read = {
            let mut guard = reader
                .lock()
                .map_err(|_| anyhow::anyhow!("MCP reader lock poisoned"))?;
            guard.read_line(&mut line)?
        };
        if read == 0 {
            anyhow::bail!("Failed to read MCP response: server closed output before responding.");
        }
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if let Ok(value) = serde_json::from_str::<serde_json::Value>(trimmed)
            && value.get("id").and_then(serde_json::Value::as_u64) == Some(id)
        {
            return Ok(value);
        }
    }
}

fn next_id() -> u64 {
    let micros = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_micros();
    u64::try_from(micros).unwrap_or(u64::MAX)
}

// === Unit Tests ===

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mcp_config_defaults() {
        let config = McpConfig::default();
        assert_eq!(config.timeouts.connect_timeout, 10);
        assert_eq!(config.timeouts.execute_timeout, 60);
        assert_eq!(config.timeouts.read_timeout, 120);
        assert!(config.servers.is_empty());
    }

    #[test]
    fn test_mcp_config_parse() {
        let json = r#"{
            "timeouts": {
                "connect_timeout": 15,
                "execute_timeout": 90
            },
            "servers": {
                "test": {
                    "command": "node",
                    "args": ["server.js"],
                    "env": {"FOO": "bar"}
                }
            }
        }"#;

        let config: McpConfig = serde_json::from_str(json).unwrap();
        assert_eq!(config.timeouts.connect_timeout, 15);
        assert_eq!(config.timeouts.execute_timeout, 90);
        assert_eq!(config.timeouts.read_timeout, 120); // default
        assert!(config.servers.contains_key("test"));

        let server = config.servers.get("test").unwrap();
        assert_eq!(server.command, "node");
        assert_eq!(server.args, vec!["server.js"]);
        assert_eq!(server.env.get("FOO"), Some(&"bar".to_string()));
    }

    #[test]
    fn test_server_effective_timeouts() {
        let global = McpTimeouts::default();

        let server_with_override = McpServerConfig {
            command: "test".to_string(),
            args: vec![],
            env: HashMap::new(),
            connect_timeout: Some(20),
            execute_timeout: None,
            read_timeout: Some(180),
            disabled: false,
        };

        assert_eq!(server_with_override.effective_connect_timeout(&global), 20);
        assert_eq!(server_with_override.effective_execute_timeout(&global), 60); // global default
        assert_eq!(server_with_override.effective_read_timeout(&global), 180);
    }

    #[test]
    fn test_mcp_pool_is_mcp_tool() {
        assert!(McpPool::is_mcp_tool("mcp_filesystem_read"));
        assert!(McpPool::is_mcp_tool("mcp_git_status"));
        assert!(!McpPool::is_mcp_tool("read_file"));
        assert!(!McpPool::is_mcp_tool("exec_shell"));
    }

    #[test]
    fn test_format_tool_result_text() {
        let result = serde_json::json!({
            "content": [
                {"type": "text", "text": "Hello, world!"}
            ]
        });
        assert_eq!(format_tool_result(&result), "Hello, world!");
    }

    #[test]
    fn test_format_tool_result_error() {
        let result = serde_json::json!({
            "isError": true,
            "content": [
                {"type": "text", "text": "Something went wrong"}
            ]
        });
        assert_eq!(format_tool_result(&result), "Error: Something went wrong");
    }

    #[test]
    fn test_format_tool_result_multiple_content() {
        let result = serde_json::json!({
            "content": [
                {"type": "text", "text": "Line 1"},
                {"type": "text", "text": "Line 2"},
                {"type": "image", "data": "base64..."}
            ]
        });
        let formatted = format_tool_result(&result);
        assert!(formatted.contains("Line 1"));
        assert!(formatted.contains("Line 2"));
        assert!(formatted.contains("[image content]"));
    }

    #[test]
    #[cfg(unix)]
    fn test_read_response_timeout_kills_child() {
        let mut child = Command::new("sh")
            .arg("-c")
            .arg("sleep 5")
            .stdout(Stdio::piped())
            .spawn()
            .expect("spawn sleep");
        let stdout = child.stdout.take().expect("stdout");
        let reader = Arc::new(Mutex::new(BufReader::new(stdout)));
        let child = Arc::new(Mutex::new(child));

        let result = read_response_with_timeout(&reader, &child, 1, Duration::from_secs(1), 1);

        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("timed out"));

        let status = child
            .lock()
            .expect("lock child")
            .wait()
            .expect("wait child");
        assert!(!status.success());
    }

    #[tokio::test]
    async fn test_mcp_pool_empty_config() {
        let pool = McpPool::new(McpConfig::default());
        assert!(pool.server_names().is_empty());
        assert!(pool.all_tools().is_empty());
    }
}
