# cozby-brain — промт для AI-агента

Полный reference для AI-агента (включая слабые модели типа Qwen), работающего над cozby-brain. Это snapshot актуальной архитектуры и всех правил. Читай **до** начала любой работы — в `CLAUDE.md` только дев-правила, здесь — полная картина.

---

## Что это за проект

Полноценный **AI-агент**: заметки, todo, напоминания, документация по проектам, обучающие треки, граф связей. Все пользовательские входы идут **через LLM** (classify + structure), с **RAG по умолчанию** — при любом ingest LLM видит существующие похожие записи (через vector-search) и избегает дублей.

Стек: Rust, Axum 0.8, Ractor 0.15, sqlx 0.8, Postgres, Qdrant (gRPC), MinIO (S3), reqwest с native-roots TLS.

Три бинарника:
- `cozby-brain` — HTTP-сервер на `:8081`
- `cozby` — CLI (clap + dialoguer) для скриптования
- `cozby-tui` — главное UI-приложение (ratatui + pulldown-cmark)

---

## Структура — 12 крейтов workspace

```
crates/
├── domain/           cb-domain         # чистый Rust, 0 фреймворков
├── application/      cb-application    # порты (traits), LLM use-cases
├── persistence/      cb-persistence    # sqlx Pg*Repository
├── actors/           cb-actors         # Ractor: Note/Todo/Reminder/Learning/Doc
├── learning/         cb-learning       # LlmLessonSplitter (MCP-подобный)
├── llm/              cb-llm            # OpenAICompatClient + Noop
├── vector/           cb-vector         # QdrantVectorStore + Noop
├── storage/          cb-storage        # S3AttachmentStore (rust-s3) + Noop
├── notifications/    cb-notifications  # log + stdout + desktop (notify-rust)
├── web/              cb-web            # Axum handlers/routes/dto/app_state
├── server/           cozby-brain       # bin: main + bootstrap + migrations
├── cli/              cozby-cli         # bin: `cozby`
└── tui/              cb-tui            # bin: `cozby-tui`
```

Направление зависимостей **строгое**: `infrastructure → application → domain`. Domain не может зависеть от sqlx/ractor/axum/reqwest — это физическая граница крейтов. Application зависит только от domain + trait-порты без реализаций.

---

## Архитектурные правила (ОБЯЗАТЕЛЬНО)

### 1. Hexagonal

