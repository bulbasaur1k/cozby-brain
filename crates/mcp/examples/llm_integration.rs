//! Интеграция MCP с LLM для вызова инструментов
//! 
//! Этот пример показывает как LLM может использовать MCP инструменты.

use cb_mcp::{McpClient, ServerConfig, ToolContent, TransportType};
use serde_json::json;

/// Инструмент для вызова из LLM
#[derive(Debug, Clone)]
pub struct LlmTool {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
}

impl LlmTool {
    /// Конвертирует MCP Tool в формат для LLM
    pub fn from_mcp_tool(name: String, tool: cb_mcp::Tool) -> Self {
        Self {
            name,
            description: tool.description.unwrap_or_default(),
            parameters: tool.input_schema,
        }
    }
}

/// Интеграция MCP с LLM
pub struct McpLlmIntegration {
    mcp_client: McpClient,
}

impl McpLlmIntegration {
    pub fn new(mcp_client: McpClient) -> Self {
        Self { mcp_client }
    }

    /// Получить список инструментов для LLM system prompt
    pub async fn get_tools_for_llm(&self) -> Vec<LlmTool> {
        let mut tools = Vec::new();
        
        for (name, tool) in self.mcp_client.list_all_tools().await {
            tools.push(LlmTool::from_mcp_tool(name, tool));
        }
        
        tools
    }

    /// Сгенерировать system prompt с описанием инструментов
    pub async fn generate_system_prompt(&self) -> String {
        let tools = self.get_tools_for_llm().await;
        
        let mut prompt = String::from(
            "You have access to the following tools:\n\n"
        );
        
        for tool in tools {
            prompt.push_str(&format!(
                "## {}\n{}\nParameters: {}\n\n",
                tool.name,
                tool.description,
                tool.parameters
            ));
        }
        
        prompt.push_str(
            "To call a tool, respond with a JSON object:\n\
            {\n  \"tool\": \"tool_name\",\n  \"arguments\": { ... }\n}\n"
        );
        
        prompt
    }

    /// Вызвать инструмент по запросу LLM
    pub async fn call_tool_from_llm(
        &self,
        tool_name: &str,
        arguments: serde_json::Value,
    ) -> Result<String, Box<dyn std::error::Error>> {
        let result = self.mcp_client.call_tool(tool_name, Some(arguments)).await?;
        
        // Конвертируем результат в текст для LLM
        let mut text = String::new();
        for content in result.content {
            match content {
                ToolContent::Text { text: t } => {
                    text.push_str(&t);
                }
                ToolContent::Image { data: _, mime_type } => {
                    text.push_str(&format!("[Image: {}]", mime_type));
                    // Можно сохранить data в файл и вернуть путь
                }
                ToolContent::Resource { resource } => {
                    text.push_str(&format!("[Resource: {}]", resource));
                }
            }
        }
        
        Ok(text)
    }

    /// Обработать запрос LLM к инструменту
    pub async fn handle_llm_tool_call(
        &self,
        llm_response: &str,
    ) -> Result<String, Box<dyn std::error::Error>> {
        // Парсим JSON ответ от LLM
        if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(llm_response) {
            if let Some(tool_name) = parsed.get("tool").and_then(|v| v.as_str()) {
                let arguments = parsed
                    .get("arguments")
                    .cloned()
                    .unwrap_or(json!({}));
                
                return self.call_tool_from_llm(tool_name, arguments).await;
            }
        }
        
        // Если это не JSON с инструментом, возвращаем как есть
        Ok(llm_response.to_string())
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Создаём MCP клиент
    let mcp_client = McpClient::new();
    
    // Добавляем сервер
    mcp_client.add_server(ServerConfig {
        name: "dpWiki".to_string(),
        transport: TransportType::Stdio {
            command: "dp".to_string(),
            args: vec!["ai".into(), "mcp".into(), "wixie".into()],
            env: vec![],
        },
    }).await?;
    
    // Создаём интеграцию
    let integration = McpLlmIntegration::new(mcp_client);
    
    // Генерируем system prompt
    let prompt = integration.generate_system_prompt().await;
    println!("System prompt для LLM:\n{}\n", prompt);
    
    // Пример обработки вызова инструмента от LLM
    let llm_response = r#"{
        "tool": "mcp__dpWiki__dp_wixie_search",
        "arguments": {"query": "API documentation"}
    }"#;
    
    let result = integration.handle_llm_tool_call(llm_response).await?;
    println!("Результат вызова инструмента:\n{}", result);
    
    Ok(())
}
