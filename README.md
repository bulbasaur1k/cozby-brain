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

### Сценарий 1 — local dev (сервер на хосте)

Инфра в docker, приложение компилируется и работает нативно. Быстрая итерация кода.

```bash
./release.sh                 # собирает release и ставит cozby / cozby-tui / cozby-brain в ~/.cargo/bin
./run.sh                     # docker-compose.local.yml: db + qdrant + minio; сервер на $HTTP_PORT (по дефолту 8081)
cozby-tui                    # в новом терминале
```

### Сценарий 2 — self-hosted (всё в docker, кроме TUI)

Сервер собирается в образ, инфра рядом, один `docker compose up` поднимает всё.

```bash
cargo install --path crates/tui    # один раз — ставит cozby-tui в ~/.cargo/bin
./run-docker.sh                    # == docker compose up -d --build (через обёртку с health-check)
# или напрямую:
docker compose up -d --build

cozby-tui                          # в новом терминале
```

Альтернативный Dockerfile / тег образа:

```bash
./run-docker.sh -f Dockerfile.dev              # путь к Dockerfile
./run-docker.sh -f my.Dockerfile -i my:tag     # +своё имя образа
# либо в .env:  COZBY_DOCKERFILE=Dockerfile.dev  COZBY_IMAGE=cozby-brain:dev
```

### Как файлы сосуществуют

- `docker-compose.local.yml` — только инфра (db + qdrant + minio). Используется в сценарии 1.
- `docker-compose.yml` — инфра через `include:` + сервис `cozby-brain`. Используется в сценарии 2.
- Контейнеры инфры одни и те же в обоих сценариях (при `COMPOSE_PROJECT_NAME=cozby-brain` из `.env`) — переключаться между сценариями можно без пересоздания БД.

## Что делает каждый скрипт

| Скрипт | Сценарий | Что делает |
|---|---|---|
| `./release.sh` | 1 | `cargo install --release` трёх бинарников в `~/.cargo/bin` + PATH для fish |
| `./run.sh` | 1 | `docker compose -f docker-compose.local.yml up -d` + сборка и запуск сервера на хосте |
| `./run-docker.sh` | 2 | `docker compose up -d --build` — инфра + сервер в Docker, ждёт `/health` |
| `cozby-tui` | 1 и 2 | TUI-клиент, подключается к `http://localhost:$HTTP_PORT` |

Команды `run.sh`: `stop` · `status` · `logs` · `clean-logs`.
Команды `run-docker.sh`: `stop` · `status` · `logs` · `rebuild` · `-f PATH` · `-i TAG`.
Команды `release.sh`: `uninstall` · `--no-path`.

## Настройка через .env

Все host-порты и URL подключения — переменные. Если надо сместить порт (скажем, 5432 занят системным postgres):

```env
DB_PORT=5433                                                  # host-side
DATABASE_URL=postgres://cozby:cozby@localhost:5433/cozby_brain  # подгоняешь URL
```

Если сервер в docker должен ходить в **внешний** managed-postgres/qdrant/s3 — переопредели `COZBY_INTERNAL_DB_URL` / `COZBY_INTERNAL_QDRANT_URL` / `COZBY_INTERNAL_S3_ENDPOINT` в `.env`.

Полный список переменных с комментариями — в [.env.example](.env.example).

Избегай reasoning-моделей (`*-thinking`, `glm-4.7-flash`) — бери instruct.

## TUI — основные клавиши

- `1`–`6` — вкладки (Inbox / Notes / Todos / Reminders / Learning / Docs)
- `j` `k` — навигация, `Enter` / `o` — открыть, `Space` — toggle todo, `d` — удалить
- `i` — ingest (писать в LLM). В этом режиме поддерживается `@путь/к/файлу` — TUI прочитает файл и отправит его содержимое
- `/` — фильтр, `:` — команды (`:notes`, `:all`, `:q`…), `r` — refresh, `q` / `Esc` — выйти / закрыть

Больше подсказок — во вкладке `Inbox`.
