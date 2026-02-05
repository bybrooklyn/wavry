# RIFT Protocol Specification v0.1.0

**Status:** Current stable baseline for development.
**Change Discipline:** This document is the source of truth. Changes to protocol behavior MUST be updated here.

---

## 1. Overview

RIFT (Remote Interactive Frame Transport) is a high-performance, low-latency UDP protocol designed for remote desktop and game streaming. It emphasizes minimal overhead, strong encryption, and flexible error correction.

A RIFT session consists of three logical planes:
1.  **Secure Plane**: Noise-based encryption establishment.
2.  **Physical Plane**: UDP framing, checksums, and session multiplexing.
3.  **Logical Plane**: Protobuf-encoded control, input, and media messages.

---

## 2. Physical Plane (Framing)

All RIFT traffic MUST travel over UDP. Each datagram MUST start with a Physical Header.

### 2.1 Header Formats

RIFT uses two header formats: **Handshake** (large ID) and **Transport** (compact alias).

#### Handshake Header (30 bytes)
Used during initial connection establishment.
| Offset | Size | Field | Description |
| :--- | :--- | :--- | :--- |
| 0 | 2 | Magic | `0x52 0x49` ("RI") |
| 2 | 2 | Version | `0x00 0x01` |
| 4 | 16 | Session ID | 128-bit unique session identifier |
| 20 | 8 | Packet ID | Monotonic 64-bit counter |
| 28 | 2 | Checksum | CRC16-KERMIT over bytes [0..28] |

#### Transport Header (18 bytes)
Used for all traffic after the session is established.
| Offset | Size | Field | Description |
| :--- | :--- | :--- | :--- |
| 0 | 2 | Magic | `0x52 0x49` ("RI") |
| 2 | 2 | Version | `0x00 0x01` |
| 4 | 4 | Session Alias | 32-bit compact session identifier |
| 8 | 8 | Packet ID | Monotonic 64-bit counter |
| 16 | 2 | Checksum | CRC16-KERMIT over bytes [0..16] |

**Note on Detection:** A receiver MUST distinguish these by the 4-byte value starting at Offset 4. If these 4 bytes are all zero (`0x00000000`) AND the total packet length is >= 30 bytes, the packet MUST be treated as a Handshake Header. Otherwise, if the total length is at least 18 bytes, it SHOULD be treated as a Transport Header. Packets failing both criteria MUST be silently dropped.

### 2.2 Integrity
All packets MUST be protected by a **CRC16-KERMIT** checksum. The checksum is calculated over the header (excluding the checksum field itself). Checksum verification MUST be performed by the receiver BEFORE any AEAD decryption or logical processing. Packets with mismatched checksums MUST be dropped.

---

## 3. Secure Transport (Noise)

Before the RIFT logical handshake occurs, peers MUST establish an encrypted session using the **Noise Protocol Framework**.

### 3.1 Handshake Pattern
RIFT MUST use the **Noise_XX_25519_ChaChaPoly_BLAKE2b** pattern by default:
- **XX**: Full 3-way handshake with mutual identity exchange.
- **IK**: (Proposed) 0-RTT resumption for re-connections.

### 3.2 Key Rotation
Packet IDs are 64-bit, providing ample headroom. However, keys SHOULD be rotated if the `packet_id` reaches its maximum value ($2^{64}-1$) or after 24 hours of continuous streaming.

### 3.3 Packet ID Semantics
- Packet IDs MUST be monotonic 64-bit counters.
- Counters are independent for each session direction (Client-to-Host vs Host-to-Client).
- Counters are global across all logical channels (Control, Input, Media) within a single direction.
- If a counter wraps ($2^{64}-1$), the session MUST be re-keyed or terminated to prevent nonce reuse in AEAD.

### 3.4 Protocol Wrapping
Noise handshake messages (MSG1, MSG2, MSG3) are transmitted as the **payload** of Physical Packets.
- MSG1/MSG2 MUST use the **Handshake Header**.
- Transport packets SHOULD use the **Transport Header** once a session alias is assigned.

