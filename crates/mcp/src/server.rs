//! MCP Server connection manager.
//! Реальные транспорты: stdio (дочерний процесс + JSON-RPC построчно) и
//! HTTP/SSE (POST JSON-RPC, ответ json или text/event-stream).

use crate::protocol::*;
use crate::tools::ToolRegistry;
use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::Value;
use std::collections::HashMap;
use std::process::Stdio;
use std::sync::{Arc, Mutex as StdMutex};
use std::time::Duration;
use thiserror::Error;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{ChildStdin, Command};
use tokio::sync::{oneshot, Mutex, RwLock};
use tracing::{debug, info, warn};

#[derive(Error, Debug)]
pub enum McpError {
    #[error("Transport error: {0}")]
    Transport(String),

    #[error("Protocol error: {0}")]
    Protocol(String),

    #[error("Server not initialized")]
    NotInitialized,

    #[error("Tool not found: {0}")]
    ToolNotFound(String),

    #[error("JSON-RPC error: {0}")]
    JsonRpc(String),
}

pub type Result<T> = std::result::Result<T, McpError>;

const DEFAULT_TIMEOUT_MS: u64 = 30_000;

/// MCP Server configuration
#[derive(Debug, Clone)]
pub struct ServerConfig {
    pub name: String,
    pub transport: TransportType,
    /// Per-request timeout (ms). Use [`ServerConfig::default_timeout`] elsewhere.
    pub timeout_ms: u64,
}

impl ServerConfig {
    pub const fn default_timeout() -> u64 {
        DEFAULT_TIMEOUT_MS
    }
}

#[derive(Debug, Clone)]
pub enum TransportType {
    Stdio {
        command: String,
        args: Vec<String>,
        env: Vec<(String, String)>,
    },
    Http {
        url: String,
        headers: Vec<(String, String)>,
    },
}

/// Pending stdio requests: request id → resolver.
type Pending = Arc<StdMutex<HashMap<u64, oneshot::Sender<JsonRpcResponse>>>>;

struct StdioConn {
    stdin: Mutex<ChildStdin>,
    pending: Pending,
    /// Keep the child alive; dropping it kills the server process.
    _child: tokio::process::Child,
}

struct HttpConn {
    client: reqwest::Client,
    url: String,
    headers: Vec<(String, String)>,
}

enum Conn {
    Stdio(StdioConn),
    Http(HttpConn),
}

/// MCP Server connection
pub struct McpServer {
    pub config: ServerConfig,
    tool_registry: ToolRegistry,
    state: Arc<RwLock<ServerState>>,
    request_id: Arc<RwLock<u64>>,
    conn: Arc<RwLock<Option<Conn>>>,
}

#[derive(Debug, Clone)]
enum ServerState {
    Disconnected,
    Connected {
        #[allow(dead_code)]
        server_info: ServerInfo,
    },
}

impl McpServer {
    pub fn new(config: ServerConfig, tool_registry: ToolRegistry) -> Self {
        Self {
            config,
            tool_registry,
            state: Arc::new(RwLock::new(ServerState::Disconnected)),
            request_id: Arc::new(RwLock::new(0)),
            conn: Arc::new(RwLock::new(None)),
        }
    }

    /// Get next request ID
    async fn next_id(&self) -> u64 {
        let mut id = self.request_id.write().await;
        *id += 1;
        *id
    }

    /// Connect to the MCP server
    pub async fn connect(&self) -> Result<()> {
        info!("Connecting to MCP server: {}", self.config.name);

        match &self.config.transport {
            TransportType::Stdio { command, args, env } => {
                self.connect_stdio(command, args, env).await?;
            }
            TransportType::Http { url, headers } => {
                self.connect_http(url, headers).await?;
            }
        }

        // Initialize the connection
        self.initialize().await?;

        // List and register tools
        self.refresh_tools().await?;

        *self.state.write().await = ServerState::Connected {
            server_info: ServerInfo {
                name: self.config.name.clone(),
                version: "unknown".to_string(),
            },
        };

        info!("MCP server {} connected and initialized", self.config.name);
        Ok(())
    }

    /// Connect via stdio transport: spawn child, route responses by id.
    async fn connect_stdio(
        &self,
        command: &str,
        args: &[String],
        env: &[(String, String)],
    ) -> Result<()> {
        debug!("Starting MCP server: {} {:?}", command, args);

        let mut cmd = Command::new(command);
        cmd.args(args);
        cmd.envs(env.iter().map(|(k, v)| (k.clone(), v.clone())));
        cmd.stdin(Stdio::piped());
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());
        cmd.kill_on_drop(true);

