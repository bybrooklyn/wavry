# Web Client Hybrid Transport (Wavry)

This document describes the web client path (WebTransport + WebRTC) and how it coexists with native RIFT clients.

## Summary
- Native clients use **RIFT** for media + control (full performance).
- Web clients use **WebTransport** for control/input and **WebRTC** for media.
- WebTransport is the primary control plane. WebRTC DataChannels are only a fallback.

## Domain Layout
This design is domain-agnostic and compatible with:
- `wavry.dev`
- `auth.wavry.dev`
- `relay.wavry.dev`
- `docs.wavry.dev`
- `app.wavry.dev`

## Connection Flow
1. Browser authenticates and receives:
   - `session_token`
   - WebTransport endpoint
   - WebRTC signaling endpoint
2. Browser connects to WebTransport immediately:
   - Datagram channel: `input`
   - Stream: `control`
3. In parallel, browser starts WebRTC signaling (SDP/ICE).
4. Once WebRTC media is ready, attach video/audio to the page.

### Fallback Logic
- Try WebTransport for control/input.
- If WebTransport fails or is unavailable, use WebRTC DataChannel for input/control.

## WebTransport Channels
- **Datagrams:** high-rate, unordered/unreliable input (mouse move, scroll, analog, gamepad).
- **Streams:** reliable control, stats, settings, connect/disconnect.

## WebRTC Media Path
- WebRTC carries **video + audio only**.
- Low-latency encoder settings (no B-frames, small GOP, no lookahead).
- Target web tier: **~1080p60** capped at **~10 Mbps**.

## Control Stream Messages (JSON)
All control messages are JSON with a `type` field.

Example:
```json
{"type":"connect","session_token":"...","client_name":"web","capabilities":{"max_width":1920,"max_height":1080,"max_fps":60,"supports_gamepad":true,"supports_touch":false}}
```

Supported control messages:
- `connect`
- `disconnect`
- `resize`
- `settings`
- `key`
- `mouse_button`
- `gamepad_button`
- `gamepad_axis`
- `stats_request`

## Input Datagram Format (Binary)
Input datagrams use a compact binary layout (little-endian):

```
byte 0: version (1)
byte 1: kind
bytes 2..9: timestamp_us (u64)
```

Kinds:
- `1` MouseMove: `i16 dx`, `i16 dy`
- `2` Scroll: `i16 dx`, `i16 dy`
- `3` Analog: `u8 axis`, `f32 value`
- `4` Gamepad: `u8 gamepad_id`, `u16 buttons`, `i16 axis0..axis3`

## Stats (Host → Browser)
Stats are sent via the control stream:
- RTT
- jitter
- packet loss
- bitrate
- encoder delay
- decoder delay (if available)

Browser merges WebRTC `getStats()` for UI presentation.

## Integration Points
- Host runs a unified gateway that exposes:
  - RIFT server (native)
  - WebTransport server (control/input)
  - WebRTC peer (media)
- Auth remains centralized. Both native + web clients use the same auth.