- **domain/** — `serde`, `chrono`, `uuid`, `thiserror`. Без `async`. Без `use ractor/axum/sqlx/reqwest`.
- **application/** — trait-порты и use-cases. Без конкретных БД/фреймворков.
- **infrastructure/** — всё внешнее: sqlx, axum, reqwest, Qdrant, S3.

### 2. Actor Model (Ractor)

- Актор = единственный владелец mutable state.
- HTTP-handlers общаются через `call!()` (RPC с ответом) или `cast!()` (fire-and-forget).
- **НЕЛЬЗЯ** `#[derive(Clone)]` на enum сообщений — `RpcReplyPort<T>` не Clone.
- Ractor 0.15 имеет **native async** — НЕ используй `#[ractor::async_trait]` на `impl Actor`.
- Blanket impl `Message for T: Send + Any + 'static` — руками `impl Message` НЕ нужен.

### 3. Write-Through Pattern

В каждом мутирующем actor-handler:
1. Валидация через domain-service
2. `repo.upsert(&entity).await?` ← сначала БД
3. При `Ok` — обновить HashMap state ← потом in-memory
4. При `Err` — НЕ трогать state

### 4. Axum 0.8

- Path params: `{id}`, НЕ `:id`.
- JSON responses: `{"status":"ok","data":...}` / `{"error":"описание"}`.
- Коды: 200 / 201 / 400 / 404 / 500 / 503.

### 5. LLM Use-Cases

- **Все infallible** — при ошибке fallback, не panic.
- NOT_CONFIGURED ошибку НЕ логируем как warn (это нормальное состояние).
- Промпты просят JSON-only. `extract_json()` переживает reasoning-модели (all balanced `{...}` → last with expected keys).

### 6. Error handling

- Никакого `unwrap()` в production (кроме wiring в `main`).
- `?` + `map_err` + `thiserror`.
- `anyhow::Result` в bin, доменные error enums в lib.

---

## Все порты (в `application/ports.rs`)

```rust
// LLM
trait LlmClient { name(); async complete_text(system, user) -> Result<String, LlmError> }

// Embedding
trait EmbeddingClient { name(); async embed(text) -> Result<Vec<f32>, LlmError> }

// Vector — kind важен!
const KIND_NOTE: &str = "note";
const KIND_DOC_PAGE: &str = "doc_page";
struct SimilarItem { id, kind, title, score: f32 }
trait VectorStore {
    async upsert(id, kind, vector, title, &tags) -> Result<(), RepoError>;
    async search(vector, limit) -> Result<Vec<SimilarItem>, RepoError>;   // cross-kind
    async search_by_kind(kind, vector, limit) -> Result<Vec<SimilarItem>, RepoError>;
    async delete(id) -> Result<(), RepoError>;
}

// Repos
trait NoteRepository { upsert, delete, get, list, search(query) }
trait TodoRepository { upsert, delete, list }
trait ReminderRepository { upsert, delete, list, set_fired(id, bool) }
trait LearningTrackRepository { upsert, delete, get, list }
trait LessonRepository { upsert, delete, get, list_by_track, next_pending }
trait ProjectRepository { upsert, delete, get_by_id, get_by_slug, find_by_title_like, list }
trait DocPageRepository { upsert, delete, get_by_id, get_by_slug, find_by_title_like, list_by_project }
trait DocPageHistoryRepository { insert, list_by_page, get_version }
trait AttachmentRepository { insert, delete, get, list_by_page }

// Attachment blobs
trait AttachmentStore { name(); async put(key, mime, bytes); async get(key); async delete(key); async presigned_url(key, ttl) }

// Lesson splitter (MCP-like)
struct LessonDraft { title, content }
trait LessonSplitter { async split(track_title, raw_text) -> Result<Vec<LessonDraft>, LlmError> }

// Notifier
struct Notification { title, body }
trait Notifier { name(); async notify(&Notification) -> Result<(), NotifyError> }
```

---

## Сообщения акторов

```rust
// NoteMsg
Create(title, content, tags, reply) / Update(id, title, content, tags, reply)
Delete(id, reply) / Get(id, reply) / List(reply) / Search(query, reply)

// TodoMsg
Create(title, due_at, reply) / Complete(id, reply) / Delete(id, reply) / List(reply)

// ReminderMsg
Create(text, remind_at, reply) / Delete(id, reply) / List(reply) / CheckDue

// LearningMsg
CreateTrack(title, source_ref, raw_text, pace_hours, tags, reply)
ListTracks(reply) / GetTrack(id, reply) / DeleteTrack(id, reply)
DeliverNext(track_id, reply) / MarkLearned(id, reply) / SkipLesson(id, reply)
ListLessons(track_id, reply) / CheckDue(reply)

// DocMsg
CreateProject(slug, title, description, tags, reply)
ListProjects(reply) / GetProject(id, reply) / ResolveProject(hint, reply) / DeleteProject(id, reply)
ListPages(project_id, reply) / GetPage(id, reply) / GetPageBySlug(pid, slug, reply) / DeletePage(id, reply)
ListPageHistory(page_id, reply) / GetPageVersion(page_id, version, reply)
IngestDoc(project_hint, page_hint, content, tags, op, section_title, author, reply)
```

Ни один enum НЕ имеет `#[derive(Clone)]`.

---

## HTTP API — полная таблица

Base: `http://localhost:8081`. Формат ответов: `{"status":"ok","data":...}` / `{"error":"..."}`.

### Notes
- `GET /api/notes` — list
- `POST /api/notes` `{title, content, tags}` — create (+ auto-index in Qdrant)
- `GET /api/notes/search?q=` — ILIKE
- `GET /api/notes/{id}` — get (+ wiki-links parsed)
- `PUT /api/notes/{id}` — update (+ re-index)
- `DELETE /api/notes/{id}` — delete (+ un-index)

### Todos
- `GET /api/todos` — list
- `POST /api/todos` `{title, due_at?}` — create
- `POST /api/todos/{id}/complete` — mark done
- `DELETE /api/todos/{id}` — delete

### Reminders
- `GET /api/reminders` — list
- `POST /api/reminders` `{text, remind_at}` — create
- `DELETE /api/reminders/{id}` — delete

### Documentation
- `GET /api/doc/projects` — list / `POST` — create `{slug?, title, description, tags}`
- `GET /api/doc/projects/{id}` / `DELETE`
- `GET /api/doc/projects/{id}/pages`
- `POST /api/doc/pages` — create/append/section/replace page `{project, page, content, tags, operation, section_title?}` (+ auto-index)
- `GET /api/doc/pages/{id}` / `DELETE`
- `GET /api/doc/pages/{id}/history` — все версии
- `GET /api/doc/pages/{id}/history/{version}` — конкретная

### Learning
- `GET /api/learning/tracks` / `POST` — create с авто-splitter
- `GET /api/learning/tracks/{id}` / `DELETE`
- `GET /api/learning/tracks/{id}/lessons`
- `POST /api/learning/tracks/{id}/next` — выдать следующий
- `POST /api/learning/lessons/{id}/learned` / `.../skip`

### LLM Universal
- `POST /api/ingest` `{raw}` — **RAG-aware**: classify + structure + auto-create (+ auto-index)
  - Перед classification делает embed → top-5 cross-kind → передаёт LLM как existing context
  - Возвращает `{type,data}` для одного item или `{items:[...]}` для нескольких
- `POST /api/ingest/note/confirm` `{action, target_id?, title, content, tags}` — 2-step note confirm (create/append)

### Smart Search (RAG)
- `GET /api/ask?q=` — **полный RAG Q&A**:
  - classify → keyword-поиск для todos/reminders
  - embed question → top-6 cross-kind → fetch full content → LLM answer с `[N]` цитатами
  - Response: `{question, answer, sources:[...], keywords, data:{...}}`

### Graph
- `GET /api/graph/{id}?depth=1-3` — семантические + wiki-link связи ноты

### Health
- `GET /health`

---

## RAG пайплайн (важно)

### При /api/ingest (RAG-aware classification)

```
raw → embed(raw) → vector.search(top 5) → existing_candidates
    ↓
    classify_and_structure_with_context(llm, raw, now, existing_candidates)
    # Классификатор в system-prompt получает:
    # "Existing items in user's knowledge base:
    #   - [note] [rust, async] "Ractor 0.15 updates" — Ractor 0.15 убрал...
    #   - [doc] [] "Architecture" — hexagonal архитектура
    #   Rules: если тема уже есть — use same title / operation=append"
    ↓
    классификатор возвращает items[]
    ↓
    create/append/... + index_async(kind, id, content, title, tags)
```

### При /api/ask (RAG QA)

```
question → embed → vector.search(top 6)  →  fetch full content (cap 2000 chars)
        ↓
        build context: "[1] (note) Title\nContent\n\n---\n\n[2] (doc_page) Title\n..."
        ↓
        LLM "Answer based ONLY on context. Cite [N]. If no info — 'нет в записях'."
        ↓
        {answer, sources:[{n, kind, id, title, score}], + old keyword data}
```

### Auto-indexing helpers (в handlers.rs)

- `index_async(state, id, kind, title, content, tags)` — fire-and-forget: embed + qdrant.upsert
- `unindex_async(state, id)` — fire-and-forget: qdrant.delete

Вызываются после:
- `create_note`, `update_note`, `delete_note`
- `create_page` (всех операций), `delete_page`
- `ingest` (для Classified::Note, Classified::Doc)
- `confirm_ingest_note` (и create и append флоу)

---

## Entities (domain)

```rust
Note { id, title, content, tags: Vec<String>, created_at, updated_at }
Todo { id, title, done, due_at: Option<DT>, created_at, completed_at: Option<DT> }
Reminder { id, text, remind_at, fired, created_at }

LearningTrack { id, title, source_ref, total_lessons, current_lesson,
                pace_hours, last_delivered_at: Option<DT>, tags, created_at }
Lesson { id, track_id, lesson_num, title, content, status (Pending|Delivered|Learned|Skipped),
         delivered_at, learned_at, created_at }

Project { id, slug, title, description, tags, created_at, updated_at }
DocPage { id, project_id, slug, title, content, version: i32, tags, created_at, updated_at }
DocPageVersion { id, page_id, version, title, content, tags, author (user/llm/api/system),
                 summary, created_at }
Attachment { id, page_id: Option<String>, note_id: Option<String>,
             filename, mime_type, size_bytes, storage_key, uploaded_at }
```

---

## Миграции

- `0001_init.sql` — notes
- `0002_todos_reminders.sql` — todos, reminders
- `0003_learning.sql` — learning_tracks, lessons
- `0004_documentation.sql` — projects, doc_pages, doc_page_versions, attachments

Запускаются автоматически при старте через `sqlx::migrate!()`.

---

## Env переменные

| Переменная | Default | Описание |
|---|---|---|
| `DATABASE_URL` | required | Postgres DSN |
| `HTTP_ADDR` | `0.0.0.0:8081` | bind |
| `RUST_LOG` | `info` | уровень логов |
| `LLM_BASE_URL` | — | OpenAI-compatible endpoint |
| `LLM_API_KEY` | — | Bearer token |
| `LLM_MODEL` | — | имя модели (предпочтительно `-instruct`!) |
| `EMBEDDING_MODEL` | — | **с префиксом провайдера!** `openai/text-embedding-3-small` |
| `QDRANT_URL` | — | gRPC `http://localhost:6334` |
| `QDRANT_COLLECTION` | `cozby_notes` | имя коллекции |
| `S3_ENDPOINT` / `S3_REGION` / `S3_ACCESS_KEY` / `S3_SECRET_KEY` / `S3_BUCKET` | — | MinIO creds |
| `COZBY_API` | `http://localhost:8081` | для CLI |

При отсутствии LLM → `NoopLlmClient` → fallback-эвристики.
При отсутствии Qdrant → `NoopVectorStore` → RAG отключается, suggestion всегда null.
При отсутствии S3 → `NoopAttachmentStore` → attachments не работают.

---

## LLM: как корректно вызывать

Текущие настройки (в `openai_compat.rs`):
- max_tokens: **4096**
- timeout: **120 sec**
- temperature: **0.2**
- reqwest TLS: **`rustls-tls-native-roots`** (читает OS keychain — для корп-MitM проксей типа Tinkoff DP auth)

Поддерживаются reasoning-модели: если `content` null → пытается достать из `reasoning` поля.

`extract_json`:
1. Strip ` ```json ` / ` ``` ` code fences
2. Strip `<think>...</think>`, `<|thinking|>...<|/thinking|>`, `"Thinking Process:"` с end-markers `Response:`/`Final Answer:`/`JSON:`
3. `find_all_balanced_blocks` — собирает все top-level `{...}` с учётом strings/escape
4. Возвращает **последний** блок который парсится + содержит expected key (`items|type|keywords|text|title|data|lessons`)
5. Fallback: последний валидный → первый

**Рекомендуемые модели** (non-reasoning, instruct):
- `qwen/qwen3-coder-480b-a35b-instruct` — top pick for JSON
- `qwen/qwen3.5-*` (если есть instruct вариант)
- `llama-3.3-70b-instruct`

**Избегать**: `*-thinking`, `glm-4.7-flash`, `deepseek-r1`.

---

## TUI управление (cozby-tui)

- **1-6** или **`[t`/`]t`** — вкладки
- **Tab** / **Shift+Tab** — фокус по panes (sidebar → list → detail)
- **h/l** — двигать фокус влево/вправо
- **j/k** — навигация
- **g/G** — первый/последний
- **Enter / o** — открыть (или раскрыть проект в Docs)
- **Space** — toggle done (для todo)
- **d / x** — удалить с y/n
- **i** — ingest (писать в LLM)
- **/** — search filter
- **:** — command mode (`:notes`, `:delete`, `:all`, `:recent`, `:q`)
- **r** — refresh
- **Esc / q** — выход или закрыть overlay

