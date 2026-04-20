# cozby-brain — project instructions + dev context

> Этот файл загружается автоматически в начале каждой сессии в этом проекте.
> Читай всё перед тем как браться за работу.

---

## 1. Git workflow (переопределяет глобальные правила)

В этом проекте **можно коммитить напрямую** без отдельной просьбы пользователя.

Правила:
- Работай только в `main`. Никаких форков/worktree-изоляции/новых веток.
- После законченной логической единицы работы: `git add -A && git commit -m "..."` + `git push`.
- Один финальный коммит, не цепочка мелких.
- Conventional commits: `feat / fix / chore / docs / refactor / test`.
- **НЕ добавлять** `Co-Authored-By:` и «🤖 Generated with Claude Code».
- Перед коммитом обязателен smoke-test: `cargo clippy --all-targets -- -D warnings` + поднять сервер + один реальный API-запрос.

НЕ коммитить когда:
- Компиляция падает / тесты красные
- Пользователь явно попросил «покажи сначала»
- Эксперимент без цели сохранить

---

## 2. Архитектура (12 крейтов, hexagonal)

```
crates/
├── domain/           cb-domain         # чистый Rust, 0 фреймворков
│   ├── entities      # Note, Todo, Reminder, LearningTrack, Lesson, Project, DocPage, DocPageVersion, Attachment
│   ├── errors        # DomainError
│   └── services      # validate_title, create_*, extract_wiki_links
├── application/      cb-application    # порты (traits), LLM use-cases
│   ├── ports         # LlmClient, EmbeddingClient, VectorStore (kind + SimilarItem),
│   │                 # NoteRepository, TodoRepository, ReminderRepository,
│   │                 # ProjectRepository, DocPageRepository, DocPageHistoryRepository,
│   │                 # AttachmentRepository, AttachmentStore, LessonSplitter, Notifier
│   └── llm_use_cases # classify_and_structure (+with_context), find_best_match,
│                     # structure_note/parse_todo/parse_reminder (legacy),
│                     # extract_search_keywords, StructuredQuestion с status+time_window
├── persistence/      cb-persistence    # sqlx Pg*Repository
│   ├── note_repo / todo_repo / reminder_repo
│   ├── learning_repo / doc_repo      # тут 4 репозитория
├── actors/           cb-actors         # Ractor: единственные owners mutable state
│   ├── note_actor   NoteMsg {Create, Update, Delete, Get, List, Search}
│   ├── todo_actor   TodoMsg {Create, Complete, Delete, List}
│   ├── reminder_actor ReminderMsg {Create, Delete, List, CheckDue}
│   ├── learning_actor LearningMsg {CreateTrack, DeliverNext, ...}
│   └── doc_actor    DocMsg {CreateProject, IngestDoc (create/append/section/replace),
│                            GetPage, ListPages, ListPageHistory, DeleteProject/Page, ...}
├── learning/         cb-learning       # LlmLessonSplitter (MCP-подобный)
├── llm/              cb-llm            # OpenAICompatClient + NoopLlmClient
│                                       # обрабатывает reasoning-модели (content=null → reasoning)
├── vector/           cb-vector         # QdrantVectorStore + NoopVectorStore
│                                       # kind в payload, search/search_by_kind
├── storage/          cb-storage        # S3AttachmentStore (rust-s3) + Noop
├── notifications/    cb-notifications  # LogNotifier + StdoutNotifier + DesktopNotifier
│                                       # DesktopNotifier = native OS popup через notify-rust
├── web/              cb-web            # Axum 0.8 handlers, routes, dto, AppState
├── server/           cozby-brain       # main.rs + bootstrap.rs + config.rs + migrations/
├── cli/              cozby-cli         # бинарник `cozby` (clap + dialoguer)
└── tui/              cb-tui            # бинарник `cozby-tui` (ratatui + pulldown-cmark)
```

Направление зависимостей строго: `infrastructure → application → domain`. domain не может зависеть от sqlx/ractor/axum — это физическая граница крейтов.

---

## 3. Три бинарника + сценарий запуска

```bash
cargo build                 # проверочная сборка
./release.sh                # release + install в ~/.cargo/bin + fish PATH
./run.sh                    # docker db+qdrant+minio + сервер на :8081
cozby-tui                   # главное UI-приложение (в новом терминале)
```

