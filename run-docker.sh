#!/usr/bin/env bash
# run-docker.sh — self-hosted сценарий: инфра + сервер в docker, TUI на хосте.
#
# Тонкая обёртка над `docker compose up --build`. Использует базовый
# docker-compose.yml, который через include подтягивает инфру из
# docker-compose.local.yml. Контейнеры инфры шарятся с ./run.sh
# при COMPOSE_PROJECT_NAME=cozby-brain в .env.
#
# Использование:
#   ./run-docker.sh [команда] [-f|--dockerfile PATH] [-i|--image TAG]
#
# Команды:
#   up | start       build + up всех сервисов
#   stop | down      остановить стек
#   logs             хвост логов cozby-brain
#   status | ps      статус сервисов
#   rebuild          пересобрать cozby-brain (no-cache) и перезапустить
#
# Флаги:
#   -f, --dockerfile PATH   путь к Dockerfile (→ $COZBY_DOCKERFILE)
#   -i, --image TAG         имя собираемого образа  (→ $COZBY_IMAGE)
#   Те же переменные можно положить в .env.
#
# Примеры:
#   ./run-docker.sh
#   ./run-docker.sh -f Dockerfile.dev -i cozby-brain:dev
#   ./run-docker.sh rebuild -f Dockerfile.prod
#
# TUI: `cargo install --path crates/tui` → `cozby-tui` в новом терминале.

set -euo pipefail
cd "$(dirname "$0")"

HEALTH_URL="http://localhost:${HTTP_PORT:-8081}/health"
COMPOSE="docker compose"

G="\033[0;32m"; Y="\033[0;33m"; R="\033[0;31m"; B="\033[0;34m"; N="\033[0m"
info()  { printf "${B}ℹ${N} %s\n" "$*"; }
ok()    { printf "${G}✓${N} %s\n" "$*"; }
warn()  { printf "${Y}⚠${N} %s\n" "$*"; }
err()   { printf "${R}✗${N} %s\n" "$*" >&2; }

# ─── flag parsing ──────────────────────────────────────────────────────

POSITIONAL=()
while [[ $# -gt 0 ]]; do
    case "$1" in
        -f|--dockerfile)
            [ -z "${2:-}" ] && { err "-f требует аргумент"; exit 1; }
            export COZBY_DOCKERFILE="$2"; shift 2 ;;
        -i|--image)
            [ -z "${2:-}" ] && { err "-i требует аргумент"; exit 1; }
            export COZBY_IMAGE="$2"; shift 2 ;;
        -h|--help|help)
            grep '^#' "$0" | sed 's/^# \?//'; exit 0 ;;
        *)
            POSITIONAL+=("$1"); shift ;;
    esac
done
CMD="${POSITIONAL[0]:-up}"

if [ -n "${COZBY_DOCKERFILE:-}" ]; then
    [ -f "$COZBY_DOCKERFILE" ] || { err "Dockerfile не найден: $COZBY_DOCKERFILE"; exit 1; }
    info "Dockerfile: $COZBY_DOCKERFILE"
fi
[ -n "${COZBY_IMAGE:-}" ] && info "Image: $COZBY_IMAGE"

# ─── commands ──────────────────────────────────────────────────────────

cmd_up() {
    docker info >/dev/null 2>&1 || { err "Docker не запущен."; exit 1; }

    if [ ! -f .env ]; then
        if [ -f .env.example ]; then
            warn ".env не найден — копирую из .env.example. Впиши LLM_API_KEY и перезапусти."
            cp .env.example .env
            exit 1
        fi
        err ".env и .env.example отсутствуют — создай .env."
        exit 1
    fi

    info "Поднимаю стек (build + up)…"
    DOCKER_BUILDKIT=1 $COMPOSE up -d --build

    info "Жду /health на $HEALTH_URL…"
    for i in $(seq 1 60); do
        if curl -sf "$HEALTH_URL" >/dev/null 2>&1; then
            ok "cozby-brain готов: $HEALTH_URL"
            printf "\n"
            info "TUI: в новом терминале → ${G}cozby-tui${N}"
            info "Логи: ${G}./run-docker.sh logs${N}"
            info "Стоп:  ${G}./run-docker.sh stop${N}"
            exit 0
        fi
        sleep 1
    done

    err "Таймаут. Смотри логи: ./run-docker.sh logs"
    $COMPOSE ps
    exit 1
}

cmd_stop()    { info "Останавливаю стек…"; $COMPOSE down; ok "Остановлено"; }
cmd_logs()    { exec $COMPOSE logs -f cozby-brain; }
cmd_status()  {
    $COMPOSE ps
    printf "\n"
    if curl -sf "$HEALTH_URL" >/dev/null 2>&1; then
        ok "cozby-brain отвечает: $HEALTH_URL"
    else
        warn "cozby-brain не отвечает"
    fi
}
cmd_rebuild() {
    info "Ребилд cozby-brain (no-cache)…"
    DOCKER_BUILDKIT=1 $COMPOSE build --no-cache cozby-brain
    $COMPOSE up -d cozby-brain
    ok "Перезапущено"
}

case "$CMD" in
    up|start|"")    cmd_up ;;
    stop|down)      cmd_stop ;;
    logs|log|tail)  cmd_logs ;;
    status|ps)      cmd_status ;;
    rebuild)        cmd_rebuild ;;
    *)
        err "Неизвестная команда: $CMD"
        err "Доступно: up | stop | logs | status | rebuild"
        exit 1 ;;
esac