Todo-фильтр по умолчанию: **последние 5 дней**. `:all` / `:recent` переключает.
Docs — tree-view, проекты раскрываются. Страницы — lazily loaded.
Detail panel рендерит markdown через pulldown-cmark.

---

## Команды разработки

```bash
cargo test --workspace                               # все тесты
cargo clippy --all-targets -- -D warnings            # строгий линтер
cargo fmt
cargo check                                          # быстро проверить
cargo run -p cozby-brain                             # сервер из исходников

./run.sh [stop|status|logs|clean-logs]              # infra + server
./release.sh [install|--no-path|uninstall]          # bin в ~/.cargo/bin

docker compose exec db psql -U cozby -d cozby_brain # psql
open http://localhost:6333/dashboard                 # Qdrant UI
open http://localhost:9001                           # MinIO console
```

---

## Как добавить новую фичу — чеклист

### Новая domain entity

1. Struct в `domain/entities.rs` (без async, без infra)
2. DomainError варианты в `domain/errors.rs`
3. Чистые функции в `domain/services.rs` + `#[cfg(test)] mod tests`
4. Trait-port в `application/ports.rs`
5. Миграция в `crates/server/migrations/NNNN_name.sql`
6. `persistence/<name>_repo.rs` — `impl *Repository` на sqlx
7. `actors/<name>_actor.rs` — `enum Msg (без Clone!)`, `impl Actor` с `handle`
8. `web/dto.rs`, `web/handlers.rs`, `web/routes.rs`
9. `web/app_state.rs` — добавить `ActorRef<Msg>`
10. `server/bootstrap.rs` — wire repo + actor + state
11. `cli/src/main.rs` — subcommand (clap)
12. `tui/app.rs` + `tui/views.rs` — если нужна вкладка

