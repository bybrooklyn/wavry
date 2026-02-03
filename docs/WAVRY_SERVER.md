# WAVRY_SERVER.md

# Wavry — Server (Host) Specification

The Wavry server (host) is responsible for capturing the local display,
encoding video, transmitting frames, and applying remote input.

Linux (Wayland) is the primary supported platform.

---

## Responsibilities

- Capture screen frames
- Encode video using hardware HEVC when available
- Maintain frame pacing
- Send RIFT_VIDEO messages
- Receive and apply RIFT_INPUT messages
- Respond to keepalive pings
- Advertise availability via mDNS (`_wavry._udp.local.`)

---

## Capture (Linux / Wayland)

- Use PipeWire + xdg-desktop-portal
- Single monitor support is sufficient initially
- Prefer DMA-BUF / zero-copy paths
- Avoid unnecessary CPU copies
- Persist portal permissions via restore tokens
- Step Two uses a CPU fallback path until DMA-BUF wiring is completed

---

## Encoding

- Default codec: HEVC (H.265)
- Hardware acceleration required if available
- Low-latency presets only
- No B-frames
- Short GOP (≤ 1s)
- GStreamer pipeline with VA-API/NVENC where available

If HEVC is unavailable, fallback to H.264 only if negotiated.

---

## Frame Scheduling

- Fixed cadence (e.g. 16.67 ms for 60 FPS)
- Encode just-in-time
- Never queue frames
- Drop frames if encoder falls behind

---

## Input Injection

Linux:
- Use uinput
- Apply events immediately
- No smoothing or prediction in v1
- Absolute mouse positioning preferred
- uinput access may require elevated privileges

Other platforms:
- Stub implementations only

Drawing tablet input is not implemented yet.

---

## Networking

- LAN-only UDP transport (no NAT traversal in Step Two)
- Media channel is unreliable
- Input > video prioritization is mandatory
- Basic FEC is required (static ratio acceptable)
- RIFT packet_id must be monotonic per session
- Step Two supports a single active client at a time

### Step Two Implementation Status

- Current implementation uses direct UDP on LAN.
- Host advertises via mDNS (`_wavry._udp.local.`); client discovery is required.
- Server must validate control sequencing and `session_id` rules per the spec.
- Capture/encode must be real PipeWire + hardware encoder.
- Controller input is deferred.

---

## Logging (Required)

- Encode time per frame
- Frame drops
- RTT from pings
- Loss stats from client reports
- Effective FPS

---

## Non-Goals

- Multi-monitor capture
- Controller input
- Encryption
- Drawing tablet support
