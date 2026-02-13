---
title: Wavry Overview
description: What Wavry is, what ships, who should use it, and where to start.
slug: /
---

Wavry is a low-latency remote compute stack for sessions where input responsiveness matters more than perfect visual smoothness.

In practical terms, Wavry is software for running and controlling remote desktop/game/app sessions with encrypted transport, Linux-first runtime behavior, and an operator-owned control plane.

Wavry is engineered with a Linux-first runtime model and aims to provide best-in-class Wayland behavior for interactive workloads.

## What Wavry Includes

Wavry is a full stack, not just a desktop UI:

- `rift-core`: transport protocol, congestion control, FEC
- `rift-crypto`: handshake, identity, replay protection, encryption
- `wavry-server` and `wavry-client`: host/client runtime loops
- `wavry-desktop`: desktop control surface (Linux and Windows Tauri, macOS native Swift release)
- Control plane services (`gateway`, `relay`) distributed as Docker images

Wavry is not a passive video CDN or buffered media playback stack. It is designed for interactive control and low-latency input round-trip.

## What Ships in Releases

Wavry release artifacts are intentionally scoped:

- Desktop apps
- Host/runtime binaries (`wavry-server`, `wavry-master`)
- Android app artifacts
- Checksums and release manifest

Control-plane services are Docker-only:

- `ghcr.io/<owner>/<repo>/gateway:<tag>`
- `ghcr.io/<owner>/<repo>/relay:<tag>`

See [Release Artifacts](/release-artifacts) for exact names.

## 60-Second Session Flow

1. Client and host negotiate via the control plane or direct route.
2. Peers establish encrypted session state.
3. Host captures and encodes frames, sends encrypted packets.
4. Client decrypts, reorders/reconstructs, decodes, and renders.
5. Input flows back to host over encrypted path.
6. Direct route is preferred; relay is fallback-only.

## Who Wavry Is For

Wavry is a fit for:

- Remote desktop where control latency is critical
- Interactive streaming where packet delay stability matters
- Teams that need self-hosting and operational control
- Linux-heavy deployments, including Wayland environments

Wavry is not designed as a long-buffer media playback/CDN product.

## Documentation Paths

Pick a path based on your role:

| Role | Start Here | Then Read |
|---|---|---|
| New evaluator | [Getting Started](/getting-started) | [Architecture](/architecture), [Lifecycle](/lifecycle) |
| Linux operator | [Linux and Wayland Support](/linux-wayland-support) | [Operations](/operations), [Troubleshooting](/troubleshooting) |
| Platform engineer | [Architecture](/architecture) | [Configuration Reference](/configuration-reference), [Security](/security) |
| Infra/DevOps | [Docker Control Plane](/docker-control-plane) | [Operations](/operations), [Runbooks and Checklists](/runbooks-and-checklists) |
| Control-plane owner | [Control Plane Deep Dive](/control-plane-deep-dive) | [Docker Control Plane](/docker-control-plane), [Versioning and Release Policy](/versioning-and-release-policy) |
| Commercial evaluator | [Deployment Modes](/deployment-modes) | [Pricing](/pricing), [FAQ](/faq) |
| New contributor | [Codebase Reference](/codebase-reference) | [Developer Workflows](/developer-workflows), [Internal Design Docs](/internal-design-docs) |

## Licensing Summary

Wavry is open-source under AGPL-3.0 for compliant use.

- AGPL-compatible usage: free
- Commercial/private distribution and certain SaaS/integration models: commercial agreement required

RIFT implementation in this repository is covered by the same open-source licensing model documented for Wavry.

See [Deployment Modes](/deployment-modes), [Pricing](/pricing), and [LICENSE](https://github.com/bybrooklyn/wavry/blob/main/LICENSE).

## Recommended Reading Order

1. [Getting Started](/getting-started)
2. [Docker Control Plane](/docker-control-plane)
3. [Architecture](/architecture)
4. [Session Lifecycle](/lifecycle)
5. [Linux and Wayland Support](/linux-wayland-support)
6. [Linux Production Playbook](/linux-production-playbook)
7. [Security](/security)
8. [Control Plane Deep Dive](/control-plane-deep-dive)
9. [Operations](/operations)
10. [Troubleshooting](/troubleshooting)