### Новый LLM use-case

1. В `application/llm_use_cases.rs`
2. **Infallible**: `pub async fn foo(llm, ...) -> Bar` (не `Result`)
3. Внутри: `try_foo(...).await → Ok(r) | Err(NotConfigured) тихо | Err(other) warn + fallback_foo(...)`
4. LLM промпт с `"respond JSON only, no prose"`
5. Парсить через `extract_json()`

### Новый notifier канал

1. Файл в `crates/notifications/src/`
2. `impl Notifier for XxxNotifier` — `async notify(&Notification) -> Result<(), NotifyError>`
3. Добавить в `CompositeNotifier::new([...])` в `bootstrap.rs`

### Новый LLM-провайдер

**НЕ нужно** — `OpenAICompatClient` универсальный. Меняй `LLM_BASE_URL` + `LLM_MODEL` в `.env`.

### Индексация новой entity в Qdrant

1. Добавить константу `KIND_XXX` в `application/ports.rs`
2. Вызывать `index_async(state, id, KIND_XXX, title, content, tags)` после `Create/Update` в handler
3. `unindex_async(state, id)` в `Delete`
4. Обновить RAG-fetchers в `rag_answer` и `gather_context_items` чтобы умели fetchить XXX по kind

---

## Anti-patterns — НЕ ДЕЛАТЬ

