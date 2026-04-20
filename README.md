# cozby-brain

Полноценный AI-агент: заметки, todo, напоминания, документация по проектам, обучающие треки, граф связей. Пишешь как думаешь — LLM сама определяет тип (note/doc/todo/reminder/question), применяет шаблоны, парсит время, ищет похожее, сохраняет историю.

Три бинарника:
- **`cozby-brain`** — сервер (HTTP API на `:8081`, акторы, LLM, vector search, notifier)
- **`cozby`** — CLI (clap + dialoguer), скриптование
- **`cozby-tui`** — современный TUI (ratatui), чат-интерфейс + браузер записей

Backend: Rust · Axum · Ractor · sqlx (Postgres) · Qdrant (gRPC) · MinIO (S3) · OpenAI-совместимый LLM.

---

## Требования

- **Docker** + **Docker Compose** — для postgres / qdrant / minio
- **Rust 1.85+** — [rustup.rs](https://rustup.rs)
- **LLM API-ключ** — routerai.ru / OpenRouter / Groq / Ollama или любой OpenAI-совместимый endpoint
- **fish** на macOS (опционально) — `release.sh` автоматически настроит PATH

---

## Запуск — один сценарий

```bash
git clone https://github.com/bulbasaur1k/cozby-brain.git
cd cozby-brain

cp .env.example .env
# Открой .env и вставь свой LLM_API_KEY

cargo build                 # компилируем workspace (первый раз ~2 мин)
./release.sh                # собираем release + ставим cozby / cozby-tui / cozby-brain в ~/.cargo/bin
./run.sh                    # поднимает db + qdrant + minio в docker, запускает сервер на :8081

# В новом терминале — главное приложение:
cozby-tui
```

Всё. Никаких других вариантов, всё через эти три скрипта и установленные бинарники.

### Что делает каждый скрипт

| Скрипт | Что делает |
|---|---|
| `cargo build` | компилирует все 12 крейтов workspace (debug-сборка, для проверки) |
| `./release.sh` | release-сборка → `cargo install --force` в `~/.cargo/bin` → настройка fish PATH |
| `./run.sh` | проверяет Docker, поднимает `db`+`qdrant`+`minio`, ждёт healthy, запускает `cozby-brain` с ротацией логов в `logs/` |

### Вспомогательные команды `run.sh`

```bash
./run.sh              # запустить всё
./run.sh stop         # остановить docker-сервисы
./run.sh status       # статус всех сервисов + health
./run.sh logs         # tail сегодняшнего лога
./run.sh clean-logs   # удалить старые логи
```

### Вспомогательные команды `release.sh`

```bash
./release.sh            # собрать + установить
./release.sh --no-path  # без правки fish config
./release.sh uninstall  # удалить бинарники
```

---

## Конфигурация (.env)

```env
DATABASE_URL=postgres://cozby:cozby@localhost:5432/cozby_brain
HTTP_ADDR=0.0.0.0:8081
RUST_LOG=info,cozby_brain=debug,tower_http=info

# LLM (OpenAI-совместимый endpoint)
LLM_BASE_URL=https://routerai.ru/api/v1
LLM_MODEL=qwen/qwen3-coder-480b-a35b-instruct
LLM_API_KEY=sk-...

# Embedding (опционально — для vector search + suggest append)
EMBEDDING_MODEL=text-embedding-3-small

# Qdrant (gRPC)
QDRANT_URL=http://localhost:6334
QDRANT_COLLECTION=cozby_notes

# MinIO / S3 (attachments для документации)
S3_ENDPOINT=http://localhost:9000
S3_REGION=us-east-1
S3_ACCESS_KEY=minioadmin
S3_SECRET_KEY=minioadmin
S3_BUCKET=cozby-attachments
```

### Альтернативные LLM

**Ollama (локально, бесплатно, без ключа):**
```env
LLM_BASE_URL=http://localhost:11434/v1
LLM_MODEL=llama3.2
LLM_API_KEY=
EMBEDDING_MODEL=nomic-embed-text
```

**OpenRouter (free модели):**
```env
LLM_BASE_URL=https://openrouter.ai/api/v1
LLM_MODEL=meta-llama/llama-3.2-3b-instruct:free
LLM_API_KEY=sk-or-...
```

**Groq (быстрый free tier):**
```env
LLM_BASE_URL=https://api.groq.com/openai/v1
LLM_MODEL=llama-3.1-8b-instant
LLM_API_KEY=gsk_...
```

**ВАЖНО**: не используй reasoning-модели (`*-thinking`, `glm-4.7-flash`) — они выдают chain-of-thought. Наш парсер их страхует, но instruct-модели надёжнее и быстрее. Выбор: `qwen3-coder-*-instruct`, `llama-3.3-70b-instruct`, `qwen2.5-*-instruct`.

---

## Главное приложение — `cozby-tui`

Современный ratatui-интерфейс. Запуск:

```bash
cozby-tui
```

### Раскладка

```
┌ cozby · http://localhost:8081 · ● connected ──────────────────┐
├─────────┬──────────────────────┬──────────────────────────────┤
│ cozby   │  Notes (12)          │ # Ractor notes               │
│ ─────── │                      │                              │
│ ▎ Inbox │ ▶ ● Ractor notes    │ tags: rust, actor            │
│   Notes │   ● Axum 0.8        │                              │
│   Todos │   ● Learning actor  │ Ractor 0.15 нативно async,   │
│   …     │                      │ убрали async_trait…          │
├─────────┴──────────────────────┴──────────────────────────────┤
│ NORMAL   loaded 12 · Tab focus · 1-6 tabs · Enter open · q quit│
└───────────────────────────────────────────────────────────────┘
```

### Клавиши

**Переключение вкладок:**
- `1`–`6` — прямо на вкладку (Inbox/Notes/Todos/Reminders/Learning/Docs)
- `]` / `[` — следующая / предыдущая

**Фокус panes:**
- `Tab` / `Shift+Tab` — цикл `sidebar → list → detail`
- `h` / `l` — влево / вправо между panes

**Действия (в списке):**
- `Enter` / `o` — открыть в оверлее (или раскрыть папку в Docs)
- `Space` — галочка для todo (toggle done)
- `d` / `x` — удалить (с подтверждением `y/n`)
- `j k` / `↓ ↑` — навигация
- `g` / `G` — первый / последний

**Режимы:**
- `i` — **Ingest** (писать в LLM) → `Enter` отправить, `Esc` отмена
- `/` — **Search** (live фильтр списка)
- `:` — **Command** (`:notes`, `:delete`, `:all`, `:recent`, `:q`…)
- `d` → **Confirm** (`y`/`n`)

**Глобально:** `r` refresh · `Esc`/`q` — выход или закрыть overlay · `Ctrl+C` exit.

### Команды `:cmd`

| | |
|---|---|
| `:inbox :notes :todos :reminders :learn :docs` | сменить вкладку |
| `:open` `:close` | открыть/закрыть detail |
| `:delete` `:d` `:rm` | удалить выбранное |
| `:all` | показать ВСЕ todo |
| `:recent` | только последние 5 дней (по умолчанию) |
| `:refresh` `:r` `:w` | обновить |
| `:ingest` `:i` | ingest-режим |
| `:q` `:quit` | выход |

### Фильтры по умолчанию

- **Todos** — только последние 5 дней (незавершённые с due ≤5д, завершённые за последние 5 дней). `:all` для всех.
- **Docs** — tree-view: проекты как папки, `Enter` раскрывает/сворачивает. Страницы загружаются лениво, показываются как отступ со страничкой внутри проекта.

---

## Скриптование — `cozby` CLI

Для автоматизации / скриптов / pipe'ов. Все команды работают из любой директории (бинарник в PATH).

### Universal ingest

```bash
# LLM классифицирует: note / doc / todo / reminder / question
cozby ingest --text "надо купить молоко завтра в 10"
cozby ingest --text "в проекте cozby-brain на страницу API добавь /api/health"
cozby ingest --text "через 30 минут позвонить маме"

# из файла / pipe
cozby ingest --file ~/meeting.md
cat raw_notes.md | cozby ingest
```

### Напрямую (без LLM)

```bash
# Notes
cozby note add --title "..." --content "..." --tags rust,axum
cozby note list | show <id> | search <q> | rm <id>

# Todos
cozby todo add "купить молоко" --due +2h     # +30m / +2h / +1d / RFC3339
cozby todo list | done <id> | rm <id>

# Reminders
cozby remind add "позвонить" --at +30m
cozby remind list | rm <id>

# Documentation
cozby doc projects                                       # все проекты
cozby doc pages <project>                                # страницы проекта
cozby doc show <page_id>                                 # markdown + meta
cozby doc history <page_id>                              # версии правок
cozby doc version <page_id> <v>                          # конкретная версия
cozby doc write <project> <page> --op append --content "..."  # ручная правка
cozby doc rm page|project <id>

# Learning
cozby learn add ~/rust.md --title "Rust" --pace 24
cozby learn list | lessons <track> | next <track> | learned <lesson> | skip <lesson> | rm <track>

# Smart search
cozby ask "что я писал про ractor"

# Graph
cozby graph <note_id> --depth 2
```

---

## Системные интеграции (curl)

Прямые CRUD endpoints **без LLM** — для скриптов, cron, bash-хуков:

```bash
curl -X POST http://localhost:8081/api/notes \
  -H 'content-type: application/json' \
  -d '{"title":"...","content":"...","tags":["..."]}'

curl -X POST http://localhost:8081/api/todos \
  -d '{"title":"Полить цветы","due_at":"2026-04-22T09:00:00Z"}'

curl -X POST http://localhost:8081/api/reminders \
  -d '{"text":"Митинг","remind_at":"2026-04-22T10:00:00Z"}'
```

Universal ingest (с LLM):
```bash
curl -X POST http://localhost:8081/api/ingest \
  -d '{"raw":"через 30 минут позвонить маме"}'
```

---

## HTTP API — сводка

| Метод | Путь | Описание |
|---|---|---|
| `GET` | `/health` | health check |
| **Notes** | | |
| `GET/POST` | `/api/notes` | list / create |
| `GET/PUT/DELETE` | `/api/notes/{id}` | CRUD + extract wiki-links |
| `GET` | `/api/notes/search?q=` | ILIKE поиск |
| **Todos** | | |
| `GET/POST` | `/api/todos` | list / create |
| `POST` | `/api/todos/{id}/complete` | toggle done |
| `DELETE` | `/api/todos/{id}` | удалить |
| **Reminders** | | |
| `GET/POST` | `/api/reminders` | list / create |
| `DELETE` | `/api/reminders/{id}` | удалить |
| **Documentation** | | |
| `GET/POST` | `/api/doc/projects` | list / create |
| `GET/DELETE` | `/api/doc/projects/{id}` | get / delete |
| `GET` | `/api/doc/projects/{id}/pages` | страницы проекта |
| `POST` | `/api/doc/pages` | create / append / section / replace |
| `GET/DELETE` | `/api/doc/pages/{id}` | get / delete |
| `GET` | `/api/doc/pages/{id}/history` | все версии |
| `GET` | `/api/doc/pages/{id}/history/{v}` | конкретная версия |
| **LLM Ingest** | | |
| `POST` | `/api/ingest` | универсальный `{raw}` → LLM классифицирует |
| `POST` | `/api/ingest/note/confirm` | подтвердить create/append заметки |
| **Search** | | |
| `GET` | `/api/ask?q=` | LLM-ассистированный поиск |
| `GET` | `/api/graph/{id}?depth=1-3` | граф связей |
| **Learning** | | |
| `GET/POST` | `/api/learning/tracks` | list / create (+split через LLM) |
| `GET/DELETE` | `/api/learning/tracks/{id}` | get / delete |
| `GET` | `/api/learning/tracks/{id}/lessons` | уроки |
| `POST` | `/api/learning/tracks/{id}/next` | выдать следующий |
| `POST` | `/api/learning/lessons/{id}/learned` | отметить |
| `POST` | `/api/learning/lessons/{id}/skip` | пропустить |

Формат ответов: `{"status":"ok","data":...}` или `{"error":"..."}`. Мульти-ingest: `{"items":[...]}`.

---

## Нотификации

При наступлении `remind_at` у напоминания сервер шлёт во все каналы:
- **Log** — в server-log с target=`notify`
- **Stdout** — в консоль `🔔 [Reminder] ...`
- **Desktop** — нативный popup macOS Notification Center (или Linux libnotify), звук `Glass`

Проверка каналов: `./run.sh logs` + `curl -X POST /api/reminders -d '{"text":"test","remind_at":"через 15 сек"}'`. macOS при первом вызове попросит разрешение — разреши раз и всё.

---

## Архитектура

12 крейтов, hexagonal, направление зависимостей `infrastructure → application → domain`:

```
crates/
├── domain/           cb-domain         # чистый Rust: entities, services
├── application/      cb-application    # порты (traits), LLM use-cases
├── persistence/      cb-persistence    # sqlx Pg*Repository
├── actors/           cb-actors         # Ractor: Note/Todo/Reminder/Learning/Doc
├── learning/         cb-learning       # LlmLessonSplitter (MCP-like)
├── llm/              cb-llm            # OpenAI-совместимый client + noop
├── vector/           cb-vector         # Qdrant gRPC + noop
├── storage/          cb-storage        # S3/MinIO attachment store + noop
├── notifications/    cb-notifications  # log/stdout/desktop/composite
├── web/              cb-web            # Axum handlers/routes/dto
├── server/           cozby-brain       # бинарник сервера + миграции + wiring
├── cli/              cozby-cli         # бинарник `cozby`
└── tui/              cb-tui            # бинарник `cozby-tui`
```

---

## Разработка

```bash
cargo test --workspace                        # unit + integration
cargo clippy --all-targets -- -D warnings     # строгий линтер
cargo fmt
RUST_LOG=debug cargo run -p cozby-brain       # сервер с debug-логами

docker compose exec db psql -U cozby -d cozby_brain     # psql
open http://localhost:6333/dashboard                    # qdrant UI
open http://localhost:9001                              # minio console (minioadmin/minioadmin)
```

---

## Troubleshooting

| Симптом | Причина / решение |
|---|---|
| Сервер зависает на старте | Docker не запущен → `docker info`. `./run.sh` проверяет сам |
| `LLM error: transport error sending request` | Проверь `LLM_API_KEY`, сеть, корректность модели. На корп-машине с MitM — уже починено: reqwest использует native CA |
| Ingest возвращает fallback (title = сырой текст) | Модель — reasoning (`*-thinking`). Наш парсер справляется, но предпочти instruct-модель |
| Embedding 400 "model not found" | `EMBEDDING_MODEL` не поддерживается провайдером. Отключи или поменяй. Без embedding работает всё кроме similarity search |
| Порт 8081 занят | `lsof -i :8081` → убей процесс |
| `./run.sh` — logs старше 7 дней висят | `./run.sh clean-logs` |
| desktop-notification не появляется | macOS требует разрешение от родительского терминала (Terminal/iTerm/ghostty). Настройки → Уведомления |
| Docker падает постоянно | перезапусти Docker Desktop; не убивай процессы по порту `kill -9 $(lsof -ti :8080)` — может задеть Docker proxy |
