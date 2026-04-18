#!/usr/bin/env bash
# release.sh — собрать и установить cozby бинарники глобально.
#
# После `./release.sh`:
#   cozby          — CLI доступен из любой директории
#   cozby-tui      — TUI доступен из любой директории
#   cozby-brain    — сервер (обычно запускается через ./run.sh)
#
# Использование:
#   ./release.sh              — собрать + установить + настроить PATH (fish)
#   ./release.sh --no-path    — только собрать и установить, не трогать PATH
#   ./release.sh uninstall    — удалить бинарники

set -euo pipefail

cd "$(dirname "$0")"

CARGO_BIN="${CARGO_HOME:-$HOME/.cargo}/bin"

G="\033[0;32m"
Y="\033[0;33m"
R="\033[0;31m"
B="\033[0;34m"
N="\033[0m"

info()  { printf "${B}[i]${N} %s\n" "$*"; }
ok()    { printf "${G}[+]${N} %s\n" "$*"; }
warn()  { printf "${Y}[!]${N} %s\n" "$*"; }
err()   { printf "${R}[x]${N} %s\n" "$*" >&2; }

cmd_uninstall() {
    info "Удаляю cozby, cozby-tui, cozby-brain из $CARGO_BIN"
    cargo uninstall cozby-cli   2>/dev/null || true
    cargo uninstall cb-tui      2>/dev/null || true
    cargo uninstall cozby-brain 2>/dev/null || true
    ok "Удалено"
}

ensure_fish_path() {
    if ! command -v fish >/dev/null 2>&1; then
        info "fish не найден — пропускаю настройку PATH"
        return
    fi

    # Проверяем, видит ли fish наш bin
    if fish -c "contains $CARGO_BIN \$fish_user_paths; and echo yes" 2>/dev/null | grep -q yes; then
        ok "fish уже видит $CARGO_BIN"
        return
    fi

    # Проверяем, может быть уже в стандартном $PATH
    if fish -c "type -q cozby" 2>/dev/null; then
        ok "fish уже находит cozby в PATH"
        return
    fi

    info "Добавляю $CARGO_BIN в fish_user_paths (универсально)"
    fish -c "fish_add_path -U $CARGO_BIN" 2>&1 | sed "s/^/    /" || {
        warn "fish_add_path не сработал, пытаюсь добавить вручную"
        mkdir -p ~/.config/fish/conf.d
        echo "fish_add_path -U $CARGO_BIN" > ~/.config/fish/conf.d/cozby.fish
        ok "записал ~/.config/fish/conf.d/cozby.fish"
    }
    ok "готово — открой новый терминал fish и проверь: cozby --version"
}

cmd_install() {
    # 1. Собираем release
    info "Собираю release-бинарники..."
    cargo build --release -p cozby-brain -p cozby-cli -p cb-tui 2>&1 | tail -3

    # 2. cargo install для каждого пакета с бинарником
    info "Устанавливаю в $CARGO_BIN ..."
    cargo install --path crates/cli    --force --quiet
    cargo install --path crates/tui    --force --quiet
    cargo install --path crates/server --force --quiet

    # 3. Сверка
    for bin in cozby cozby-tui cozby-brain; do
        if [ -x "$CARGO_BIN/$bin" ]; then
            ok "установлен: $CARGO_BIN/$bin"
        else
            err "не найден: $CARGO_BIN/$bin"
            exit 1
        fi
    done

    # 4. Настройка fish PATH
    if [ "${1:-}" != "--no-path" ]; then
        ensure_fish_path
    fi

    printf "\n${G}Готово.${N} Проверь:\n"
    printf "  ${B}cozby --version${N}\n"
    printf "  ${B}cozby-tui --help${N}\n"
}

case "${1:-install}" in
    uninstall|remove) cmd_uninstall ;;
    install|--no-path) cmd_install "${1:-}" ;;
    -h|--help|help)
        grep "^#" "$0" | sed "s/^# \?//"
        ;;
    *)
        err "Неизвестная команда: $1"
        err "Используй: ./release.sh [install | uninstall | --no-path]"
        exit 1
        ;;
esac
