# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

Wavry is a latency-first, cross-platform remote desktop and game streaming platform. It uses a custom UDP-based protocol called RIFT (Remote Interactive Frame Transport) with end-to-end encryption (Noise XX + ChaCha20-Poly1305) and delay-oriented congestion control (DELTA algorithm).

## Build Commands

```bash
# Build entire Rust workspace
cargo build --workspace

# Build release
cargo build --release --workspace

# Build a specific crate
cargo build -p wavry-server
cargo build -p wavry-client
cargo build -p wavry-gateway
cargo build -p rift-core

# Run tests
cargo test --workspace
cargo test -p rift-crypto --test integration

# Desktop app (Tauri + SvelteKit) — requires bun
cd crates/wavry-desktop && bun install && bun tauri dev

# Type-check desktop frontend
cd crates/wavry-desktop && bun run check

# Android FFI build (requires NDK + cargo-ndk)
./scripts/build-android-ffi.sh            # release, arm64-v8a + x86_64
./scripts/build-android-ffi.sh --debug     # debug build
./scripts/build-android-ffi.sh --abi arm64-v8a  # single ABI

# Full Android build + install
./scripts/dev-android.sh
./scripts/run-android.sh                   # build, install, launch

# Full distribution build (master, desktop, relay, macOS native)
./scripts/build-all.sh
```

## Running Infrastructure Locally

```bash
cargo run --bin wavry-gateway                              # Signaling gateway
cargo run --bin wavry-relay -- --master-url http://localhost:8080  # Relay server
cargo run --bin wavry-server -- --listen 127.0.0.1:5000    # Streaming host
cargo run --bin wavry-client -- --connect 127.0.0.1:5000   # Client
```

## Architecture

### Workspace Structure (16 crates)

**Protocol layer** — the RIFT wire format and crypto, independent of Wavry application logic:
- `rift-core` — UDP packet framing, Protobuf definitions (`proto/RIFT.proto`), DELTA congestion control, STUN, FEC (XOR parity groups)
- `rift-crypto` — Noise_XX handshake via `snow`, ChaCha20-Poly1305 AEAD per-packet encryption, Ed25519 identity keypairs

**Application layer** — session management and streaming:
- `wavry-server` — host-side streaming session (capture → encode → packetize → send)
- `wavry-client` — client-side session (receive → depacketize → decode → present), signaling, mDNS discovery
- `wavry-common` — shared types and utilities

**Media & platform** — hardware-accelerated, platform-native:
- `wavry-media` — capture and encode/decode using platform APIs (ScreenCaptureKit/VideoToolbox on macOS, WGC/Media Foundation on Windows, PipeWire/VA-API on Linux). Audio via CPAL + Opus.
- `wavry-platform` — input injection and platform utilities

**Infrastructure** — coordination and connectivity:
- `wavry-gateway` — Axum HTTP/WebSocket signaling server for peer discovery, SDP routing, auth. Has SQLite database (sqlx) and admin panel.
- `wavry-relay` — blind UDP packet forwarder with PASETO lease validation
- `wavry-master` — identity, matchmaking, lease issuance

**Frontends:**
- `wavry-desktop` — Tauri 2 + SvelteKit + Vite (Windows/Linux desktop app). Frontend in `crates/wavry-desktop/src/`, Rust backend in `crates/wavry-desktop/src-tauri/`
- `wavry-ffi` — C/JNI FFI bridge via `safer-ffi` for Android
- `wavry-web` — WebTransport/WebRTC types (skeleton)
- `wavry-cli` — CLI tool
- `wavry-vr`, `wavry-vr-alvr` — OpenXR PCVR support with ALVR adapter

**Native apps (outside workspace):**
- `apps/android/` — Kotlin + Jetpack Compose, uses NDK/CMake to link `wavry-ffi`
- `apps/macos/` — Swift native macOS app
- `apps/web-reference/` — browser reference client

### Data Flow (Host → Client)

Capture (platform-native) → Encode (hardware H.264/HEVC/AV1) → RIFT packetize (sequence numbers, group IDs) → FEC parity generation → Encrypt (ChaCha20-Poly1305) → UDP send → Decrypt → FEC recovery → Depacketize/reorder → Decode (hardware) → Present

### Key Design Constraints

- Input processing runs on high-priority threads independent of video encoding
- DELTA CC tracks one-way queuing delay trends, not throughput — states: STABLE, RISING, CONGESTED
- P2P first (STUN for NAT traversal), relay only as fallback
- Identity is Ed25519 keypairs with challenge-response auth (no passwords)
- Protobuf3 for all RIFT control/input/media messages

## Platform-Specific Build Prerequisites

- **All**: Rust 1.75+, `protobuf-compiler`, `pkg-config`
- **macOS**: Xcode 15+ (Metal, ScreenCaptureKit, VideoToolbox)
- **Linux**: PipeWire, GStreamer plugins (VA-API/NVENC), Wayland with xdg-desktop-portal
- **Windows**: Windows 10/11, DirectX, Visual Studio/MSVC
- **Android**: Android SDK, NDK, `cargo-ndk` (auto-installed by build script)

## Key Documentation

- `docs/RIFT_SPEC_V1.md` — full RIFT protocol spec (physical/secure/logical planes)
- `docs/DELTA_CC_SPEC.md` — congestion control algorithm specification
- `docs/WAVRY_ARCHITECTURE.md` — system design and component relationships
- `docs/WAVRY_TESTING.md` — testing runbooks and validation procedures
- `docs/WAVRY_SECURITY.md` — security model and threat analysis
