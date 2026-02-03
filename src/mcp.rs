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
    pub command: Option<String>,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: HashMap<String, String>,
    pub url: Option<String>,
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

/// Resource discovered from an MCP server
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct McpResource {
    pub uri: String,
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(rename = "mimeType", default)]
    pub mime_type: Option<String>,
}

/// Resource template discovered from an MCP server
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct McpResourceTemplate {
    #[serde(rename = "uriTemplate")]
    pub uri_template: String,
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(rename = "mimeType", default)]
    pub mime_type: Option<String>,
}

/// Prompt discovered from an MCP server
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct McpPrompt {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub arguments: Vec<McpPromptArgument>,
}

/// Argument for an MCP prompt
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct McpPromptArgument {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub required: bool,
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

// === Transport Trait ===

#[async_trait::async_trait]
pub trait McpTransport: Send + Sync {
    async fn send(&mut self, msg: serde_json::Value) -> Result<()>;
    async fn recv(&mut self) -> Result<serde_json::Value>;
}

pub struct StdioTransport {
    _child: Child,
    stdin: ChildStdin,
    reader: tokio::io::BufReader<ChildStdout>,
}

#[async_trait::async_trait]
impl McpTransport for StdioTransport {
    async fn send(&mut self, msg: serde_json::Value) -> Result<()> {
        let line = serde_json::to_string(&msg)? + "\n";
        self.stdin.write_all(line.as_bytes()).await?;
        self.stdin.flush().await?;
        Ok(())
    }

    async fn recv(&mut self) -> Result<serde_json::Value> {
        let mut line = String::new();
        loop {
            line.clear();
            let bytes = self.reader.read_line(&mut line).await?;
            if bytes == 0 {
                anyhow::bail!("Stdio transport closed");
            }

            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }

            if let Ok(value) = serde_json::from_str::<serde_json::Value>(trimmed) {
                return Ok(value);
            }
        }
    }
}

pub struct SseTransport {
    client: reqwest::Client,
    base_url: String,
    endpoint_url: Option<String>,
    receiver: tokio::sync::mpsc::UnboundedReceiver<serde_json::Value>,
}

impl SseTransport {
    pub async fn connect(client: reqwest::Client, url: String) -> Result<Self> {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let client_clone = client.clone();
        let url_clone = url.clone();

        // Start SSE background task
        tokio::spawn(async move {
            if let Err(e) = Self::run_sse_loop(client_clone, url_clone, tx).await {
                tracing::error!("SSE loop error: {}", e);
            }
        });

        // The endpoint URL will be discovered from the first "endpoint" event
        Ok(Self {
            client,
            base_url: url,
            endpoint_url: None,
            receiver: rx,
        })
    }

    async fn run_sse_loop(
        client: reqwest::Client,
        url: String,
        tx: tokio::sync::mpsc::UnboundedSender<serde_json::Value>,
    ) -> Result<()> {
        let response = client.get(&url).send().await?;
        if !response.status().is_success() {
            anyhow::bail!("Failed to connect to SSE: {}", response.status());
        }

        let mut stream = response.bytes_stream();
        use futures_util::StreamExt;
        let mut buffer = String::new();

        while let Some(item) = stream.next().await {
            let chunk = item?;
            let s = String::from_utf8_lossy(&chunk);
            buffer.push_str(&s);

            while let Some(pos) = buffer.find("\n\n") {
                let event_block = buffer[..pos].to_string();
                buffer = buffer[pos + 2..].to_string();

                let mut event_type = "message";
                let mut data = String::new();

                for line in event_block.lines() {
                    if let Some(stripped) = line.strip_prefix("event: ") {
                        event_type = stripped;
                    } else if let Some(stripped) = line.strip_prefix("data: ") {
                        data.push_str(stripped);
                    }
                }

                match event_type {
                    "endpoint" => {
                        // Special internal message to set endpoint
                        let _ = tx.send(serde_json::json!({
                            "__internal_sse_endpoint__": data
                        }));
                    }
                    "message" => {
                        if let Ok(val) = serde_json::from_str::<serde_json::Value>(&data) {
                            let _ = tx.send(val);
                        }
                    }
                    _ => {}
                }
            }
        }
        Ok(())
    }
}

#[async_trait::async_trait]
impl McpTransport for SseTransport {
    async fn send(&mut self, msg: serde_json::Value) -> Result<()> {
        let endpoint = self
            .endpoint_url
            .as_ref()
            .context("SSE endpoint not yet discovered")?;
        let response = self.client.post(endpoint).json(&msg).send().await?;
        if !response.status().is_success() {
            anyhow::bail!("Failed to send message via SSE POST: {}", response.status());
        }
        Ok(())
    }

