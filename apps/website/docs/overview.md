---
title: Wavry Overview
description: What Wavry is, who it is for, how it works, and how licensing works.
slug: /
---

Wavry is a low-latency remote compute platform built for sessions where responsiveness matters more than static image quality.

It is designed for:

- Remote desktop control where input delay is unacceptable
- Interactive game and media streaming with tight latency budgets
- Cloud-hosted application sessions that still need local-feeling control

## What Wavry Actually Is

Wavry combines protocol, cryptography, runtimes, and operations guidance into one stack:

- `RIFT` transport for real-time media and input traffic
- End-to-end encryption (`Noise XX` + `ChaCha20-Poly1305`)
- Control-plane services for signaling and relay fallback
- Host/client runtimes for capture, encode, send, decode, render, and input injection

Wavry is not just a UI app. It is an implementation stack for interactive remote sessions.

## Why It Exists

Typical streaming stacks optimize for throughput and video smoothness first. Wavry is optimized for interaction quality first.

Key design choices:

1. Keep input loop delay bounded.
2. Favor delay stability over perfect visual quality.
3. Adapt quickly to congestion changes.
4. Encrypt by default and separate control-plane from encrypted data-plane traffic.

## What Wavry Is Not

Wavry is not intended to be:

- A static video CDN
- A long-buffer media playback system
- A browser-only toy protocol with no production operations model

## How a Session Works (High-Level)

1. Client discovers/signals to a host directly or through the gateway.
2. Host and client negotiate encrypted session keys.
3. Host sends encoded media over RIFT while client sends input events back.
4. Runtime adapts bitrate/FEC based on RTT, loss, and jitter.
5. Relay is used only when direct transport cannot be established.

## Core Components

| Component | Responsibility |
|---|---|
| `rift-core` | Packet model, DELTA congestion control, FEC, control messages |
| `rift-crypto` | Identity, handshake, replay protection, authenticated encryption |
| `wavry-server` | Host runtime: capture, encode, stream, input handling |
| `wavry-client` | Client runtime: receive, decode, render, input send |
| `wavry-gateway` | Signaling and coordination APIs |
| `wavry-relay` | Encrypted UDP relay fallback |
| `wavry-desktop` | Desktop UX and operator controls |

## Licensing and Commercial Use

Wavry’s open-source core, including RIFT implementation, is released under **AGPL-3.0**.

- If your use follows AGPL requirements, you can use Wavry for free.
- If you need exclusion from AGPL obligations for commercial/private derivative usage, use commercial licensing.
- If you want to run Wavry as a SaaS service or deeply integrate it into a commercial platform, direct contact is required.

Read details in [Pricing](/pricing) and [Deployment Modes](/deployment-modes).

## Evaluation Path

For a practical technical evaluation, follow this order:

1. [Getting Started](/getting-started) for an end-to-end local run.
2. [Architecture](/architecture) and [Lifecycle](/lifecycle) for implementation boundaries.
3. [Networking and Relay](/networking-and-relay) for path behavior.
4. [Security](/security) before internet-facing deployments.
5. [Operations](/operations) and [Troubleshooting](/troubleshooting) for production readiness.

## Recommended Next Reads

- [Product Use Cases](/product-use-cases)
- [Configuration Reference](/configuration-reference)
- [Desktop App](/desktop-app)
- [FAQ](/faq)
