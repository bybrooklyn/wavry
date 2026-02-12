---
title: Product Use Cases
description: Practical ways teams use Wavry in production and what each workload needs.
---

This page maps Wavry capabilities to real product scenarios so you can evaluate fit quickly.

## 1. Remote Workstation Access

### Typical users

- Developers working on powerful remote build machines
- Designers using GPU-backed remote desktops
- IT and support teams assisting user machines

### Why Wavry fits

- Input responsiveness is prioritized for pointer and keyboard accuracy
- End-to-end encryption protects remote session payloads
- P2P-first transport reduces avoidable relay latency

### Key implementation concerns

- Access control and user/session authorization model
- Display and audio routing policies per OS
- Operational observability for support teams

## 2. Cloud Gaming and Interactive Apps

### Typical users

- Game streaming startups
- Interactive simulation/training platforms
- Real-time content platforms requiring fast control feedback

### Why Wavry fits

- Congestion control is tuned for interactive latency bounds
- FEC and transport adaptation reduce quality collapse on unstable links
- Host/client runtime can be integrated into product-specific orchestration

### Key implementation concerns

- GPU session scheduling and host density economics
- Region-aware routing and relay placement
- Controller/gamepad input mapping and deadzone tuning

## 3. Embedded Streaming Core in Proprietary Products

### Typical users

- Companies shipping internal remote-control tooling
- ISVs integrating remote capabilities into existing products
- Enterprises that need private modifications and commercial licensing terms

### Why Wavry fits

- Rust-native core is modular and integration-friendly
- Strong protocol/control-plane boundaries simplify extension work
- Commercial path exists for closed/private derivative distribution

### Key implementation concerns

- License model selection (AGPL vs commercial)
- API boundaries and upgrade strategy across releases
- Security and compliance review for internal deployment standards

## 4. Secure Internal Infrastructure Access

### Typical users

- Engineering organizations with strict network controls
- Regulated environments requiring clear trust boundaries

### Why Wavry fits

- Relay is forwarding-focused and does not require payload decryption
- Strong defaults around encrypted transport and replay resistance
- Self-hosting support for full infrastructure ownership

### Key implementation concerns

- Secret management and rotation procedures
- Audit logging and admin controls for gateway operations
- Incident response runbooks for auth and session abuse

## Choosing the Right Starting Architecture

| If your priority is... | Start with... |
|---|---|
| Full control and compliance ownership | Self-hosted OSS stack |
| Private/proprietary product integration | Commercial deployment planning |
| Fastest launch with less ops burden | Hosted control-plane usage |

See [Deployment Modes](/deployment-modes) for the full model comparison.

## Evaluation Checklist

1. Confirm your latency budget and interaction sensitivity.
2. Confirm your license/commercial constraints.
3. Confirm your control-plane ownership requirements.
4. Confirm your required OS/platform support paths.
5. Run a pilot using [Getting Started](/getting-started) before making architecture commitments.
