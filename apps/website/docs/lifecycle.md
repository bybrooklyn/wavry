---
title: Session Lifecycle
description: Detailed walkthrough of how Wavry sessions are established, maintained, adapted, and terminated.
---

This document explains the lifecycle of a Wavry session from discovery to teardown.

## Phase 1: Discovery and Signaling

A client needs a reachable path to a host.

Common patterns:

- Direct target address (LAN/static endpoint)
- Gateway-assisted signaling and coordination
- Candidate path evaluation before media start

At this stage, control-plane metadata is exchanged to prepare the connection, but media flow is not active yet.

## Phase 2: Crypto Handshake

Before interactive traffic begins, peers establish session security state.

- Identity material is used to establish trust context
- A handshake derives shared encrypted transport keys
- Replay protections and sequence validation are initialized

Only after this stage should media and input payloads be transmitted.

## Phase 3: Media and Input Pipeline Start

### Host side

1. Capture display/audio sources
2. Encode media frames
3. Packetize payloads into protocol messages
4. Encrypt and send over UDP transport

### Client side

1. Receive encrypted packets
2. Validate/decrypt/reorder
3. Apply correction/recovery strategy (as needed)
4. Decode and present frames

Input follows the reverse direction:

- Client captures input events
- Events are encrypted and sent to host
- Host injects events into local runtime context

## Phase 4: Adaptive Runtime Control

Interactive quality depends on continuous adaptation.

Wavry adjusts runtime behavior using measured network state:

- RTT and delay trends
- Loss behavior
- Jitter and burst conditions

Adjustment actions can include:

- Bitrate increases/decreases
- FEC behavior tuning
- Queue/transport pacing updates

The goal is stable responsiveness, not just maximum bitrate.

## Phase 5: Path Fallback and Recovery

When direct connectivity degrades or fails:

- Session can move to relay-forwarded transport path
- Encrypted payload assumptions remain unchanged
- Runtime adaptation continues based on new path behavior

If quality recovers, routing strategy can be re-evaluated by deployment policy.

## Phase 6: Teardown

A session ends when:

- User disconnects intentionally
- Runtime process exits
- Control-plane policy revokes/invalidates session
- Connection loss exceeds recovery policy

Expected teardown behaviors:

- Stop capture/encode/decode loops
- Release session state and transport resources
- Emit status/log data for operators

## Practical Observability Points

For each lifecycle phase, operators should track:

- Session start/stop timestamps
- Handshake success/failure reasons
- Direct-vs-relay path usage
- Runtime adaptation events (bitrate/FEC state changes)
- User-visible failures (disconnects, prolonged freezes)

## Related Docs

- [Architecture](/docs/architecture)
- [Networking and Relay](/docs/networking-and-relay)
- [Security](/docs/security)
- [Troubleshooting](/docs/troubleshooting)
