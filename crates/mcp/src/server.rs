//! MCP Server connection manager
//! Handles stdio/HTTP transport to MCP servers

use crate::protocol::*;
use crate::tools::ToolRegistry;
use serde_json::Value;
use std::process::{Command, Stdio};
use std::sync::Arc;
use thiserror::Error;
use tokio::sync::RwLock;
use tracing::{debug, info};

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

/// MCP Server configuration
#[derive(Debug, Clone)]
pub struct ServerConfig {
    pub name: String,
    pub transport: TransportType,
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

/// MCP Server connection
pub struct McpServer {
    pub config: ServerConfig,
    tool_registry: ToolRegistry,
    state: Arc<RwLock<ServerState>>,
    request_id: Arc<RwLock<u64>>,
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
            TransportType::Http { url, headers: _ } => {
                self.connect_http(url).await?;
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

    /// Connect via stdio transport
    async fn connect_stdio(
        &self,
        command: &str,
        args: &[String],
        env: &[(String, String)],
    ) -> Result<()> {
        debug!("Starting MCP server: {} {:?}", command, args);

        let mut cmd = Command::new(command);
        cmd.args(args);
        cmd.envs(env.iter().map(|(k, v)| (k, v)));
        cmd.stdin(Stdio::piped());
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());

        let _child = cmd.spawn().map_err(|e| {
            McpError::Transport(format!("Failed to spawn {}: {}", command, e))
        })?;

        // TODO: Store child process and spawn I/O handlers
        // For now, just verify the command exists
        Ok(())
    }

    /// Connect via HTTP transport
    async fn connect_http(&self, _url: &str) -> Result<()> {
        // TODO: Implement HTTP transport (SSE or WebSocket)
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

        let _result: InitializeResult = self
            .send_request("initialize", Some(params))
            .await?;

        // Send initialized notification
        self.send_notification::<Value>("notifications/initialized", None).await?;

        Ok(())
    }

    /// Refresh tools list from server
    pub async fn refresh_tools(&self) -> Result<()> {
        debug!("Refreshing tools for server: {}", self.config.name);

        // Remove old tools from this server
        self.tool_registry
            .remove_server_tools(&self.config.name)
            .await;

        let result: ListToolsResult = self
            .send_request("tools/list", None::<Value>)
            .await?;

        let tools_count = result.tools.len();
        for tool in result.tools {
            self.tool_registry.register(&self.config.name, tool).await;
        }

        info!(
            "Registered {} tools from server {}",
            tools_count,
            self.config.name
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

    /// Send JSON-RPC request and wait for response
    async fn send_request<P: serde::Serialize, R: serde::de::DeserializeOwned>(
        &self,
        method: &str,
        params: Option<P>,
    ) -> Result<R> {
        let id = self.next_id().await;
        let _request = JsonRpcRequest::new(method, params, id);

        debug!("Sending request: {} (id={})", method, id);

        // TODO: Actually send request and read response
        // This is a placeholder that returns an error
        Err(McpError::Protocol(format!(
            "Transport not implemented for {}",
            self.config.name
        )))
    }

    /// Send JSON-RPC notification (no response expected)
    async fn send_notification<P: serde::Serialize>(
        &self,
        method: &str,
        _params: Option<P>,
    ) -> Result<()> {
        debug!("Sending notification: {}", method);
        
        // TODO: Implement notification sending
        Ok(())
    }

    /// Disconnect from the server
    pub async fn disconnect(&self) -> Result<()> {
        info!("Disconnecting from MCP server: {}", self.config.name);
        
        // Remove all tools from this server
        self.tool_registry
            .remove_server_tools(&self.config.name)
            .await;

        *self.state.write().await = ServerState::Disconnected;
        
        Ok(())
    }

    /// Check if server is connected
    pub async fn is_connected(&self) -> bool {
        matches!(*self.state.read().await, ServerState::Connected { .. })
    }

    /// Get tool registry reference
    pub fn tool_registry(&self) -> &ToolRegistry {
        &self.tool_registry
    }
}

impl Drop for McpServer {
    fn drop(&mut self) {
        // TODO: Clean up child processes
        debug!("MCP server dropped: {}", self.config.name);
    }
}
