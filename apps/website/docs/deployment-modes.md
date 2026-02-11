---
title: Deployment Modes
description: Understand OSS, commercial licensing, and hosted services.
---

Wavry supports three usage models.

## At a Glance

| Mode | Best For | Source Obligations | Control Plane |
|---|---|---|---|
| Open source self-hosted | Teams that want full control | AGPL-3.0 obligations apply | Self-hosted |
| Commercial license | Closed-source/private derivatives | Commercial terms | Usually self-hosted or hybrid |
| Official hosted services | Fast onboarding and managed control plane | Service terms apply | Wavry-hosted |

## Open Source (Self-Hosted, AGPL-3.0)

- Full access to code, protocol, and runtime behavior.
- You can modify and self-host freely under AGPL-3.0.
- If you run modified versions as a network service, AGPL obligations apply.

## Commercial License

- Intended for closed-source forks, proprietary embedding, and private modifications.
- Licensing terms are documented in the repository commercial terms file.
- Use when AGPL distribution obligations do not fit your operating model.

Reference: [COMMERCIAL.md](https://github.com/bybrooklyn/wavry/blob/main/COMMERCIAL.md)

## Official Hosted Services

Hosted services can include:

- Authentication
- Matchmaking
- Relay fallback

Policy highlights:

- Personal/non-commercial usage is generally free.
- Commercial usage requires a commercial agreement.
- Relay is fallback-first and capacity-constrained.

Reference: [TERMS.md](https://github.com/bybrooklyn/wavry/blob/main/TERMS.md)

## Choosing a Model

1. Choose OSS self-hosted when compliance and infra ownership are the priority.
2. Choose commercial when you need proprietary product integration.
3. Choose hosted when speed-to-launch is the priority and service constraints are acceptable.
