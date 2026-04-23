#!/usr/bin/env bash
# run.sh — локальный сценарий: инфра в docker, сервер на хосте.
#
#   ./run.sh              поднять db + qdrant + minio и запустить cozby-brain
#   ./run.sh stop         остановить docker-инфру
#   ./run.sh logs         tail сегодняшнего лога приложения
#   ./run.sh status       статус docker-сервисов + health приложения
#   ./run.sh clean-logs   удалить старые логи
#
# Логи: logs/cozby-brain-YYYYMMDD.log, retention 7 дней.
# Docker-инфра живёт в docker-compose.local.yml (без самого приложения).

set -euo pipefail
cd "$(dirname "$0")"

COMPOSE_FILE="docker-compose.local.yml"
COMPOSE="docker compose -f $COMPOSE_FILE"

LOG_DIR="logs"
LOG_FILE="$LOG_DIR/cozby-brain-$(date +%Y%m%d).log"
KEEP_DAYS=7
HEALTH_URL="http://localhost:${HTTP_PORT:-8081}/health"

G="\033[0;32m"; Y="\033[0;33m"; R="\033[0;31m"; B="\033[0;34m"; N="\033[0m"
info()  { printf "${B}ℹ${N} %s\n" "$*"; }
ok()    { printf "${G}✓${N} %s\n" "$*"; }
warn()  { printf "${Y}⚠${N} %s\n" "$*"; }
err()   { printf "${R}✗${N} %s\n" "$*" >&2; }

svc_health() {
    # $1 — service name. Возвращает healthy/starting/unhealthy/missing
    $COMPOSE ps "$1" --format '{{.Health}}' 2>/dev/null | head -1 || echo missing
}

cmd_stop() {
    info "Останавливаю docker-инфру..."
    $COMPOSE down
    ok "Остановлено"
}

cmd_status() {
    printf "\n"
    info "Docker-сервисы:"
    $COMPOSE ps
    printf "\n"
    info "Приложение ($HEALTH_URL):"
    if curl -sf "$HEALTH_URL" >/dev/null 2>&1; then
        ok "cozby-brain работает"
    else
        warn "cozby-brain не отвечает"
    fi
}

cmd_logs() {
    if [ ! -f "$LOG_FILE" ]; then
        warn "Сегодняшнего лога нет: $LOG_FILE"
        exit 1
    fi
    exec tail -f "$LOG_FILE"
}

cmd_clean_logs() {
    if [ -d "$LOG_DIR" ]; then
        count=$(find "$LOG_DIR" -name 'cozby-brain-*.log' 2>/dev/null | wc -l | tr -d ' ')
        find "$LOG_DIR" -name 'cozby-brain-*.log' -delete 2>/dev/null || true
        ok "Удалено логов: $count"
    fi
}

cmd_run() {
    mkdir -p "$LOG_DIR"
    find "$LOG_DIR" -name 'cozby-brain-*.log' -mtime +$KEEP_DAYS -delete 2>/dev/null || true

    if ! docker info >/dev/null 2>&1; then
        err "Docker не запущен. Запусти Docker Desktop и повтори."
        exit 1
    fi

    # Явный down перед up: если предыдущий запуск оставил контейнеры от
    # другого compose-файла (или yaml сменился), docker не успевает отпустить
    # host-порты при recreate → "port already allocated". С down-up всё чисто.
    info "Сброс старой инфры (если была)..."
    $COMPOSE down --remove-orphans >/dev/null 2>&1 || true

    info "Поднимаю инфру (db + qdrant + minio)..."
    $COMPOSE up -d

    info "Жду healthy..."
    for i in $(seq 1 30); do
        db=$(svc_health db)
        qd=$(svc_health qdrant)
        if [ "$db" = "healthy" ] && [ "$qd" = "healthy" ]; then
            ok "Инфра готова"
            break
        fi
        if [ $i -eq 30 ]; then
            err "Таймаут (db=$db qdrant=$qd)"
            exit 1
        fi
        sleep 1
    done

    local port="${HTTP_PORT:-8081}"
    if lsof -ti :"$port" >/dev/null 2>&1; then
        err "Порт $port уже занят. Убей процесс: lsof -ti :$port | xargs kill"
        exit 1
    fi

    info "Сборка cozby-brain (release)..."
    if ! cargo build --release -p cozby-brain 2>&1 | grep -E "error|warning:" | head -10; then
        true
    fi

    printf "\n"
    ok "Запуск cozby-brain на http://localhost:$port"
    info "Логи: $LOG_FILE  (просмотр: ./run.sh logs)"
    info "Ctrl+C для остановки приложения (docker-сервисы останутся работать)"
    printf "%s\n" "──────────────────────────────────────────────────────────────"

    exec ./target/release/cozby-brain 2>&1 | tee -a "$LOG_FILE"
}

case "${1:-run}" in
    run|up|start|"") cmd_run ;;
    stop|down)       cmd_stop ;;
    status|ps)       cmd_status ;;
    logs|log|tail)   cmd_logs ;;
    clean-logs)      cmd_clean_logs ;;
    -h|--help|help)
        grep '^#' "$0" | sed 's/^# \?//'
        ;;
    *)
        err "Неизвестная команда: $1"
        err "Используй: ./run.sh [run|stop|status|logs|clean-logs]"
        exit 1
        ;;
esac
