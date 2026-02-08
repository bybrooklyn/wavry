# syntax=docker/dockerfile:1.7

FROM rust:1-bookworm AS builder
WORKDIR /app

RUN apt-get update && apt-get install -y --no-install-recommends \
    pkg-config \
    protobuf-compiler \
    libsqlite3-dev \
    && rm -rf /var/lib/apt/lists/*

COPY Cargo.toml Cargo.lock ./
COPY crates ./crates

RUN cargo build --locked --release -p wavry-gateway

FROM debian:bookworm-slim AS runtime
RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    tzdata \
    libsqlite3-0 \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app
COPY --from=builder /app/target/release/wavry-gateway /usr/local/bin/wavry-gateway

ENV RUST_LOG=info
ENV WAVRY_ALLOW_PUBLIC_BIND=1
ENV DATABASE_URL=sqlite:gateway.db

EXPOSE 3000
ENTRYPOINT ["/usr/local/bin/wavry-gateway"]
