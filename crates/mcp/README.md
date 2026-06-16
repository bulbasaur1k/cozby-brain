# MCP Integration для cozby-brain

Model Context Protocol (MCP) реализация для подключения cozby-brain к внешним инструментам и сервисам **без привязки к dp auth**.

## Архитектура

```
┌─────────────────┐     ┌──────────────┐     ┌─────────────────┐
│  cozby-brain    │────▶│  MCP Client  │────▶│  MCP Servers    │
│  (LLM/Agent)    │     │              │     │  (stdio/HTTP)   │
└─────────────────┘     └──────────────┘     └─────────────────┘
                              │
                              ▼
                       ┌──────────────┐
                       │ Tool Registry│
                       │              │
                       │ - context7   │
                       │ - filesystem │
                       │ - git        │
                       │ - github     │
                       │ - postgres   │
                       └──────────────┘
```

## Компоненты

### Protocol (`protocol.rs`)
JSON-RPC 2.0 based MCP protocol types:
- `JsonRpcRequest/Response` - RPC envelope
- `InitializeParams/Result` - handshake
- `Tool`, `ListToolsResult` - tool discovery
- `CallToolParams/Result` - tool execution

### Config (`config.rs`)
**Универсальная конфигурация MCP серверов** (независимая от dp auth):
- Загрузка из JSON файла
- Environment variables
- Фильтрация инструментов (include/exclude)

### Server (`server.rs`)
Single MCP server connection:
- Stdio transport (spawn child process)
- HTTP transport (SSE/WebSocket)
- Tool registration
- Connection lifecycle

### Client (`client.rs`)
Multi-server manager:
- Add/remove servers
- Unified tool calling
- Tool registry integration

### Agent (`agent.rs`)
**Автономный AI агент** для поиска и структурирования информации:
- Автоматический поиск по доступным источникам
- Синтез результатов
- Цитирование источников
- State tracking (Idle/Thinking/Executing/Summarizing/Done)

### Tools (`tools.rs`)
Deferred tool registry:
- Lazy tool loading
- Server namespace isolation
- Thread-safe access

## Быстрый старт

### 1. Установка MCP серверов

```bash
# Context7 - документация библиотек
npm install -g @upstash/context7-mcp

# Filesystem - доступ к файлам
npm install -g @modelcontextprotocol/server-filesystem

# Git - операции с git
npm install -g @modelcontextprotocol/server-git
```

### 2. Конфигурация

Создайте `mcp-config.json`:

```json
{
  "servers": {
    "context7": {
      "name": "context7",
      "description": "Library documentation",
      "enabled": true,
      "transport": {
        "type": "stdio",
        "command": "npx",
        "args": ["-y", "@upstash/context7-mcp"]
      }
    }
  },
  "timeout_ms": 30000,
  "enabled_servers": ["context7"]
}
```

### 3. Использование

```rust
use cb_mcp::{McpClient, McpConfig, AgentBuilder, AgentTask};

// Загрузка конфигурации
let config = McpConfig::from_file("mcp-config.json")?;

// Создание клиента
let mut client = McpClient::new();
for server_config in config.to_server_configs() {
    client.add_server(server_config).await?;
}

// Создание агента
let agent = AgentBuilder::new()
    .mcp_client(client)
    .build()
    .unwrap();

// Задача агенту
let task = AgentTask {
    description: "Найти документацию по React Hooks".to_string(),
    goal: "Получить примеры использования useEffect".to_string(),
    constraints: vec!["Только официальная документация".to_string()],
    priority: 8,
};

// Выполнение
let result = agent.execute(task).await;
println!("{}", result.summary);
```

## Именование инструментов

Инструменты именуются с префиксом сервера:
```
mcp__{server}__{tool_name}
```

Примеры:
- `mcp__context7__get_documentation`
- `mcp__filesystem__read_file`
- `mcp__git__search_commits`

## Агентские возможности

CozbyAgent умеет:

1. **Понимать задачу** - анализировать описание и цель
2. **Планировать** - определять какие источники нужны
3. **Искать** - использовать доступные MCP инструменты
4. **Синтезировать** - объединять находки в связный ответ
5. **Цитировать** - указывать источники информации

### Пример сценария

```rust
let task = AgentTask {
    description: "Исследовать лучшие практики аутентификации в Rust".to_string(),
    goal: "Сравнить JWT, OAuth2, session-based подходы".to_string(),
    constraints: vec![
        "Указать популярные библиотеки".to_string(),
        "Привести примеры кода".to_string(),
    ],
    priority: 7,
};

let result = agent.execute(task).await;

// result.summary содержит:
// - Заголовок задачи
// - Цель
// - Ограничения
// - Найденная информация по каждому источнику
// - Список источников
```

## Доступные MCP серверы

| Сервер | Описание | Транспорт |
|--------|----------|-----------|
| context7 | Документация библиотек | stdio (npx) |
| filesystem | Доступ к файловой системе | stdio (npx) |
| git | Git операции | stdio (npx) |
| github | GitHub API | stdio (npx) |
| postgres | PostgreSQL доступ | stdio (npx) |
| custom | Ваши серверы | stdio/HTTP |

## Конфигурация

### JSON файл

```json
{
  "servers": {
    "context7": {
      "name": "context7",
      "enabled": true,
      "transport": {
        "type": "stdio",
        "command": "npx",
        "args": ["-y", "@upstash/context7-mcp"]
      },
      "tools": {
        "include": [],
        "exclude": ["dangerous_tool"]
      }
    }
  },
  "timeout_ms": 30000,
  "enabled_servers": ["context7"]
}
```

### Environment variables

```bash
export MCP_CONFIG_PATH=/path/to/config.json
export MCP_TIMEOUT_MS=60000
```

## Отличия от Nessy CLI

| Функция | Nessy CLI | cozby-brain MCP |
|---------|-----------|-----------------|
| Auth | dp auth (internal) | Независимый |
| Конфигурация | settings.json | mcp-config.json |
| Агент | Нет | CozbyAgent |
| Транспорт | stdio | stdio + HTTP |
| Инструменты | mcp__* | mcp__* (совместимы) |

## TODO

- [ ] Завершить stdio транспорт (I/O loop)
- [ ] HTTP transport (SSE)
- [ ] Интеграция с LLM crate
- [ ] Кэширование результатов
- [ ] Roots/resources поддержка
- [ ] OAuth для HTTP серверов
- [ ] Retry logic
- [ ] Rate limiting

## Примеры

- `examples/basic_usage.rs` - Базовое использование MCP
- `examples/llm_integration.rs` - Интеграция с LLM
- `examples/agent_example.rs` - Автономный агент

## Лицензия

MIT
