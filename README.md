# WAVRY
### Ultra-Low Latency Remote Streaming Platform

[![Rust](https://img.shields.io/badge/language-Rust-orange.svg)](https://www.rust-lang.org/)
[![Protocol](https://img.shields.io/badge/protocol-RIFT--v1.0-blue)](docs/RIFT_SPEC_V1.md)
[![License](https://img.shields.io/badge/license-AGPL--3.0-blue)](LICENSE)

---

## Overview

Wavry is a high-performance, secure remote desktop streaming platform built in Rust. It's designed for sub-frame latency using the custom **RIFT (Remote Interactive Frame Transport)** protocol. Wavry supports both **LAN-only** (fully offline) and **Cloud** (signaling + relay) connectivity modes.

### Key Features

- **RIFT Protocol**: Custom UDP transport with ChaCha20-Poly1305 encryption, minimal framing, and hardware-accelerated codecs
- **Noise XX Handshake**: Mutual authentication with persistent device identity keys
- **Connect via ID**: P2P connection establishment via username using the Gateway signaling server
- **LAN-Only Mode**: Fully offline operation with mDNS discovery — no external servers needed
- **macOS Native Client**: SwiftUI app with Metal rendering and hardware VideoToolbox encoding

---

## Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                          Applications                           │
├─────────────────┬─────────────────┬─────────────────────────────┤
│  apps/macos     │ apps/desktop    │ crates/wavry-cli (planned)  │
│  (SwiftUI)      │ (Svelte/Tauri)  │                             │
└─────────────────┴─────────────────┴─────────────────────────────┘
                            │
              ┌─────────────┴─────────────┐
              │       wavry-ffi           │
              │  (C FFI Bridge Layer)     │
              └─────────────┬─────────────┘
                            │
┌─────────────────────────────────────────────────────────────────┐
│                         Core Crates                             │
├────────────────┬───────────────┬────────────────┬───────────────┤
│  wavry-client  │  wavry-media  │  rift-crypto   │  rift-core    │
│  (Session/Net) │  (Codecs/GPU) │  (Noise/AEAD)  │  (Protocol)   │
└────────────────┴───────────────┴────────────────┴───────────────┘
                            │
              ┌─────────────┴─────────────┐
              │      wavry-gateway        │
              │  (Auth + Signaling + Relay)│
              └───────────────────────────┘
```

---

## Project Structure

| Component | Path | Description |
|:----------|:-----|:------------|
| **Gateway** | `crates/wavry-gateway` | Auth server + WebSocket signaling + UDP relay |
| **Client Core** | `crates/wavry-client` | Session management, networking, signaling client |
| **Media** | `crates/wavry-media` | Hardware-accelerated codecs (VideoToolbox, QSV, NVENC) |
| **Crypto** | `crates/rift-crypto` | Noise XX handshake, ChaCha20-Poly1305 encryption |
| **Protocol** | `crates/rift-core` | RIFT wire format and framing |
| **FFI** | `crates/wavry-ffi` | C-compatible FFI for native clients |
| **macOS App** | `apps/macos` | Native SwiftUI client with Metal rendering |
| **Desktop** | `apps/wavry-desktop` | Cross-platform Svelte/Tauri client (draft) |

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

## API Endpoints (Gateway)

| Method | Path | Description |
|:-------|:-----|:------------|
| POST | `/auth/register` | Create account (email, username, public_key) |
| POST | `/auth/login` | Login, returns session token |
| WS | `/ws` | Signaling WebSocket (BIND, OFFER, ANSWER, CANDIDATE) |

---

## Security

- **Noise XX Handshake**: Mutual identity verification
- **ChaCha20-Poly1305**: All UDP packets authenticated and encrypted
- **Anti-Replay**: Sequence number window protects against replay attacks
- **Persistent Identity**: Device keys stored locally (`Application Support/Wavry/identity.key`)

---

## Development Status

| Phase | Status |
|:------|:-------|
| RIFT Protocol Core | ✅ Complete |
| Noise XX Encryption | ✅ Complete |
| macOS Native Client | ✅ Complete |
| Gateway (Auth + Signaling) | ✅ Complete |
| Connect via ID | ✅ Complete |
| UDP Relay | ✅ Complete |
| DELTA Congestion Control | ✅ Complete |
| Windows/Linux Clients | 📋 Planned |

---

## License

Wavry is released under the **GNU Affero General Public License v3.0** (AGPL-3.0). See [LICENSE](LICENSE) for details.
