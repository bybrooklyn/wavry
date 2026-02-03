# WAVRY_CLIENT.md

# Wavry — Client Specification

The Wavry client receives, decodes, and displays video while capturing and
sending user input to the host.

---

## Responsibilities

- Discover hosts via mDNS and connect
- Negotiate codec and stream parameters
- Receive and decode video
- Render frames immediately
- Capture and send input
- Measure latency

---

## Decoding

- Hardware HEVC decode preferred
- Software decode only if declared
- Decode on dedicated thread
- No frame queueing
- GStreamer decode pipeline with hardware acceleration where available

---

## Rendering

- Present frames immediately
- Avoid compositor-induced buffering
- Use mailbox / immediate presentation modes where possible

---

## Input Capture

- Keyboard press/release
- Mouse buttons
- Relative mouse motion

Input must:
- Be timestamped
- Be sent immediately
- Never wait on video
- Absolute mouse positioning preferred when supported
- Linux input capture uses `evdev` and may require elevated privileges

Drawing tablet input is not implemented yet.

---

## Networking

- LAN-only UDP transport (no NAT traversal in Step Two)
- Maintain media and input channels
- Send RIFT_PING every 500 ms
- Input > video prioritization is mandatory
- Basic FEC is required (static ratio acceptable)
- RIFT packet_id must be monotonic per session
- Step Two connects to a single host per client

### Step Two Implementation Status

- Current implementation uses direct UDP on LAN.
- Client discovery via mDNS (`_wavry._udp.local.`) is required.
- Client must validate control sequencing and `session_id` rules per the spec.
- Decode/render must be real and hardware-accelerated where possible.
- Controller input is deferred.

---

## Diagnostics

Client must log:
- Decode time
- Presentation latency
- RTT
- Dropped frames
- Loss/FEC recovery counters

---

## Non-Goals

- Controller support
- UI polish
- Drawing tablet support
