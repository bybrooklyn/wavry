# GEMINI instructional context

Wavry is a high-performance, low-latency remote streaming platform built on the RIFT (Remote Interactive Frame Transport) protocol. It is designed for remote desktop and game streaming with a focus on sub-20ms latency and high responsiveness.

## Project Overview

- **Purpose**: Latency-first, cross-platform remote streaming.
- **Protocol**: **RIFT** (Remote Interactive Frame Transport) over UDP.
- **Congestion Control**: **DELTA** (Differential Latency Estimation and Tuning Algorithm), a delay-oriented controller prioritizing queuing delay over throughput.
- **Security**: Mandatory end-to-end encryption using **Noise XX** handshake and **ChaCha20-Poly1305** AEAD. Identity is based on Ed25519 keypairs.
- **Core Technologies**:
    - **Backend/Core**: Rust (workspace with 16+ crates).
    - **Frontend (Desktop)**: Tauri 2 + SvelteKit + Vite + Bun.
    - **Frontend (Web)**: SvelteKit reference client with WebTransport/WebRTC hybrid transport.
    - **Mobile**: Kotlin + Jetpack Compose (Android) and Swift + SwiftUI (macOS).
    - **Database**: SQLite (via `sqlx`) for the Gateway.
    - **Containerization**: Docker for Gateway and Relay servers.

### System Architecture

1.  **Gateway (`wavry-gateway`)**: Axum-based HTTP/WebSocket signaling server for auth, peer discovery, and SDP exchange.
2.  **Relay (`wavry-relay`)**: Blind UDP packet forwarder for NAT traversal fallback, using PASETO leases.
3.  **Master (`wavry-master`)**: Central coordination for identity, relay pool management, and matchmaking.
4.  **Server (`wavry-server`)**: The host machine sharing its display (capture → encode → packetize → encrypt → send).
5.  **Client (`wavry-client`)**: The viewer machine (receive → decrypt → depacketize → decode → present).
6.  **Core (`rift-core`, `rift-crypto`)**: Protocol wire format, framing, FEC, and encryption.

## Building and Running

### Prerequisites
- **Rust 1.75+**
- **Bun** (for desktop and web frontends)
- **Protobuf Compiler** (`protoc`)
- **pkg-config**
- Platform-specific toolchains (Xcode 15+ for macOS, PipeWire for Linux, Android SDK/NDK for mobile).

### Key Commands

- **Build Workspace**: `cargo build --workspace`
- **Run Tests**: `cargo test --workspace`
- **Check Code**: `cargo check --workspace`
- **Format Code**: `cargo fmt --all`
- **Lint Code**: `cargo clippy --workspace --all-targets -- -D warnings`
- **Run Gateway**: `cargo run --bin wavry-gateway`
- **Run Relay**: `cargo run --bin wavry-relay -- --master-url http://localhost:8080`
- **Desktop Dev**: `cd crates/wavry-desktop && bun install && bun tauri dev`
- **Android Build**: `./scripts/dev-android.sh`
- **Full Distribution Build**: `./scripts/build-all.sh`

### Docker Deployment
The Auth/Gateway and Relay servers are designed to run in Docker.
- **Gateway**: `docker build -f docker/gateway.Dockerfile -t wavry-gateway .`
- **Relay**: `docker build -f docker/relay.Dockerfile -t wavry-relay .`
Images are automatically built and pushed to GHCR via GitHub Actions.

## Development Conventions

### Coding Style
- **Rust**: Edition 2021, 4-space indentation, `snake_case` for functions/variables, `PascalCase` for types.
- **TypeScript**: Use `bun` for package management.
- **Formatting**: Always run `cargo fmt --all` before committing.
- **Linting**: Keep `clippy` clean (no warnings).

### Error Handling
- Use `anyhow` for application-level errors and `thiserror` for library/crate-specific errors.
- Propagate errors with `?` and provide context with `.context()`.

### Networking & Security
- **P2P First**: Aggressively prioritize direct connections using STUN.
- **Encrypted by Default**: All traffic MUST be end-to-end encrypted.
- **Dynamic Ports**: Use `:0` for random UDP port binding where possible to avoid hardcoded port conflicts.
- **Environment Variables**: Prefer environment variables (e.g., `WAVRY_ALLOW_PUBLIC_BIND=1`) for configuration in containerized/production environments.

### Testing
- Unit tests go in `mod tests` within source files.
- Integration tests go in `crates/<crate>/tests/`.
- Use descriptive test names like `fn test_connection_timeout()`.

### Logging
- Use the `tracing` crate (`info!`, `warn!`, `error!`, `debug!`).
- Never log sensitive information (secrets, keys, tokens).
