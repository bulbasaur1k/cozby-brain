#!/usr/bin/env bash
# run.sh — запуск cozby-brain локально (db + qdrant в Docker, app на хосте)
#
# Использование:
#   ./run.sh              — запустить всё (db + qdrant + app на :8081)
#   ./run.sh stop         — остановить db + qdrant
#   ./run.sh logs         — хвост сегодняшнего лога
#   ./run.sh status       — статус всех сервисов
#   ./run.sh clean-logs   — удалить все старые логи
#
# Логи: logs/cozby-brain-YYYYMMDD.log (автоматическая ротация по дням, retention 7 дней).

set -euo pipefail

cd "$(dirname "$0")"

LOG_DIR="logs"
LOG_FILE="$LOG_DIR/cozby-brain-$(date +%Y%m%d).log"
KEEP_DAYS=7
HEALTH_URL="http://localhost:8081/health"

# ANSI цвета
G="\033[0;32m"  # green
Y="\033[0;33m"  # yellow
R="\033[0;31m"  # red
B="\033[0;34m"  # blue
N="\033[0m"     # reset

info()  { printf "${B}ℹ${N} %s\n" "$*"; }
ok()    { printf "${G}✓${N} %s\n" "$*"; }
warn()  { printf "${Y}⚠${N} %s\n" "$*"; }
err()   { printf "${R}✗${N} %s\n" "$*" >&2; }

# ─── команды ──────────────────────────────────────────────────────────

cmd_stop() {
    info "Останавливаю db + qdrant..."
    docker compose down
    ok "Остановлено"
}

cmd_status() {
    printf "\n"
    info "Docker-сервисы:"
    docker compose ps
    printf "\n"
    info "Приложение (порт 8081):"
    if curl -sf "$HEALTH_URL" >/dev/null 2>&1; then
        ok "cozby-brain работает: $HEALTH_URL"
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

    # ротация — удаляем логи старше KEEP_DAYS
    find "$LOG_DIR" -name 'cozby-brain-*.log' -mtime +$KEEP_DAYS -delete 2>/dev/null || true

    # проверяем Docker
    if ! docker info >/dev/null 2>&1; then
        err "Docker не запущен. Запусти Docker Desktop и повтори."
        exit 1
    fi

    # поднимаем db + qdrant
    info "Поднимаю db + qdrant в Docker..."
    docker compose up -d db qdrant

    # ждём healthy (макс 30 сек)
    info "Жду готовности сервисов..."
    for i in $(seq 1 30); do
        db_state=$(docker inspect --format '{{.State.Health.Status}}' cozby-brain-db-1 2>/dev/null || echo "starting")
        qd_state=$(docker inspect --format '{{.State.Health.Status}}' cozby-brain-qdrant-1 2>/dev/null || echo "starting")
        if [ "$db_state" = "healthy" ] && [ "$qd_state" = "healthy" ]; then
            ok "db + qdrant готовы"
            break
        fi
        if [ $i -eq 30 ]; then
            err "Таймаут ожидания (db=$db_state, qdrant=$qd_state)"
            exit 1
        fi
        sleep 1
    done

    # проверяем что порт 8081 свободен
    if lsof -ti :8081 >/dev/null 2>&1; then
        err "Порт 8081 уже занят другим процессом."
        err "Останови его или запусти: lsof -ti :8081 | xargs kill"
        exit 1
    fi

    # билдим release (кеш + fast)
    info "Сборка cozby-brain (release)..."
    if ! cargo build --release -p cozby-brain 2>&1 | grep -E "error|warning:" | head -10; then
        true # нет ошибок/варнингов — ок
    fi

    printf "\n"
    ok "Запуск cozby-brain на http://localhost:8081"
    info "Логи: $LOG_FILE  (просмотр: ./run.sh logs)"
    info "Ctrl+C для остановки приложения (Docker-сервисы останутся работать)"
    printf "%s\n" "──────────────────────────────────────────────────────────────"

    # Запускаем приложение, stdout+stderr → терминал + лог-файл
    # exec заменяет шелл, так что Ctrl+C работает чисто
    exec ./target/release/cozby-brain 2>&1 | tee -a "$LOG_FILE"
}

# ─── main ─────────────────────────────────────────────────────────────

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
