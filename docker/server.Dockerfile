# syntax=docker/dockerfile:1.7

FROM rust:1-bookworm AS builder
WORKDIR /app

RUN apt-get update && apt-get install -y --no-install-recommends \
    pkg-config \
    protobuf-compiler \
    libgstreamer1.0-dev \
    libgstreamer-plugins-base1.0-dev \
    libx11-dev \
    libxtst-dev \
    libudev-dev \
    && rm -rf /var/lib/apt/lists/*

COPY Cargo.toml Cargo.lock ./
COPY crates ./crates

RUN cargo build --locked --release -p wavry-server

FROM debian:bookworm-slim AS runtime
RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    tzdata \
    gstreamer1.0-tools \
    gstreamer1.0-plugins-base \
    gstreamer1.0-plugins-good \
    gstreamer1.0-plugins-bad \
    gstreamer1.0-plugins-ugly \
    gstreamer1.0-libav \
    libx11-6 \
    libxtst6 \
    libxrandr2 \
    libevdev2 \
    libudev1 \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app
COPY --from=builder /app/target/release/wavry-server /usr/local/bin/wavry-server

ENV RUST_LOG=info
ENV WAVRY_SERVER_ALLOW_PUBLIC_BIND=1

EXPOSE 5000/udp
ENTRYPOINT ["/usr/local/bin/wavry-server"]
CMD ["--listen", "0.0.0.0:5000"]
