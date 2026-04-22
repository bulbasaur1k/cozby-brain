#!/usr/bin/env bash
# run-docker.sh — второй сценарий: всё в Docker кроме TUI.
#
# Использование:
#   ./run-docker.sh            — build + up профиль `full` (db + qdrant + minio + cozby-brain)
#   ./run-docker.sh stop       — docker compose down (все контейнеры этого проекта)
#   ./run-docker.sh logs       — хвост логов cozby-brain
#   ./run-docker.sh status     — статус всех контейнеров
#   ./run-docker.sh rebuild    — пересобрать образ cozby-brain и перезапустить
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

case "${1:-up}" in
    up|start|"")    cmd_up ;;
    stop|down)      cmd_stop ;;
    logs|log|tail)  cmd_logs ;;
    status|ps)      cmd_status ;;
    rebuild)        cmd_rebuild ;;
    -h|--help|help)
        grep '^#' "$0" | sed 's/^# \?//'
        ;;
    *)
        err "Неизвестная команда: $1"
        err "Доступно: up | stop | logs | status | rebuild"
        exit 1
        ;;
esac
