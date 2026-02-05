# WAVRY: Latency-First Streaming Platform

Wavry is a low-latency remote desktop and media streaming platform built in Rust. It prioritizes direct peer-to-peer connectivity, hardware-accelerated media pipelines, and cryptographic identity.

## Principles

Wavry is designed around several core engineering principles:

1.  **Input Priority**: Input processing occurs on a dedicated high-priority thread to ensure responsiveness independent of video encoding latency.
2.  **Delay-Oriented Congestion Control**: The DELTA algorithm manages bandwidth based on queuing delay trends rather than packet loss, maintaining minimal buffers.
3.  **Cryptographic Identity**: Peer identity is established via Ed25519 keypairs. Authentication uses challenge-response signatures.
4.  **Native Performance**: Media pipelines utilize platform-native APIs (DirectX/Windows Graphics Capture on Windows, ScreenCaptureKit/Metal on macOS) to minimize overhead.
5.  **Direct Connectivity**: Peer-to-peer connectivity is prioritized using STUN for NAT traversal, with encrypted relay fallback only when necessary.

---

## System Architecture

Wavry is a modular system composed of several specialized crates.

```mermaid
graph TD
    Client[Client (macOS/Linux/Windows)]
    Host[Host (Windows/Linux/macOS)]
    Gateway[Wavry Gateway]
    Relay[Blind Relay]
    STUN[STUN Server]

    Client -- "1. Auth & Signal" --> Gateway
    Host -- "1. Register & Signal" --> Gateway
    
    Client -. "2. Discovery" .-> STUN
    Host -. "2. Discovery" .-> STUN
    
    Client -- "3. P2P Media (RIFT)" --> Host
    Host -- "3. P2P Media (RIFT)" --> Client
    
    Client -. "Fallback" .-> Relay
    Relay -. "Fallback" .-> Host
```

### Core Ecosystem

| Crate | Layer | Description |
|:---|:---|:---|
| **`rift-core`** | **Protocol** | Implementation of the RIFT wire format, DELTA congestion control, and FEC. |
| **`rift-crypto`** | **Security** | Noise_XX handshake, ChaCha20-Poly1305 AEAD, and identity management. |
| **`wavry-media`** | **Hardware** | Hardware-accelerated capture and encoding (WGC, Media Foundation, Metal). |
| **`wavry-client`** | **Session** | Client-side session management, signaling, and RTT tracking. |
| **`wavry-desktop`** | **Integration** | Tauri-based host and client application for Windows and Linux. |
| **`wavry-gateway`** | **Signaling** | Real-time signaling gateway for peer coordination and SDP exchange. |
| **`wavry-relay`** | **Transport** | Blind UDP forwarder for encrypted traffic. |

---

## RIFT Protocol

Remote Interactive Frame Transport (RIFT) is a UDP-based protocol designed for high-performance interactive streaming.

### Congestion Control (DELTA)
The Differential Latency Estimation and Tuning Algorithm (DELTA) is a delay-oriented controller:
- **Measurement**: Tracks one-way queuing delay through RTT smoothing.
- **Reaction**: Adjusts bitrate based on slope trends (Rising, Stable, Congested).
- **FEC**: Dynamically adjusts Forward Error Correction redundancy based on network stability.

### Forward Error Correction (FEC)
RIFT employs XOR-based parity groups to recover from packet loss:
- **Host**: Generates parity shards for groups of video/audio packets.
- **Client**: Reconstructs missing packets using the parity payload to avoid retransmission delays.

---

## Security

- **Identity**: Users are identified by Ed25519 public keys.
- **Lease Validation**: Relay access requires a signed PASETO lease token.
- **Encryption**: Mandatory end-to-end encryption using ChaCha20-Poly1305.

---

## Installation and Development

### Prerequisites
- **Rust**: 1.75+ (Stable)
- **Dependencies**: `protobuf-compiler`, `pkg-config`
- **Windows**: Windows 10/11 with DirectX support.
- **macOS**: Xcode 15+ for Metal and ScreenCaptureKit.

### Building
```bash
git clone https://github.com/bybrooklyn/wavry.git
cd wavry
cargo build --release --workspace
```

### Running Infrastructure (Local)
1.  **Gateway**
    ```bash
    cargo run --bin wavry-gateway
    ```
2.  **Relay**
    ```bash
    cargo run --bin wavry-relay -- --master-url http://localhost:8080
    ```

---

## Contributing

Technical contributions are welcome. Please ensure any changes to the protocol are documented and accompanied by relevant unit tests.

---

## License

Wavry is licensed under the **GNU Affero General Public License v3.0 (AGPL-3.0)**. See [`LICENSE`](LICENSE) for details.
