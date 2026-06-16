//! Agent Module for cozby-brain
//! 
//! Provides autonomous agent capabilities:
//! - Information gathering from multiple sources
//! - Task planning and execution
//! - Result synthesis and structuring

use crate::{McpClient, ToolContent};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, info, warn};

/// Agent state
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum AgentState {
    Idle,
    Thinking,
    Executing,
    Summarizing,
    Done,
}

/// Agent task definition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentTask {
    /// Task description
    pub description: String,
    
    /// Expected outcome
    pub goal: String,
    
    /// Constraints or requirements
    #[serde(default)]
    pub constraints: Vec<String>,
    
    /// Priority (1-10)
    #[serde(default = "default_priority")]
    pub priority: u8,
}

fn default_priority() -> u8 {
    5
}

/// Agent action (tool call or internal operation)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentAction {
    /// Action name
    pub name: String,
    
    /// Action description
    pub description: String,
    
    /// Tool to call (if applicable)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool: Option<String>,
    
    /// Tool arguments
    #[serde(skip_serializing_if = "Option::is_none")]
    pub arguments: Option<Value>,
    
    /// Expected output
    pub expected_output: String,
}

/// Agent execution result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentResult {
    /// Task description
    pub task: String,
    
    /// Final answer/summary
    pub summary: String,
    
    /// Actions taken
    pub actions: Vec<AgentAction>,
    
    /// Sources used
    pub sources: Vec<String>,
    
    /// Confidence score (0-1)
    pub confidence: f32,
}

/// Cozby Agent - autonomous information gathering and synthesis
pub struct CozbyAgent {
    mcp_client: Arc<McpClient>,
    state: Arc<RwLock<AgentState>>,
    /// System prompt for the agent
    system_prompt: String,
    /// Maximum iterations per task
    max_iterations: usize,
}

impl CozbyAgent {
    /// Create new agent
    pub fn new(mcp_client: Arc<McpClient>) -> Self {
        Self {
            mcp_client,
            state: Arc::new(RwLock::new(AgentState::Idle)),
            system_prompt: Self::default_system_prompt(),
            max_iterations: 10,
        }
    }

    /// Default system prompt for the agent
    fn default_system_prompt() -> String {
        r#"You are Cozby Agent, an autonomous AI assistant.

## Capabilities
You can use tools to gather information, search documentation, and execute tasks.

## Process
1. **Understand** the user's request
2. **Plan** what information you need
3. **Search** using available tools
4. **Synthesize** findings into a clear answer
5. **Cite** your sources

## Guidelines
- Be thorough but efficient
- Prefer official documentation
- Cross-reference multiple sources when possible
- Admit uncertainty when appropriate
- Structure answers clearly with headings and lists

## Available Tools
{tools}

Respond with your reasoning and tool calls in this format:
<thinking>
Analyze the request and plan your approach
</thinking>

<tool_call>
{{"tool": "tool_name", "arguments": {{...}}}}
</tool_call>

<answer>
Final synthesized answer with sources
</answer>"#.to_string()
    }

    /// Set custom system prompt
    pub fn with_system_prompt(mut self, prompt: String) -> Self {
        self.system_prompt = prompt;
        self
    }

    /// Set maximum iterations
    pub fn with_max_iterations(mut self, max: usize) -> Self {
        self.max_iterations = max;
        self
    }

    /// Execute a task autonomously
    pub async fn execute(&self, task: AgentTask) -> AgentResult {
        info!("Agent starting task: {}", task.description);
        *self.state.write().await = AgentState::Thinking;

        let actions = Vec::new();
        let mut sources = Vec::new();

        // Phase 1: Information gathering
        let gathered_info = self.gather_information(&task).await;
        
        for (source, _info) in &gathered_info {
            sources.push(source.clone());
            debug!("Gathered from {}: ...", source);
        }

        // Phase 2: Synthesis
        *self.state.write().await = AgentState::Summarizing;
        let summary = self.synthesize_findings(&task, &sources, &gathered_info).await;

        *self.state.write().await = AgentState::Done;

        AgentResult {
            task: task.description,
            summary,
            actions,
            sources,
            confidence: 0.8, // TODO: Calculate based on source quality
        }
    }

