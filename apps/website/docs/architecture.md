---
title: Architecture
description: System layout, data flow, and transport strategy.
---

## System Shape

Wavry uses a modular architecture with clear control-plane and data-plane boundaries.

- Control plane: signaling, auth coordination, relay selection.
- Data plane: encrypted media/input traffic over P2P or relay fallback.

## Primary Components

| Layer | Component | Responsibility |
|---|---|---|
| Protocol | `rift-core` | Framing, DELTA control, FEC |
| Crypto | `rift-crypto` | Noise XX + ChaCha20-Poly1305 |
| Session runtime | `wavry-server`, `wavry-client` | Capture/encode/send and receive/decode/input |
| Control plane | `wavry-gateway` | Signaling, operator APIs |
| Transport fallback | `wavry-relay` | Blind forwarding only |
| UX surfaces | Desktop/mobile/web apps | User workflows and platform integration |

## Connection Strategy

1. Attempt direct peer-to-peer connectivity.
2. Use relay only when direct path fails or is blocked.
3. Keep relay blind to decrypted payload content.

## Media/Input Path

```text
Host Capture -> Encode -> Packetize (RIFT) -> Encrypt -> UDP Transport
Client UDP Receive -> Decrypt -> Reorder/FEC -> Decode -> Present
Client Input -> Encrypted Control Path -> Host Injection
```

## Performance Principles

- Keep queues short; drop stale frames over growing latency.
- Keep input processing on high-priority paths.
- Use hardware encode/decode where available.
- Track RTT/jitter continuously and adapt bitrate quickly.

## Deep Specs

- [WAVRY_ARCHITECTURE.md](https://github.com/bybrooklyn/wavry/blob/main/docs/WAVRY_ARCHITECTURE.md)
- [RIFT_SPEC_V1.md](https://github.com/bybrooklyn/wavry/blob/main/docs/RIFT_SPEC_V1.md)
- [DELTA_CC_SPEC.md](https://github.com/bybrooklyn/wavry/blob/main/docs/DELTA_CC_SPEC.md)