- ❌ `#[derive(Clone)]` на enum сообщений — `RpcReplyPort` не Clone
- ❌ `#[ractor::async_trait]` на `impl Actor` — Ractor 0.15 native async
- ❌ `:id` вместо `{id}` в Axum 0.8
- ❌ async в `domain/`
- ❌ `unwrap()` в prod
- ❌ Прямой `PgPool` в handler — только через актор/репо
- ❌ Бизнес-логика в handler — только парсинг, вызов актора, формирование ответа
- ❌ `Mutex<State>` — только актор
- ❌ State update ДО DB write — сначала БД
- ❌ `impl Message for XxxMsg` руками — есть blanket impl
- ❌ panic в LLM use-case — все infallible
- ❌ Новые бинарники — три уже есть
- ❌ `git commit` с Co-Authored-By / "Generated with Claude Code"
- ❌ `git worktree` / форки / новые ветки — работаем в `main`
- ❌ `lsof -ti :PORT | xargs kill -9` без проверки — может задеть Docker proxy

---

## Известные ограничения (tech debt)

### Критично
- **Нет chunking** для больших текстов (Lesson splitter обрезает 500KB учебник, /api/ingest то же самое на 100KB)
- **Нет input size validation** — 100MB JSON пройдёт

### Средне
- **Hybrid search (RRF)** не реализован — keyword и vector работают отдельно
- **Unbounded content growth** в doc_pages (+ history snapshots), notes

### Nice-to-have
- Streaming responses (SSE)
- Metrics / OpenTelemetry
- Nested docs (страницы внутри страниц)

По продукту решено НЕ делать (пользователь явно подтвердил):
- ❌ Export/import
- ❌ Auth / rate limiting