        let mut child = cmd
            .spawn()
            .map_err(|e| McpError::Transport(format!("Failed to spawn {command}: {e}")))?;

        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| McpError::Transport("child has no stdin".into()))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| McpError::Transport("child has no stdout".into()))?;

        let pending: Pending = Arc::new(StdMutex::new(HashMap::new()));

        // Reader task: read JSON-RPC responses line-by-line and resolve waiters.
        let pending_reader = pending.clone();
        let server_name = self.config.name.clone();
        tokio::spawn(async move {
            let mut lines = BufReader::new(stdout).lines();
            loop {
                match lines.next_line().await {
                    Ok(Some(line)) => {
                        if line.trim().is_empty() {
                            continue;
                        }
                        route_stdio_line(&server_name, &line, &pending_reader);
                    }
                    Ok(None) => break,
                    Err(e) => {
                        warn!("mcp {server_name}: stdout read error: {e}");
                        break;
                    }
                }
            }
            debug!("mcp {server_name}: stdout reader ended");
        });

        // Drain stderr to logs (so the pipe doesn't fill and block the child).
        if let Some(stderr) = child.stderr.take() {
            let name = self.config.name.clone();
            tokio::spawn(async move {
                let mut lines = BufReader::new(stderr).lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    debug!("mcp {name} [stderr]: {line}");
                }
            });
        }

        *self.conn.write().await = Some(Conn::Stdio(StdioConn {
            stdin: Mutex::new(stdin),
            pending,
            _child: child,
        }));
        Ok(())
    }

    /// Connect via HTTP transport (no persistent socket — each request is a POST).
    async fn connect_http(&self, url: &str, headers: &[(String, String)]) -> Result<()> {
        let client = reqwest::Client::builder()
            .build()
            .map_err(|e| McpError::Transport(e.to_string()))?;
        *self.conn.write().await = Some(Conn::Http(HttpConn {
            client,
            url: url.to_string(),
            headers: headers.to_vec(),
        }));
        Ok(())
    }

    /// Send initialize request
    async fn initialize(&self) -> Result<()> {
        let params = InitializeParams {
            protocol_version: "2024-11-05".to_string(),
            capabilities: ClientCapabilities {
                roots: Some(RootsCapability {
                    list_changed: Some(true),
                }),
                sampling: None,
            },
            client_info: ClientInfo {
                name: "cozby-brain".to_string(),
                version: env!("CARGO_PKG_VERSION").to_string(),
            },
        };

        let _result: InitializeResult = self.send_request("initialize", Some(params)).await?;

        // Send initialized notification
        self.send_notification::<Value>("notifications/initialized", None)
            .await?;

        Ok(())
    }

    /// Refresh tools list from server
    pub async fn refresh_tools(&self) -> Result<()> {
        debug!("Refreshing tools for server: {}", self.config.name);

        self.tool_registry
            .remove_server_tools(&self.config.name)
            .await;

        let result: ListToolsResult = self.send_request("tools/list", None::<Value>).await?;

        let tools_count = result.tools.len();
        for tool in result.tools {
            self.tool_registry.register(&self.config.name, tool).await;
        }

        info!(
            "Registered {} tools from server {}",
            tools_count, self.config.name
        );

        Ok(())
    }

    /// Call a tool on this server
    pub async fn call_tool(&self, name: &str, arguments: Option<Value>) -> Result<CallToolResult> {
        let params = CallToolParams {
            name: name.to_string(),
            arguments,
        };
        let result: CallToolResult = self.send_request("tools/call", Some(params)).await?;
        Ok(result)
    }

    /// Send JSON-RPC request and wait for response.
    async fn send_request<P: Serialize, R: DeserializeOwned>(
        &self,
        method: &str,
        params: Option<P>,
    ) -> Result<R> {
        let id = self.next_id().await;
        let request = JsonRpcRequest::new(method, params, id);
        let body = serde_json::to_string(&request).map_err(|e| McpError::Protocol(e.to_string()))?;
        let timeout = Duration::from_millis(self.config.timeout_ms);

        debug!("→ mcp {} {} (id={})", self.config.name, method, id);

        let guard = self.conn.read().await;
        let conn = guard.as_ref().ok_or(McpError::NotInitialized)?;

        let response: JsonRpcResponse = match conn {
            Conn::Stdio(s) => {
                let (tx, rx) = oneshot::channel();
                s.pending.lock().unwrap().insert(id, tx);
                {
                    let mut stdin = s.stdin.lock().await;
                    write_line(&mut stdin, &body).await?;
                }
                match tokio::time::timeout(timeout, rx).await {
                    Ok(Ok(resp)) => resp,
                    Ok(Err(_)) => {
                        s.pending.lock().unwrap().remove(&id);
                        return Err(McpError::Transport("response channel closed".into()));
                    }
                    Err(_) => {
                        s.pending.lock().unwrap().remove(&id);
                        return Err(McpError::Transport(format!(
                            "timeout after {}ms ({method})",
                            self.config.timeout_ms
                        )));
                    }
                }
            }
            Conn::Http(h) => {
                let mut req = h
                    .client
                    .post(&h.url)
                    .header("content-type", "application/json")
                    .header("accept", "application/json, text/event-stream")
                    .body(body);
                for (k, v) in &h.headers {
                    req = req.header(k, v);
                }
                let resp = tokio::time::timeout(timeout, req.send())
                    .await
                    .map_err(|_| McpError::Transport(format!("http timeout ({method})")))?
                    .map_err(|e| McpError::Transport(e.to_string()))?;
                let text = resp
                    .text()
                    .await
                    .map_err(|e| McpError::Transport(e.to_string()))?;
                parse_http_response(&text, id)?
            }
        };

        if let Some(err) = response.error {
            return Err(McpError::JsonRpc(format!("{} (code {})", err.message, err.code)));
        }
        let result = response
            .result
            .ok_or_else(|| McpError::Protocol("response missing result".into()))?;
        serde_json::from_value(result).map_err(|e| McpError::Protocol(e.to_string()))
    }

    /// Send JSON-RPC notification (no id, no response expected).
    async fn send_notification<P: Serialize>(
        &self,
        method: &str,
        params: Option<P>,
    ) -> Result<()> {
        let mut msg = serde_json::json!({ "jsonrpc": "2.0", "method": method });
        if let Some(p) = params {
            msg["params"] = serde_json::to_value(p).map_err(|e| McpError::Protocol(e.to_string()))?;
        }
        let body = serde_json::to_string(&msg).map_err(|e| McpError::Protocol(e.to_string()))?;

        let guard = self.conn.read().await;
        let conn = guard.as_ref().ok_or(McpError::NotInitialized)?;
        match conn {
            Conn::Stdio(s) => {
                let mut stdin = s.stdin.lock().await;
                write_line(&mut stdin, &body).await?;
            }
            Conn::Http(h) => {
                let mut req = h
                    .client
                    .post(&h.url)
                    .header("content-type", "application/json")
                    .body(body);
                for (k, v) in &h.headers {
                    req = req.header(k, v);
                }
                let _ = req.send().await; // best-effort
            }
        }
        Ok(())
    }

    /// Disconnect from the server
    pub async fn disconnect(&self) -> Result<()> {
        info!("Disconnecting from MCP server: {}", self.config.name);
        self.tool_registry
            .remove_server_tools(&self.config.name)
            .await;
        // Dropping the Conn kills the child (kill_on_drop) and drops the client.
        *self.conn.write().await = None;
        *self.state.write().await = ServerState::Disconnected;
        Ok(())
    }

    /// Check if server is connected
    pub async fn is_connected(&self) -> bool {
        matches!(*self.state.read().await, ServerState::Connected { .. })
    }
}

