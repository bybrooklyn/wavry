# syntax=docker/dockerfile:1.7

# 1. Planner stage
FROM lukemathwalker/cargo-chef:latest-rust-1-bookworm AS planner
WORKDIR /app
COPY . .
# Scope the dependency graph to the relay binary only.
RUN cargo chef prepare --recipe-path recipe.json --bin wavry-relay

# 2. Cacher stage
FROM lukemathwalker/cargo-chef:latest-rust-1-bookworm AS cacher
WORKDIR /app

RUN apt-get update && apt-get install -y --no-install-recommends \
    pkg-config \
    protobuf-compiler \
    && rm -rf /var/lib/apt/lists/*

COPY --from=planner /app/recipe.json recipe.json
RUN cargo chef cook --release --recipe-path recipe.json --bin wavry-relay

# 3. Builder stage
FROM rust:1-bookworm AS builder
WORKDIR /app

RUN apt-get update && apt-get install -y --no-install-recommends \
    pkg-config \
    protobuf-compiler \
    && rm -rf /var/lib/apt/lists/*

COPY . .
COPY --from=cacher /app/target target
COPY --from=cacher /usr/local/cargo /usr/local/cargo

RUN cargo build --locked --release -p wavry-relay

# 4. Runtime stage
FROM debian:bookworm-slim AS runtime
RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    tzdata \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app
COPY --from=builder /app/target/release/wavry-relay /usr/local/bin/wavry-relay
COPY docker/relay-entrypoint.sh /usr/local/bin/relay-entrypoint.sh
RUN chmod +x /usr/local/bin/relay-entrypoint.sh

ENV RUST_LOG=info

EXPOSE 4000/udp
ENTRYPOINT ["/usr/local/bin/relay-entrypoint.sh"]
CMD ["--listen", "0.0.0.0:4000", "--master-url", "http://wavry-master:8080"]
