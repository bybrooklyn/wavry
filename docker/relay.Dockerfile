# syntax=docker/dockerfile:1.7

# Base image pinned by digest for security and reproducibility
# Tag: lukemathwalker/cargo-chef:latest-rust-1-bookworm
# Last updated: 2026-02-13
FROM lukemathwalker/cargo-chef:latest-rust-1-bookworm@sha256:68c8f8b92cca1647e7622e8d76754b922412915e556e687d797667171fd7ef23 AS chef-base
WORKDIR /app

RUN --mount=type=cache,target=/var/cache/apt,sharing=locked \
    --mount=type=cache,target=/var/lib/apt,sharing=locked \
    apt-get update && apt-get install -y --no-install-recommends \
      pkg-config \
      protobuf-compiler \
      && rm -rf /var/lib/apt/lists/*

FROM chef-base AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json --bin wavry-relay

FROM chef-base AS builder
COPY --from=planner /app/recipe.json recipe.json
RUN --mount=type=cache,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,target=/usr/local/cargo/git,sharing=locked \
    --mount=type=cache,target=/app/target,sharing=locked \
    cargo chef cook --release --recipe-path recipe.json --bin wavry-relay
COPY . .
RUN --mount=type=cache,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,target=/usr/local/cargo/git,sharing=locked \
    --mount=type=cache,target=/app/target,sharing=locked \
    cargo build --locked --release -p wavry-relay --bin wavry-relay

# Runtime base image pinned by digest for security
# Tag: debian:bookworm-slim
# Last updated: 2026-02-13
FROM debian:bookworm-slim@sha256:98f4b71de414932439ac6ac690d7060df1f27161073c5036a7553723881bffbe AS runtime
RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    tzdata \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --system --create-home --home-dir /var/lib/wavry-relay --shell /usr/sbin/nologin wavry

WORKDIR /var/lib/wavry-relay
COPY --from=builder /app/target/release/wavry-relay /usr/local/bin/wavry-relay
COPY docker/relay-entrypoint.sh /usr/local/bin/relay-entrypoint.sh
RUN chmod +x /usr/local/bin/relay-entrypoint.sh

ENV RUST_LOG=info
ENV HOME=/var/lib/wavry-relay
ENV WAVRY_RELAY_HEALTH_LISTEN=0.0.0.0:9091

VOLUME ["/var/lib/wavry-relay"]

EXPOSE 4000/udp
EXPOSE 9091
USER wavry
ENTRYPOINT ["/usr/local/bin/relay-entrypoint.sh"]
CMD ["--listen", "0.0.0.0:4000", "--master-url", "http://wavry-master:8080", "--health-listen", "0.0.0.0:9091"]
