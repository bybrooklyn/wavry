---
title: Architecture
description: Layered architecture, trust boundaries, and data/control flow for Wavry.
---

Wavry is structured as a modular stack with clear separation between control and encrypted transport.

## Layer Model

| Layer | Components | Responsibility |
|---|---|---|
| Protocol | `rift-core` | packet model, DELTA congestion control, FEC, control primitives |
| Crypto | `rift-crypto` | identity, handshake, replay protection, authenticated encryption |
| Runtime | `wavry-server`, `wavry-client` | capture/encode/send and receive/decode/render/input loops |
| Control plane | `gateway` service | signaling, auth, routing coordination |
| Transport fallback | `relay` service | blind forwarding for encrypted UDP payloads |
| Product surfaces | desktop/mobile/web apps | user workflows and integration surfaces |

## Control Plane vs Data Plane

Control plane:

- session negotiation
- policy and routing coordination
- auth and admission controls

Data plane:

- encrypted media and input transport
- latency-sensitive adaptation and recovery

Design requirement:

- control-plane services do not require access to decrypted payloads.

## Session Path (Simplified)

```text
Client <-> Gateway (signal/auth)
Client <-> Host (direct encrypted path preferred)
Client <-> Relay <-> Host (fallback encrypted path)
```

## Runtime Pipeline

Host side:

1. capture display/audio
2. encode media
3. packetize and encrypt
4. transmit over UDP

Client side:

1. receive encrypted packets
2. validate/decrypt/reorder/FEC recovery
3. decode and present

Input path:

1. client captures input events
2. encrypts and sends events
3. host injects events

## Adaptation Strategy

Wavry optimizes for responsiveness:

- maintain low standing queue
- adapt bitrate based on delay/loss/jitter trends
- tune correction behavior for interactive workloads
- prefer stable control feel over peak throughput

## Security Boundaries

- endpoint keys remain at host/client
- relay operates on encrypted blobs
- gateway is hardened as internet-facing API surface

See [Security](/security) for deployment controls.

## Linux and Wayland Design Focus

Linux is a first-class runtime target:

- Wayland capture via portal + PipeWire path
- runtime backend defaults tuned for Wayland stability
- dedicated Linux preflight and runtime diagnostics

See [Linux and Wayland Support](/linux-wayland-support).

## Extension Areas

Common extension points:

- platform capture/render backends
- policy/auth integration
- deployment automation and observability
- product-specific UX flows

## Deep Technical References

- [WAVRY_ARCHITECTURE.md](https://github.com/bybrooklyn/wavry/blob/main/docs/WAVRY_ARCHITECTURE.md)
- [RIFT_SPEC_V1.md](https://github.com/bybrooklyn/wavry/blob/main/docs/RIFT_SPEC_V1.md)
- [DELTA_CC_SPEC.md](https://github.com/bybrooklyn/wavry/blob/main/docs/DELTA_CC_SPEC.md)
