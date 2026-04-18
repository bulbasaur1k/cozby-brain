# cozby-brain — Промт для AI-агента (поддержка и разработка)

Ты — разработчик, который поддерживает и расширяет Rust-приложение **cozby-brain**. Ниже — полное описание архитектуры, всех файлов, правил и паттернов. Следуй им строго.

---

## Что это за проект

AI-органайзер заметок, задач и напоминаний. Два независимых бинарника:
- **cozby-brain** — HTTP-сервер (backend)
- **cozby** — console UI (frontend, отдельный крейт)

Стек: Rust, Axum 0.8, Ractor 0.15 (акторы), sqlx 0.8 (Postgres), qdrant-client (vector search), reqwest (LLM/embedding).

---

## Структура проекта

```
cozby-brain/
├── Cargo.toml                          # workspace root
├── crates/
│   ├── backend/                        # бинарник cozby-brain
│   │   ├── Cargo.toml
│   │   ├── migrations/
│   │   │   ├── 0001_init.sql           # таблица notes
│   │   │   └── 0002_todos_reminders.sql # таблицы todos, reminders
│   │   └── src/
│   │       ├── main.rs                 # entrypoint: tracing + axum::serve
│   │       ├── lib.rs                  # re-exports модулей
│   │       ├── app_state.rs            # AppState {actors, llm, embedding, vector, db}
│   │       ├── bootstrap.rs            # wiring: config → db → llm → qdrant → actors → router
│   │       ├── domain/
│   │       │   ├── entities.rs         # Note, Todo, Reminder (чистые структуры)
│   │       │   ├── errors.rs           # DomainError
│   │       │   └── services.rs         # чистые функции (validate, create_*, extract_wiki_links)
│   │       ├── application/
│   │       │   ├── ports.rs            # ВСЕ trait-интерфейсы (порты)
│   │       │   └── llm_use_cases.rs    # LLM use-cases с fallback
│   │       └── infrastructure/
│   │           ├── actors/             # NoteActor, TodoActor, ReminderActor
│   │           ├── persistence/        # PgNoteRepository, PgTodoRepository, PgReminderRepository
│   │           ├── vector/             # QdrantVectorStore, NoopVectorStore
│   │           ├── llm/               # OpenAICompatClient, NoopLlmClient
│   │           ├── notifications/     # LogNotifier, StdoutNotifier, CompositeNotifier
│   │           ├── web/               # dto.rs, handlers.rs, routes.rs
│   │           └── config.rs          # AppConfig из env-переменных
│   └── cli/                           # бинарник cozby
│       ├── Cargo.toml
│       └── src/main.rs                # clap + dialoguer + reqwest (HTTP-клиент)
├── docker-compose.yml                 # postgres + qdrant + app
├── Dockerfile                         # multi-stage rust:1.90 → debian:bookworm-slim
├── .env.example
└── README.md
```

---

## Архитектурные правила (ОБЯЗАТЕЛЬНО СОБЛЮДАТЬ)

### 1. Hexagonal Architecture

Направление зависимостей: `infrastructure → application → domain`. Никогда наоборот.

