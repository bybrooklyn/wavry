---
title: FAQ
description: Common questions about what Wavry is, licensing, and deployment.
---

## What is Wavry in one sentence?

Wavry is a low-latency, end-to-end encrypted remote session platform for desktop control, game streaming, and cloud-hosted interactive apps.

## Is Wavry open source?

Yes. The core project is available under AGPL-3.0.

## When do I need a commercial license?

Use a commercial license when AGPL obligations do not fit your operating model, especially for closed-source/private derivative use.

## Can I self-host everything?

Yes. You can self-host gateway, relay, and runtime components.

## What do hosted services provide?

Hosted services may provide:

- Authentication
- Matchmaking/signaling assistance
- Relay fallback

## Is relay always used?

No. Wavry is direct-path first. Relay is a fallback for restrictive network environments.

## Does relay decrypt my media stream?

No. Relay forwards encrypted packets and should not require payload decryption to function.

## Where should I start for evaluation?

1. Read [Overview](/).
2. Follow [Getting Started](/getting-started).
3. Compare [Deployment Modes](/deployment-modes).
4. Review [Security](/security) before internet exposure.
