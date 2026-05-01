# cozby-brain

Личный AI-органайзер: заметки, todo, напоминания, документация, learning-треки.
Пишешь как думаешь — LLM сама классифицирует и сохраняет.

![cozby-tui](docs/cozby-tui.png)

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

Один скрипт-ярлык собирает TUI (через `release.sh`) и поднимает весь стек в docker:

```bash
./start.sh                   # == ./release.sh + docker compose up -d --build
cozby-tui                    # в новом терминале
```

Если не нужна TUI на хосте — можно вызывать compose напрямую:

```bash
docker compose up -d --build
```

### Как устроены compose-файлы

- `docker-compose.local.yml` — только инфра (`db + qdrant + minio + minio-init`). Используется `./run.sh`.
- `docker-compose.yml` — те же сервисы + `cozby-brain`. Используется `./start.sh` / `docker compose up`.
- Файлы стоят независимо друг от друга, инфра дублируется осознанно — так проще читать и менять без сюрпризов `include:`.
- При `COMPOSE_PROJECT_NAME=cozby-brain` из `.env` имена контейнеров и волюмы совпадают между сценариями — переключаться можно без потери БД.

## Что делает каждый скрипт

| Скрипт | Сценарий | Что делает |
|---|---|---|
| `./release.sh` | 1 и 2 | `cargo install --release` трёх бинарников в `~/.cargo/bin` + PATH для fish |
| `./run.sh` | 1 | `docker compose -f docker-compose.local.yml up -d` + сборка и запуск сервера на хосте |
| `./start.sh` | 2 | `./release.sh && docker compose up -d --build` — минимальный ярлык, без логики |
| `cozby-tui` | 1 и 2 | TUI-клиент, подключается к `http://localhost:$HTTP_PORT` |

Команды `run.sh`: `stop` · `status` · `logs` · `clean-logs`.
Команды `release.sh`: `uninstall` · `--no-path`.

Альтернативный Dockerfile для сценария 2 — через `.env`:
```env
COZBY_DOCKERFILE=Dockerfile.dev
COZBY_IMAGE=cozby-brain:dev
```

## Настройка через .env

Все host-порты и URL подключения — переменные. Если надо сместить порт (скажем, 5432 занят системным postgres):

```env
DB_PORT=5433                                                  # host-side
DATABASE_URL=postgres://cozby:cozby@localhost:5433/cozby_brain  # подгоняешь URL
```

Если сервер в docker должен ходить в **внешний** managed-postgres/qdrant/s3 — переопредели `COZBY_INTERNAL_DB_URL` / `COZBY_INTERNAL_QDRANT_URL` / `COZBY_INTERNAL_S3_ENDPOINT` в `.env`.

> **MinIO console (порт 9001) по умолчанию не пробрасывается** — 9001 часто занят у людей чем-то другим (Portainer, Prometheus и т.п.). Если нужна web-консоль MinIO, раскомментируй строку `- "${MINIO_CONSOLE_PORT:-9001}:9001"` в `docker-compose.yml` (и/или `.local.yml`) и поменяй `MINIO_CONSOLE_PORT` в `.env` если 9001 занят. Приложению консоль не нужна.

Полный список переменных с комментариями — в [.env.example](.env.example).

Избегай reasoning-моделей (`*-thinking`, `glm-4.7-flash`) — бери instruct.

## Календарь (Apple / Google / Outlook)

cozby отдаёт все напоминания одним iCalendar-файлом — Apple Calendar и
другие умеют **subscribe** к этому URL и сами обновляют события.
Никаких OAuth, app-passwords, CalDAV не нужно.

```
http://localhost:8081/api/ical/feed.ics
```

**Apple Calendar** (macOS): `File → New Calendar Subscription` → вставь URL
выше → выбери частоту обновления (15 минут / час / день).
Recurring reminders из cozby превращаются в повторяющиеся события с
нативным алармом в момент срабатывания.

Если cozby крутится не на той же машине, что Calendar.app — пробрось
порт `8081` или подними сервер на доступном хосте. Auth у feed нет —
имей в виду на публичных сетях (на старте предполагается localhost-only).

## TUI — основные клавиши

- `1`–`6` — вкладки (Inbox / Notes / Todos / Reminders / Learning / Docs)
- `j` `k` — навигация, `Enter` / `o` — открыть, `Space` — toggle todo, `d` — удалить
- `i` — ingest (писать в LLM). В этом режиме поддерживается `@путь/к/файлу` — TUI прочитает файл и отправит его содержимое
- `/` — фильтр, `:` — команды (`:notes`, `:all`, `:q`…), `r` — refresh, `q` / `Esc` — выйти / закрыть

Больше подсказок — во вкладке `Inbox`.
