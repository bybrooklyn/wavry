---
title: Wavry Overview
description: What Wavry is, who it is for, and how the platform fits together.
---

Wavry is a low-latency remote compute platform for:

- Remote desktop control
- Interactive game streaming
- Cloud-hosted application sessions

At its core, Wavry is a Rust-first protocol and runtime stack that prioritizes responsiveness over static image quality. It is designed for sessions where user input must feel immediate.

## What Wavry Actually Is

Wavry combines:

- A transport protocol (`RIFT`) for real-time media + input
- End-to-end encrypted session security (`Noise XX` + `ChaCha20-Poly1305`)
- A control plane for signaling, coordination, and relay fallback
- Host and client runtimes for capture, encode, send, decode, and input injection

In practice: Wavry helps you run interactive sessions between machines and adapt quickly to real-world network conditions.

## What Wavry Is Not

- Not a static video CDN
- Not a "buffer and play later" streaming stack
- Not a browser-only toy protocol

Wavry is intended for high-interactivity workloads where a delayed click or key press breaks the experience.

## Who Uses It

- Product teams building remote workstation or cloud gaming features
- Operators running private remote infrastructure
- Companies embedding streaming/control into a commercial product

## Core Design Principles

1. Input responsiveness first.
2. Delay stability over maximum visual fidelity.
3. Mandatory encryption by default.
4. P2P-first connectivity with relay fallback.

## High-Level Session Flow

1. Client discovers/signals to host (direct or via control plane).
2. Host and client establish encrypted session keys.
3. Host sends encoded media over RIFT; client sends input events back.
4. Congestion control continuously adjusts bitrate/FEC to keep latency bounded.
5. Relay is only used when direct path is unavailable.

## Major Components

| Component | Responsibility |
|---|---|
| `rift-core` | Packet framing, DELTA congestion control, FEC, control messages |
| `rift-crypto` | Identity, handshake, replay protection, encrypted transport |
| `wavry-server` | Host runtime: capture, encode, stream, input handling |
| `wavry-client` | Client runtime: receive, decode, render, input send |
| `wavry-gateway` | Signaling and session coordination APIs |
| `wavry-relay` | Encrypted UDP forwarding fallback |
| `wavry-desktop` | Desktop UX built with Tauri + Svelte |

## Deployment Options

Wavry supports three operating models:

- Open-source self-hosted (AGPL-3.0)
- Commercial license for closed/private derivative use
- Official hosted control-plane services

See [Deployment Modes](/docs/deployment-modes) for exact usage boundaries.

## Where To Go Next

- [Getting Started](/docs/getting-started): run a full local session
- [Architecture](/docs/architecture): component boundaries and data flow
- [Security](/docs/security): threat model and security posture
- [Operations](/docs/operations): CI/CD, release, and production guidance
