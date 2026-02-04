# WAVRY
### Ultra-Low Latency Remote Streaming Platform

[![Rust](https://img.shields.io/badge/language-Rust-orange.svg)](https://www.rust-lang.org/)
[![Protocol](https://img.shields.io/badge/protocol-RIFT--v0.1.0-blue)](docs/RIFT_SPEC_V1.md)
[![License](https://img.shields.io/badge/license-AGPL--3.0-blue)](LICENSE)

---

## Project Status

**Wavry is currently in Pre-Alpha.** This is a protocol-first implementation focused on establishing the RIFT baseline.
- **Unstable APIs**: Breaking changes to the wire format and internal APIs are expected.
- **Experimental**: Not production-ready. Use at your own risk.
- **Desktop Shell**: The UI in `apps/wavry-desktop` is a draft and may not be functional.

---

## Overview

Wavry is a high-performance, secure, and extensible remote desktop and gaming streaming platform implemented in Rust. The project is designed to solve the challenges of high-fidelity, low-latency data transmission over unpredictable network environments. By leveraging a custom transport protocol and a modular media pipeline, Wavry aims to approach the responsiveness of local hardware.

The project is built around the **RIFT (Remote Interactive Frame Transport)** protocol, which optimizes for sub-frame latency by minimizing serialization overhead and utilizing advanced jitter-buffering techniques.

---

## Core Features

### High-Performance Transport
The foundation of Wavry is the RIFT protocol (v0.1.0). Unlike generic streaming protocols, RIFT is designed specifically for interactive workloads.
*   **Protobuf Message Layer**: High-efficiency serialization using `prost`, allowing for protocol evolution without breaking binary compatibility.
*   **Dual-Layer Framing**: Physical headers support both 128-bit Session IDs for global uniqueness during handshakes and 32-bit Session Aliases for minimized per-packet overhead during active transport.
*   **Low-Latency UDP**: Direct UDP transmission avoids head-of-line blocking, ensuring late packets do not delay the rendering of more recent data.

### Security and Identity
Security is implemented through the `rift-crypto` crate. Encryption is enabled by default for all sessions.
*   **Noise Protocol Framework**: Utilizes the Noise XX handshake for mutual identity exchange and establishment of ephemeral symmetric keys.
*   **Authenticated Encryption**: All packets are encrypted using ChaCha20-Poly1305. The `--no-encrypt` flag is intended **only** for development and debugging.
*   **Anti-Replay Protection**: Integrated sequence windows prevent replay attacks at the transport level.

### Adaptive Media Pipeline
The `wavry-media` crate provides a unified interface for platform-specific hardware acceleration.
*   **Hardware Acceleration**: Support for Intel Arc (QSV), NVIDIA (NVENC), and macOS (VideoToolbox) targets for zero-copy encoding and decoding.
*   **Intelligent Frame Pacing**: A specialized jitter buffer that prioritizes immediate rendering of current frames over buffering for smoothness.
*   **Error Resilience**: Integrated Forward Error Correction (FEC) using XOR-based grouping to recover from packet loss without retransmission delays.

---

## Non-Goals

To maintain focus on core low-latency streaming, the following are explicitly out of scope:
- **Cloud Relay Service**: Wavry is a peer-to-peer/self-hosted tool, not a centralized relay provider.
- **Browser Clients**: We target native performance; WebRTC/WASM browser clients are not currently planned.
- **Generic Livestreaming**: Wavry is optimized for 1:1 interaction, not 1:N broadcast distribution.

---

## Project Structure

Wavry is architected as a modular Rust workspace:

| Component | Path | Description |
| :------- | :--- | :---------- |
| **Server** | `crates/wavry-server` | Host application for screen capture, audio encoding, and input injection. |
| **Client** | `crates/wavry-client` | End-user application for video decoding, rendering, and input capture. |
| **Protocol** | `crates/rift-core` | Implementation of the RIFT wire format and physical framing. |
| **Crypto** | `crates/rift-crypto` | Secure session establishment and authenticated encryption. |
| **Media** | `crates/wavry-media` | Abstraction layer for hardware-accelerated codecs and rendering. |
| **Desktop** | `apps/wavry-desktop` | Svelte-based desktop shell for session management (Draft). |

---

## Technical Specifications

### The RIFT Protocol
Wavry uses the **RIFT (Remote Interactive Frame Transport)** protocol to manage the logical separation of control, input, and media data.
1.  **Control Channel**: Session negotiation, capability exchange, and statistics reporting.
2.  **Input Channel**: Priority path for HID events (keyboard, mouse, gamepad) for minimal "click-to-photon" latency.
3.  **Media Channel**: High-bandwidth path for encoded video and audio, utilizing loss-tolerant framing.

Refer to the [RIFT Specification](docs/RIFT_SPEC_V1.md) for header formats and checksum details.

### Networking and Discovery
Wavry utilizes mDNS (Multicast DNS) for zero-configuration host discovery within local broadcast domains. 
- **Note**: mDNS discovery will generally not cross routed subnets or VLANs; use the `--connect` flag for cross-network connections.

---

## Getting Started

### System Requirements

*   **Operating System**: Linux (primary), macOS (secondary), Windows (Planned).
*   **Compiler**: Rust 1.75 or later.
*   **Hardware**: A GPU supporting HEVC/H.264 hardware encoding (Intel Arc, NVIDIA 10-series+, or Apple Silicon).

### Installation

Ensure you have the Protobuf compiler installed:

```bash
# macOS
brew install protobuf

# Linux (Debian/Ubuntu)
sudo apt install protobuf-compiler
```

Build the workspace:

```bash
git clone https://github.com/wavry/wavry.git
cd wavry
cargo build --release
```

### Usage Examples

*Note: Flags and arguments are subject to change during pre-alpha.*

#### Hosting a Server
The server requires permissions to capture the screen and inject input events.

```bash
cargo run -p wavry-server -- --name "workstation-01"
```

#### Connecting a Client

```bash
# Connect to a specific host with encryption disabled for debugging
cargo run -p wavry-client -- --connect 10.0.0.5:50051 --no-encrypt
```

---

## Development Roadmap

The Wavry project is under active development. Our planned milestones include:
1.  **RIFT-CC (Planned)**: Advanced delay-based congestion control for dynamic bitrate adjustment.
2.  **Reed-Solomon FEC (Planned)**: Transitioning to Leopard-RS for superior packet loss recovery.
3.  **Zero-Copy Rendering (Planned)**: Direct GPU memory transfers for decoded frames.
4.  **Multi-Link Support (Planned)**: Seamless handover between network interfaces.

---

## License

Wavry is released under the GNU Affero General Public License Version 3.0 (AGPL v3). Detailed information can be found in the [LICENSE](LICENSE) file.
