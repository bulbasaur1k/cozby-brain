# cozby-brain

Личный AI-органайзер: заметки, todo, напоминания, документация, learning-треки.
Пишешь как думаешь — LLM сама классифицирует и сохраняет.

## Требования

- Docker + Docker Compose
- Rust 1.85+ ([rustup.rs](https://rustup.rs)) — нужен только для TUI
- Ключ к любому OpenAI-совместимому LLM (routerai / OpenRouter / Groq / Ollama)

## Запуск — два сценария

У обоих общее: сначала положить ключ в `.env`.

```bash
cp .env.example .env         # впиши LLM_API_KEY
```

### Сценарий 1 — native dev (быстрая итерация кода)

Сервер компилируется и работает на хосте, в Docker только инфра.

```bash
./release.sh                 # собирает release и ставит cozby / cozby-tui / cozby-brain в ~/.cargo/bin
./run.sh                     # docker: db + qdrant + minio; сервер на :8081 (на хосте)
cozby-tui                    # в новом терминале
```

### Сценарий 2 — всё в Docker, кроме TUI

Сервер собирается в образ, БД/Qdrant/MinIO — рядом. Rust нужен только чтобы поставить TUI.

```bash
cargo install --path crates/tui    # один раз — ставит cozby-tui в ~/.cargo/bin
./run-docker.sh                    # build + up всех контейнеров (сервер на :8081)
cozby-tui                          # в новом терминале
```

Альтернативный Dockerfile (например, slim/debug-вариант):

```bash
./run-docker.sh -f Dockerfile.dev              # путь к Dockerfile
./run-docker.sh -f my.Dockerfile -i my:tag     # +своё имя образа
# либо в .env:  COZBY_DOCKERFILE=Dockerfile.dev
```

## Что делает каждый скрипт

| Скрипт | Сценарий | Что делает |
|---|---|---|
| `./release.sh` | 1 | `cargo install --release` трёх бинарников в `~/.cargo/bin` + PATH для fish |
| `./run.sh` | 1 | поднимает docker-инфру, стартует `cozby-brain` на хосте, лог → `logs/` |
| `./run-docker.sh` | 2 | `docker compose --profile full up -d --build` — инфра + сервер в Docker |
| `cozby-tui` | 1 и 2 | TUI-клиент, подключается к `http://localhost:8081` |

Команды `run.sh`: `stop` · `status` · `logs` · `clean-logs`.
Команды `run-docker.sh`: `stop` · `status` · `logs` · `rebuild`.
Команды `release.sh`: `uninstall` · `--no-path`.

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
