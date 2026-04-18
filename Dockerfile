# syntax=docker/dockerfile:1.6

# Builder: Alpine + musl = статический бинарник
FROM rust:1.90-alpine AS builder
RUN apk add --no-cache musl-dev pkgconfig
WORKDIR /app
COPY Cargo.toml Cargo.lock* ./
COPY crates ./crates
RUN cargo build --release -p cozby-brain -p cozby-cli

# Runtime: Alpine ~7MB (вместо ~80MB debian-slim)
FROM alpine:3.21
RUN apk add --no-cache ca-certificates
WORKDIR /app
COPY --from=builder /app/target/release/cozby-brain /usr/local/bin/cozby-brain
COPY --from=builder /app/target/release/cozby       /usr/local/bin/cozby
ENV RUST_LOG=info,cozby_brain=debug,tower_http=info \
    HTTP_ADDR=0.0.0.0:8080
EXPOSE 8080
CMD ["cozby-brain"]