    /// Gather information from available tools
    async fn gather_information(&self, task: &AgentTask) -> Vec<(String, String)> {
        let mut results = Vec::new();
        
        // Get available tools
        let tools = self.mcp_client.list_all_tools().await;
        
        // Strategy: Search documentation first, then code if needed
        for (tool_name, _tool) in tools {
            if tool_name.contains("search") || tool_name.contains("docs") {
                match self.call_search_tool(&tool_name, &task.description).await {
                    Ok(info) => {
                        results.push((tool_name.clone(), info));
                    }
                    Err(e) => {
                        warn!("Tool {} failed: {}", tool_name, e);
                    }
                }
            }
        }
        
        results
    }

    /// Call a search tool
    async fn call_search_tool(&self, tool_name: &str, query: &str) -> Result<String, String> {
        let args = json!({
            "query": query,
            "limit": 5
        });
        
        match self.mcp_client.call_tool(tool_name, Some(args)).await {
            Ok(result) => {
                let mut text = String::new();
                for content in result.content {
                    if let ToolContent::Text { text: t } = content {
                        text.push_str(&t);
                    }
                }
                Ok(text)
            }
            Err(e) => Err(format!("{}", e)),
        }
    }

    /// Synthesize findings into coherent answer
    async fn synthesize_findings(
        &self,
        task: &AgentTask,
        sources: &[String],
        info: &[(String, String)],
    ) -> String {
        let mut summary = String::new();
        
        summary.push_str(&format!("# {}\n\n", task.description));
        summary.push_str(&format!("**Goal:** {}\n\n", task.goal));
        
        if !task.constraints.is_empty() {
            summary.push_str("**Constraints:**\n");
            for constraint in &task.constraints {
                summary.push_str(&format!("- {}\n", constraint));
            }
            summary.push('\n');
        }
        
        summary.push_str("## Findings\n\n");
        
        for (source, data) in info {
            summary.push_str(&format!("### From {}\n\n", source));
            summary.push_str(data);
            summary.push_str("\n\n");
        }
        
        summary.push_str("## Sources\n\n");
        for source in sources {
            summary.push_str(&format!("- {}\n", source));
        }
        
        summary
    }

    /// Get current agent state
    pub async fn state(&self) -> AgentState {
        self.state.read().await.clone()
    }

    /// Check if agent is busy
    pub async fn is_busy(&self) -> bool {
        let state = self.state.read().await;
        matches!(*state, AgentState::Thinking | AgentState::Executing | AgentState::Summarizing)
    }
}

/// Agent builder for fluent configuration
pub struct AgentBuilder {
    mcp_client: Option<Arc<McpClient>>,
    system_prompt: Option<String>,
    max_iterations: usize,
}

impl AgentBuilder {
    pub fn new() -> Self {
        Self {
            mcp_client: None,
            system_prompt: None,
            max_iterations: 10,
        }
    }

    pub fn mcp_client(mut self, client: McpClient) -> Self {
        self.mcp_client = Some(Arc::new(client));
        self
    }

    pub fn system_prompt(mut self, prompt: String) -> Self {
        self.system_prompt = Some(prompt);
        self
    }

    pub fn max_iterations(mut self, max: usize) -> Self {
        self.max_iterations = max;
        self
    }

    pub fn build(self) -> Option<CozbyAgent> {
        let agent = CozbyAgent::new(self.mcp_client?);
        
        let agent = if let Some(prompt) = self.system_prompt {
            agent.with_system_prompt(prompt)
        } else {
            agent
        };
        
        Some(agent.with_max_iterations(self.max_iterations))
    }
}

impl Default for AgentBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::McpClient;

    #[test]
    fn test_agent_builder() {
        let client = McpClient::new();
        let agent = AgentBuilder::new()
            .mcp_client(client)
            .max_iterations(5)
            .build();
        
        assert!(agent.is_some());
    }

    #[tokio::test]
    async fn test_agent_state() {
        let client = Arc::new(McpClient::new());
        let agent = CozbyAgent::new(client);
        
        assert_eq!(agent.state().await, AgentState::Idle);
        assert!(!agent.is_busy().await);
    }
}
