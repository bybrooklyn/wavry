---
title: Architecture
description: How Wavry is structured across protocol, crypto, media runtime, and control plane.
---

Wavry is built as a modular stack with explicit trust boundaries between signaling and encrypted media traffic.

## Architectural Layers

| Layer | Components | Purpose |
|---|---|---|
| Protocol + control | `rift-core` | Frame/packet model, DELTA congestion control, FEC, control messages |
| Cryptography | `rift-crypto` | Identity keys, handshake, replay protection, authenticated encryption |
| Session runtime | `wavry-server`, `wavry-client` | Capture/encode/send and receive/decode/render/input loops |
| Control plane | `wavry-gateway` | Session signaling, coordination APIs, operator-facing controls |
| Transport fallback | `wavry-relay` | Blind forwarding for encrypted UDP payloads |
| User surfaces | Desktop/mobile/web apps | UX and platform-specific interaction paths |

## Control Plane vs Data Plane

### Control plane

Used to coordinate session setup and routing decisions:

- Peer signaling
- Session metadata exchange
- Relay allocation when needed

### Data plane

Carries encrypted real-time traffic:

- Media payloads (video/audio)
- Input/control traffic
- Reliability/correction metadata (for low-latency recovery)

Wavry keeps these concerns separate so relay/gateway services do not need decrypted payload access.

## Session Lifecycle

1. **Discovery and signaling**
   - Client resolves host (direct or gateway-assisted).
2. **Crypto handshake**
   - Peers establish shared secrets and authenticated transport state.
3. **Media/input loop start**
   - Host sends encoded media; client sends encrypted input events.
4. **Adaptive runtime control**
   - DELTA adjusts bitrate/FEC based on RTT/loss/jitter.
5. **Path fallback (if required)**
   - Session uses relay only if direct transport is not viable.

## Data Flow (Simplified)

```text
Host capture -> encode -> packetize (RIFT) -> encrypt -> UDP transport
Client receive -> decrypt -> reorder/FEC -> decode -> present
Client input -> encrypt -> control path -> host injection
```

## Latency Strategy

Wavry favors responsiveness by design:

- Keep buffers/queues short
- Prefer dropping stale work over building delay
- Adapt bitrate quickly when congestion appears
- Use FEC/retransmit strategy tuned for interactive deadlines

## Security Boundaries

- Relay forwards encrypted blobs and should not require payload visibility.
- Keys and identity material stay in trusted host/client context.
- Gateway control APIs should be hardened and audited separately.

For deeper security details, see [Security](/docs/security).

## Extension Points

Teams usually extend Wavry in these areas:

- Platform capture/render backends
- Session admission/auth integrations
- Policy controls for routing and relay usage
- Product-specific UI and provisioning workflows

## Detailed Specs

- [WAVRY_ARCHITECTURE.md](https://github.com/bybrooklyn/wavry/blob/main/docs/WAVRY_ARCHITECTURE.md)
- [RIFT_SPEC_V1.md](https://github.com/bybrooklyn/wavry/blob/main/docs/RIFT_SPEC_V1.md)
- [DELTA_CC_SPEC.md](https://github.com/bybrooklyn/wavry/blob/main/docs/DELTA_CC_SPEC.md)
