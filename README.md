# WAVRY
### Ultra-Low Latency Remote Streaming Platform

[![Rust](https://img.shields.io/badge/language-Rust-orange.svg)](https://www.rust-lang.org/)
[![Protocol](https://img.shields.io/badge/protocol-RIFT--v1.0-blue)](docs/RIFT_SPEC_V1.md)
[![License](https://img.shields.io/badge/license-AGPL--3.0-blue)](LICENSE)

---

## Overview

Wavry is a high-performance, secure remote desktop streaming platform built in Rust. It's designed for sub-frame latency using the custom **RIFT (Remote Interactive Frame Transport)** protocol. Wavry supports both **LAN-only** (fully offline) and **Cloud** (signaling + relay) connectivity modes.

### Key Features

- **RIFT Protocol**: Custom UDP transport with ChaCha20-Poly1305 encryption and DELTA congestion control.
- **Noise XX Handshake**: Mutual authentication with persistent Ed25519 device identity keys.
- **Wavry ID**: Cryptographically secure identity—users are identified by their public key, no central password storage.
- **P2P by Default**: Integrated STUN discovery for direct NAT hole punching, minimizing relay dependency.
- **macOS Native Client**: SwiftUI app with Metal rendering and performance-tuned VideoToolbox encoding.

---

## Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                          Applications                           │
├─────────────────┬─────────────────┬─────────────────────────────┤
│  apps/macos     │ apps/desktop    │ crates/wavry-cli            │
│  (SwiftUI)      │ (Svelte/Tauri)  │ (Planned)                   │
└─────────────────┴─────────────────┴─────────────────────────────┘
                            │
              ┌─────────────┴─────────────┐
              │       wavry-ffi           │
              │  (C FFI Bridge Layer)     │
              └─────────────┬─────────────┘
                            │
┌───────────────────────────┴─────────────────────────────────────┐
│                         Core Crates                             │
├────────────────┬───────────────┬────────────────┬───────────────┤
│  wavry-client  │  wavry-media  │  rift-crypto   │  rift-core    │
│  (Session/Net) │  (Codecs/GPU) │  (Noise/AEAD)  │  (Protocol)   │
└────────────────┴───────────────┴────────────────┴───────────────┘
                            │
              ┌─────────────┴─────────────┐
              │      wavry-master         │
              │ (Identity + Matchmaking)  │
              └───────────────────────────┘
```

---

## Project Structure

| Component | Path | Description |
|:----------|:-----|:------------|
| **Master** | `crates/wavry-master` | Coordination server: Identity (Ed25519), Signaling, and Matchmaking |
| **Client Core** | `crates/wavry-client` | Session management, networking, signaling client, STUN discovery |
| **Media** | `crates/wavry-media` | Hardware-accelerated codecs (VideoToolbox, QSV, NVENC) |
| **Crypto** | `crates/rift-crypto` | Noise XX handshake, ChaCha20-Poly1305 encryption |
| **Protocol** | `crates/rift-core` | RIFT wire format and DELTA congestion control |
| **FFI** | `crates/wavry-ffi` | C-compatible FFI with background P2P signaling support |
| **macOS App** | `apps/macos` | Native SwiftUI client with performance-tuned VideoToolbox |

---

## Identity: Wavry ID

Wavry uses **cryptographic identity**. Your "account" is an Ed25519 keypair.
- **Wavry ID**: The Base64url-encoded public key.
- **No Passwords**: Authentication is done via a single-use random challenge signed by your private key.
- **Privacy**: The Master server never sees your private key or your media data.

---

## Connectivity Modes

### LAN Only (Default for Privacy)
- **No login required** — fully offline
- Uses **mDNS discovery** (`_wavry._udp.local.`) for local network hosts
- Direct UDP connection to host IP
- Zero external network calls

### Cloud Mode
- **Account-based** — register with email/username + public key
- **Signaling server** routes connection offers/answers
- **UDP Relay** available for NAT traversal fallback
- Host your own Gateway or use Wavry Cloud

---

## Quick Start

### Prerequisites
- Rust 1.75+
- macOS 14+ (for macOS client) or Linux
- `protobuf-compiler` for proto generation

### Build All Crates
```bash
git clone https://github.com/wavry/wavry.git
cd wavry
cargo build --release
```

### Run Gateway (for Cloud Mode)
```bash
cd crates/wavry-gateway
touch gateway.db
DATABASE_URL="sqlite:gateway.db" cargo run
# Listening on http://0.0.0.0:3000 (API) + udp://0.0.0.0:3478 (Relay)
```

### Run macOS Client
```bash
./scripts/dev-macos.sh
# Or open apps/macos in Xcode
```

---

## API Endpoints (Master)

| Method | Path | Description |
|:-------|:-----|:------------|
| POST | `/v1/auth/register` | Start identity registration (returns challenge) |
| POST | `/v1/auth/register/verify` | Prove identity with signature (returns session token) |
| POST | `/v1/connect` | Request matchmaking for a Peer ID |
| WS | `/ws` | Signaling WebSocket (OFFER, ANSWER, CANDIDATE) |

---

## Security

- **Noise XX Handshake**: Mutual identity verification for every session.
- **STUN / NAT Traversal**: Automatic public address discovery for direct P2P.
- **ChaCha20-Poly1305**: All media traffic is authenticated and encrypted.
- **Persistent Identity**: Secure key storage in `Application Support/Wavry/`.

---

## Development Status

| Phase | Status |
|:------|:-------|
| RIFT Protocol | ✅ Complete |
| Noise XX Encryption | ✅ Complete |
| DELTA Congestion Control | ✅ Complete |
| STUN / NAT Traversal | ✅ Complete |
| Master (Identity + Signaling) | ✅ Complete |
| macOS Screen/Video Tuning | ✅ Complete |
| Audio Streaming | ✅ Complete |
| Windows/Linux Clients | 📋 In Progress |

---

## License

Wavry is released under the **GNU Affero General Public License v3.0** (AGPL-3.0). See [LICENSE](LICENSE) for details.
