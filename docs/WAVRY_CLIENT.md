# Wavry Client Specification

**Version:** 1.0  
**Last Updated:** 2026-02-07

The Wavry client receives, decodes, and displays video while capturing and sending user input to the host.

---

## Table of Contents

1. [Responsibilities](#1-responsibilities)
2. [Platform Support](#2-platform-support)
3. [Discovery](#3-discovery)
4. [Decoding](#4-decoding)
5. [Rendering](#5-rendering)
6. [Input Capture](#6-input-capture)
7. [Networking](#7-networking)
8. [Diagnostics](#8-diagnostics)
9. [Non-Goals](#9-non-goals)

---

## 1. Responsibilities

- Discover hosts via mDNS and manual entry
- Negotiate codec and stream parameters with host
- Receive and decode video with hardware acceleration
- Render frames with minimal latency
- Capture and send user input with timestamps
- Measure and report network statistics
- Handle connection failures gracefully

---

## 2. Platform Support

| Platform | Status | Decode API | Rendering |
|:---------|:-------|:-----------|:----------|
| Linux (Wayland) | ✅ Primary | VA-API | OpenGL / Vulkan |
| Linux (X11) | ✅ Supported | VA-API | OpenGL |
| Windows | ✅ Supported | Media Foundation | DirectX / DXGI |
| macOS | 🚧 Development | VideoToolbox | Metal |
| Android | 🚧 Planned | MediaCodec | OpenGL ES |

---

## 3. Discovery

### mDNS Discovery

- Listen for `_wavry._udp.local.` advertisements
- Parse TXT records for host capabilities
- Display discovered hosts in UI

### Manual Connection

- Support direct IP:port entry
- Validate connection before showing in UI
- Test connectivity with ICE-style probing

### Connection Flow

1. Discover or manually enter host
2. Send RIFT_HELLO with client capabilities
3. Receive RIFT_HELLO_ACK with negotiated parameters
4. Establish media and input channels
5. Begin streaming

---

## 4. Decoding

### Requirements

- Hardware decode preferred (HEVC/H.264)
- Software decode only if explicitly declared as fallback
- Decode on dedicated thread(s)
- No frame queuing before decode

### Platform Details

| Platform | API | Notes |
|:---------|:----|:------|
| Linux | GStreamer + VA-API | Zero-copy to GPU if possible |
| Windows | Media Foundation | D3D11 texture output |
| macOS | VideoToolbox | CVPixelBuffer / Metal texture |

### Decode Pipeline

```
Network → Depacketize → FEC Recovery → Decode → Present
```

- Handle out-of-order packets via sequence numbers
- Use FEC for loss recovery when possible
- Trigger NACK for unrecoverable gaps

---

## 5. Rendering

### Requirements

- Present frames immediately (mailbox / immediate mode)
- Avoid compositor-induced buffering
- Target < 5ms from decode to display

### Techniques

| Platform | Method |
|:---------|:-------|
| Linux (Wayland) | wp_presentation protocol for timing |
| Linux (X11) | Present extension, disable vsync |
| Windows | DXGI present with DXGI_PRESENT_DO_NOT_WAIT |
| macOS | CAMetalLayer with display link |

### Frame Timing

- Track presentation timestamps
- Report actual vs. target timing to host
- Enable host pacing optimization

---

## 6. Input Capture

### Input Types

| Type | Events |
|:-----|:-------|
| Keyboard | Press, release, repeat |
| Mouse | Button press/release, relative motion |
| Scroll | Horizontal, vertical |

### Requirements

- Timestamp all input events
- Send immediately (no batching)
- Never wait on video path
- Absolute mouse positioning preferred when supported

### Platform Details

| Platform | Method | Permissions |
|:---------|:-------|:------------|
| Linux | evdev | May require input group membership |
| Windows | Raw Input | No special permissions |
| macOS | CGEventTap | Accessibility permissions required |

### Input Prioritization

Input messages use highest priority channel:
- Bypass jitter buffering
- Trigger immediate send
- May use dedicated socket with DSCP marking

---

## 7. Networking

### Transport

- **RIFT over UDP** (see [RIFT_SPEC_V1.md](RIFT_SPEC_V1.md))
- Maintain separate channels for media and input
- Support both direct P2P and relay modes

### Keepalive

- Send RIFT_PING every **500 ms**
- Track RTT from PONG responses
- Trigger reconnect if 3 consecutive pings fail

### Feedback

Send periodic reports to host:
- RTT and jitter statistics
- Packet loss and FEC recovery counts
- Decoder queue status
- Render timing information

### Session Management

- Validate all incoming packets for correct session_id
- Handle sequence gaps with NACK
- Support graceful disconnect via control message

---

## 8. Diagnostics

Required logging:

| Metric | Description |
|:-------|:------------|
| Decode time | Time from packet to decoded frame |
| Presentation latency | Time from decode to screen |
| RTT | Round-trip time to host |
| Dropped frames | Count and reasons |
| Loss counters | Total, recovered via FEC, unrecovered |
| Jitter buffer | Current depth and adaptations |
| Input latency | Capture to send time |

### User-Facing Stats

Optional overlay or panel showing:
- Current FPS
- Estimated latency
- Network quality indicator
- Codec and resolution info

---

## 9. Non-Goals

Features not in current scope:

- ❌ Controller/gamepad support (planned)
- ❌ Audio output (planned for v1.1)
- ❌ Host-side input injection (security concern)
- ❌ Recording/playback of sessions
- ❌ Multiple simultaneous host connections
- ❌ UI polish and theming (separate effort)

---

## Related Documents

- [RIFT_SPEC_V1.md](RIFT_SPEC_V1.md) - Protocol specification
- [DELTA_CC_SPEC.md](DELTA_CC_SPEC.md) - Congestion control feedback
- [WAVRY_ARCHITECTURE.md](WAVRY_ARCHITECTURE.md) - System overview
- [WAVRY_TESTING.md](WAVRY_TESTING.md) - Testing procedures