---

## 4. Logical Plane (Protobuf)

Once the encryption is established, all logical messages MUST be Protobuf v3 encoded.

### 4.1 Message Structure
The top-level `Message` MUST contain a `oneof` content field:
- **Control**: Session management (Hello, Ping, Stats).
- **Input**: User interaction (Keyboard, Mouse, Gamepad).
- **Media**: Stream data (Video chunks, FEC).

### 4.2 Logical Plane Ordering Policy
- **Control Channel**: Messages SHOULD be processed with highest priority to maintain session stability.
- **Input Channel**: Messages MUST be processed before Media messages to minimize "click-to-photon" latency.
- **Media Channel**: Messages MAY be buffered for jitter compensation, but the receiver SHOULD prioritize dropping stale frames over inducing rendering lag.

### 4.3 Logical Message Types

#### Control Messages
- **Hello**: Client capabilities and preferences.
- **HelloAck**: Host accepted parameters and session identifiers.
- **Ping/Pong**: Keepalives and RTT measurement.
- **StatsReport**: Loss data for congestion control.
- **CongestionControl**: Host signals to adjust bitrate/FPS.
- **ReferenceInvalidation (RFI)**: Client signals the last successfully rendered `frame_id`. The host encoder SHOULD use this frame as a reference for future P-frames to recover from loss without a full I-frame.

#### Input Messages
- **MouseButton**: 32-bit button ID and pressed state.
- **Key**: 32-bit keycode and pressed state.
- **MouseMove**: Normalized `0.0` to `1.0` float coordinates.
- **Scroll**: Horizontal and vertical scroll offsets.

#### Media Messages
- **VideoChunk**: Segmented encoded video data.
- **FecPacket**: Parity data for sequence-based error recovery.

---

## 5. Media Handling

### 5.1 Video Chunking
Large video frames are split into `VideoChunk` messages.
- `chunk_index` / `chunk_count` facilitate reassembly.
- `frame_id` groups chunks.
- Chunks for a single frame SHOULD be sent in rapid succession.

### 5.2 Forward Error Correction (FEC)
RIFT uses an interleaved XOR-based FEC.
- **Group Size**: Typically 18 fragments (16 data + 2 parity) or dynamic based on network stats.
- **Payload**: Contains a parity block derived from XOR operations on group members.
- **Recovery**: Enables reconstruction of single-packet gaps per group without retransmission.

---

## 6. Implementation Modules

### 6.1 DELTA Congestion Control
DELTA is a delay-oriented congestion controller designed for real-time interactive streams.
- **Metric**: Uses queuing delay slope trends to transition between state states (Stable, Rising, Congested).
- **Adaptivity**: Automatically scales slope noise floor (epsilon) based on measured jitter.
- **Enforcement**: Regulates bitrate to resolve congestion before packet loss triggers.

### 6.2 NAT Traversal
- **STUN**: Used to discover reflexive public addresses.
- **P2P Branch**: Attempt simultaneous UDP hole punching before falling back to relay.

## 7. Future Roadmap & Improvements

The following features are under consideration for v0.2.0 and beyond.

### 7.1 Zero-RTT Resumption [Planned]
Using Noise PSK (Pre-Shared Key) or Session Tickets to eliminate the 3-packet handshake overhead on reconnection.

### 7.2 PMTU Discovery [Exploratory]
Dynamic path MTU discovery to find the largest possible datagram size without fragmentation, maximizing throughput.

### 7.3 Zero-Copy Media Framing [Experimental]
Optimizing the `PhysicalPacket` -> `VideoChunk` path to avoid intermediate buffer allocations. This involves using a specialized "Tail-Header" for media packets where Protobuf metadata is appended after the raw NAL units.

### 7.4 Audio Synchronization [Planned]
Implementation of Opus-based audio channels with Lip-Sync timestamps locked to `frame_id`.

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