`./run.sh` команды: `stop`, `status`, `logs`, `clean-logs`. Лог-ротация: `logs/cozby-brain-YYYYMMDD.log`, retention 7 дней, чистка при старте.

`./release.sh` команды: `install` (default), `--no-path`, `uninstall`.

---

## 4. Инфра (.env)

```env
DATABASE_URL=postgres://cozby:cozby@localhost:5432/cozby_brain
HTTP_ADDR=0.0.0.0:8081

# LLM
LLM_BASE_URL=https://routerai.ru/api/v1
LLM_MODEL=qwen/qwen3-coder-480b-a35b-instruct   # инструкт-модели, НЕ reasoning
LLM_API_KEY=sk-...

# Embedding — ВАЖНО: для routerai нужен префикс провайдера!
EMBEDDING_MODEL=openai/text-embedding-3-small

# Qdrant (gRPC)
QDRANT_URL=http://localhost:6334
QDRANT_COLLECTION=cozby_notes

# MinIO
S3_ENDPOINT=http://localhost:9000
S3_BUCKET=cozby-attachments
S3_ACCESS_KEY=minioadmin
S3_SECRET_KEY=minioadmin
```

---

## 5. Gotchas и известные подводные камни

### LLM

- **Reasoning-модели запрещены** — `glm-4.7-flash`, `*-thinking`. Они шлют chain-of-thought и `content=null`. `extract_json` в `llm_use_cases.rs` это переживает (ищет все balanced `{...}` блоки, выбирает последний с ожидаемым ключом), но это не идеальный fallback — лучше не использовать такие модели.
- **Предпочтительны**: `qwen3-coder-*-instruct`, `llama-3.3-*-instruct`, `qwen2.5-*-instruct`.
- `max_tokens=4096`, timeout `120s`, temperature `0.2` в `openai_compat.rs`.
- Запросы идут через **reqwest с `rustls-tls-native-roots`** — читает системный trust store (macOS Keychain / /etc/ssl/certs / Windows CA). Критично для корп-среды с MitM-прокси (Tinkoff DP auth, tinkoff-bundle).

### Embeddings для routerai

- Модели требуют префикс: `openai/text-embedding-3-small` (не голое имя!). Доступны: `openai/*`, `qwen/*`, `google/*`, `mistralai/*`, `perplexity/*`. Список: `GET /v1/models` → filter `embed`.
- `text-embedding-3-small` → 1536-мерный вектор. При смене модели — **старая Qdrant-коллекция станет несовместимой** (нужно удалить и переиндексировать).

### Axum 0.8

- Path-параметры: `{id}`, НЕ `:id`.
- Формат ответов: `{"status":"ok","data":...}` / `{"error":"..."}`.

### Ractor 0.15

- `RpcReplyPort<T>` НЕ `Clone` → enum сообщений НЕ может `#[derive(Clone)]`.
- Native async, **не используй** `#[ractor::async_trait]` на `impl Actor`.
- Tuple-variants для совместимости с `call!()` макросом.

### Write-through паттерн

Во всех актор-handlers для мутаций:
1. Валидация через domain service
2. `repo.upsert(&entity).await?`  ← сначала БД
3. При успехе — `state.insert(id, entity)` ← потом HashMap-кэш

Обратный порядок — баг (расхождение кэша и БД при ошибке).

### RAG — включён по умолчанию

- `POST /api/ingest` сначала делает `gather_context_items` (embed raw → top-5 cross-kind) → передаёт классификатору как «existing items» → LLM избегает дублей (предлагает тот же title / operation=append).
- `GET /api/ask` теперь **real RAG Q&A**: embed question → top-6 → fetch full content (cap 2000 chars each) → LLM генерирует ответ с `[N]` цитатами.
- Auto-index на всех mutation-paths (`POST/PUT/DELETE /api/notes`, `/api/doc/pages`, `/api/ingest`) через `index_async` (fire-and-forget `tokio::spawn`).

### Docker

- `docker-compose.yml` поднимает только `db` + `qdrant` + `minio`. Приложение на хосте.
- Не делай `lsof -ti :PORT | xargs kill -9` без проверки — может задеть Docker proxy процессы и уронить Docker Desktop.

---

## 6. Быстрые команды

