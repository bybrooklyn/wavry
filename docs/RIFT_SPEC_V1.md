# RIFT Protocol Specification v1.2

**Status:** Current stable baseline for development  
**Last Updated:** 2026-02-07  
**Change Discipline:** This document is the source of truth. Changes to protocol behavior MUST be updated here.

---

## Table of Contents

1. [Overview](#1-overview)
2. [Physical Plane (Framing)](#2-physical-plane-framing)
3. [Secure Transport (Noise)](#3-secure-transport-noise)
4. [Logical Plane (Protobuf)](#4-logical-plane-protobuf)
5. [Media Handling](#5-media-handling)
6. [Advanced Features](#6-advanced-features)
7. [Future Roadmap](#7-future-roadmap)

---

## 1. Overview

RIFT (Remote Interactive Frame Transport) is a high-performance, low-latency UDP protocol designed for remote desktop and game streaming. It emphasizes minimal overhead, strong encryption, and flexible error correction.

A RIFT session consists of three logical planes:

1. **Secure Plane**: Noise-based encryption establishment
2. **Physical Plane**: UDP framing, checksums, and session multiplexing
3. **Logical Plane**: Protobuf-encoded control, input, and media messages

### Key Characteristics

| Property | Value |
|:---------|:------|
| **Transport** | UDP only |
| **Encryption** | ChaCha20-Poly1305 (Noise XX handshake) |
| **Packet ID** | 64-bit monotonic per direction |
| **Session ID** | 128-bit UUID |
| **Checksum** | CRC16-KERMIT |

---

## 2. Physical Plane (Framing)

All RIFT traffic MUST travel over UDP. Each datagram MUST start with a Physical Header.

### 2.1 Header Formats

RIFT uses two header formats: **Handshake** (large ID) and **Transport** (compact alias).

#### Handshake Header (30 bytes)

Used during initial connection establishment.

| Offset | Size | Field | Description |
|:-------|:-----|:------|:------------|
| 0 | 2 | Magic | `0x52 0x49` ("RI") |
| 2 | 2 | Version | `0x00 0x01` |
| 4 | 16 | Session ID | 128-bit unique session identifier |
| 20 | 8 | Packet ID | Monotonic 64-bit counter |
| 28 | 2 | Checksum | CRC16-KERMIT over bytes [0..28] |

#### Transport Header (18 bytes)

Used for all traffic after the session is established.

| Offset | Size | Field | Description |
|:-------|:-----|:------|:------------|
| 0 | 2 | Magic | `0x52 0x49` ("RI") |
| 2 | 2 | Version | `0x00 0x01` |
| 4 | 4 | Session Alias | 32-bit compact session identifier |
| 8 | 8 | Packet ID | Monotonic 64-bit counter |
| 16 | 2 | Checksum | CRC16-KERMIT over bytes [0..16] |

**Header Detection:**

A receiver MUST distinguish these by the 4-byte value starting at Offset 4:
- If these 4 bytes are all zero (`0x00000000`) AND the total packet length is >= 30 bytes, treat as **Handshake Header**
- Otherwise, if the total length is at least 18 bytes, treat as **Transport Header**
- Packets failing both criteria MUST be silently dropped

### 2.2 Integrity

All packets MUST be protected by a **CRC16-KERMIT** checksum. The checksum is calculated over the header (excluding the checksum field itself). Checksum verification MUST be performed by the receiver BEFORE any AEAD decryption or logical processing. Packets with mismatched checksums MUST be dropped.

---

## 3. Secure Transport (Noise)

Before the RIFT logical handshake occurs, peers MUST establish an encrypted session using the **Noise Protocol Framework**.

### 3.1 Handshake Pattern

RIFT uses the **Noise_XX_25519_ChaChaPoly_BLAKE2b** pattern by default:

- **XX**: Full 3-way handshake with mutual identity exchange
- **IK**: (Future) 0-RTT resumption for re-connections

### 3.2 Key Rotation

Packet IDs are 64-bit, providing ample headroom. However, keys SHOULD be rotated if the `packet_id` reaches its maximum value ($2^{64}-1$) or after 24 hours of continuous streaming.

### 3.3 Packet ID Semantics

- Packet IDs MUST be monotonic 64-bit counters
- Counters are independent for each session direction (Client-to-Host vs Host-to-Client)
- Counters are global across all logical channels (Control, Input, Media) within a single direction
- If a counter wraps ($2^{64}-1$), the session MUST be re-keyed or terminated to prevent nonce reuse in AEAD

### 3.4 Protocol Wrapping

Noise handshake messages (MSG1, MSG2, MSG3) are transmitted as the **payload** of Physical Packets:
- MSG1/MSG2 MUST use the **Handshake Header**
- Transport packets SHOULD use the **Transport Header** once a session alias is assigned

---

## 4. Logical Plane (Protobuf)

Once the encryption is established, all logical messages MUST be Protobuf v3 encoded.

### 4.1 Message Structure

The top-level `Message` MUST contain a `oneof` content field:

| Channel | Purpose | Priority |
|:--------|:--------|:---------|
| **Control** | Session management (Hello, Ping, Stats) | Highest |
| **Input** | User interaction (Keyboard, Mouse, Gamepad) | High |
| **Media** | Stream data (Video chunks, FEC) | Normal |

### 4.2 Logical Plane Ordering Policy

- **Control Channel**: Messages SHOULD be processed with highest priority to maintain session stability
- **Input Channel**: Messages MUST be processed before Media messages to minimize "click-to-photon" latency
- **Media Channel**: Messages MAY be buffered for jitter compensation, but the receiver SHOULD prioritize dropping stale frames over inducing rendering lag

### 4.3 Logical Message Types

#### Control Messages

| Message | Purpose |
|:--------|:--------|
| **Hello** | Client capabilities and preferences |
| **HelloAck** | Host accepted parameters and session identifiers |
| **Ping/Pong** | Keepalives and RTT measurement |
| **StatsReport** | Loss data for congestion control |
| **CongestionControl** | Host signals to adjust bitrate/FPS |
| **ReferenceInvalidation (RFI)** | Client signals the last successfully rendered `frame_id`. The host encoder SHOULD use this frame as a reference for future P-frames to recover from loss without a full I-frame |
| **Nack** | Receiver-driven missing packet report. The receiver SHOULD emit a NACK immediately upon detecting gaps in the transport packet ID sequence (sliding window 64–256) |
| **EncoderControl** | Receiver hint to skip encoder output frames (e.g., 1–2 frames) when sudden RTT spikes are detected to allow network buffers to drain |
| **PoseUpdate** | Headset pose update (position + orientation). These packets MUST be treated as ultra-high priority and MUST bypass any jitter buffer |
| **VrTiming** | VR timing hints from the client (refresh rate + vsync offset) to align pacing and prediction |

#### Input Messages

| Message | Fields |
|:--------|:-------|
| **MouseButton** | 32-bit button ID and pressed state |
| **Key** | 32-bit keycode and pressed state |
| **MouseMove** | Normalized `0.0` to `1.0` float coordinates |
| **Scroll** | Horizontal and vertical scroll offsets |

#### Media Messages

| Message | Purpose |
|:--------|:--------|
| **VideoChunk** | Segmented encoded video data |
| **FecPacket** | Parity data for sequence-based error recovery |
| **AudioPacket** | Opus-encoded audio payloads with microsecond timestamps |

---

## 5. Media Handling

### 5.1 Video Chunking

Large video frames are split into `VideoChunk` messages:

- `chunk_index` / `chunk_count` facilitate reassembly
- `frame_id` groups chunks
- Chunks for a single frame SHOULD be sent in rapid succession

### 5.2 Forward Error Correction (FEC)

RIFT uses an interleaved XOR-based FEC:

- **Group Size**: Typically 18 fragments (16 data + 2 parity) or dynamic based on network stats
- **Payload**: Contains a parity block derived from XOR operations on group members
- **Recovery**: Enables reconstruction of single-packet gaps per group without retransmission

### 5.3 Audio (Opus)

Audio payloads use **raw Opus packets** (no container) with minimal framing overhead:

| Parameter | Value |
|:----------|:------|
| **Codec** | Opus |
| **Sample Rate** | 48 kHz |
| **Channels** | Stereo (2) |
| **Frame Size** | 5 ms (240 samples per channel) |
| **Bitrate** | ~128 kbps (target) |
| **Timestamp** | `AudioPacket.timestamp_us` refers to the first sample in the Opus frame |

Receivers SHOULD decode and play audio immediately with a short buffer (≤ 20 ms). If buffers grow, drop the oldest audio first to preserve motion-to-photon latency.

---

## 6. Advanced Features

### 6.1 DELTA Congestion Control

DELTA is a delay-oriented congestion controller designed for real-time interactive streams. See [DELTA_CC_SPEC.md](DELTA_CC_SPEC.md) for complete specification.

**Key Features:**
- Uses queuing delay slope trends to transition between states (Stable, Rising, Congested)
- Scales the slope noise floor (epsilon) relative to smoothed RTT
- Regulates bitrate to resolve congestion before packet loss triggers

### 6.2 Adaptive Packet Pacing

Media packets SHOULD be micro-paced after packetization rather than sent as a burst:

- Initial spacing: ~20–50µs
- Adapt based on smoothed RTT, recent arrival jitter, and current bitrate
- Increase spacing when RTT rises or packets arrive bunched
- Decrease spacing when RTT is stable and arrivals are evenly spaced

### 6.3 Receiver-Driven Aggressive NACK

Receivers MUST track transport packet IDs in a sliding window and emit a NACK immediately on gap detection (no sender timeout). This enables fast retransmission of missing packets without waiting for loss to compound.

### 6.4 Adaptive Client Jitter Buffer

Receivers SHOULD adjust jitter buffer size dynamically:
- Shrink toward 0ms under stable conditions
- Grow toward 5–10ms when jitter is detected

### 6.5 Encoder Panic Skip Mode

On sudden RTT spikes (e.g., +30–50ms over smoothed RTT), the receiver SHOULD signal the sender to skip 1–2 frames to drain buffers and avoid congestion spirals.

### 6.6 DSCP/WMM Tagging

Senders SHOULD set DSCP to prioritize traffic:

| Traffic Type | DSCP Value | Binary |
|:-------------|:-----------|:-------|
| Pose + Input | EF (Expedited Forwarding) | 46 / 0x2E |
| Media | EF or CS6 | 46/48 / 0x2E/0x30 |

### 6.7 NAT Traversal

- **STUN**: Used to discover reflexive public addresses
- **P2P Branch**: Attempt simultaneous UDP hole punching before falling back to relay

---

## 7. Future Roadmap

The following features are under consideration for v1.1.0 and beyond:

### 7.1 Zero-RTT Resumption [Planned]

Using Noise PSK (Pre-Shared Key) or Session Tickets to eliminate the 3-packet handshake overhead on reconnection.

### 7.2 PMTU Discovery [Exploratory]

Dynamic path MTU discovery to find the largest possible datagram size without fragmentation, maximizing throughput.

### 7.3 Zero-Copy Media Framing [Experimental]

Optimizing the `PhysicalPacket` -> `VideoChunk` path to avoid intermediate buffer allocations. This involves using a specialized "Tail-Header" for media packets where Protobuf metadata is appended after the raw NAL units.

### 7.4 Audio Synchronization & Lip-Sync [Planned]

Refine audio/video alignment by locking audio timestamps to `frame_id` and improving drift correction without increasing latency.

### 7.5 Z-Frame Padding (Pipe Warming) [Experimental]

Optional low-priority "Zero Frames" sent during idle periods to keep NAT mappings active and prevent ISP "sleep" states from inducing first-packet jitter.

### 7.6 Header Bitfields [Experimental]

Optimizing the `PhysicalPacket` header to use bitfields for `Option` flags (e.g., `has_alias`, `is_encrypted`), saving several bytes per packet.

### 7.7 Multi-Link & Mobility [Planned]

Support for seamless session migration between network interfaces (e.g., Wi-Fi to 5G) using a signed "Session Rebind" message.

### 7.8 Tiled Streaming [Exploratory]

Parallelizing high-resolution streams (4K/8K) by splitting frames into tiles, each with independent FEC and timestamps, allowing for distributed decoding.

### 7.9 Hybrid QUIC Support [Exploratory]

Exploring the use of standard QUIC as a parallel transport specifically for the **Control** and **Input** channels, leveraging its mature congestion control and reliability, while keeping `RIFT-UDP` for high-throughput Media.

---

## Related Documents

- [DELTA_CC_SPEC.md](DELTA_CC_SPEC.md) - Congestion control specification
- [WAVRY_SECURITY.md](WAVRY_SECURITY.md) - Security model and threat mitigations
- [WAVRY_ARCHITECTURE.md](WAVRY_ARCHITECTURE.md) - System architecture overview
