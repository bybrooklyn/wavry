# RIFT Protocol Specification v0.1.0

**Status:** Current stable baseline for development.
**Change Discipline:** This document is the source of truth. Changes to protocol behavior must be updated here.

---

## 1. Overview

RIFT (Real-time Interactive Frame Transport) is a high-performance, low-latency UDP protocol designed for remote desktop and game streaming. It emphasizes minimal overhead, strong encryption, and flexible error correction.

A RIFT session consists of three logical planes:
1.  **Secure Plane**: Noise-based encryption establishmemt.
2.  **Physical Plane**: UDP framing, checksums, and session multiplexing.
3.  **Logical Plane**: Protobuf-encoded control, input, and media messages.

---

## 2. Physical Plane (Framing)

All RIFT traffic travels over UDP. Each datagram starts with a Physical Header.

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

**Note on Detection:** A receiver distinguishes these by the value at offset 4. If the first 4 bytes of the ID space are zero AND the total length is at least 30 bytes, it is treated as a Handshake Header.

### 2.2 Integrity
All packets are protected by a **CRC16-KERMIT** checksum. The checksum is calculated over the header (excluding the checksum field itself) and verified by the receiver. Packets with mismatched checksums MUST be dropped.

---

## 3. Secure Transport (Noise)

Before the RIFT logical handshake occurs, peers MUST establish an encrypted session using the **Noise Protocol Framework**.

### 3.1 Handshake Pattern
RIFT uses the **Noise_XX_25519_ChaChaPoly_BLAKE2b** pattern by default:
- **XX**: Full 3-way handshake with mutual identity exchange.
- **IK**: (Proposed) 0-RTT resumption for re-connections.

### 3.2 Key Rotation
Packet IDs are 64-bit, providing ample headroom. However, keys SHOULD be rotated if the `packet_id` wraps or after 24 hours of continuous streaming.

### 3.3 Protocol Wrapping
Noise handshake messages (MSG1, MSG2, MSG3) are transmitted as the **payload** of Physical Packets.
- MSG1/MSG2 typically use the **Handshake Header**.
- MSG3/Transport typically use the **Transport Header**.

---

## 4. Logical Plane (Protobuf)

Once the encryption is established, all logical messages are Protobuf v3 encoded.

### 4.1 Message Structure
The top-level `Message` contains a `oneof` content field:
- **Control**: Session management (Hello, Ping, Stats).
- **Input**: User interaction (Keyboard, Mouse, Gamepad).
- **Media**: Stream data (Video chunks, FEC).

### 4.2 Logical Message Types

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
RIFT uses an interleaved XOR-based FEC by default.
- A `FecPacket` contains a parity block for a group of `N` media packets.
- **Proposed**: Migration to Reed-Solomon (Leopard) for multi-packet recovery.

---

## 6. Future Roadmap & Improvements

The following features are planned for v0.2.0 and beyond.

### 6.1 RIFT-CC (Congestion Control)
A delay-based congestion control algorithm (inspired by BBR and GCC) to:
- Dynamically adjust `initial_bitrate_kbps`.
- Adjust FEC redundancy ratio based on packet loss.
- Signal `target_fps` to the encoder.

### 6.2 Zero-RTT Resumption
Using Noise PSK (Pre-Shared Key) or Session Tickets to eliminate the 3-packet handshake overhead on reconnection.

### 6.3 PMTU Discovery
Dynamic path MTU discovery to find the largest possible datagram size without fragmentation, maximizing throughput.

### 6.4 Zero-Copy Media Framing
Optimizing the `PhysicalPacket` -> `VideoChunk` path to avoid intermediate buffer allocations. This involves using a specialized "Tail-Header" for media packets where Protobuf metadata is appended after the raw NAL units.

### 6.5 Audio Synchronization
Implementation of Opus-based audio channels with Lip-Sync timestamps locked to `frame_id`.

### 6.6 Z-Frame Padding (Pipe Warming)
Optional low-priority "Zero Frames" sent during idle periods to keep NAT mappings active and prevent ISP "sleep" states from inducing first-packet jitter.

### 6.7 Header Bitfields
Optimizing the `PhysicalPacket` header to use bitfields for `Option` flags (e.g., `has_alias`, `is_encrypted`), saving several bytes per packet.

### 6.8 Multi-Link & Mobility
Support for seamless session migration between network interfaces (e.g., Wi-Fi to 5G) using a signed "Session Rebind" message.

### 6.9 Tiled Streaming
Parallelizing high-resolution streams (4K/8K) by splitting frames into tiles, each with independent FEC and timestamps, allowing for distributed decoding.

### 6.10 Hybrid QUIC Support
Exploring the use of standard QUIC as a parallel transport specifically for the **Control** and **Input** channels, leveraging its mature congestion control and reliability, while keeping `RIFT-UDP` for high-throughput Media.
