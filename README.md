# cozby-brain

Личный AI-органайзер: заметки, todo, напоминания, документация, learning-треки.
Пишешь как думаешь — LLM сама классифицирует и сохраняет.

## Требования

- Docker + Docker Compose
- Rust 1.85+ ([rustup.rs](https://rustup.rs))
- Ключ к любому OpenAI-совместимому LLM (routerai / OpenRouter / Groq / Ollama)

## Запуск

```bash
cp .env.example .env         # впиши LLM_API_KEY и, если надо, поменяй модель
./release.sh                 # собирает release и ставит cozby / cozby-tui / cozby-brain в ~/.cargo/bin
./run.sh                     # docker (db + qdrant + minio) + сервер на :8081
cozby-tui                    # в новом терминале — основное UI
```

Это всё. Отдельного `cargo build` / `cargo run` не надо.

## Что делает каждый скрипт

| Скрипт | Что делает |
|---|---|
| `./release.sh` | `cargo install --release` трёх бинарников в `~/.cargo/bin` + PATH для fish |
| `./run.sh` | поднимает docker-сервисы, ждёт healthy, стартует `cozby-brain`, пишет лог в `logs/` |
| `cozby-tui` | TUI-клиент: inbox/notes/todos/reminders/learning/docs |

Полезное у `run.sh`: `stop` · `status` · `logs` · `clean-logs`.
Полезное у `release.sh`: `uninstall` · `--no-path`.

## .env (минимум)

```env
DATABASE_URL=postgres://cozby:cozby@localhost:5432/cozby_brain
HTTP_ADDR=0.0.0.0:8081

LLM_BASE_URL=https://routerai.ru/api/v1
LLM_MODEL=qwen/qwen3-coder-480b-a35b-instruct
LLM_API_KEY=sk-...

EMBEDDING_MODEL=openai/text-embedding-3-small

QDRANT_URL=http://localhost:6334
QDRANT_COLLECTION=cozby_notes

S3_ENDPOINT=http://localhost:9000
S3_BUCKET=cozby-attachments
S3_ACCESS_KEY=minioadmin
S3_SECRET_KEY=minioadmin
```

Избегай reasoning-моделей (`*-thinking`, `glm-4.7-flash`) — бери instruct.

## TUI — основные клавиши

- `1`–`6` — вкладки (Inbox / Notes / Todos / Reminders / Learning / Docs)
- `j` `k` — навигация, `Enter` / `o` — открыть, `Space` — toggle todo, `d` — удалить
- `i` — ingest (писать в LLM). В этом режиме поддерживается `@путь/к/файлу` — TUI прочитает файл и отправит его содержимое
- `/` — фильтр, `:` — команды (`:notes`, `:all`, `:q`…), `r` — refresh, `q` / `Esc` — выйти / закрыть

Больше подсказок — во вкладке `Inbox`.
