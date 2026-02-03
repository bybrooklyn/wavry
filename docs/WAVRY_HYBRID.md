# WAVRY_HYBRID.md

# Wavry — Hybrid / Architecture Notes

This document describes shared behavior, abstractions, and long-term direction
for Wavry. For the full system architecture, see `docs/WAVRY_ARCHITECTURE.md`.

---

## Linux-First Philosophy

Linux (Wayland) is the reference platform.
All architectural decisions must be validated on Linux first.

Other platforms must fit the model without compromising latency.

---

## Cross-Platform Strategy

- Core logic in Rust
- Platform-specific code isolated behind traits
- No platform-specific hacks in protocol logic

---

## Media Abstraction

The media layer must:
- Probe hardware encode/decode capabilities
- Select the lowest-latency option
- Expose explicit timing metrics

---

## Transport Model (Step Two)

- LAN-only UDP (no NAT traversal).
- No encryption.
- Custom RIFT packetization with basic FEC.
- Step Two traffic is unencrypted; E2EE is required before production.

---

## Latency Budget (Target)

LAN:
- Capture + encode: ≤ 8 ms
- Network: ≤ 2 ms
- Decode + present: ≤ 5 ms
- Total: ~15 ms

---

## Rules That Must Never Be Broken

- Dropped frames are better than delayed frames
- Input must always be prioritized
- No hidden buffering
- Latency must be measurable at all stages

---

## Future (Post Step Two)

- Encryption
- Adaptive bitrate
- Multi-monitor capture
- Windows/macOS production support
- Audio

Explicitly deferred:
- Drawing tablet support (pen/tilt/pressure)
- VR / OpenXR / SteamVR
