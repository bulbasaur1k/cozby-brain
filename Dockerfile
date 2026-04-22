# syntax=docker/dockerfile:1.6
#
# Собирает только серверный бинарник cozby-brain.
# TUI остаётся на хосте (cargo install --path crates/tui).
#
# Миграции БД (crates/server/migrations/*.sql) зашиваются в бинарь
# макросом sqlx::migrate!() на этапе компиляции, поэтому runtime-слой
# ничего больше не копирует.
#
# Базовый образ — debian-slim (а не alpine/musl), потому что rust-s3 0.34
# транзитивно тянет native-tls через hyper-tls даже при tokio-rustls-tls
# фиче. debian + libssl-dev = без боли с static openssl на musl.

FROM rust:1.90-slim-bookworm AS builder
RUN apt-get update && apt-get install -y --no-install-recommends \
        pkg-config libssl-dev ca-certificates \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /app

COPY Cargo.toml Cargo.lock ./
COPY crates ./crates

# BuildKit-кэш: registry+git+target переживают ребилды → быстрее итерации.
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git \
    --mount=type=cache,target=/app/target \
    cargo build --release -p cozby-brain && \
    cp target/release/cozby-brain /usr/local/bin/cozby-brain

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends \
        ca-certificates libssl3 curl \
    && rm -rf /var/lib/apt/lists/*
COPY --from=builder /usr/local/bin/cozby-brain /usr/local/bin/cozby-brain

ENV RUST_LOG=info,cozby_brain=debug,tower_http=info \
    HTTP_ADDR=0.0.0.0:8081

EXPOSE 8081

HEALTHCHECK --interval=10s --timeout=3s --start-period=30s --retries=5 \
    CMD curl -fsS http://localhost:8081/health || exit 1

CMD ["cozby-brain"]
