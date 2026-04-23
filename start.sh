#!/usr/bin/env bash
# start.sh — self-hosted: release-сборка + вся инфра и сервер в docker.
#
# Использование:
#   ./start.sh                    # дефолтный Dockerfile
#   ./start.sh -f Dockerfile.dev  # альтернативный Dockerfile для cozby-brain
#
# release.sh нужен чтобы положить cozby-tui на хост (TUI — единственная
# часть, не завёрнутая в контейнер). docker compose поднимает всё
# остальное: db + qdrant + minio + minio-init + cozby-brain.

set -e
cd "$(dirname "$0")"

if [ "${1:-}" = "-f" ] || [ "${1:-}" = "--dockerfile" ]; then
    [ -n "${2:-}" ] || { echo "ошибка: -f требует путь к Dockerfile"; exit 1; }
    [ -f "$2" ]     || { echo "ошибка: Dockerfile не найден: $2"; exit 1; }
    export COZBY_DOCKERFILE="$2"
    echo "→ Dockerfile: $COZBY_DOCKERFILE"
fi

./release.sh
docker compose up -d --build
