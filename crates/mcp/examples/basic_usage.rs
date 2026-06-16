//! Пример использования MCP в cozby-brain
//! 
//! Этот пример показывает как подключать MCP серверы и вызывать их инструменты.

use cb_mcp::{McpClient, ServerConfig, TransportType, ToolContent};
use serde_json::json;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Создаём MCP клиент
    let client = McpClient::new();
    
    // Пример 1: Добавление MCP сервера через stdio (как в Nessy CLI)
    // dpWiki - работа с Confluence
    client.add_server(ServerConfig {
        name: "dpWiki".to_string(),
        timeout_ms: ServerConfig::default_timeout(),
        transport: TransportType::Stdio {
            command: "dp".to_string(),
            args: vec!["ai".into(), "mcp".into(), "wixie".into()],
            env: vec![],
        },
    }).await?;
    
    // Пример 2: Добавление MCP сервера dpJira
    client.add_server(ServerConfig {
        name: "dpJira".to_string(),
        timeout_ms: ServerConfig::default_timeout(),
        transport: TransportType::Stdio {
            command: "dp".to_string(),
            args: vec!["ai".into(), "mcp".into(), "jira".into()],
            env: vec![],
        },
    }).await?;
    
    // Пример 3: HTTP транспорт (для удалённых серверов)
    client.add_server(ServerConfig {
        name: "remote-server".to_string(),
        timeout_ms: ServerConfig::default_timeout(),
        transport: TransportType::Http {
            url: "http://localhost:8080/mcp".to_string(),
            headers: vec![],
        },
    }).await?;
    
    // Список доступных инструментов
    println!("Доступные инструменты:");
    for (name, tool) in client.list_all_tools().await {
        println!("  {} - {}", name, tool.description.unwrap_or_default());
    }
    
    // Пример 4: Вызов инструмента поиска в Wiki
    let search_result = client.call_tool(
        "mcp__dpWiki__dp_wixie_search",
        Some(json!({
            "query": "API документация",
            "space": "DEV"
        }))
    ).await?;
    
    println!("\nРезультаты поиска:");
    for content in search_result.content {
        if let ToolContent::Text { text } = content {
            println!("{}", text);
        }
    }
    
    // Пример 5: Получение задачи из Jira
    let issue_result = client.call_tool(
        "mcp__dpJira__dp_jira_issue",
        Some(json!({
            "key": "PROJ-123"
        }))
    ).await?;
    
    for content in issue_result.content {
        if let ToolContent::Text { text } = content {
            println!("\nЗадача: {}", text);
        }
    }
    
    // Пример 6: Проверка статуса сервера
    if let Some(connected) = client.get_server_status("dpWiki").await {
        println!("\ndpWiki подключён: {}", connected);
    }
    
    // Пример 7: Удаление сервера
    client.remove_server("remote-server").await?;
    
    Ok(())
}
