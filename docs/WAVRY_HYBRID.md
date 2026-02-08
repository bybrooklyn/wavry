# Wavry Hybrid Architecture Notes

**Version:** 1.0  
**Last Updated:** 2026-02-07

This document describes shared behavior, abstractions, and long-term direction for Wavry.

---

## Table of Contents

1. [Core Principles](#1-core-principles)
2. [Linux-First Philosophy](#2-linux-first-philosophy)
3. [Cross-Platform Strategy](#3-cross-platform-strategy)
4. [Media Abstraction](#4-media-abstraction)
5. [Transport Model](#5-transport-model)
6. [Latency Budget](#6-latency-budget)
7. [Design Rules](#7-design-rules)
8. [Future Roadmap](#8-future-roadmap)

---

## 1. Core Principles

- **Latency over quality**: Dropped frames are acceptable, delayed frames are not
- **Input priority**: User input must never be delayed by video processing
- **Transparency**: All latency sources must be measurable and logged
- **Hardware leverage**: Use GPU acceleration wherever possible
- **Determinism**: Prefer predictable performance over peak throughput

---

## 2. Linux-First Philosophy

Linux (Wayland) is the **reference platform**. All architectural decisions must be validated on Linux first.

### Why Linux First?

- Open source stack enables debugging at all layers
- PipeWire provides modern, compositor-agnostic capture
- Standard APIs (VAAPI, NVENC) for hardware acceleration
- Deterministic behavior in low-latency scenarios

### Platform Parity

Other platforms must fit the Linux model without compromising latency:
- Same pipeline stages
- Same timing requirements
- Same measurement points

---

## 3. Cross-Platform Strategy

### Code Organization

```
crates/
├── wavry-core/          # Platform-agnostic logic
│   ├── src/
│   │   ├── capture.rs   # Trait definitions
│   │   ├── encode.rs    # Trait definitions
│   │   └── transport.rs # Protocol implementation
├── wavry-media/         # Platform-specific implementations
│   ├── src/
│   │   ├── linux.rs
│   │   ├── windows.rs
│   │   └── macos.rs
```

### Principles

- Core logic in **Rust** with strict trait boundaries
- Platform-specific code isolated behind traits
- No platform-specific hacks in protocol logic
- Compile-time feature flags for platform selection

---

## 4. Media Abstraction

### Trait Requirements

```rust
trait VideoCapture {
    fn probe_capabilities() -> Vec<Codec>;
    fn start(&self, params: CaptureParams) -> Result<FrameStream>;
    fn stop(&self) -> Result<()>;
}

trait VideoEncoder {
    fn supports(codec: Codec) -> bool;
    fn configure(&mut self, params: EncodeParams) -> Result<()>;
    fn encode(&self, frame: Frame) -> Result<EncodedFrame>;
}
```

### Responsibilities

- Probe hardware encode/decode capabilities at startup
- Select lowest-latency option available
- Expose explicit timing metrics for each stage
- Handle capability negotiation between host and client

---

## 5. Transport Model

### Current State (Step Two)

- **LAN-only UDP** (no NAT traversal)
- **No encryption** (plaintext RIFT)
- Custom RIFT packetization with basic FEC

> [!WARNING]
> Step Two traffic is unencrypted. E2EE is required before production deployment.

### Future State

- **P2P with relay fallback** via hole punching
- **Noise XX encryption** for all traffic
- **STUN/TURN** for NAT traversal
- **Relay network** for blocked or failed P2P

---

## 6. Latency Budget

### Target: 15ms end-to-end on LAN

| Stage | Budget | Notes |
|:------|:-------|:------|
| Capture | 2 ms | System capture API overhead |
| Encode | 6 ms | Hardware encoder processing |
| Network | 2 ms | LAN propagation + queuing |
| Decode | 4 ms | Hardware decoder processing |
| Present | 1 ms | Display output |
| **Total** | **15 ms** | Worst-case budget |

### Measurement Points

Every stage must log timing:
- Capture complete → Encode start
- Encode complete → Packetize start
- Transmit complete → Receive
- Receive → Decode start
- Decode complete → Present

---

## 7. Design Rules

Rules that must never be broken:

1. **Dropped frames > Delayed frames**
   - Never queue frames for "smoother" playback
   - Drop immediately if behind schedule

2. **Input is sacred**
   - Input processing never blocks on video
   - Input thread has highest priority
   - Input messages bypass all buffering

3. **No hidden buffering**
   - All buffers must be configurable and measurable
   - Default to zero buffering where possible
   - Document every buffer in the pipeline

4. **Latency must be measurable**
   - Every stage exports timing metrics
   - Logs must enable end-to-end latency calculation
   - Real-time stats available to users

5. **Fail fast**
   - Detect failures immediately
   - Report clear error messages
   - Recover gracefully without user intervention

---

## 8. Future Roadmap

### Near Term (v1.0)

- ✅ End-to-end encryption (Noise XX)
- ✅ Relay network support (with custom bitrate & hardening)
- ✅ macOS platform support
- ✅ Gamepad/controller input
- ✅ Web client (WebTransport/WebRTC bridge)
- ✅ Android client & VR/OpenXR support
- ✅ Multi-monitor capture & switching

### Medium Term (v1.1)

- 🚧 Audio capture and playback (Opus)
- 🚧 Adaptive bitrate (DELTA improvements)
- 🚧 Android hardware acceleration hardening
- 🚧 CI/CD Cross-platform validation (Linux/Windows)

### Long Term (v2.0)

- 📋 Windows/macOS as primary platforms
- 📋 Cloud hosting options
- 📋 10-bit HDR support

---

## 9. Web Client Architecture (Hybrid Transport)

The Web client uses a hybrid transport model to overcome browser limitations:

- **Signaling**: WebSocket or WebTransport streams.
- **Input/Control**: WebTransport datagrams (lowest latency) with fallback to WebRTC DataChannels.
- **Media**: WebRTC (SRTP) for video/audio, bridged from the RIFT native host.

### WebTransport Requirements

WebTransport requires a valid TLS certificate. For local development, self-signed certificates with a short validity (< 14 days) can be used by providing their SHA-256 hash to the browser.

---

## Related Documents

- [WAVRY_ARCHITECTURE.md](WAVRY_ARCHITECTURE.md) - Complete system architecture
- [RIFT_SPEC_V1.md](RIFT_SPEC_V1.md) - Protocol specification
- [DELTA_CC_SPEC.md](DELTA_CC_SPEC.md) - Congestion control
- [PLATFORM_UI_STRATEGY.md](PLATFORM_UI_STRATEGY.md) - Platform-specific UI approaches
