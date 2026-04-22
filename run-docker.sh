#!/usr/bin/env bash
# run-docker.sh — второй сценарий: всё в Docker кроме TUI.
#
# Использование:
#   ./run-docker.sh [команда] [-f|--dockerfile PATH] [-i|--image TAG]
#
# Команды:
#   up | start       — build + up профиля `full` (db + qdrant + minio + cozby-brain)
#   stop | down      — docker compose down (все контейнеры этого проекта)
#   logs             — хвост логов cozby-brain
#   status | ps      — статус всех контейнеров
#   rebuild          — пересобрать образ cozby-brain (no-cache) и перезапустить
#
# Флаги:
#   -f, --dockerfile PATH    путь к Dockerfile (по умолчанию ./Dockerfile)
#                            экспортирует COZBY_DOCKERFILE для docker compose
#   -i, --image TAG          имя/тег собираемого образа (по умолчанию cozby-brain:local)
#                            экспортирует COZBY_IMAGE
#
# Те же переменные можно положить в .env — compose подхватит их сам.
#
# Примеры:
#   ./run-docker.sh                              # дефолтный Dockerfile
#   ./run-docker.sh -f Dockerfile.dev            # альтернативный dockerfile
#   ./run-docker.sh rebuild -f Dockerfile.prod   # флаги можно после команды
#   ./run-docker.sh -f dockerfiles/slim.Dockerfile -i cozby-brain:slim
#
# TUI остаётся на хосте: `cargo install --path crates/tui` → `cozby-tui`.

set -euo pipefail
cd "$(dirname "$0")"

HEALTH_URL="http://localhost:8081/health"
COMPOSE="docker compose --profile full"

G="\033[0;32m"; Y="\033[0;33m"; R="\033[0;31m"; B="\033[0;34m"; N="\033[0m"
info()  { printf "${B}ℹ${N} %s\n" "$*"; }
ok()    { printf "${G}✓${N} %s\n" "$*"; }
warn()  { printf "${Y}⚠${N} %s\n" "$*"; }
err()   { printf "${R}✗${N} %s\n" "$*" >&2; }

# ─── parse flags + command ─────────────────────────────────────────────

CMD=""
POSITIONAL=()
while [[ $# -gt 0 ]]; do
    case "$1" in
        -f|--dockerfile)
            if [ -z "${2:-}" ]; then err "-f требует аргумент"; exit 1; fi
            export COZBY_DOCKERFILE="$2"
            shift 2
            ;;
        -i|--image)
            if [ -z "${2:-}" ]; then err "-i требует аргумент"; exit 1; fi
            export COZBY_IMAGE="$2"
            shift 2
            ;;
        -h|--help|help)
            grep '^#' "$0" | sed 's/^# \?//'
            exit 0
            ;;
        *)
            POSITIONAL+=("$1")
            shift
            ;;
    esac
done
CMD="${POSITIONAL[0]:-up}"

# Sanity-check на Dockerfile, если переопределён — ранний, до любого docker-вызова.
if [ -n "${COZBY_DOCKERFILE:-}" ]; then
    if [ ! -f "$COZBY_DOCKERFILE" ]; then
        err "Dockerfile не найден: $COZBY_DOCKERFILE"
        exit 1
    fi
    info "Dockerfile: $COZBY_DOCKERFILE"
fi
if [ -n "${COZBY_IMAGE:-}" ]; then
    info "Image: $COZBY_IMAGE"
fi

# ─── commands ──────────────────────────────────────────────────────────

cmd_up() {
    if ! docker info >/dev/null 2>&1; then
        err "Docker не запущен. Запусти Docker Desktop и повтори."
        exit 1
    fi

    if [ ! -f .env ]; then
        if [ -f .env.example ]; then
            warn ".env не найден — копирую из .env.example. Впиши LLM_API_KEY и перезапусти."
            cp .env.example .env
            exit 1
        fi
        err ".env и .env.example отсутствуют — создай .env с LLM_API_KEY."
        exit 1
    fi

    info "Собираю образ cozby-brain (BuildKit)…"
    DOCKER_BUILDKIT=1 $COMPOSE build cozby-brain

    info "Поднимаю стек…"
    $COMPOSE up -d

    info "Жду /health на $HEALTH_URL…"
    for i in $(seq 1 60); do
        if curl -sf "$HEALTH_URL" >/dev/null 2>&1; then
            ok "cozby-brain готов: $HEALTH_URL"
            printf "\n"
            info "TUI: в новом терминале → ${G}cozby-tui${N}"
            info "Логи: ${G}./run-docker.sh logs${N}"
            info "Остановить: ${G}./run-docker.sh stop${N}"
            exit 0
        fi
        sleep 1
    done

    err "Таймаут. Проверь логи: ./run-docker.sh logs"
    $COMPOSE ps
    exit 1
}

cmd_stop() {
    info "Останавливаю стек…"
    docker compose down
    ok "Остановлено"
}

cmd_logs() {
    exec $COMPOSE logs -f cozby-brain
}

cmd_status() {
    $COMPOSE ps
    printf "\n"
    if curl -sf "$HEALTH_URL" >/dev/null 2>&1; then
        ok "cozby-brain отвечает: $HEALTH_URL"
    else
        warn "cozby-brain не отвечает на $HEALTH_URL"
    fi
}

cmd_rebuild() {
    info "Ребилд cozby-brain…"
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
        exit 1
        ;;
esac
