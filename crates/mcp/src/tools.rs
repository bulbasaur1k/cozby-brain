//! Tool registry for MCP tools
//! Manages available tools from connected MCP servers

use crate::protocol::Tool;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Registry of available MCP tools
#[derive(Debug, Default, Clone)]
pub struct ToolRegistry {
    /// Map of tool_name -> (server_name, tool_definition)
    tools: Arc<RwLock<HashMap<String, (String, Tool)>>>,
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self {
            tools: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Register a tool from a specific MCP server
    pub async fn register(&self, server_name: &str, tool: Tool) {
        let mut tools = self.tools.write().await;
        let full_name = format!("mcp__{}__{}", server_name, tool.name);
        tools.insert(full_name, (server_name.to_string(), tool));
    }

    /// Remove all tools from a specific server
    pub async fn remove_server_tools(&self, server_name: &str) {
        let mut tools = self.tools.write().await;
        tools.retain(|_, (srv, _)| srv != server_name);
    }

    /// Get all registered tools
    pub async fn list_tools(&self) -> Vec<(String, Tool)> {
        let tools = self.tools.read().await;
        tools
            .iter()
            .map(|(name, (_, tool))| (name.clone(), tool.clone()))
            .collect()
    }

    /// Get a specific tool by name
    pub async fn get_tool(&self, name: &str) -> Option<(String, Tool)> {
        let tools = self.tools.read().await;
        tools.get(name).map(|(_, tool)| (name.to_string(), tool.clone()))
    }

    /// Check if a tool exists
    pub async fn has_tool(&self, name: &str) -> bool {
        let tools = self.tools.read().await;
        tools.contains_key(name)
    }

    /// Get the server name for a tool
    pub async fn get_tool_server(&self, tool_name: &str) -> Option<String> {
        let tools = self.tools.read().await;
        tools.get(tool_name).map(|(srv, _)| srv.clone())
    }

    /// Clear all tools
    pub async fn clear(&self) {
        let mut tools = self.tools.write().await;
        tools.clear();
    }

    /// Get count of registered tools
    pub async fn count(&self) -> usize {
        let tools = self.tools.read().await;
        tools.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[tokio::test]
    async fn test_register_tool() {
        let registry = ToolRegistry::new();
        let tool = Tool {
            name: "test_tool".to_string(),
            description: Some("Test tool".to_string()),
            input_schema: json!({"type": "object"}),
        };

        registry.register("test_server", tool).await;
        
        assert!(registry.has_tool("mcp__test_server__test_tool").await);
        assert_eq!(registry.count().await, 1);
    }

    #[tokio::test]
    async fn test_remove_server_tools() {
        let registry = ToolRegistry::new();
        
        registry.register("server1", Tool {
            name: "tool1".to_string(),
            description: None,
            input_schema: json!({}),
        }).await;
        
        registry.register("server2", Tool {
            name: "tool2".to_string(),
            description: None,
            input_schema: json!({}),
        }).await;

        registry.remove_server_tools("server1").await;
        
        assert!(!registry.has_tool("mcp__server1__tool1").await);
        assert!(registry.has_tool("mcp__server2__tool2").await);
    }
}