- **domain/** — чистый Rust. ЗАПРЕЩЕНЫ: `use ractor`, `use axum`, `use tokio`, `use sqlx`, `use reqwest`. Разрешены: `serde`, `chrono`, `uuid`, `thiserror`. Без `async`.
- **application/** — trait-порты (интерфейсы) и use-cases. Не знает про конкретные БД и фреймворки.
- **infrastructure/** — всё внешнее: БД, HTTP, акторы, LLM, Qdrant, конфиг.

### 2. Actor Model (Ractor)

- Акторы — единственные владельцы mutable state.
- HTTP-handler'ы общаются с акторами через `call!()` (RPC с ответом) или `cast!()` (fire-and-forget).
- **НЕЛЬЗЯ** добавлять `#[derive(Clone)]` на enum сообщений — `RpcReplyPort<T>` не Clone.
- Ractor 0.15 использует native async — **НЕ** нужен `#[ractor::async_trait]` на `impl Actor`.

### 3. Write-Through Pattern

В каждом мутирующем обработчике актора:
1. Валидируй через domain service
2. Запиши в БД через repo
3. **Только при успехе** обнови in-memory HashMap
4. При ошибке БД — верни ошибку, НЕ трогай state

### 4. Axum 0.8

- Path-параметры через `{id}`, **НЕ** `:id`
- JSON-ответы: `{"status":"ok","data":{...}}` или `{"error":"описание"}`
- Коды: 200 / 201 / 400 / 404 / 500

### 5. LLM Use-Cases

- Все функции **infallible** — при ошибке LLM тихо используют fallback.
- Не логируют warn если ошибка = `NotConfigured` (это нормально).
- LLM промпты просят JSON — `extract_json()` вытаскивает первый `{...}` блок.
- **Reasoning-модели** (GLM-4.7, o1) возвращают `content: null` + `reasoning: "..."`. Клиент OpenAICompat обрабатывает это: content → reasoning → error.

---

## Все порты (trait-интерфейсы) — файл `application/ports.rs`

```rust
// LLM
trait LlmClient: Send + Sync {
    fn name(&self) -> &str;
    async fn complete_text(&self, system: &str, user: &str) -> Result<String, LlmError>;
}

// Embedding
trait EmbeddingClient: Send + Sync {
    fn name(&self) -> &str;
    async fn embed(&self, text: &str) -> Result<Vec<f32>, LlmError>;
}

// Vector Store (Qdrant)
trait VectorStore: Send + Sync {
    async fn upsert(&self, id: &str, vector: Vec<f32>, title: &str, tags: &[String]) -> Result<(), RepoError>;
    async fn search(&self, vector: Vec<f32>, limit: usize) -> Result<Vec<SimilarNote>, RepoError>;
    async fn delete(&self, id: &str) -> Result<(), RepoError>;
}

// Repositories
trait NoteRepository: Send + Sync {
    async fn upsert(&self, note: &Note) -> Result<(), RepoError>;
    async fn delete(&self, id: &str) -> Result<(), RepoError>;
    async fn get(&self, id: &str) -> Result<Option<Note>, RepoError>;
    async fn list(&self) -> Result<Vec<Note>, RepoError>;
    async fn search(&self, query: &str) -> Result<Vec<Note>, RepoError>;
}

trait TodoRepository: Send + Sync {
    async fn upsert(&self, todo: &Todo) -> Result<(), RepoError>;
    async fn delete(&self, id: &str) -> Result<(), RepoError>;
    async fn list(&self) -> Result<Vec<Todo>, RepoError>;
}

trait ReminderRepository: Send + Sync {
    async fn upsert(&self, reminder: &Reminder) -> Result<(), RepoError>;
    async fn delete(&self, id: &str) -> Result<(), RepoError>;
    async fn list(&self) -> Result<Vec<Reminder>, RepoError>;
    async fn set_fired(&self, id: &str, fired: bool) -> Result<(), RepoError>;
}

// Notifications
trait Notifier: Send + Sync {
    fn name(&self) -> &str;
    async fn notify(&self, n: &Notification) -> Result<(), NotifyError>;
}
```

---

## Сообщения акторов (message enums)

### NoteMsg

```rust
pub enum NoteMsg {
    Create(String, String, Vec<String>, RpcReplyPort<Result<Note, String>>),
    //     title   content tags         reply
    Update(String, String, String, Vec<String>, RpcReplyPort<Result<Note, String>>),
    //     id      title   content tags         reply
    Delete(String, RpcReplyPort<Result<(), String>>),
    Get(String, RpcReplyPort<Option<Note>>),
    List(RpcReplyPort<Vec<Note>>),
    Search(String, RpcReplyPort<Vec<Note>>),
}
```

### TodoMsg

```rust
pub enum TodoMsg {
    Create(String, Option<DateTime<Utc>>, RpcReplyPort<Result<Todo, String>>),
    //     title   due_at                 reply
    Complete(String, RpcReplyPort<Result<Todo, String>>),
    Delete(String, RpcReplyPort<Result<(), String>>),
    List(RpcReplyPort<Vec<Todo>>),
}
```

### ReminderMsg

```rust
pub enum ReminderMsg {
    Create(String, DateTime<Utc>, RpcReplyPort<Result<Reminder, String>>),
    //     text    remind_at      reply
    Delete(String, RpcReplyPort<Result<(), String>>),
    List(RpcReplyPort<Vec<Reminder>>),
    CheckDue,  // нет reply — fire-and-forget через cast!()
}
```

---

## Все HTTP-эндпоинты

```
GET  /health

# Notes (CRUD)
GET  /api/notes                    → list
POST /api/notes                    → create {title, content, tags[]}
GET  /api/notes/search?q=          → search
GET  /api/notes/{id}               → get (+ links: wiki-links)
PUT  /api/notes/{id}               → update {title, content, tags[]}
DEL  /api/notes/{id}               → delete

# Todos
GET  /api/todos                    → list
POST /api/todos                    → create {title, due_at?}
POST /api/todos/{id}/complete      → done
DEL  /api/todos/{id}               → delete

# Reminders
GET  /api/reminders                → list
POST /api/reminders                → create {text, remind_at}
DEL  /api/reminders/{id}           → delete

# LLM Ingest (2-step для notes, 1-step для todo/reminder)
POST /api/ingest/note              → step 1: {raw} → {structured, suggestion}
POST /api/ingest/note/confirm      → step 2: {action, target_id?, title, content, tags}
POST /api/ingest/todo              → {raw} → created todo
POST /api/ingest/reminder          → {raw} → created reminder

# Smart Search
GET  /api/ask?q=                   → {keywords, data: {notes, todos, reminders}}
```

---

## 2-step Ingest Flow (для notes)

**Step 1** (`POST /api/ingest/note` с `{raw}`):
1. `structure_note(llm, raw)` → `{title, content, tags}`
2. `embedding.embed(content)` → `Vec<f32>` (если configured)
3. `vector.search(embedding, limit=5)` → `Vec<SimilarNote>` (score_threshold=0.5)
4. `find_best_match(llm, structured, candidates)` → `Option<AppendSuggestion>`
5. Возвращает `{structured, suggestion}` — **НЕ создаёт заметку**

**Step 2** (`POST /api/ingest/note/confirm`):
- `action="create"` → `NoteMsg::Create`
- `action="append"` → `NoteMsg::Get` + мерджим content через `\n\n---\n\n` + merge tags → `NoteMsg::Update`
- После успеха: fire-and-forget `tokio::spawn` → embed → qdrant upsert

---

## Env-переменные

| Переменная | Обязательная | Default | Описание |
|---|---|---|---|
| `DATABASE_URL` | да | — | Postgres DSN |
| `HTTP_ADDR` | нет | `0.0.0.0:8080` | адрес сервера |
| `RUST_LOG` | нет | `info` | уровень логов |
| `LLM_BASE_URL` | нет | — | OpenAI-compatible base URL |
| `LLM_API_KEY` | нет | — | Bearer token (пусто для Ollama) |
| `LLM_MODEL` | нет | — | имя модели |
| `EMBEDDING_MODEL` | нет | — | имя embedding модели |
| `QDRANT_URL` | нет | — | gRPC URL Qdrant (e.g. `http://localhost:6334`) |
| `QDRANT_COLLECTION` | нет | `cozby_notes` | имя коллекции |
| `COZBY_API` | нет | `http://localhost:8080` | для CLI: URL бэкенда |

Если `LLM_BASE_URL` или `LLM_MODEL` пусты → используется `NoopLlmClient`.
Если `QDRANT_URL` пуста → используется `NoopVectorStore`.
Если `EMBEDDING_MODEL` пуста → embedding не работает → suggestion всегда null.

---

## Wiring в bootstrap.rs (порядок важен)

1. `AppConfig::from_env()`
2. `PgPoolOptions::new().max_connections(10).acquire_timeout(10s).connect(url)`
3. `sqlx::migrate!().run(&pool)` — миграции при старте
4. LLM: `cfg.llm()` → `OpenAICompatClient::with_embedding()` или `NoopLlmClient`
   - **Один экземпляр** `OpenAICompatClient` используется как `Arc<dyn LlmClient>` И как `Arc<dyn EmbeddingClient>`
5. Vector: `cfg.qdrant_url()` → `QdrantVectorStore` или `NoopVectorStore`
6. Notifier: `CompositeNotifier([LogNotifier, StdoutNotifier])`
7. Repositories: `PgNoteRepository`, `PgTodoRepository`, `PgReminderRepository`
8. Spawn actors: `NoteActor`, `TodoActor`, `ReminderActor`
9. `spawn_reminder_ticker` — tokio loop, каждые 10 сек `cast!(actor, ReminderMsg::CheckDue)`
10. `AppState { note_actor, todo_actor, reminder_actor, llm, embedding, vector, db }`
11. `create_router(state)`

---

## Domain entities

```rust
struct Note {
    id: String,                    // UUID v4 as String
    title: String,
    content: String,               // markdown
    tags: Vec<String>,             // lowercase, sorted, deduped
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

struct Todo {
    id: String,
    title: String,
    done: bool,
    due_at: Option<DateTime<Utc>>,
    created_at: DateTime<Utc>,
    completed_at: Option<DateTime<Utc>>,
}

struct Reminder {
    id: String,
    text: String,
    remind_at: DateTime<Utc>,
    fired: bool,
    created_at: DateTime<Utc>,
}
```

---

## Таблицы БД (Postgres)

```sql
-- notes
CREATE TABLE notes (
    id TEXT PRIMARY KEY, title TEXT NOT NULL, content TEXT NOT NULL,
    tags TEXT[] NOT NULL DEFAULT '{}',
    created_at TIMESTAMPTZ NOT NULL, updated_at TIMESTAMPTZ NOT NULL
);
-- indexes: notes_title_idx, notes_tags_idx (GIN), notes_updated_idx

-- todos
CREATE TABLE todos (
    id TEXT PRIMARY KEY, title TEXT NOT NULL, done BOOLEAN NOT NULL DEFAULT FALSE,
    due_at TIMESTAMPTZ, created_at TIMESTAMPTZ NOT NULL, completed_at TIMESTAMPTZ
);

-- reminders
CREATE TABLE reminders (
    id TEXT PRIMARY KEY, text TEXT NOT NULL, remind_at TIMESTAMPTZ NOT NULL,
    fired BOOLEAN NOT NULL DEFAULT FALSE, created_at TIMESTAMPTZ NOT NULL
);
```

---

## CLI (cozby) — структура команд

```
cozby [--api URL]
  note add [--title T] [--file PATH] [--content C] [--tags a,b]
  note list | show <id> | search <q> | rm <id>
  todo add <title> [--due +30m|RFC3339]
  todo list | done <id> | rm <id>
  remind add <text> --at <+30m|RFC3339>
  remind list | rm <id>
  ingest note [--text T | --file P | stdin]    # 2-step: ask user create/append
  ingest todo <text>
  ingest remind <text>
  ask <query>
  (без subcommand) → интерактивный режим с dialoguer
```

CLI — чистый HTTP-клиент (reqwest), 0 зависимости от backend crate.

---

## Как добавить новую фичу — пошаговый алгоритм

### Новая domain entity

1. Добавь struct в `domain/entities.rs` (без async, без фреймворков)
2. Добавь ошибки в `domain/errors.rs`
3. Добавь чистые функции в `domain/services.rs` + unit-тесты
4. Добавь trait-порт в `application/ports.rs`
5. Добавь миграцию в `migrations/`
6. Создай `infrastructure/persistence/<name>_repo.rs` (sqlx, `impl порта`)
7. Создай `infrastructure/actors/<name>_actor.rs` (enum сообщений без Clone, HashMap state, write-through)
8. Добавь DTO в `web/dto.rs`, handlers в `web/handlers.rs`, routes в `web/routes.rs`
9. Расширь `AppState` в `app_state.rs`
10. Зарегистрируй в `bootstrap.rs`
11. Добавь CLI-команды в `crates/cli/src/main.rs`

### Новый LLM use-case

1. Добавь async fn в `application/llm_use_cases.rs`
2. Сделай его **infallible** — ошибки → fallback, не паника
3. Для `NotConfigured` не логируй warn
4. LLM промпт просит JSON → `extract_json()` парсит ответ

### Новый notifier

1. Создай файл в `infrastructure/notifications/`
2. `impl Notifier for XxxNotifier`
3. Добавь в `CompositeNotifier` в `bootstrap.rs`

### Новый LLM-провайдер

Не нужно — `OpenAICompatClient` уже работает с любым OpenAI-совместимым API. Просто смени `LLM_BASE_URL` + `LLM_MODEL` в `.env`.

---

## Команды для проверки

```bash
cargo build                                   # сборка обоих крейтов
cargo test --lib                              # unit-тесты (domain + llm fallbacks)
cargo clippy --all-targets -- -D warnings     # линтер, 0 warnings
cargo fmt                                     # форматирование
cargo run -p cozby-brain                      # запуск бэкенда
cargo run --bin cozby                         # запуск CLI
docker compose up -d db qdrant                # только инфра
docker compose up -d --build                  # всё
```

---

## Частые ошибки — НЕ ДЕЛАЙ ЭТО

- ❌ `#[derive(Clone)]` на enum сообщений акторов — `RpcReplyPort` не Clone
- ❌ `#[ractor::async_trait]` на `impl Actor` — Ractor 0.15 имеет native async
- ❌ Path params `:id` — в Axum 0.8 только `{id}`
- ❌ async в domain/ — domain чистый, без async
- ❌ `unwrap()` в production коде — используй `?` или `match`
- ❌ Прямой `PgPool` в handlers — только через актор
- ❌ Бизнес-логика в handlers — handler только парсит запрос → зовёт актор → формирует ответ
- ❌ Обновление state актора ДО записи в БД — сначала БД, потом state
- ❌ `impl Message for XxxMsg` — Ractor имеет blanket impl, вручную не нужно
- ❌ Паника в LLM use-cases — все должны быть infallible с fallback
