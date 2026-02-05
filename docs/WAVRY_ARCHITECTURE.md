# Wavry — Architecture

Wavry is a low-latency remote desktop and game streaming platform. This architecture is designed to prioritize input responsiveness, frame pacing, and deterministic performance.

Wavry is cross-platform (Windows, Linux, macOS) and implements end-to-end encryption. Protocol details are specified in `docs/RIFT_SPEC_V1.md`.

---

## Terminology

- User-facing brand: **Wavry**
- Protocol: **RIFT** (Remote Interactive Frame Transport)
- Gateway: `wavry-gateway` (Signaling + Coordination)
- Client Libraries: `wavry-client`, `wavry-ffi`
- Native Apps: `apps/macos`, `crates/wavry-desktop`
- Config/env prefix: `WAVRY_`

---

## Principles

- Latency is the primary priority.
- Frame dropping is preferred over frame queuing.
- Input processing is prioritized over video data.
- Deterministic behavior with transparent control logic.
- Comprehensive telemetry for performance tuning.

---

## System Components

### Core Crates

| Crate | Purpose |
|:------|:--------|
| `rift-core` | RIFT wire format, Protobuf definitions, and DELTA congestion control. |
| `rift-crypto` | Noise XX handshake and ChaCha20-Poly1305 encryption. |
| `wavry-client` | Session management, signaling client, and RTT tracking. |
| `wavry-media` | Hardware-accelerated capture and encoding abstractions. |
| `wavry-ffi` | C-compatible interface for foreign language integrations. |

### Infrastructure

| Component | Purpose |
|:----------|:--------|
| `wavry-gateway` | Signaling gateway for peer discovery and SDP routing. |
| `wavry-relay` | Blind UDP packet forwarder with PASETO lease validation. |

### Applications

| App | Platform | Stack |
|:----|:---------|:------|
| `apps/macos` | macOS | Swift + Metal + ScreenCaptureKit |
| `crates/wavry-desktop` | Windows / Linux | Tauri + Rust + Media Foundation / PipeWire |

---

## Connectivity Modes

### LAN Mode
- mDNS discovery (`_wavry._udp.local.`).
- Direct UDP socket binding.
- Local identity storage with no external dependencies.

### Global Mode
- Identity registration via Wavry Gateway.
- WebSocket-based signaling for handshake coordination.
- UDP hole punching and relay fallback for NAT traversal.

---

## Data Pipeline (Host → Client)

1. **Capture**: System-native capture (GDI/WGC, PipeWire, SCKit).
2. **Encode**: Hardware-accelerated H.264/HEVC/AV1.
3. **Packetize**: RIFT framing with sequence numbers and group IDs.
4. **Transport**: Encrypted UDP with congestion control and FEC.
5. **Depacketize**: Reordering and FEC recovery.
6. **Decode**: Hardware-accelerated decoding.
7. **Present**: Low-latency presentation (DirectX/DXGI, AVSampleBufferDisplayLayer).

Input travels in the reverse direction on a high-priority path.

---

## Subsystems

### Capture and Encode
- **Windows**: Windows Graphics Capture (WGC) + Media Foundation.
- **Linux**: PipeWire + VA-API.
- **macOS**: ScreenCaptureKit + VideoToolbox.

### Transport and Security
- **Protocol**: RIFT over UDP.
- **Encryption**: ChaCha20-Poly1305 authenticated encryption.
- **Congestion Control**: DELTA (Delay-based).
- **Error Correction**: XOR-based Forward Error Correction (FEC).

### Decoder and Renderer
- **Windows**: Media Foundation + DXGI surfaces.
- **macOS**: VideoToolbox + AVSampleBufferDisplayLayer.
- Mailbox presentation to prevent queuing delay.

---

## Security Model

1. **Identity**: Ed25519 keypairs for mutual authentication.
2. **Handshake**: Noise XX protocol for ephemeral session key derivation.
3. **Lease Validation**: Relay access requires PASETO tokens signed by an authorized gateway.
4. **Encryption**: All application data is end-to-end encrypted.
5. **Anti-Replay**: Sequence window tracking for all authenticated packets.

---

## Telemetry and Observability

- Real-time RTT and jitter statistics.
- Frame capture, encode, and decode timing.
- FEC recovery and packet loss metrics.
- Bitrate and FPS adaptation logs from the DELTA controller.

---

## Feature Roadmap

| Feature | Status |
|:--------|:-------|
| Windows Support | Complete |
| Audio Capture | Complete |
| DELTA Tuning | Complete |
| FEC Implementation | Complete |
| Gamepad Support | Planned |
| macOS Client | In Development |

---

## Documentation

- [RIFT Protocol Specification](RIFT_SPEC_V1.md)
- [Congestion Control (DELTA)](DELTA_CC_SPEC.md)
