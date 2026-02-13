---
title: Security
description: Security model, hardening baseline, and operational controls for production use.
---

Wavry is designed for encryption-first interactive sessions with explicit trust boundaries.

## Security Model Summary

Core principles:

1. media and input payloads remain encrypted end-to-end
2. control plane is separated from encrypted data plane
3. replay and tamper resistance are built into session transport
4. relay should operate as a blind forwarder for encrypted payloads

## Cryptographic Baseline

- handshake: `Noise_XX_25519_ChaChaPoly_BLAKE2s`
- transport encryption: `ChaCha20-Poly1305`
- peer identity: key-based identity model

## Trust Boundaries

Endpoint boundary:

- keys and decrypted payloads must stay on trusted endpoints

Gateway boundary:

- gateway handles auth, signaling, and policy
- treat as internet-facing security surface

Relay boundary:

- relay forwards encrypted packets
- relay should not require decryption capability

## Production Hardening Checklist

### Control Plane

1. terminate TLS at trusted ingress
2. enforce strict auth and admin token controls
3. keep rate limiting enabled (`WAVRY_GLOBAL_RATE_LIMIT*`)
4. trust proxy headers only behind trusted proxy (`WAVRY_TRUST_PROXY_HEADERS=1`)

### Signaling Security

1. use `wss://` for signaling in production
2. avoid insecure signaling override except controlled dev
3. pin signaling cert fingerprints when high assurance is required (`WAVRY_SIGNALING_TLS_PINS_SHA256`)

### Secret Handling

1. keep secrets out of logs and source-controlled files
2. rotate tokens/keys on a defined schedule
3. use scoped credentials with least privilege

### Relay Security

1. do not run insecure relay mode in production
2. set and validate relay master public key
3. monitor relay registration and heartbeat anomalies

## Detection and Monitoring

Alert on:

- auth failure spikes
- handshake failure surges
- abnormal relay usage changes
- unusual admin API access patterns

Log with enough context for incident reconstruction, but avoid sensitive payload disclosure.

## Incident Response

1. identify impacted surface (gateway, relay, endpoint)
2. rotate exposed credentials/tokens
3. isolate abusive traffic/users/ranges
4. patch and redeploy
5. record timeline, blast radius, and preventive controls

## Security Validation Before Release

1. run standard lint/tests and security-relevant checks
2. verify production signaling/TLS posture
3. verify no insecure relay flags in production deployment
4. verify admin access boundaries and auditability

## Deep References

- [WAVRY_SECURITY.md](https://github.com/bybrooklyn/wavry/blob/main/docs/WAVRY_SECURITY.md)
- [WAVRY_SECURITY_GUIDELINES.md](https://github.com/bybrooklyn/wavry/blob/main/docs/WAVRY_SECURITY_GUIDELINES.md)
