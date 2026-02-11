---
title: Security
description: Encryption model, threats, and secure-by-default operations.
---

## Security Goals

- Mandatory end-to-end encryption.
- Relay infrastructure that cannot inspect session payloads.
- Strong default protections against replay and abuse.

## Encryption Stack

- Handshake: `Noise_XX_25519_ChaChaPoly_BLAKE2s`
- Transport encryption: ChaCha20-Poly1305
- Identity: key-based peer authentication

## Threat Model Themes

- Abuse and resource exhaustion against gateway/relay paths.
- Metadata correlation attempts by network observers.
- Token misuse/replay attempts.

## Operational Controls

- Rate limits at gateway and relay entry points.
- Short-lived session authorization material.
- Audit logging without payload inspection.
- Manual and automated revocation workflows.

## Security Checklist for Operators

1. Keep admin tokens and signing keys out of logs.
2. Enforce TLS and trusted cert chains on internet-facing services.
3. Rotate secrets on a defined schedule.
4. Alert on unusual relay bandwidth and failed auth spikes.
5. Patch gateway/relay binaries on each security release.

## Deep Security Docs

- [WAVRY_SECURITY.md](https://github.com/bybrooklyn/wavry/blob/main/docs/WAVRY_SECURITY.md)
- [WAVRY_SECURITY_GUIDELINES.md](https://github.com/bybrooklyn/wavry/blob/main/docs/WAVRY_SECURITY_GUIDELINES.md)
