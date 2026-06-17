# Настройка MCP-серверов для агента cozby-brain

Агент `cozby-brain` умеет собирать контекст из внешних **MCP-серверов** (Model
Context Protocol) — например, внутренней wiki, Jira, сервис-каталога компании —
и использовать их при формировании записей/узлов.

Поддерживаются два транспорта: **stdio** (дочерний процесс, JSON-RPC построчно)
и **HTTP/SSE** (POST JSON-RPC, ответ `application/json` либо `text/event-stream`).

---

## 1. Создать конфиг

Скопируй пример в рабочий файл (он в `.gitignore` — секреты не попадут в git):

```bash
cp mcp-config.example.json mcp-config.json
```

По умолчанию сервер ищет `./mcp-config.json` в рабочей директории. Путь можно
переопределить переменной окружения:

```bash
export MCP_CONFIG=/path/to/your/mcp-config.json
```

Если файла нет — это не ошибка: MCP просто отключён, агент работает только с
внутренними инструментами (RAG-поиск по своей базе).

---

## 2. Формат конфига

```jsonc
{
  "servers": {
    "<имя>": {
      "name": "<имя>",                 // используется в имени tool: mcp__<имя>__<tool>
      "description": "что это",
      "enabled": true,                  // false → сервер пропускается
      "transport": { ... },             // stdio или http (см. ниже)
      "tools": {                        // опционально: фильтр инструментов
        "include": [],                  //   пусто = все
        "exclude": ["delete_file"]      //   что исключить
      }
    }
  },
  "timeout_ms": 30000,                  // таймаут одного запроса (мс)
  "enabled_servers": ["<имя>", ...]     // [] = все с enabled:true; иначе только перечисленные
}
```

### Транспорт stdio (локальный процесс)

```jsonc
"transport": {
  "type": "stdio",
  "command": "dp",
  "args": ["ai", "mcp", "wixie"],
  "env": { "SOME_VAR": "value" }        // опционально
}
```

### Транспорт HTTP/SSE (удалённый сервер)

```jsonc
"transport": {
  "type": "http",
  "url": "https://mcp.internal.company/mcp",
  "headers": { "Authorization": "Bearer <token>" }
}
```

---

## 3. Включить агентный ingest

Сам по себе MCP подключается всегда (если есть конфиг), но многошаговый
**агент**, который вызывает MCP-инструменты при `POST /api/ingest`, включается
опт-ин через переменную окружения:

```bash
# в .env или окружении сервера
INGEST_AGENT=1
```

Без флага `/api/ingest` работает по-старому (одношаговый классификатор).

---

## 4. Проверка

После старта сервера в логах должно появиться:

```
mcp server connected server=dpWiki
Registered N tools from server dpWiki
```

Команды:

```bash
./run.sh logs                 # хвост лога
curl -sf http://localhost:8081/health
```

Затем прогони агентный ingest (с `INGEST_AGENT=1`), который потребует внешних
данных — в логах увидишь шаг `tool mcp__dpWiki__... → N chars`:

```bash
curl -s -X POST http://localhost:8081/api/ingest \
  -H 'content-type: application/json' \
  -d '{"raw":"заступаю на дежурство billing, подтяни runbook из wiki и собери узел"}'
```

---

## 5. Как агент видит инструменты

- Все tool'ы со всех серверов получают имя `mcp__<server>__<tool>` и
  передаются агенту в каталог инструментов system-промпта.
- Агент сам решает, когда вызвать внешний инструмент, а когда — внутренний
  `search_kb` (RAG по личной базе).
- Результаты сжимаются (`summary_compression`) перед вставкой в контекст —
  важно для слабых внутренних LLM (qwen3) с маленьким окном.

---

## 6. Безопасность

- `mcp-config.json` **в `.gitignore`** — корп-эндпоинты и токены не коммитятся.
- Для секретов лучше держать токен в env и подставлять в `headers` руками при
  деплое, а не хранить в репозитории.
- Фильтр `tools.exclude` помогает не давать агенту опасные операции
  (например, запись/удаление), оставив только чтение/поиск.
