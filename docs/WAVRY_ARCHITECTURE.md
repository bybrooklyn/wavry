# Wavry Architecture

**Version:** 1.0  
**Last Updated:** 2026-02-07  
**Status:** Current

Wavry is a low-latency remote desktop and game streaming platform designed for sub-20ms latency on local networks and optimized performance over the internet. This document provides a comprehensive overview of the system architecture.

---

## Table of Contents

1. [Overview](#overview)
2. [Terminology](#terminology)
3. [Design Principles](#design-principles)
4. [System Components](#system-components)
5. [Connectivity Modes](#connectivity-modes)
6. [Data Pipeline](#data-pipeline)
7. [Security Model](#security-model)
8. [Platform Support](#platform-support)
9. [Documentation References](#documentation-references)

---

## Overview

Wavry implements a custom protocol (RIFT - Remote Interactive Frame Transport) optimized for real-time interactive streaming. The architecture prioritizes:

- **Latency**: Sub-20ms target for local networks
- **Responsiveness**: Input prioritized over video
- **Determinism**: Predictable performance with transparent control logic
- **Security**: End-to-end encryption with mutual authentication
- **Cross-platform**: Native support for Linux, Windows, macOS, and Android

---

## Terminology

| Term | Definition |
|------|------------|
| **Wavry** | User-facing brand and project name |
| **RIFT** | Remote Interactive Frame Transport protocol |
| **Host** | The machine sharing its display (server) |
| **Client** | The machine receiving and displaying the stream |
| **Wavry ID** | Base64url-encoded Ed25519 public key (43 characters) |
| **Master** | Central coordination service (auth, matchmaking, relay management) |
| **Relay** | Volunteer-run UDP packet forwarder for NAT traversal |
| **Lease** | Short-lived PASETO token authorizing relay access |

---

## Design Principles

1. **Latency First**: Frame dropping is preferred over frame queuing
2. **Input Priority**: Input processing takes precedence over video data
3. **Zero-Copy Paths**: Minimize buffer copies in the pipeline
4. **Hardware Acceleration**: Leverage GPU encode/decode where available
5. **Deterministic Behavior**: Transparent control logic with comprehensive telemetry
6. **E2EE by Default**: All application data is end-to-end encrypted

---

## System Components

### Core Protocol Crates

| Crate | Purpose | Location |
|:------|:--------|:---------|
| `rift-core` | RIFT wire format, Protobuf definitions, DELTA congestion control | `crates/rift-core/` |
| `rift-crypto` | Noise XX handshake and ChaCha20-Poly1305 encryption | `crates/rift-crypto/` |

### Infrastructure Services

| Component | Purpose | Location |
|:----------|:--------|:---------|
| `wavry-gateway` | Signaling gateway for peer discovery and session coordination | `crates/wavry-gateway/` |
| `wavry-relay` | Blind UDP packet forwarder with PASETO lease validation | `crates/wavry-relay/` |
| `wavry-master` | Central identity, relay pool, and matchmaking service | `crates/wavry-master/` |

### Session Libraries

| Crate | Purpose | Location |
|:------|:--------|:---------|
| `wavry-client` | Session management, signaling client, and RTT tracking | `crates/wavry-client/` |
| `wavry-server` | Host-side capture, encode, and input injection | `crates/wavry-server/` |
| `wavry-media` | Hardware-accelerated capture and encoding abstractions | `crates/wavry-media/` |
| `wavry-ffi` | C-compatible interface for foreign language integrations | `crates/wavry-ffi/` |

### Platform Applications

| App | Platform | Technology Stack | Location |
|:----|:---------|:-----------------|:---------|
| Desktop | Linux / Windows | Tauri + SvelteKit + Rust | `crates/wavry-desktop/` |
| macOS | macOS | SwiftUI + Metal + ScreenCaptureKit | `apps/macos/` |
| Android | Android / Quest | Kotlin + Jetpack Compose + NDK | `apps/android/` |

---

## Connectivity Modes

### LAN Mode (Direct)

- **Discovery**: mDNS (`_wavry._udp.local.`)
- **Transport**: Direct UDP socket binding
- **Authentication**: Ed25519 mutual authentication via Noise XX
- **Use Case**: Same network, lowest latency

```
┌─────────┐      UDP      ┌─────────┐
│ Client  │◄─────────────►│  Host   │
└─────────┘               └─────────┘
```

### Global Mode (Internet)

- **Discovery**: WebSocket-based signaling via Master
- **NAT Traversal**: UDP hole punching with relay fallback
- **Relay**: PASETO-authorized volunteer relays for blocked/direct-fail scenarios
- **Use Case**: Remote connections, different networks

```
┌─────────┐      WS       ┌─────────┐
│ Client  │◄─────────────►│ Master  │
└────┬────┘               └─────────┘
     │
     │  UDP (P2P or Relay)
     │
┌────┴────┐      ┌────────┐      ┌─────────┐
│  Host   │◄────►│ Relay  │◄────►│ Client  │
└─────────┘      └────────┘      └─────────┘
```

---

## Data Pipeline

### Host → Client Flow

```
┌──────────┐   ┌──────────┐   ┌──────────┐   ┌──────────┐   ┌──────────┐   ┌──────────┐   ┌──────────┐
│ Capture  │──►│  Encode  │──►│Packetize │──►│ Encrypt  │──►│ Transport│──►│Depacketize│──►│  Decode  │──►│ Present  │
│          │   │          │   │  (RIFT)  │   │(ChaCha20)│   │   UDP    │   │          │   │          │   │          │
└──────────┘   └──────────┘   └──────────┘   └──────────┘   └──────────┘   └──────────┘   └──────────┘   └──────────┘
```

1. **Capture**: System-native capture (GDI/WGC on Windows, PipeWire on Linux, SCKit on macOS)
2. **Encode**: Hardware-accelerated H.264/HEVC/AV1
3. **Packetize**: RIFT framing with sequence numbers and FEC groups
4. **Encrypt**: ChaCha20-Poly1305 (Noise-established keys)
5. **Transport**: Encrypted UDP with DELTA congestion control and FEC
6. **Depacketize**: Reordering, FEC recovery, and decryption
7. **Decode**: Hardware-accelerated decoding
8. **Present**: Low-latency presentation (mailbox/immediate mode)

### Client → Host Flow (Input)

Input travels in reverse on a high-priority path, bypassing jitter buffering.

---

## Security Model

1. **Identity**: Ed25519 keypairs generated locally; Wavry ID = public key
2. **Handshake**: Noise XX protocol for ephemeral session key derivation
3. **Encryption**: ChaCha20-Poly1305 authenticated encryption
4. **Relay Access**: PASETO v4.public leases signed by Master
5. **Anti-Replay**: 128-packet sequence window tracking
6. **Relay Blindness**: Relays see only opaque encrypted packets

For detailed security specifications, see [WAVRY_SECURITY.md](WAVRY_SECURITY.md).

---

## Platform Support

| Platform | Status | Capture | Encode | Decode | Input |
|:---------|:-------|:--------|:-------|:-------|:------|
| Linux (Wayland) | ✅ Primary | PipeWire | VA-API/NVENC | VA-API | uinput/evdev |
| Linux (X11) | ✅ Supported | PipeWire | VA-API/NVENC | VA-API | uinput/evdev |
| Windows | ✅ Supported | WGC | Media Foundation | Media Foundation | SendInput |
| macOS | ✅ Supported | ScreenCaptureKit | VideoToolbox | VideoToolbox | CGEvent |
| Android | ✅ Supported | MediaProjection | MediaCodec | MediaCodec | Android Input |
| Quest/VR | 🚧 Planned | OpenXR | NDK Media | NDK Media | OpenXR Input |

Legend: ✅ Available | 🚧 In Development | ❌ Not Supported

---

## Documentation References

| Document | Description |
|:---------|:------------|
| [RIFT_SPEC_V1.md](RIFT_SPEC_V1.md) | Complete RIFT protocol specification |
| [DELTA_CC_SPEC.md](DELTA_CC_SPEC.md) | DELTA congestion control algorithm |
| [WAVRY_SECURITY.md](WAVRY_SECURITY.md) | Threat model, mitigations, and operations |
| [WAVRY_MASTER.md](WAVRY_MASTER.md) | Master service API and data model |
| [WAVRY_RELAY.md](WAVRY_RELAY.md) | Relay node specification |
| [WAVRY_RELAY_SELECTION.md](WAVRY_RELAY_SELECTION.md) | Relay selection and reputation |
| [WAVRY_TESTING.md](WAVRY_TESTING.md) | Testing runbooks and validation |
| [PLATFORM_UI_STRATEGY.md](PLATFORM_UI_STRATEGY.md) | Platform UI technology choices |
| [WEB_CLIENT.md](WEB_CLIENT.md) | WebTransport/WebRTC hybrid client |
| [WAVRY_ALVR_ADAPTER.md](WAVRY_ALVR_ADAPTER.md) | VR/OpenXR integration |

---

## Telemetry and Observability

- Real-time RTT and jitter statistics
- Frame capture, encode, and decode timing
- FEC recovery and packet loss metrics
- Bitrate and FPS adaptation logs from DELTA controller
- Relay selection and session quality metrics

---

## Build and Development

See [AGENTS.md](../AGENTS.md) for build commands and development workflows.

Quick reference:

```bash
# Build all crates
cargo build --workspace

# Run tests
cargo test --workspace

# Run control plane locally (Docker-only distribution)
docker compose -f docker/control-plane.compose.yml up -d gateway
WAVRY_RELAY_MASTER_URL=http://host.docker.internal:8080 \
docker compose -f docker/control-plane.compose.yml --profile relay up -d relay

# Desktop app (Tauri)
cd crates/wavry-desktop && bun install && bun tauri dev
```
