# Wavry — Architecture (Step Two)

Wavry is a latency-first remote desktop and game streaming system. This
architecture exists to preserve input feel, frame pacing, and deterministic
behavior at all times.

Step Two is Linux-only (Wayland-first).
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

## Modular Layout (Step Two)

Wavry is split into discrete subsystems to avoid hidden coupling:

- `Capture`: PipeWire + xdg-desktop-portal (restore tokens).
- `Encode`: GPU encoder abstraction (HEVC primary, H.264 fallback).
- `Packetize`: RIFT framing + scheduling + prioritization.
- `Transport`: UDP (RIFT) over LAN.
- `Decode`: hardware decode + low-latency pipelines.
- `Present`: compositor-aware presentation (mailbox/immediate).
- `Input`: raw input capture and injection.
- `UI`: minimal control plane (connect/host), non-blocking.
- `Discovery`: mDNS service advertisement and browsing.

Capture note:
- Step Two uses a CPU fallback path while DMA-BUF zero-copy wiring is in progress.

Encode note:
- Step Two uses GStreamer VA-API encoders by default; NVENC support is planned.

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

## Transport Strategy (Step Two)

- Direct UDP over LAN only.
- No ICE/STUN/TURN.
- No encryption.
- Basic XOR FEC on media packets.

---

## Observability (Required)

- Frame timing logs (capture/encode/decode/present).
- Input latency measurements.
- Network RTT and jitter tracking.
- Encoder queue depth metrics.
- Optional on-screen overlay (FPS, bitrate, latency).
- Packet loss and FEC recovery counters.

## Dependency Notes

- GStreamer (LGPL) is used for capture/encode/decode in Step Two.
- Review licensing implications for future dual-licensing plans.

---

## Scope Notes

- Drawing tablet support is explicitly deferred.
- Controller support is deferred.
- Audio is deferred.
- Multi-monitor and virtual displays are deferred.
- VR is explicitly out of scope.
- Relays, auth, ICE, and UPnP are out of scope for Step Two.
- End-to-end encryption (Noise/snow) is mandatory for production but not implemented in Step Two.

## Testing

See `docs/WAVRY_TESTING.md` for the Step Two test plan and metrics.