    async fn recv(&mut self) -> Result<serde_json::Value> {
        loop {
            let msg = self.receiver.recv().await.context("SSE transport closed")?;
            if let Some(endpoint) = msg.get("__internal_sse_endpoint__") {
                let url_str = endpoint.as_str().context("Invalid endpoint format")?;
                // Handle relative vs absolute URLs
                if url_str.starts_with("http") {
                    self.endpoint_url = Some(url_str.to_string());
                } else {
                    let base = reqwest::Url::parse(&self.base_url)?;
                    let joined = base.join(url_str)?;
                    self.endpoint_url = Some(joined.to_string());
                }
                continue;
            }
            return Ok(msg);
        }
    }
}

// === McpConnection - Async Connection Management ===

/// Manages a single async connection to an MCP server
pub struct McpConnection {
    name: String,
    transport: Box<dyn McpTransport>,
    tools: Vec<McpTool>,
    resources: Vec<McpResource>,
    resource_templates: Vec<McpResourceTemplate>,
    prompts: Vec<McpPrompt>,
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

        let transport: Box<dyn McpTransport> = if let Some(url) = &config.url {
            let client = reqwest::Client::builder()
                .timeout(Duration::from_secs(connect_timeout_secs))
                .build()?;
            Box::new(SseTransport::connect(client, url.clone()).await?)
        } else if let Some(command) = &config.command {
            let mut cmd = tokio::process::Command::new(command);
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

            Box::new(StdioTransport {
                _child: child,
                stdin,
                reader: tokio::io::BufReader::new(stdout),
            })
        } else {
            anyhow::bail!(
                "MCP server '{}' config must have either 'command' or 'url'",
                name
            );
        };

        let mut conn = Self {
            name: name.clone(),
            transport,
            tools: Vec::new(),
            resources: Vec::new(),
            resource_templates: Vec::new(),
            prompts: Vec::new(),
            request_id: AtomicU64::new(1),
            state: ConnectionState::Connecting,
            config,
        };

        // Initialize with timeout
        tokio::time::timeout(Duration::from_secs(connect_timeout_secs), conn.initialize())
            .await
            .with_context(|| format!("MCP server '{name}' initialization timed out"))??;

        // Discover tools, resources, and prompts with timeout
        tokio::time::timeout(
            Duration::from_secs(connect_timeout_secs),
            conn.discover_all(),
        )
        .await
        .with_context(|| format!("MCP server '{name}' discovery timed out"))??;

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
                "capabilities": {
                    "tools": {},
                    "resources": {},
                    "prompts": {}
                }
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

