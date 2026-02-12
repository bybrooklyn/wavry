---
title: Security
description: Security posture, threat model assumptions, and operator hardening guidance.
---

Wavry is designed for secure interactive sessions by default, with encryption-first transport and clear trust boundaries.

## Security Goals

1. Keep session payloads encrypted end-to-end.
2. Prevent replay/tampering in transport paths.
3. Limit control-plane abuse and unauthorized session actions.
4. Preserve operator visibility without exposing sensitive content.

## Cryptographic Baseline

- Handshake: `Noise_XX_25519_ChaChaPoly_BLAKE2s`
- Encrypted transport: `ChaCha20-Poly1305`
- Identity/authentication: key-based peer identity

## Threat Model Themes

Wavry designs around practical threats such as:

- Unauthorized session initiation attempts
- Token/session replay
- Control-plane abuse and resource exhaustion
- Network observers attempting metadata correlation

## Trust Boundaries

### Endpoint boundary

- Session keys and decrypted payload handling should stay at trusted endpoints.

### Relay boundary

- Relay should forward encrypted traffic without needing decryption capability.

### Gateway boundary

- Gateway coordinates sessions and policy decisions; it should be hardened like any internet-facing control service.

## Secure Operator Practices

### Authentication and secrets

- Keep signing tokens/keys out of logs.
- Rotate secrets and tokens on a defined cadence.
- Scope credentials to minimal required permissions.

### Network hardening

- Use TLS for internet-facing control-plane traffic.
- Restrict management/admin surfaces by network policy.
- Apply DDoS/rate-limit controls at ingress boundaries.

### Runtime monitoring

Track and alert on:

- Failed auth spikes
- Session setup anomalies
- Relay bandwidth outliers
- Unusual admin API usage

## Incident Response Guidelines

1. Identify affected surfaces (gateway, relay, endpoint).
2. Revoke or rotate potentially exposed credentials.
3. Isolate abusive clients/users/network ranges.
4. Patch and redeploy impacted components.
5. Document timeline and prevention controls.

## Security Validation Checklist

- Encryption enabled in all production environments
- Replay protections active and tested
- Admin access constrained and audited
- Logs reviewed for secret leakage
- Upgrade process defined for security patch releases

## Deep Security References

- [WAVRY_SECURITY.md](https://github.com/bybrooklyn/wavry/blob/main/docs/WAVRY_SECURITY.md)
- [WAVRY_SECURITY_GUIDELINES.md](https://github.com/bybrooklyn/wavry/blob/main/docs/WAVRY_SECURITY_GUIDELINES.md)
