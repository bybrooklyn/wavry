# syntax=docker/dockerfile:1.7

FROM lukemathwalker/cargo-chef:latest-rust-1-bookworm AS chef-base
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
    cargo chef cook --release --recipe-path recipe.json --bin wavry-relay
COPY . .
RUN --mount=type=cache,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,target=/usr/local/cargo/git,sharing=locked \
    cargo build --locked --release -p wavry-relay

FROM debian:bookworm-slim AS runtime
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

VOLUME ["/var/lib/wavry-relay"]

EXPOSE 4000/udp
USER wavry
ENTRYPOINT ["/usr/local/bin/relay-entrypoint.sh"]
CMD ["--listen", "0.0.0.0:4000", "--master-url", "http://wavry-master:8080"]
