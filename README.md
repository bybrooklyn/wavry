# WAVRY: The Latency-First Streaming Platform

[![Rust](https://img.shields.io/badge/language-Rust-orange.svg)](https://www.rust-lang.org/)
[![Protocol](https://img.shields.io/badge/protocol-RIFT--v1.0-blue)](docs/RIFT_SPEC_V1.md)
[![License](https://img.shields.io/badge/license-AGPL--3.0-blue)](LICENSE)

---

## ⚡ The Manifesto

Wavry is not just another remote desktop app. It is an engineering statement that **latency is a feature**.

Existing solutions (Parsec, Moonlight, Sunshine) are excellent, but they are often tied to specific hardware vendors (NVIDIA GameStream) or proprietary ecosystems. Wavry is built from first principles in Rust to provide a fully open-source, vendor-agnostic, and mathematically optimized streaming experience.

We believe:
1.  **Input is Sacred**: A dropped video frame is annoying; a delayed input event is a failure. We process input on a separate, high-priority thread that never waits for video encoding.
2.  **Physics Over Heuristics**: Our congestion control (DELTA) is based on queuing theory and first-derivative latency trends, not arbitrary packet loss thresholds.
3.  **Privacy by Default**: Your identity is your keypair. We do not store passwords. We do not inspect your traffic. Encryption is mandatory and end-to-end.
4.  **Native Performance**: We use Direct Metal/VideoToolbox on macOS and VA-API/PipeWire on Linux. No Electron wrappers for the core engine.
5.  **P2P First**: We aggressively punch holes in NATs (STUN). Your data should flow directly between your devices, not through our servers, whenever physically possible.

---

## 🏗️ System Architecture

Wavry is a distributed system composed of several modular crates.

```mermaid
graph TD
    Client[Client (macOS/Linux)]
    Host[Host (Linux/macOS)]
    Master[Wavry Master]
    Relay[Blind Relay]
    STUN[STUN Server]

    Client -- "1. Auth & Signal" --> Master
    Host -- "1. Register & Signal" --> Master
    
    Client -. "2. Discovery" .-> STUN
    Host -. "2. Discovery" .-> STUN
    
    Client -- "3. P2P Media (RIFT)" --> Host
    Host -- "3. P2P Media (RIFT)" --> Client
    
    Client -. "Fallback" .-> Relay
    Relay -. "Fallback" .-> Host
```

### Core Ecosystem

| Crate | Logic Layer | Description |
|:---|:---|:---|
| **`rift-core`** | **Protocol** | Defines the **RIFT** wire format, packet framing, and **DELTA** congestion control logic. Pure Rust, `no_std` compatible. |
| **`rift-crypto`** | **Security** | Implements the **Noise_XX** handshake and ChaCha20-Poly1305 AEAD. Handles key management. |
| **`wavry-media`** | **Hardware** | Abstraction layer for hardware-accelerated codecs (`VideoToolbox`, `VA-API`, `NVENC`) and audio capture (`PipeWire`, `ScreenCaptureKit`). |
| **`wavry-client`** | **Session** | The "brain" of the client. Manages state, signaling, socket binding, and STUN discovery. |
| **`wavry-ffi`** | **Bridge** | Exposes `wavry-client` functionality to other languages (C, Swift, Kotlin) via a stable C ABI. |
| **`wavry-master`** | **Coordination** | The central authority. Handles Ed25519 identity verification, node discovery, relay registry, and signaling routing. |
| **`wavry-relay`** | **Transport** | A "dumb", blind UDP packet forwarder. Verifies cryptographic leases but cannot decrypt traffic. |
| **`apps/macos`** | **UI/UX** | Native Swift application using `wavry-ffi`. Standard-bearer for the "Wavry Experience." |

---

## 📡 The RIFT Protocol

**Remote Interactive Frame Transport (RIFT)** is our custom UDP protocol, specified in [`docs/RIFT_SPEC_V1.md`](docs/RIFT_SPEC_V1.md).

It is designed to be:
*   **Zero-Copy Friendly**: Headers are designed to be prepended to media buffers without reallocation.
*   **Multiplexed**: Control, Input, and Media flow over a single UDP socket pair (or relayed path).
*   **Secure**: The handshake (Noise XX) creates session keys. Every subsequent packet is authenticated.

### Congestion Control: DELTA
**Differential Latency Estimation and Tuning Algorithm (DELTA)** is our custom congestion controller. Unlike TCP-CUBIC (throughput oriented) or BBR (bandwidth oriented), DELTA is **delay oriented**.

1.  **Measurement**: We measure the one-way queuing delay trend ($\frac{d}{dt} \text{RTT}$).
2.  **Reaction**: If the queue is growing (trend > 0), we throttle encoding bitrate *before* packet loss occurs.
3.  **Result**: We maintain a "standing queue" of near zero, ensuring the lowest possible round-trip time for input.

---

## 🔐 Security & Identity

We use **Ed25519** for all identity operations.

*   **Wavry ID**: A user's public key (e.g., `8f4b...3a1c`).
*   **Authentication**: To log in, the Master sends a random challenge. The client signs it with their private key. No passwords ever leave the device.
*   **Perfect Forward Secrecy**: Each session generates ephemeral keys via the Noise XX handshake. Even if your long-term key is compromised, past sessions cannot be decrypted.

---

## 🚀 Getting Started

### Prerequisites
*   **Rust**: 1.75+ (Stable)
*   **System Deps**: `protobuf-compiler` (for `rift-core`), `pkg-config`
*   **Linux Host**: `libva-dev`, `libpipewire-0.3-dev`, `libclang-dev`
*   **macOS Host**: Xcode 15+ (for Metal/ScreenCaptureKit)

### Quick Build (All Components)
```bash
git clone https://github.com/bybrooklyn/wavry.git
cd wavry
cargo build --release --workspace
```

### Running the Infrastructure (Local Dev)
1.  **Start the Master Server**
    ```bash
    # Runs on localhost:8080
    cargo run --bin wavry-master
    ```

2.  **Start a Relay (Optional)**
    ```bash
    # Runs on UDP 4000, registers with Master
    cargo run --bin wavry-relay -- --master-url http://localhost:8080
    ```

### Running the Client (macOS)
The macOS client is a native Swift app that links against the Rust core.
```bash
./scripts/dev-macos.sh
```
This script builds `wavry-ffi`, generates the header, and opens/runs the Xcode project.

---

## 🤝 Contributing

We welcome contributions that align with our **Manifesto**.

*   **Performance PRs**: Must be accompanied by benchmarks (latency/CPU usage).
*   **Protocol Changes**: Must update `docs/RIFT_SPEC_V1.md`.
*   **Code Style**: We use `rustfmt` and `clippy`. Please run `cargo clippy --workspace` before submitting.

### Project Structure
*   `crates/` - Core Rust libraries and servers.
*   `apps/` - Platform-specific frontends (macOS, Desktop, Android planned).
*   `docs/` - Architecture decisions and specifications.
*   `scripts/` - CI and formatting utilities.

---

## 📜 License

Wavry is open source software licensed under the **GNU Affero General Public License v3.0 (AGPL-3.0)**. 
See [`LICENSE`](LICENSE) for the full text.

*   **You are free to**: Use, modify, and distribute the code.
*   **You must**: Open-source any modifications if you distribute them (including over a network, i.e., SaaS).