```bash
# Проверки
cargo test --workspace
cargo clippy --all-targets -- -D warnings
cargo check                                       # быстро, без тестов

# Работа с сервисами
./run.sh                                          # всё поднять
./run.sh logs                                     # хвост сегодняшнего лога
./run.sh status                                   # статус
docker compose exec db psql -U cozby -d cozby_brain
open http://localhost:6333/dashboard              # Qdrant UI
open http://localhost:9001                        # MinIO console (minioadmin:minioadmin)

# Smoke-test API
curl -sf http://localhost:8081/health
curl -X POST http://localhost:8081/api/ingest -H 'content-type: application/json' \
  -d '{"raw":"тест"}'
curl "http://localhost:8081/api/ask?q=test"        # RAG QA
```

---

## 7. Ключевые решения продукта (принято пользователем)

- ✅ **RAG по умолчанию** — реализовано
- ✅ **Нативные нотификации** — desktop popup + sound через `notify-rust`
- ✅ **Документация как отдельный тип** — проекты/страницы/версии, с tree-view в TUI
- ✅ **История изменений** для doc_pages — snapshot на каждую правку
- ✅ **Learning-треки через LLM splitter** — автодоставка уроков по `pace_hours`
- ✅ **Auto-commit в этом проекте** (отличается от глобального правила)
- ❌ **Export/Import** — не делаем
- ❌ **Auth / rate limit** — не делаем (только localhost)

---

## 8. Нерешённые проблемы (tech debt)

### Критично

- **Chunking больших текстов отсутствует** — `LlmLessonSplitter` и `/api/ingest` шлют весь raw_text в один LLM-вызов. Учебник на 500KB получит усечённый ответ. Нужно: разбивать на chunks ~10K chars с overlap, обрабатывать последовательно, объединять.
- **Input size validation нет** — 100MB JSON пройдёт. Нужен лимит и ранний отказ.

### Средне

- **Hybrid search (RRF)** — сейчас `/api/ask` делает keyword search (ILIKE для todos/reminders) отдельно от vector search (notes/docs). Объединить через Reciprocal Rank Fusion.
- **Unbounded content growth** — doc_pages растут без лимита, история snapshot'ов тоже. На страницах в 10MB+ RAG начнёт тормозить.
- **TUI: скролл в detail-панели** — есть только в overlay, в обычном preview wrap без scroll.

### Nice to have

- **Streaming responses** — LLM ответ буферизуется целиком. Можно стримить через SSE для TUI «typing effect».
- **Metrics / observability** — только локальные `tracing` логи.
- **Doc page tree** — только project→pages, без вложенности страниц внутри проекта.

---

## 9. Карта файлов — где что править

| Задача | Файл(ы) |
|---|---|
| Новая entity | `domain/entities.rs` + `domain/services.rs` + migration + port в `application/ports.rs` |
| Новый LLM use-case | `application/llm_use_cases.rs` (всегда с fallback!) |
| Новый endpoint | `web/handlers.rs` + `web/routes.rs` + `web/dto.rs` |
| Новый actor message | `actors/<name>_actor.rs` (enum + handle match) |
| Новая репа | `persistence/<name>_repo.rs` |
| Новый notifier канал | `notifications/<name>.rs` + добавить в `CompositeNotifier` в `bootstrap.rs` |
| Новый LLM-провайдер | НЕ нужно — `OpenAICompatClient` универсальный, меняй `.env` |
| TUI — новая вкладка | `tui/app.rs` Tab enum + `tui/views.rs` render |
| CLI — новая команда | `cli/src/main.rs` (clap Subcommand + handler) |

---

## 10. Правила для кода

- **Все LLM use-cases infallible** — при ошибке возвращают fallback, не паникуют. Пример: `structure_note` пустил ошибку → `fallback_note(raw)` → продолжаем.
- **Fire-and-forget** для side-effects (Qdrant index, notify): `tokio::spawn(async move { ... })` — не блокируем handler.
- **Никогда** не использовать `unwrap()` в production-коде (кроме wiring в `main`).
- **Никогда** `Mutex<State>` — только актор.
- **Никогда** прямой `PgPool` в handler'е — только через репо/актор.
- Не придумывать новые бинарники — три уже есть (`cozby-brain`, `cozby`, `cozby-tui`).
