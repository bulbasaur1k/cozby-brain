//! MCP Client - manages connections to multiple MCP servers

use crate::protocol::{CallToolResult, Tool};
use crate::server::{McpServer, Result, ServerConfig};
use crate::tools::ToolRegistry;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{info, warn};

/// MCP Client that manages multiple server connections
pub struct McpClient {
    servers: Arc<RwLock<HashMap<String, McpServer>>>,
    tool_registry: ToolRegistry,
}

impl McpClient {
    pub fn new() -> Self {
        Self {
            servers: Arc::new(RwLock::new(HashMap::new())),
            tool_registry: ToolRegistry::new(),
        }
    }

    /// Get shared tool registry
    pub fn tool_registry(&self) -> &ToolRegistry {
        &self.tool_registry
    }

    /// Add an MCP server configuration
    pub async fn add_server(&self, config: ServerConfig) -> Result<()> {
        let name = config.name.clone();
        
        let server = McpServer::new(config, self.tool_registry.clone());
        
        // Connect to the server
        server.connect().await?;
        
        let mut servers = self.servers.write().await;
        servers.insert(name, server);
        
        Ok(())
    }

    /// Remove an MCP server
    pub async fn remove_server(&self, name: &str) -> Result<()> {
        let mut servers = self.servers.write().await;
        
        if let Some(server) = servers.remove(name) {
            server.disconnect().await?;
            info!("Removed MCP server: {}", name);
        } else {
            warn!("Attempted to remove unknown server: {}", name);
        }
        
        Ok(())
    }

    /// List all configured servers
    pub async fn list_servers(&self) -> Vec<String> {
        let servers = self.servers.read().await;
        servers.keys().cloned().collect()
    }

    /// Get server status
    pub async fn get_server_status(&self, name: &str) -> Option<bool> {
        let servers = self.servers.read().await;
        if let Some(server) = servers.get(name) {
            Some(server.is_connected().await)
        } else {
            None
        }
    }

    /// Call a tool by its fully qualified name (mcp__server__tool)
    pub async fn call_tool(&self, full_name: &str, arguments: Option<Value>) -> Result<CallToolResult> {
        // Parse tool name: mcp__{server}__{tool}
        let parts: Vec<&str> = full_name.split("__").collect();
        
        if parts.len() != 3 || parts[0] != "mcp" {
            return Err(crate::server::McpError::Protocol(
                format!("Invalid tool name format: {}", full_name)
            ));
        }
        
        let server_name = parts[1];
        let tool_name = parts[2];
        
        let servers = self.servers.read().await;
        let server = servers.get(server_name)
            .ok_or_else(|| crate::server::McpError::ToolNotFound(
                format!("Server '{}' not found", server_name)
            ))?;
        
        server.call_tool(tool_name, arguments).await
    }

    /// List all available tools from all servers
    pub async fn list_all_tools(&self) -> Vec<(String, Tool)> {
        self.tool_registry.list_tools().await
    }

    /// Get a specific tool
    pub async fn get_tool(&self, name: &str) -> Option<(String, Tool)> {
        self.tool_registry.get_tool(name).await
    }

    /// Refresh tools from all connected servers
    pub async fn refresh_all_tools(&self) {
        let servers = self.servers.read().await;
        for server in servers.values() {
            if let Err(e) = server.refresh_tools().await {
                warn!("Failed to refresh tools for {}: {}", server.config.name, e);
            }
        }
    }

    /// Disconnect all servers
    pub async fn disconnect_all(&self) {
        let servers = self.servers.read().await;
        for server in servers.values() {
            let _ = server.disconnect().await;
        }
    }
}

impl Default for McpClient {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for McpClient {
    fn drop(&mut self) {
        // Note: Can't await in drop, but servers will clean up themselves
        info!("MCP Client dropped");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_client_creation() {
        let client = McpClient::new();
        assert!(client.list_servers().await.is_empty());
    }
}
