# Wavry Server (Host) Specification

**Version:** 1.0  
**Last Updated:** 2026-02-07

The Wavry server (host) is responsible for capturing the local display, encoding video, transmitting frames, and applying remote input.

---

## Table of Contents

1. [Responsibilities](#1-responsibilities)
2. [Platform Support](#2-platform-support)
3. [Capture](#3-capture)
4. [Encoding](#4-encoding)
5. [Frame Scheduling](#5-frame-scheduling)
6. [Input Injection](#6-input-injection)
7. [Networking](#7-networking)
8. [Logging](#8-logging)
9. [Non-Goals](#9-non-goals)

---

## 1. Responsibilities

- Capture screen frames using system-native APIs
- Encode video using hardware acceleration when available
- Maintain frame pacing and timing
- Send RIFT_VIDEO messages with proper chunking
- Receive and apply RIFT_INPUT messages
- Respond to keepalive pings and control messages
- Advertise availability via mDNS (`_wavry._udp.local.`)

---

## 2. Platform Support

| Platform | Status | Capture API | Encode API |
|:---------|:-------|:------------|:-----------|
| Linux (Wayland) | ✅ Primary | PipeWire + xdg-desktop-portal | VA-API / NVENC |
| Linux (X11) | ✅ Supported | PipeWire | VA-API / NVENC |
| Windows | ✅ Supported | Windows Graphics Capture | Media Foundation |
| macOS | 🚧 Development | ScreenCaptureKit | VideoToolbox |

---

## 3. Capture

### Linux / Wayland

- Use **PipeWire** with **xdg-desktop-portal**
- Single monitor support is baseline; multi-monitor is future work
- Prefer **DMA-BUF** / zero-copy paths
- Avoid unnecessary CPU copies
- Persist portal permissions via restore tokens

**Implementation Notes:**
- Step Two uses a CPU fallback path until DMA-BUF wiring is completed
- Portal dialog must be handled gracefully on first run

### Windows

- Use **Windows Graphics Capture (WGC)** API
- Support windowed and fullscreen capture modes
- Handle DPI scaling correctly

### macOS

- Use **ScreenCaptureKit** (macOS 12.3+)
- Requires Screen Recording permission
- Handle permission denial gracefully

---

## 4. Encoding

### Codec Priority

1. **HEVC (H.265)** - Default, best compression efficiency
2. **H.264** - Fallback for compatibility
3. **AV1** - Future option for supported hardware

### Requirements

- Hardware acceleration required if available
- Low-latency presets only
- **No B-frames**
- Short GOP (≤ 1 second)
- Target bitrate adapts via DELTA congestion control

### Platform Details

| Platform | API | Notes |
|:---------|:----|:------|
| Linux | GStreamer + VA-API/NVENC | VA-API for Intel/AMD, NVENC for NVIDIA |
| Windows | Media Foundation | Hardware encode via DXGI/D3D11 |
| macOS | VideoToolbox | HEVC supported on Apple Silicon and recent Intel |

### Fallback

If HEVC is unavailable, fallback to H.264 **only if negotiated** with client during handshake.

---

## 5. Frame Scheduling

### Timing

- Fixed cadence based on target FPS (e.g., 16.67 ms for 60 FPS)
- Encode just-in-time before transmission
- Never queue frames for display

### Frame Drops

- Drop frames if encoder falls behind schedule
- Prefer frame drops over increased latency
- Log dropped frames for diagnostics

### Rate Adaptation

- Respond to `CongestionControl` messages from client
- Adjust bitrate smoothly via encoder rate control
- Step down FPS only if bitrate reduction insufficient

---

## 6. Input Injection

### Linux

- Use **uinput** kernel interface
- Apply events immediately without smoothing
- Absolute mouse positioning preferred
- Keyboard scancode mapping required

**Permissions:**
- uinput access may require elevated privileges or udev rules

### Windows

- Use **SendInput** API
- Handle key repeat correctly
- Absolute mouse positioning via normalized coordinates

### macOS

- Use **CGEvent** APIs
- Requires Accessibility permissions
- Handle application focus correctly

### Controller Input

- Deferred to future milestone
- Will use platform-native gamepad APIs

---

## 7. Networking

### Transport

- **RIFT over UDP** (see [RIFT_SPEC_V1.md](RIFT_SPEC_V1.md))
- Media channel is unreliable (no ACKs)
- Control channel uses reliable retransmission

### Priorities

1. **Input messages** - Highest priority, immediate processing
2. **Control messages** - High priority, reliable delivery
3. **Media chunks** - Normal priority, adaptive pacing

### Session Management

- Validate `session_id` and packet sequencing per spec
- Support single active client per session (v1)
- Handle client disconnections gracefully

### Discovery

- Advertise via **mDNS** (`_wavry._udp.local.`)
- Include display name and capabilities in TXT records

---

## 8. Logging

Required log events:

| Event | Fields |
|:------|:-------|
| Frame encoded | frame_id, encode_time_ms, size_bytes |
| Frame dropped | frame_id, reason (encoder_lag, congestion) |
| Input received | event_type, timestamp |
| Input applied | event_type, processing_time_us |
| RTT measurement | rtt_ms, smoothed_rtt_ms |
| Loss report | loss_pct, packets_lost |
| Effective FPS | fps, target_fps, dropped_frames |
| Connection state | state (connecting, active, disconnected) |

---

## 9. Non-Goals

Features explicitly not in scope for current version:

- ❌ Multi-monitor simultaneous capture
- ❌ Controller/gamepad input
- ❌ End-to-end encryption (Step Two - plaintext RIFT)
- ❌ Drawing tablet support (pressure/tilt)
- ❌ Audio capture (planned for v1.1)
- ❌ Multiple concurrent clients

---

## Related Documents

- [RIFT_SPEC_V1.md](RIFT_SPEC_V1.md) - Protocol specification
- [DELTA_CC_SPEC.md](DELTA_CC_SPEC.md) - Congestion control
- [WAVRY_ARCHITECTURE.md](WAVRY_ARCHITECTURE.md) - System overview
- [WAVRY_TESTING.md](WAVRY_TESTING.md) - Testing procedures
