//! MCP (Model Context Protocol) client implementation for cozby-brain
//! 
//! This module provides MCP client functionality to connect to external MCP servers
//! and use their tools. Based on the MCP specification (JSON-RPC 2.0 over stdio/HTTP).

pub mod agent;
pub mod client;
pub mod config;
pub mod protocol;
pub mod server;
pub mod tools;

pub use agent::{AgentBuilder, AgentState, AgentTask, CozbyAgent};
pub use client::McpClient;
pub use config::McpConfig;
pub use protocol::*;
pub use server::McpServer;
pub use server::ServerConfig;
pub use server::TransportType;
pub use tools::ToolRegistry;
pub use protocol::ToolContent;
