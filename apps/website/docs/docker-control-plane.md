---
title: Docker Control Plane
description: Run Wavry gateway (auth/control plane) and relay as Docker-only services.
---

Wavry distributes the control plane as containers:

- `gateway` (auth/control plane)
- `relay` (UDP fallback relay)

Raw host binaries for these services are no longer part of GitHub release assets.

## Quick Start

From repository root:

```bash
docker compose -f docker/control-plane.compose.yml up -d gateway
```

Enable relay in the same stack:

```bash
WAVRY_RELAY_MASTER_URL=http://host.docker.internal:8080 \
docker compose -f docker/control-plane.compose.yml --profile relay up -d relay
```

For local development relay testing only, `WAVRY_RELAY_ALLOW_INSECURE_DEV=1` is enabled by default in the compose file.

## Image Names and Tags

- `ghcr.io/<owner>/<repo>/gateway`
- `ghcr.io/<owner>/<repo>/relay`

Recommended tag usage:

- Release tags: `vX.Y.Z` (or `vX.Y.Z-canary...`)
- Main branch environments: `latest` or `main`

## Production Notes

1. Set `WAVRY_RELAY_MASTER_PUBLIC_KEY` for relay in production.
2. Persist volumes:
   - Gateway: `/var/lib/wavry`
   - Relay: `/var/lib/wavry-relay`
3. Pin explicit image tags; avoid floating tags in regulated environments.
4. Put gateway behind TLS termination and standard reverse-proxy controls.

## Related Docs

- [Getting Started](/getting-started)
- [Operations](/operations)
- [Security](/security)
- [Release Artifacts](/release-artifacts)