    /// Discover tools, resources, and prompts
    async fn discover_all(&mut self) -> Result<()> {
        // We use join! to discover everything concurrently if possible,
        // but for now let's keep it sequential for simplicity in error handling
        self.discover_tools().await?;
        self.discover_resources().await?;
        self.discover_resource_templates().await?;
        self.discover_prompts().await?;
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

    /// Discover available resources from the MCP server
    async fn discover_resources(&mut self) -> Result<()> {
        let list_id = self.next_id();
        self.send(serde_json::json!({
            "jsonrpc": "2.0",
            "id": list_id,
            "method": "resources/list",
            "params": {}
        }))
        .await?;

        let response = self.recv(list_id).await?;

        if let Some(result) = response.get("result")
            && let Some(resources) = result.get("resources")
        {
            self.resources = serde_json::from_value(resources.clone()).unwrap_or_default();
        }

        Ok(())
    }

    /// Discover available resource templates from the MCP server
    async fn discover_resource_templates(&mut self) -> Result<()> {
        let list_id = self.next_id();
        self.send(serde_json::json!({
            "jsonrpc": "2.0",
            "id": list_id,
            "method": "resources/templates/list",
            "params": {}
        }))
        .await?;

        let response = self.recv(list_id).await?;

        if let Some(result) = response.get("result") {
            let templates = result
                .get("resourceTemplates")
                .or_else(|| result.get("templates"))
                .or_else(|| result.get("resource_templates"));
            if let Some(templates) = templates {
                self.resource_templates =
                    serde_json::from_value(templates.clone()).unwrap_or_default();
            }
        }

        Ok(())
    }

    /// Discover available prompts from the MCP server
    async fn discover_prompts(&mut self) -> Result<()> {
        let list_id = self.next_id();
        self.send(serde_json::json!({
            "jsonrpc": "2.0",
            "id": list_id,
            "method": "prompts/list",
            "params": {}
        }))
        .await?;

        let response = self.recv(list_id).await?;

        if let Some(result) = response.get("result")
            && let Some(prompts) = result.get("prompts")
        {
            self.prompts = serde_json::from_value(prompts.clone()).unwrap_or_default();
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
        self.call_method(
            "tools/call",
            serde_json::json!({
                "name": tool_name,
                "arguments": arguments
            }),
            timeout_secs,
        )
        .await
    }

    /// Read a resource from this MCP server
    pub async fn read_resource(
        &mut self,
        uri: &str,
        timeout_secs: u64,
    ) -> Result<serde_json::Value> {
        self.call_method(
            "resources/read",
            serde_json::json!({
                "uri": uri
            }),
            timeout_secs,
        )
        .await
    }

    /// Get a prompt from this MCP server
    pub async fn get_prompt(
        &mut self,
        prompt_name: &str,
        arguments: serde_json::Value,
        timeout_secs: u64,
    ) -> Result<serde_json::Value> {
        self.call_method(
            "prompts/get",
            serde_json::json!({
                "name": prompt_name,
                "arguments": arguments
            }),
            timeout_secs,
        )
        .await
    }

    /// Generic method to call an MCP method
    async fn call_method(
        &mut self,
        method: &str,
        params: serde_json::Value,
        timeout_secs: u64,
    ) -> Result<serde_json::Value> {
        if self.state != ConnectionState::Ready {
            anyhow::bail!(
                "Failed to call MCP method '{}': connection '{}' is not ready",
                method,
                self.name
            );
        }

        let call_id = self.next_id();
        self.send(serde_json::json!({
            "jsonrpc": "2.0",
            "id": call_id,
            "method": method,
            "params": params
        }))
        .await?;

        let response = tokio::time::timeout(Duration::from_secs(timeout_secs), self.recv(call_id))
            .await
            .with_context(|| {
                format!(
                    "MCP method '{}' on server '{}' timed out after {}s",
                    method, self.name, timeout_secs
                )
            })??;

        if let Some(error) = response.get("error") {
            return Err(anyhow::anyhow!(
                "MCP error in '{}': {}",
                method,
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

    /// Get discovered resources
    pub fn resources(&self) -> &[McpResource] {
        &self.resources
    }

    /// Get discovered resource templates
    pub fn resource_templates(&self) -> &[McpResourceTemplate] {
        &self.resource_templates
    }

    /// Get discovered prompts
    pub fn prompts(&self) -> &[McpPrompt] {
        &self.prompts
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
        self.transport.send(msg).await
    }

    async fn recv(&mut self, expected_id: u64) -> Result<serde_json::Value> {
        loop {
            let value = self.transport.recv().await.inspect_err(|_e| {
                self.state = ConnectionState::Disconnected;
            })?;

            // Check if this is a response with the expected id
            if value.get("id").and_then(serde_json::Value::as_u64) == Some(expected_id) {
                return Ok(value);
            }
            // Skip notifications (no id) and responses with different ids
        }
    }

    /// Gracefully close the connection
    pub fn close(&mut self) {
        self.state = ConnectionState::Disconnected;
    }
}

impl Drop for McpConnection {
    fn drop(&mut self) {
        // StdioTransport will be dropped and child killed
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

    /// Get all discovered resources with server-prefixed names
    pub fn all_resources(&self) -> Vec<(String, &McpResource)> {
        let mut resources = Vec::new();
        for (server, conn) in &self.connections {
            for resource in conn.resources() {
                // Format: mcp_{server}_{resource_name}
                // Note: resource names might contain spaces, we should probably slugify them
                let safe_name = resource.name.replace(' ', "_").to_lowercase();
                resources.push((format!("mcp_{}_{}", server, safe_name), resource));
            }
        }
        resources
    }

    /// Get all discovered resource templates with server-prefixed names
    pub fn all_resource_templates(&self) -> Vec<(String, &McpResourceTemplate)> {
        let mut templates = Vec::new();
        for (server, conn) in &self.connections {
            for template in conn.resource_templates() {
                let safe_name = template.name.replace(' ', "_").to_lowercase();
                templates.push((format!("mcp_{}_{}", server, safe_name), template));
            }
        }
        templates
    }

    async fn list_resources(&mut self, server: Option<String>) -> Result<Vec<serde_json::Value>> {
        if let Some(server_name) = server {
            let conn = self.get_or_connect(&server_name).await?;
            let resources = conn
                .resources()
                .iter()
                .map(|resource| {
                    serde_json::json!({
                        "server": server_name.clone(),
                        "uri": resource.uri,
                        "name": resource.name,
                        "description": resource.description,
                        "mime_type": resource.mime_type,
                    })
                })
                .collect();
            return Ok(resources);
        }

        let _ = self.connect_all().await;
        let mut items = Vec::new();
        for (server, conn) in &self.connections {
            for resource in conn.resources() {
                items.push(serde_json::json!({
                    "server": server,
                    "uri": resource.uri,
                    "name": resource.name,
                    "description": resource.description,
                    "mime_type": resource.mime_type,
                }));
            }
        }
        Ok(items)
    }

    async fn list_resource_templates(
        &mut self,
        server: Option<String>,
    ) -> Result<Vec<serde_json::Value>> {
        if let Some(server_name) = server {
            let conn = self.get_or_connect(&server_name).await?;
            let templates = conn
                .resource_templates()
                .iter()
                .map(|template| {
                    serde_json::json!({
                        "server": server_name.clone(),
                        "uri_template": template.uri_template,
                        "name": template.name,
                        "description": template.description,
                        "mime_type": template.mime_type,
                    })
                })
                .collect();
            return Ok(templates);
        }

        let _ = self.connect_all().await;
        let mut items = Vec::new();
        for (server, conn) in &self.connections {
            for template in conn.resource_templates() {
                items.push(serde_json::json!({
                    "server": server,
                    "uri_template": template.uri_template,
                    "name": template.name,
                    "description": template.description,
                    "mime_type": template.mime_type,
                }));
            }
        }
        Ok(items)
    }

    /// Get all discovered prompts with server-prefixed names
    pub fn all_prompts(&self) -> Vec<(String, &McpPrompt)> {
        let mut prompts = Vec::new();
        for (server, conn) in &self.connections {
            for prompt in conn.prompts() {
                // Format: mcp_{server}_{prompt}
                prompts.push((format!("mcp_{}_{}", server, prompt.name), prompt));
            }
        }
        prompts
    }

    /// Read a resource from a specific server
    pub async fn read_resource(
        &mut self,
        server_name: &str,
        uri: &str,
    ) -> Result<serde_json::Value> {
        let global_timeouts = self.config.timeouts;
        let conn = self.get_or_connect(server_name).await?;
        let timeout = conn.config().effective_read_timeout(&global_timeouts);
        conn.read_resource(uri, timeout).await
    }

    /// Get a prompt from a specific server
    pub async fn get_prompt(
        &mut self,
        server_name: &str,
        prompt_name: &str,
        arguments: serde_json::Value,
    ) -> Result<serde_json::Value> {
        let global_timeouts = self.config.timeouts;
        let conn = self.get_or_connect(server_name).await?;
        let timeout = conn.config().effective_execute_timeout(&global_timeouts);
        conn.get_prompt(prompt_name, arguments, timeout).await
    }

    /// Parse a prefixed name into (server_name, tool_name)
    fn parse_prefixed_name<'a>(&self, prefixed_name: &'a str) -> Result<(&'a str, &'a str)> {
        if !prefixed_name.starts_with("mcp_") {
            anyhow::bail!("Invalid MCP tool name: {}", prefixed_name);
        }
        let rest = &prefixed_name[4..];
        let Some((server, tool)) = rest.split_once('_') else {
            anyhow::bail!("Invalid MCP tool name format: {}", prefixed_name);
        };
        Ok((server, tool))
    }

    /// Convert discovered tools to API Tool format
    pub fn to_api_tools(&self) -> Vec<crate::models::Tool> {
        let mut api_tools = Vec::new();

        // Add regular tools
        for (name, tool) in self.all_tools() {
            api_tools.push(crate::models::Tool {
                name,
                description: tool.description.clone().unwrap_or_default(),
                input_schema: tool.input_schema.clone(),
                cache_control: None,
            });
        }

        if !self.config.servers.is_empty() {
            api_tools.push(crate::models::Tool {
                name: "list_mcp_resources".to_string(),
                description: "List available MCP resources across servers (optionally filtered by server).".to_string(),
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "server": { "type": "string", "description": "Optional MCP server name to filter by" }
                    }
                }),
                cache_control: None,
            });
            api_tools.push(crate::models::Tool {
                name: "list_mcp_resource_templates".to_string(),
                description: "List available MCP resource templates across servers (optionally filtered by server).".to_string(),
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "server": { "type": "string", "description": "Optional MCP server name to filter by" }
                    }
                }),
                cache_control: None,
            });
        }

        // Add resource reading tools if resources exist
        let resources = self.all_resources();
        if !resources.is_empty() {
            api_tools.push(crate::models::Tool {
                name: "mcp_read_resource".to_string(),
                description: "Read a resource from an MCP server using its URI".to_string(),
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "server": { "type": "string", "description": "The name of the MCP server" },
                        "uri": { "type": "string", "description": "The URI of the resource to read" }
                    },
                    "required": ["server", "uri"]
                }),
                            cache_control: None,
                        });
            api_tools.push(crate::models::Tool {
                name: "read_mcp_resource".to_string(),
                description: "Alias for mcp_read_resource.".to_string(),
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "server": { "type": "string", "description": "The name of the MCP server" },
                        "uri": { "type": "string", "description": "The URI of the resource to read" }
                    },
                    "required": ["server", "uri"]
                }),
                cache_control: None,
            });
        }

        // Add prompt getting tools if prompts exist
        let prompts = self.all_prompts();
        if !prompts.is_empty() {
            api_tools.push(crate::models::Tool {
                name: "mcp_get_prompt".to_string(),
                description: "Get a prompt from an MCP server".to_string(),
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "server": { "type": "string", "description": "The name of the MCP server" },
                        "name": { "type": "string", "description": "The name of the prompt" },
                        "arguments": {
                            "type": "object",
                            "description": "Optional arguments for the prompt",
                            "additionalProperties": { "type": "string" }
                        }
                    },
                    "required": ["server", "name"]
                }),
                cache_control: None,
            });
        }

        api_tools
    }

    /// Call a tool by its prefixed name (mcp_{server}_{tool})
    pub async fn call_tool(
        &mut self,
        prefixed_name: &str,
        arguments: serde_json::Value,
    ) -> Result<serde_json::Value> {
        if prefixed_name == "list_mcp_resources" {
            let server = arguments
                .get("server")
                .and_then(|v| v.as_str())
                .map(str::to_string);
            let resources = self.list_resources(server).await?;
            return Ok(serde_json::json!({ "resources": resources }));
        }

        if prefixed_name == "list_mcp_resource_templates" {
            let server = arguments
                .get("server")
                .and_then(|v| v.as_str())
                .map(str::to_string);
            let templates = self.list_resource_templates(server).await?;
            return Ok(serde_json::json!({ "templates": templates }));
        }

        if prefixed_name == "mcp_read_resource" {
            let server_name = arguments
                .get("server")
                .and_then(|v| v.as_str())
                .context("Missing 'server' argument")?;
            let uri = arguments
                .get("uri")
                .and_then(|v| v.as_str())
                .context("Missing 'uri' argument")?;
            return self.read_resource(server_name, uri).await;
        }

        if prefixed_name == "read_mcp_resource" {
            let server_name = arguments
                .get("server")
                .and_then(|v| v.as_str())
                .context("Missing 'server' argument")?;
            let uri = arguments
                .get("uri")
                .and_then(|v| v.as_str())
                .context("Missing 'uri' argument")?;
            return self.read_resource(server_name, uri).await;
        }

        if prefixed_name == "mcp_get_prompt" {
            let server_name = arguments
                .get("server")
                .and_then(|v| v.as_str())
                .context("Missing 'server' argument")?;
            let name = arguments
                .get("name")
                .and_then(|v| v.as_str())
                .context("Missing 'name' argument")?;
            let args = arguments
                .get("arguments")
                .cloned()
                .unwrap_or(serde_json::json!({}));
            return self.get_prompt(server_name, name, args).await;
        }

        let (server_name, tool_name) = self.parse_prefixed_name(prefixed_name)?;
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
            || matches!(
                name,
                "list_mcp_resources" | "list_mcp_resource_templates" | "read_mcp_resource"
            )
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
        assert_eq!(server.command, Some("node".to_string()));
        assert_eq!(server.args, vec!["server.js"]);
        assert_eq!(server.env.get("FOO"), Some(&"bar".to_string()));
    }

    #[test]
    fn test_server_effective_timeouts() {
        let global = McpTimeouts::default();

        let server_with_override = McpServerConfig {
            command: Some("test".to_string()),
            args: vec![],
            env: HashMap::new(),
            url: None,
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
        assert!(McpPool::is_mcp_tool("list_mcp_resources"));
        assert!(McpPool::is_mcp_tool("list_mcp_resource_templates"));
        assert!(McpPool::is_mcp_tool("read_mcp_resource"));
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
