#!/usr/bin/env bash
# start.sh — self-hosted: release-сборка + вся инфра и сервер в docker.
#
# release.sh нужен чтобы положить cozby-tui на хост (TUI — единственная
# часть, не завёрнутая в контейнер). docker compose поднимает всё
# остальное: db + qdrant + minio + minio-init + cozby-brain.

set -e
cd "$(dirname "$0")"

./release.sh
docker compose up -d --build
