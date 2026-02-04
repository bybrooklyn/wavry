# Wavry — Architecture

Wavry is a latency-first remote desktop and game streaming system. This
architecture exists to preserve input feel, frame pacing, and deterministic
behavior at all times.

Wavry is now cross-platform (Linux + macOS) and features end-to-end encryption.
Protocol details live in `docs/RIFT_SPEC_V1.md`.

---


## Naming

- User-facing brand: Wavry
- Protocol: RIFT (Remote Interactive Frame Transport)
- Binaries: `wavry-server`, `wavry-client`, `wavry-relay`
- Config/env prefix: `WAVRY_`

## Principles (Non-Negotiable)

- Latency beats features.
- Dropped frames are better than delayed frames.
- Input is always prioritized over video.
- Deterministic behavior; no opaque heuristics.
- Everything is measurable and debuggable.

---

## Modular Layout

Wavry is split into discrete subsystems to avoid hidden coupling:

- `Capture`: 
    - Linux: PipeWire + xdg-desktop-portal.
    - macOS: ScreenCaptureKit (High-performance API).
- `Encode`: GPU encoder abstraction.
    - Linux: VA-API (Hevc/H.264).
    - macOS: VideoToolbox (Hardware HEVC/H.264).
- `Packetize`: RIFT framing + scheduling + prioritization.
- `Transport`: UDP (RIFT) with Encrypted Tunneling.
- `Decode`: Hardware decode + low-latency pipelines.
    - macOS: VideoToolbox + AVSampleBufferDisplayLayer.
- `Present`: Compositor-aware presentation (mailbox/immediate).
- `Input`: Raw input capture and injection.
    - macOS: CoreGraphics (CGEvent) injection.
- `UI`: Native control plane.
    - macOS: SwiftUI (Session-centric, responsive).
    - Linux/Windows: Tauri + SvelteKit.
- `Discovery`: mDNS service advertisement and browsing.
- `Signaling`: Master Server (`auth.wavry.dev`) for global discovery.

---

## Data Flow (Host → Client)

Capture → Encode → Packetize → Transport → Depacketize → Decode → Present

Input travels in the reverse direction and must never wait on video.

---

## Concurrency Model

- Dedicated threads for capture, encode, decode, and present.
- Control/handshake on a reliable path, independent of media loops.
- Explicit queues with bounded sizes; drop on overflow.
- No blocking calls in real-time loops.
- Allocation-free hot paths where possible; reuse buffers.

---

## Transport & Security

- Direct UDP over LAN with UPnP support for automatic port forwarding.
- **End-to-End Encryption**: Mandatory Noise-based (RIFT Msg1-3) handshake.
- Media packets are encrypted via secure transport keys established during handshake.
- Basic XOR FEC on media packets for error recovery.

---

## Observability (Required)

- Frame timing logs (capture/encode/decode/present).
- Input latency measurements.
- Network RTT and jitter tracking.
- Encoder queue depth metrics.
- Optional on-screen overlay (FPS, bitrate, latency).
- Packet loss and FEC recovery counters.

---

## Scope Notes

- Drawing tablet support is explicitly deferred.
- Controller support is deferred.
- Audio is deferred.
- Multi-monitor and virtual displays are deferred.
- VR is explicitly out of scope.

## Testing

See `docs/WAVRY_TESTING.md` for the Step Two test plan and metrics.