async fn write_line(stdin: &mut ChildStdin, body: &str) -> Result<()> {
    stdin
        .write_all(body.as_bytes())
        .await
        .map_err(|e| McpError::Transport(e.to_string()))?;
    stdin
        .write_all(b"\n")
        .await
        .map_err(|e| McpError::Transport(e.to_string()))?;
    stdin
        .flush()
        .await
        .map_err(|e| McpError::Transport(e.to_string()))?;
    Ok(())
}

/// Parse one stdout line and resolve the matching pending request (if any).
fn route_stdio_line(server_name: &str, line: &str, pending: &Pending) {
    let Ok(value) = serde_json::from_str::<Value>(line) else {
        debug!("mcp {server_name}: non-json line: {line}");
        return;
    };
    let is_response =
        value.get("id").is_some() && (value.get("result").is_some() || value.get("error").is_some());
    if !is_response {
        // Server-initiated request/notification — not handled in MVP.
        debug!("mcp {server_name}: unhandled inbound: {line}");
        return;
    }
    let Some(id) = value.get("id").and_then(|i| i.as_u64()) else {
        return;
    };
    let Ok(resp) = serde_json::from_value::<JsonRpcResponse>(value) else {
        return;
    };
    if let Some(tx) = pending.lock().unwrap().remove(&id) {
        let _ = tx.send(resp);
    }
}

/// Parse an HTTP response body: plain JSON-RPC or SSE (`data: {...}`).
fn parse_http_response(text: &str, id: u64) -> Result<JsonRpcResponse> {
    let trimmed = text.trim();
    if let Ok(resp) = serde_json::from_str::<JsonRpcResponse>(trimmed) {
        return Ok(resp);
    }
    for line in text.lines() {
        let line = line.trim();
        let Some(payload) = line.strip_prefix("data:") else {
            continue;
        };
        let payload = payload.trim();
        if payload.is_empty() || payload == "[DONE]" {
            continue;
        }
        if let Ok(value) = serde_json::from_str::<Value>(payload) {
            if value.get("id").and_then(|i| i.as_u64()) == Some(id) {
                if let Ok(resp) = serde_json::from_value::<JsonRpcResponse>(value) {
                    return Ok(resp);
                }
            }
        }
    }
    Err(McpError::Protocol(format!(
        "could not parse http response: {}",
        text.chars().take(200).collect::<String>()
    )))
}
