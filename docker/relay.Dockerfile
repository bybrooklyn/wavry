# syntax=docker/dockerfile:1.7

FROM rust:1-bookworm AS builder
WORKDIR /app

RUN apt-get update && apt-get install -y --no-install-recommends \
    pkg-config \
    protobuf-compiler \
    && rm -rf /var/lib/apt/lists/*

COPY Cargo.toml Cargo.lock ./
COPY crates ./crates

RUN cargo build --locked --release -p wavry-relay

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
