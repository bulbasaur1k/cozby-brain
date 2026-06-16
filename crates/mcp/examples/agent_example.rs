//! Пример использования CozbyAgent для автономного поиска информации

use cb_mcp::{AgentBuilder, AgentTask, McpClient, McpConfig};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Загружаем конфигурацию MCP серверов
    let config = McpConfig::default_config();
    
    // Создаём MCP клиент
    let mcp_client = McpClient::new();
    
    // Добавляем сервера из конфига
    for server_config in config.to_server_configs() {
        let name = server_config.name.clone();
        println!("Подключение к серверу: {}", name);
        if let Err(e) = mcp_client.add_server(server_config).await {
            eprintln!("Ошибка подключения к {}: {}", name, e);
        }
    }
    
    // Создаём агента
    let agent = AgentBuilder::new()
        .mcp_client(mcp_client)
        .max_iterations(15)
        .build()
        .expect("Failed to create agent");
    
    // Пример 1: Поиск информации о библиотеке
    let task = AgentTask {
        description: "Найти документацию по React Hooks useEffect".to_string(),
        goal: "Получить актуальную документацию и примеры использования".to_string(),
        constraints: vec![
            "Использовать только официальную документацию".to_string(),
            "Привести примеры кода".to_string(),
        ],
        priority: 8,
    };
    
    println!("\n🤔 Агент думает...");
    let result = agent.execute(task).await;
    
    println!("\n✅ Результат:");
    println!("{}", result.summary);
    
    // Пример 2: Исследование темы
    let task2 = AgentTask {
        description: "Исследовать лучшие практики аутентификации в Rust".to_string(),
        goal: "Собрать информацию о JWT, OAuth2, session-based auth".to_string(),
        constraints: vec![
            "Сравнить подходы".to_string(),
            "Указать библиотеки".to_string(),
        ],
        priority: 7,
    };
    
    println!("\n🤔 Агент исследует тему...");
    let result2 = agent.execute(task2).await;
    
    println!("\n✅ Результат:");
    println!("{}", result2.summary);
    
    Ok(())
}
