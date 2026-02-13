# syntax=docker/dockerfile:1.7

FROM lukemathwalker/cargo-chef:latest-rust-1-bookworm AS chef-base
WORKDIR /app

RUN --mount=type=cache,target=/var/cache/apt,sharing=locked \
    --mount=type=cache,target=/var/lib/apt,sharing=locked \
    apt-get update && apt-get install -y --no-install-recommends \
      pkg-config \
      protobuf-compiler \
      libsqlite3-dev \
      && rm -rf /var/lib/apt/lists/*

FROM chef-base AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json --bin wavry-gateway

FROM chef-base AS builder
COPY --from=planner /app/recipe.json recipe.json
RUN --mount=type=cache,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,target=/usr/local/cargo/git,sharing=locked \
    cargo chef cook --release --recipe-path recipe.json --bin wavry-gateway
COPY . .
RUN --mount=type=cache,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,target=/usr/local/cargo/git,sharing=locked \
    cargo build --locked --release -p wavry-gateway

FROM debian:bookworm-slim AS runtime
RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    tzdata \
    libsqlite3-0 \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --system --create-home --home-dir /var/lib/wavry --shell /usr/sbin/nologin wavry

WORKDIR /var/lib/wavry
COPY --from=builder /app/target/release/wavry-gateway /usr/local/bin/wavry-gateway
COPY docker/gateway-entrypoint.sh /usr/local/bin/gateway-entrypoint.sh
RUN chmod +x /usr/local/bin/gateway-entrypoint.sh

ENV RUST_LOG=info
ENV WAVRY_ALLOW_PUBLIC_BIND=1
ENV DATABASE_URL=sqlite:gateway.db

VOLUME ["/var/lib/wavry"]

EXPOSE 3000
USER wavry
ENTRYPOINT ["/usr/local/bin/gateway-entrypoint.sh"]
