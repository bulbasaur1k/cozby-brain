//! MCP Configuration
//! 
//! Configures MCP servers independently from dp auth.
//! Servers can be configured via JSON file or environment variables.

use crate::server::{ServerConfig, TransportType};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum McpConfigError {
    #[error("Failed to read config file: {0}")]
    Io(#[from] std::io::Error),
    
    #[error("Failed to parse config: {0}")]
    Json(#[from] serde_json::Error),
    
    #[error("Invalid server configuration: {0}")]
    InvalidServer(String),
}

/// MCP configuration for cozby-brain
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpConfig {
    /// MCP servers configuration
    pub servers: HashMap<String, McpServerConfig>,
    
    /// Default timeout for tool calls (milliseconds)
    #[serde(default = "default_timeout")]
    pub timeout_ms: u64,
    
    /// Enable/disable specific servers
    #[serde(default)]
    pub enabled_servers: Vec<String>,
}

fn default_timeout() -> u64 {
    30000 // 30 seconds
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpServerConfig {
    /// Server name (used in tool naming: mcp__{name}__{tool})
    pub name: String,
    
    /// Transport type
    pub transport: TransportConfig,
    
    /// Optional description
    #[serde(default)]
    pub description: Option<String>,
    
    /// Whether this server is enabled
    #[serde(default = "default_true")]
    pub enabled: bool,
    
    /// Optional tool filters (include/exclude)
    #[serde(default)]
    pub tools: ToolFilter,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum TransportConfig {
    Stdio {
        /// Command to execute (e.g., "npx", "node", "python")
        command: String,
        
        /// Command arguments
        #[serde(default)]
        args: Vec<String>,
        
        /// Environment variables
        #[serde(default)]
        env: HashMap<String, String>,
    },
    Http {
        /// Server URL (SSE or WebSocket endpoint)
        url: String,
        
        /// HTTP headers
        #[serde(default)]
        headers: HashMap<String, String>,
    },
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ToolFilter {
    /// Include only these tools (empty = all)
    #[serde(default)]
    pub include: Vec<String>,
    
    /// Exclude these tools
    #[serde(default)]
    pub exclude: Vec<String>,
}

impl McpConfig {
    /// Load configuration from file
    pub fn from_file<P: AsRef<Path>>(path: P) -> Result<Self, McpConfigError> {
        let content = std::fs::read_to_string(path)?;
        let config: McpConfig = serde_json::from_str(&content)?;
        Ok(config)
    }

    /// Save configuration to file
    pub fn to_file<P: AsRef<Path>>(&self, path: P) -> Result<(), McpConfigError> {
        let content = serde_json::to_string_pretty(self)?;
        std::fs::write(path, content)?;
        Ok(())
    }

    /// Create default configuration with common MCP servers
    pub fn default_config() -> Self {
        let mut servers = HashMap::new();
        
        // Example: Context7 MCP (documentation)
        servers.insert("context7".to_string(), McpServerConfig {
            name: "context7".to_string(),
            transport: TransportConfig::Stdio {
                command: "npx".to_string(),
                args: vec!["-y".into(), "@upstash/context7-mcp".into()],
                env: HashMap::new(),
            },
            description: Some("Library documentation and code examples".into()),
            enabled: true,
            tools: ToolFilter::default(),
        });
        
        // Example: Filesystem MCP
        servers.insert("filesystem".to_string(), McpServerConfig {
            name: "filesystem".to_string(),
            transport: TransportConfig::Stdio {
                command: "npx".to_string(),
                args: vec!["-y".into(), "@modelcontextprotocol/server-filesystem".into(), ".".into()],
                env: HashMap::new(),
            },
            description: Some("Local file system access".into()),
            enabled: false, // Disabled by default for security
            tools: ToolFilter::default(),
        });
        
        Self {
            servers,
            timeout_ms: 30000,
            enabled_servers: vec!["context7".to_string()],
        }
    }

    /// Convert to ServerConfig for McpClient
    pub fn to_server_configs(&self) -> Vec<ServerConfig> {
        self.servers
            .values()
            .filter(|s| s.enabled && self.enabled_servers.is_empty() || self.enabled_servers.contains(&s.name))
            .map(|s| ServerConfig {
                name: s.name.clone(),
                timeout_ms: self.timeout_ms,
                transport: match &s.transport {
                    TransportConfig::Stdio { command, args, env } => {
                        TransportType::Stdio {
                            command: command.clone(),
                            args: args.clone(),
                            env: env.iter().map(|(k, v)| (k.clone(), v.clone())).collect(),
                        }
                    }
                    TransportConfig::Http { url, headers } => {
                        TransportType::Http {
                            url: url.clone(),
                            headers: headers.iter().map(|(k, v)| (k.clone(), v.clone())).collect(),
                        }
                    }
                },
            })
            .collect()
    }
}

impl Default for McpConfig {
    fn default() -> Self {
        Self::default_config()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = McpConfig::default();
        assert!(config.servers.contains_key("context7"));
        assert_eq!(config.timeout_ms, 30000);
    }

    #[test]
    fn test_serialize_deserialize() {
        let config = McpConfig::default();
        let json = serde_json::to_string(&config).unwrap();
        let parsed: McpConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(config.servers.len(), parsed.servers.len());
    }
}
