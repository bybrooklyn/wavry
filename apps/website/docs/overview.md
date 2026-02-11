---
title: Wavry Overview
description: Public-facing introduction to Wavry and where to start.
---

Wavry is a latency-first platform for remote desktop, cloud gaming, and interactive streaming.

It is designed around three constraints:

1. Input responsiveness comes before visual perfection.
2. Network adaptation must prioritize delay stability.
3. Encryption must be mandatory, not optional.

## What You Get

- A Rust workspace for protocol, crypto, clients, server runtime, gateway, and relay.
- Desktop, mobile, and platform-specific app surfaces.
- Open-source self-hosting under AGPL-3.0, with commercial and hosted options.

## Documentation Map

- [Getting Started](/docs/getting-started): local setup and first session.
- [Deployment Modes](/docs/deployment-modes): OSS, commercial, and hosted usage model.
- [Architecture](/docs/architecture): protocol and component boundaries.
- [Security](/docs/security): threat model and secure defaults.
- [Operations](/docs/operations): CI/CD, release artifacts, and production runbook.

## Core Components

| Component | Role |
|---|---|
| `rift-core` | RIFT packet format, DELTA congestion control, FEC |
| `rift-crypto` | Noise XX handshake + ChaCha20-Poly1305 transport encryption |
| `wavry-gateway` | Signaling, auth-adjacent coordination, operator APIs |
| `wavry-relay` | Blind encrypted UDP forwarding fallback |
| `wavry-server` / `wavry-client` | Host and client runtime behavior |
| `wavry-desktop` | Tauri desktop distribution |

## Who This Is For

- Teams building low-latency remote experiences.
- Operators running private infrastructure with strict control.
- Companies embedding a streaming core into commercial products.
