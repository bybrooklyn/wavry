# RIFT Spec v0.0.1

**Status:** Experimental and unstable. Expect breaking changes.

**Change Discipline:** This spec evolves in lockstep with code. Any behavioral
change must be documented here immediately.

---

## Channel Types

A session consists of three logical channels:

1. **Control**
   - Reliable
   - Handshake, configuration, keepalive, reporting

2. **Input**
   - Low-latency
   - Unreliable allowed
   - Always prioritized

3. **Media**
   - Unreliable
   - Frame-paced
   - Drop frames on loss

Priority order: Control > Input > Media.

---

## Packet Encoding (v0.0.1 Implementation)

The current implementation uses a single UDP socket and serializes packets with
`bincode` (v1). Each datagram is a full `RIFT_PACKET` encoded structure; there is
no additional length prefix beyond the UDP datagram boundary.

### RIFT_PACKET

Fields:
- version: u16 (currently `1`)
- session_id: u128
- packet_id: u64 (monotonic per session)
- channel: enum { control, input, media }
- message: union (control | input | video | fec)

Notes:
- The host generates `session_id` and includes it in `RIFT_HELLO_ACK`.
- All packets **after** `RIFT_HELLO_ACK` must include the assigned `session_id`.
- Packets **before** `RIFT_HELLO_ACK` must use `session_id = 0`.
- `RIFT_HELLO_ACK` packets must use the assigned `session_id` in both the packet
  header and the ack body; they must match.
- Packets must use the appropriate priority order (Control > Input > Media)
  when scheduled for send.
- `packet_id` is global per session; FEC groups only include contiguous media
  packets and reset on gaps or interleaved control traffic.

---

## Handshake

### Client → Host: RIFT_HELLO

Fields:
- client_name: string
- platform: enum { linux, windows, macos, freebsd, openbsd, netbsd }
- supported_codecs: list { hevc, h264 }
- max_resolution: (u16 width, u16 height)
- max_fps: u16
- input_caps: bitflags

---

### Host → Client: RIFT_HELLO_ACK

Fields:
- accepted: bool
- selected_codec: enum { hevc, h264 }
- stream_resolution: (u16 width, u16 height)
- fps: u16
- initial_bitrate_kbps: u32
- keyframe_interval_ms: u32
- session_id: u128 (host-generated; non-zero when accepted)

Streaming begins immediately after ACK.

### Handshake Rules (v0.0.1)

- Client must send `RIFT_HELLO` before receiving `RIFT_HELLO_ACK`.
- Host must receive `RIFT_HELLO` before sending `RIFT_HELLO_ACK`.
- Duplicate `RIFT_HELLO` messages are protocol errors.
- `RIFT_HELLO_ACK.accepted = true` requires a non-zero `session_id`.
- `RIFT_HELLO_ACK.accepted = false` indicates rejection; session is not established.
- `RIFT_HELLO` and `RIFT_HELLO_ACK` must be sent on the Control channel.
- Packets must have matching `channel` and `message` types.
- Peers must reject or ignore packets that violate ordering, have mismatched
  `session_id`, or send non-control traffic before session establishment.

---

## Video Messages

### RIFT_VIDEO_CHUNK

Fields:
- frame_id: u64
- chunk_index: u16
- chunk_count: u16
- timestamp_us: u64
- keyframe: bool
- payload: bytes (encoded video chunk)

Rules:
- Encoded frames may be split into multiple chunks.
- The receiver must reassemble chunks by `frame_id`.
- If a frame is incomplete past a small deadline (implementation-defined), drop it.
- No retransmission.
- No reordering beyond reassembly.

---

## FEC Messages (Basic XOR)

### RIFT_FEC

Fields:
- group_id: u64
- first_packet_id: u64
- shard_count: u8 (number of data packets)
- max_payload_len: u16
- payload_sizes: list<u16> (length = shard_count)
- parity_payload: bytes (XOR parity over padded payloads)

Rules:
- One parity packet covers a contiguous group of `shard_count` media packets.
- This recovers **at most one** lost packet per group.
- If more than one packet is missing, the group is unrecoverable.
- FEC packets are sent on the Media channel.
- Parity is computed over the full encoded media packet bytes (header + payload).
- Step Two uses XOR parity; `leopard-rs` is planned for production-grade FEC.

---

## Input Messages

### RIFT_INPUT

Supported events:
- Keyboard press/release (keycode: u32)
- Mouse button press/release (button: u8)
- Relative mouse motion (dx, dy)
- Optional absolute mouse motion (x, y)

Fields:
- event: enum (typed fields per event)
- timestamp_us: u64

---

## Keepalive & Reporting

- Client sends RIFT_PING every 500 ms
- Host replies with RIFT_PONG
- RTT logged for diagnostics only

### RIFT_STATS

Fields:
- period_ms: u32
- received_packets: u32
- lost_packets: u32
- rtt_us: u64

Rules:
- Stats are sent on the Control channel.
- Loss is computed over the last period.

---

## Latency Rules

- Never buffer more than one frame
- Drop frames instead of delaying
- Input must not wait on video
- Frame pacing is mandatory

---

## Versioning

This document describes RIFT v0.0.1 (experimental).
Backward compatibility is not guaranteed.
