# syntax=docker/dockerfile:1.7

# Base image pinned by digest for security and reproducibility
# Tag: lukemathwalker/cargo-chef:latest-rust-1-bookworm
# Last updated: 2026-02-13
FROM lukemathwalker/cargo-chef:latest-rust-1-bookworm@sha256:68c8f8b92cca1647e7622e8d76754b922412915e556e687d797667171fd7ef23 AS chef-base
WORKDIR /app
ENV CARGO_TARGET_DIR=/app/target

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
    --mount=type=cache,target=/app/target,sharing=locked \
    cargo chef cook --release --recipe-path recipe.json --bin wavry-gateway
COPY . .
RUN --mount=type=cache,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,target=/usr/local/cargo/git,sharing=locked \
    --mount=type=cache,target=/app/target,sharing=locked \
    cargo build --locked --release -p wavry-gateway --bin wavry-gateway && \
    mkdir -p /out && \
    if [ -x /app/target/release/wavry-gateway ]; then \
      install -m 0755 /app/target/release/wavry-gateway /out/wavry-gateway; \
    elif [ -x /app/crates/target/release/wavry-gateway ]; then \
      install -m 0755 /app/crates/target/release/wavry-gateway /out/wavry-gateway; \
    else \
      echo "wavry-gateway binary not found in expected Cargo target directories" >&2; \
      exit 1; \
    fi

# Runtime base image pinned by digest for security
# Tag: debian:bookworm-slim
# Last updated: 2026-02-13
FROM debian:bookworm-slim@sha256:98f4b71de414932439ac6ac690d7060df1f27161073c5036a7553723881bffbe AS runtime
RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    tzdata \
    libsqlite3-0 \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --system --create-home --home-dir /var/lib/wavry --shell /usr/sbin/nologin wavry

WORKDIR /var/lib/wavry
COPY --from=builder /out/wavry-gateway /usr/local/bin/wavry-gateway
COPY docker/gateway-entrypoint.sh /usr/local/bin/gateway-entrypoint.sh
RUN chmod +x /usr/local/bin/gateway-entrypoint.sh

ENV RUST_LOG=info
ENV WAVRY_ALLOW_PUBLIC_BIND=1
ENV DATABASE_URL=sqlite:gateway.db

VOLUME ["/var/lib/wavry"]

EXPOSE 3000
USER wavry
ENTRYPOINT ["/usr/local/bin/gateway-entrypoint.sh"]
