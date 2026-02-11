# syntax=docker/dockerfile:1.7

# 1. Planner stage: determine dependencies
FROM lukemathwalker/cargo-chef:latest-rust-1-bookworm AS planner
WORKDIR /app
COPY . .
# Scope the dependency graph to the gateway binary only.
RUN cargo chef prepare --recipe-path recipe.json --bin wavry-gateway

# 2. Cacher stage: build dependencies
FROM lukemathwalker/cargo-chef:latest-rust-1-bookworm AS cacher
WORKDIR /app

RUN apt-get update && apt-get install -y --no-install-recommends \
    pkg-config \
    protobuf-compiler \
    libsqlite3-dev \
    && rm -rf /var/lib/apt/lists/*

COPY --from=planner /app/recipe.json recipe.json
RUN cargo chef cook --release --recipe-path recipe.json --bin wavry-gateway

# 3. Builder stage: build the actual source
FROM rust:1-bookworm AS builder
WORKDIR /app

RUN apt-get update && apt-get install -y --no-install-recommends \
    pkg-config \
    protobuf-compiler \
    libsqlite3-dev \
    && rm -rf /var/lib/apt/lists/*

COPY . .
# Copy the compiled dependencies from the cacher stage
COPY --from=cacher /app/target target
COPY --from=cacher /usr/local/cargo /usr/local/cargo

RUN cargo build --locked --release -p wavry-gateway

# 4. Runtime stage
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
