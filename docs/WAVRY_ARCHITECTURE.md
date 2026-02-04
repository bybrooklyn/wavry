# Wavry — Architecture

Wavry is a latency-first remote desktop and game streaming system. This
architecture exists to preserve input feel, frame pacing, and deterministic
behavior at all times.

Wavry is cross-platform (Linux + macOS) and features end-to-end encryption.
Protocol details live in `docs/RIFT_SPEC_V1.md`.

---

## Naming

- User-facing brand: **Wavry**
- Protocol: **RIFT** (Remote Interactive Frame Transport)
- Gateway: `wavry-gateway` (Auth + Signaling + Relay)
- Client Libraries: `wavry-client`, `wavry-ffi`
- Native Apps: `apps/macos`, `apps/wavry-desktop`
- Config/env prefix: `WAVRY_`

---

## Principles (Non-Negotiable)

- Latency beats features.
- Dropped frames are better than delayed frames.
- Input is always prioritized over video.
- Deterministic behavior; no opaque heuristics.
- Everything is measurable and debuggable.

---

## System Components

### Core Crates

| Crate | Purpose |
|:------|:--------|
| `rift-core` | RIFT wire format, protobuf messages, framing |
| `rift-crypto` | Noise XX handshake, ChaCha20-Poly1305 encryption |
| `wavry-client` | Networking, session management, signaling client |
| `wavry-media` | Hardware-accelerated codecs (VideoToolbox, QSV, NVENC) |
| `wavry-ffi` | C-compatible FFI for native clients |

### Infrastructure

| Component | Purpose |
|:----------|:--------|
| `wavry-gateway` | Auth server (SQLite), WebSocket signaling, UDP relay |
| `wavry-master` | Discovery registry (planned, currently in gateway) |

### Applications

| App | Platform | Tech Stack |
|:----|:---------|:-----------|
| `apps/macos` | macOS | SwiftUI + Metal + VideoToolbox |
| `apps/wavry-desktop` | Cross-platform | Tauri + SvelteKit |

---

## Connectivity Modes

### LAN Only Mode
- **Zero external dependencies** — fully offline
- mDNS discovery (`_wavry._udp.local.`)
- Direct UDP connection
- Identity key stored locally, never uploaded

### Cloud Mode
- Account registration (email + username + public_key)
- WebSocket signaling for OFFER/ANSWER exchange
- UDP relay fallback for NAT traversal
- Self-hostable Gateway

---

## Data Flow (Host → Client)

```
Capture → Encode → Packetize → Transport → Depacketize → Decode → Present
```

Input travels in the reverse direction and must never wait on video.

---

## Subsystem Details

### Capture
- **Linux**: PipeWire + xdg-desktop-portal
- **macOS**: ScreenCaptureKit (Hardware-accelerated)

### Encode
- **Linux**: VA-API (H.264/HEVC)
- **macOS**: VideoToolbox (Hardware H.264/HEVC)
- Abstraction in `wavry-media` via `Encoder` trait

### Transport
- UDP with RIFT framing
- ChaCha20-Poly1305 authenticated encryption
- XOR-based FEC for packet loss recovery

### Decode
- **macOS**: VideoToolbox + AVSampleBufferDisplayLayer
- Mailbox presentation for lowest latency

### Input Injection
- **macOS**: CoreGraphics (CGEvent) for mouse/keyboard

### Discovery
- mDNS for LAN discovery
- Gateway for global "Connect via ID"

### Signaling (Gateway)
- WebSocket protocol: `BIND`, `OFFER`, `ANSWER`, `CANDIDATE`
- Routes by username (not email)
- Auto-reply to OFFER when hosting

---

## Security Model

1. **Noise XX Handshake**: Mutual identity verification
2. **Persistent Device Keys**: Stored in `Application Support/Wavry/identity.key`
3. **Packet Encryption**: All UDP packets encrypted with ChaCha20-Poly1305
4. **Anti-Replay**: Sequence number window protection
5. **Session Tokens**: JWT-style tokens with expiry for API auth

---

## Observability

- Frame timing logs (capture/encode/decode/present)
- Input latency measurements
- Network RTT and jitter tracking
- Packet loss and FEC recovery counters
- On-screen overlay (FPS, bitrate, latency)

---

## Scope Notes

| Feature | Status |
|:--------|:-------|
| Audio | Deferred |
| Multi-monitor | Deferred |
| Drawing tablet | Deferred |
| Gamepad | Planned |
| VR | Out of scope |

---

## Testing

See `docs/WAVRY_TESTING.md` for the test plan and metrics.
