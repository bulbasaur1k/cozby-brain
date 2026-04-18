# cozby-brain

Полноценный AI-агент: заметки, todo, напоминания, обучающие треки по файлам, граф связей. Пишешь как думаешь — LLM сама определяет тип (заметка / задача / напоминание / поиск), применяет строгие шаблоны, парсит время, ищет похожее через vector search.

Backend на Rust (Axum + Ractor + sqlx + Postgres + Qdrant). Три бинарника: **сервер**, **CLI**, **TUI на ratatui**.

---

## Требования

- **Docker** + **Docker Compose** (для postgres и qdrant)
- **Rust 1.85+** (для сборки из исходников) — [rustup.rs](https://rustup.rs)
- **LLM API-ключ** (routerai.ru / OpenRouter / Groq / Ollama — любой OpenAI-совместимый endpoint)

---

## Быстрый старт

```bash
# 1. Клонируй и настрой env
git clone <repo-url> cozby-brain
cd cozby-brain
cp .env.example .env
# Открой .env и вставь свой LLM_API_KEY

# 2. Собери релиз (сервер + CLI + TUI)
cargo build --release

# 3. Один скрипт поднимает всё
./run.sh
```

`run.sh` сам:
1. Проверит что Docker работает
2. Поднимет `db` (postgres) и `qdrant` в Docker
3. Дождётся их healthy-статуса
4. Проверит что порт `:8081` свободен
5. Соберёт release-бинарник
6. Запустит сервер на `:8081` с логами в `logs/cozby-brain-YYYYMMDD.log`

**Проверка**:
```bash
curl http://localhost:8081/health
# → {"status":"ok"}
```

---

## Управление через `./run.sh`

```bash
./run.sh              # поднять всё (db + qdrant + app на :8081)
./run.sh stop         # остановить docker (app убить через Ctrl+C)
./run.sh status       # статус сервисов + health
./run.sh logs         # tail сегодняшнего лога
./run.sh clean-logs   # удалить старые логи
./run.sh --help       # справка
```

Логи автоматически ротируются по дням, старше 7 дней удаляются при старте.

---

## Конфигурация (.env)

```env
# ── Postgres (docker-compose поднимает на :5432) ──
DATABASE_URL=postgres://cozby:cozby@localhost:5432/cozby_brain

# ── HTTP сервер ──
HTTP_ADDR=0.0.0.0:8081
RUST_LOG=info,cozby_brain=debug,tower_http=info

# ── LLM (OpenAI-совместимый endpoint) ──
LLM_BASE_URL=https://routerai.ru/api/v1
LLM_MODEL=z-ai/glm-4.7-flash
LLM_API_KEY=sk-ВАШ_КЛЮЧ

# ── Embedding (опционально, для vector search) ──
EMBEDDING_MODEL=text-embedding-3-small

# ── Qdrant (docker-compose поднимает на :6334 gRPC) ──
QDRANT_URL=http://localhost:6334
QDRANT_COLLECTION=cozby_notes
```

### Альтернативные LLM-провайдеры

**Ollama (локально, бесплатно):**
```env
LLM_BASE_URL=http://localhost:11434/v1
LLM_MODEL=llama3.2
LLM_API_KEY=
EMBEDDING_MODEL=nomic-embed-text
```

**OpenRouter (есть free-модели):**
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

---

## Три бинарника

После `cargo build --release` в `target/release/` лежат:

```
target/release/cozby-brain    ← сервер (HTTP API на :8081)
target/release/cozby          ← CLI (clap + dialoguer)
target/release/cozby-tui      ← терминальный UI (ratatui)
```

---

## Использование CLI (`cozby`)

### Universal ingest — пиши как думаешь, LLM сама решит что это

```bash
# LLM классифицирует: note / todo / reminder / question
./target/release/cozby ingest --text "надо купить молоко завтра в 10 утра"
# → [TODO] Купить молоко, due_at: 2026-04-18T10:00:00Z

./target/release/cozby ingest --text "через 30 минут позвонить маме"
# → [REMINDER] Позвонить маме, remind_at: +30min

./target/release/cozby ingest --text "ractor 0.15 нативно async, убрали async_trait"
# → [NOTE] TECH-шаблон с заголовками и тегами [ractor, async, rust]

./target/release/cozby ingest --text "что я писал про rust"
# → [QUESTION] ключевые слова: [rust], найдено: notes=4

# Из файла
./target/release/cozby ingest --file ~/drafts/meeting.md

# Из stdin (pipe)
cat draft.md | ./target/release/cozby ingest
```

### Обучение — файл в ежедневные уроки

```bash
# LLM разбивает файл на логические уроки
./target/release/cozby learn add ~/learning/rust.md \
  --title "Rust Basics" \
  --pace 24 \
  --tags rust,programming

# Список треков
./target/release/cozby learn list

# Уроки трека
./target/release/cozby learn lessons <track_id>

# Ручная выдача следующего урока (иначе — автоматически раз в pace часов)
./target/release/cozby learn next <track_id>

# Отметить изученным / пропустить
./target/release/cozby learn learned <lesson_id>
./target/release/cozby learn skip <lesson_id>

# Удалить трек
./target/release/cozby learn rm <track_id>
```

Scheduler автоматически каждые 30 минут проверяет все треки. Когда подошло время (pace_hours с `last_delivered_at`) — создаёт Reminder "Новый урок: {title}" и Note с содержимым.

### Граф связей

```bash
# ASCII-дерево semantic + wiki-link связей
./target/release/cozby graph <note_id>
./target/release/cozby graph <note_id> --depth 2
```

Индикаторы: `●` semantic (цвет по score), `○` wiki-link. Зелёный=strong ≥0.8, жёлтый=medium 0.6-0.8, серый=weak <0.6, циан=wiki.

### Ручные команды (без LLM, для скриптов)

```bash
# Notes
cozby note add --title "..." --content "..." --tags a,b
cozby note list | show <id> | search <q> | rm <id>

# Todos
cozby todo add "купить молоко" --due +2h
cozby todo list | done <id> | rm <id>

# Reminders
cozby remind add "позвонить" --at +30m
cozby remind list | rm <id>

# Smart search
cozby ask "что я писал про rust"
```

---

## TUI (`cozby-tui`)

Терминальный UI на ratatui с 5 вкладками.

```bash
./target/release/cozby-tui
```

**Навигация:**
- `Tab` / `Shift+Tab` — переключение вкладок
- `i` — ввод в Inbox (chat-like поле)
- `Enter` — отправить (LLM классифицирует)
- `Esc` — отменить ввод
- `r` — обновить данные
- `↑` `↓` — навигация по списку
- `q` — выход

**Вкладки:**
- **Inbox** — chat-like поле, пишешь → LLM классифицирует → auto-save
- **Notes / Todos / Reminders / Learning** — просмотр записей

Индикаторы без эмоджи (`●` `○` `■` `□` `✓` `✗` `▶`), цвет по статусу.

---

## Интеграции и автоматизация

Прямые CRUD endpoints **без LLM** — для скриптов, cron, bash-хуков:

```bash
# добавить заметку
curl -X POST http://localhost:8081/api/notes \
  -H 'content-type: application/json' \
  -d '{"title":"...","content":"...","tags":["..."]}'

# добавить todo
curl -X POST http://localhost:8081/api/todos \
  -d '{"title":"Полить цветы","due_at":"2026-04-18T09:00:00Z"}'

# добавить reminder
curl -X POST http://localhost:8081/api/reminders \
  -d '{"text":"Митинг","remind_at":"2026-04-18T10:00:00Z"}'
```

Universal ingest для пользовательского ввода:

```bash
# LLM сама разберёт
curl -X POST http://localhost:8081/api/ingest \
  -d '{"raw":"через 30 минут позвонить маме"}'
```

---

## HTTP API — полный список

| Метод | Путь | Описание |
|---|---|---|
| `GET` | `/health` | health check |
| **Notes** | | |
| `GET/POST` | `/api/notes` | list / create |
| `GET/PUT/DELETE` | `/api/notes/{id}` | CRUD + extract wiki-links |
| `GET` | `/api/notes/search?q=` | ILIKE поиск |
| **Todos** | | |
| `GET/POST` | `/api/todos` | list / create |
| `POST` | `/api/todos/{id}/complete` | отметить done |
| `DELETE` | `/api/todos/{id}` | удалить |
| **Reminders** | | |
| `GET/POST` | `/api/reminders` | list / create |
| `DELETE` | `/api/reminders/{id}` | удалить |
| **LLM Ingest (универсальный)** | | |
| `POST` | `/api/ingest` | `{raw}` → LLM классифицирует и возвращает structured |
| `POST` | `/api/ingest/note/confirm` | подтвердить create или append для note |
| **Smart search** | | |
| `GET` | `/api/ask?q=` | LLM-ассистированный поиск |
| **Graph** | | |
| `GET` | `/api/graph/{id}?depth=1-3` | граф связей (semantic + wiki) |
| **Learning** | | |
| `GET/POST` | `/api/learning/tracks` | list / create |
| `GET/DELETE` | `/api/learning/tracks/{id}` | get / delete |
| `GET` | `/api/learning/tracks/{id}/lessons` | уроки трека |
| `POST` | `/api/learning/tracks/{id}/next` | ручная выдача следующего |
| `POST` | `/api/learning/lessons/{id}/learned` | отметить изученным |
| `POST` | `/api/learning/lessons/{id}/skip` | пропустить |

Формат ответов: `{"status":"ok","data":...}` либо `{"error":"..."}`.

---

## Архитектура — 11 независимых крейтов

```
crates/
├── domain/           cb-domain         # чистый Rust: entities, services
├── application/      cb-application    # порты (traits), LLM use-cases, classify_and_structure
├── persistence/      cb-persistence    # sqlx Pg*Repository
├── actors/           cb-actors         # Ractor: Note/Todo/Reminder/Learning actors
├── learning/         cb-learning       # LlmLessonSplitter (MCP-подобный)
├── llm/              cb-llm            # OpenAI-совместимый клиент + noop
├── vector/           cb-vector         # Qdrant gRPC + noop
├── notifications/    cb-notifications  # log/stdout/composite notifiers
├── web/              cb-web            # Axum handlers/routes/dto
├── server/           cozby-brain       # бинарник сервера + миграции + wiring
├── cli/              cozby-cli         # бинарник cozby (CLI)
└── tui/              cb-tui            # бинарник cozby-tui (ratatui)
```

**Граф зависимостей (compile-time enforced):**
```
domain ← application ← { persistence, actors, llm, vector, notifications, learning }
         application + actors ← web
         ALL ← server
         HTTP-only → cli, tui
```

Domain не может зависеть от sqlx/ractor/axum — это физическая граница крейтов, а не просто конвенция.

---

## Разработка

```bash
# Сборка всего workspace
cargo build --release

# Unit-тесты
cargo test --workspace                         # 10/10

# Линтер строгий
cargo clippy --all-targets -- -D warnings

# Форматирование
cargo fmt

# Логи с debug уровнем
RUST_LOG=debug,cozby_brain=trace cargo run -p cozby-brain

# Подключиться к postgres вручную
docker compose exec db psql -U cozby -d cozby_brain

# Подключиться к qdrant dashboard
open http://localhost:6333/dashboard
```

---

## Troubleshooting

**Сервер зависает на старте:**
- Postgres не запущен: `docker compose up -d db`
- Проверь `DATABASE_URL` в `.env`

**LLM возвращает fallback (title = сырой текст):**
- Проверь `LLM_API_KEY` в `.env`
- Проверь что модель доступна у провайдера: `curl https://routerai.ru/api/v1/models -H "Authorization: Bearer sk-..."`
- Reasoning-модели (glm-4.7-flash) могут занимать 20-60 секунд — это нормально

**Embedding fails (400 "model not found"):**
- У routerai.ru нет `text-embedding-3-small` — поменяй `EMBEDDING_MODEL` на поддерживаемую или оставь пустым
- При пустом `EMBEDDING_MODEL` — vector search и suggestions не работают, остальное ок

**Vector search ничего не находит:**
- Qdrant не запущен: `docker compose up -d qdrant`
- `EMBEDDING_MODEL` пуст в `.env`
- Проверь Qdrant: `curl http://localhost:6333/healthz`

**CLI не подключается к серверу:**
- Сервер не запущен на `:8081`
- Используй флаг `--api http://HOST:PORT` или `COZBY_API` env var

**Порт 8081 занят:**
- `lsof -i :8081` — посмотри что занимает
- `lsof -ti :8081 | xargs kill` — убить (осторожно, не задеть Docker proxy)

**Learning splitter таймаутит:**
- Reasoning-модель + большой файл → >120с
- Разбивай файл руками на части или используй не-reasoning модель (например `llama-3.1-8b-instant` на Groq)

---

## Docker

Для локальной разработки `docker-compose.yml` поднимает только `db` + `qdrant`. Приложение запускается на хосте через `./run.sh`.

```bash
# только инфра
docker compose up -d db qdrant

# статус
docker compose ps

# логи БД / Qdrant
docker compose logs -f db
docker compose logs -f qdrant

# полный сброс (удаляет volumes)
docker compose down -v
```

В прод-окружении (K8s/корп-инфра) БД и Qdrant приходят как внешние managed-сервисы — приложение подключается через env-переменные.

---

## Roadmap

- Explicit append/create UI в TUI при наличии suggestion
- Telegram-нотифаер
- Desktop notifications (`notify-rust`)
- Экспорт в plain markdown файлы (Obsidian vault compatibility)
- Reverse-connections (backlinks) в graph
- Spaced repetition для обучающих треков
