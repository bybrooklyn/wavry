---
title: Docker Control Plane
description: Production-oriented guide for running Wavry gateway and relay as Docker-only services.
---

Wavry distributes control-plane services as containers only.

Services:

- `gateway`: auth and control APIs
- `relay`: encrypted UDP relay fallback

Raw release binaries for these services are intentionally not published.

## Quick Start

Start gateway:

```bash
docker compose -f docker/control-plane.compose.yml up -d gateway
```

Optional relay:

```bash
WAVRY_RELAY_MASTER_URL=http://host.docker.internal:8080 \
docker compose -f docker/control-plane.compose.yml --profile relay up -d relay
```

Check status:

```bash
docker compose -f docker/control-plane.compose.yml ps
```

## Images and Tags

Images:

- `ghcr.io/<owner>/<repo>/gateway`
- `ghcr.io/<owner>/<repo>/relay`

Tag policy:

- production: pin explicit release tag (`vX.Y.Z` or `vX.Y.Z-canary...`)
- development: `main` or `latest`

## Environment Variables

Key values in `docker/control-plane.compose.yml`:

| Variable | Purpose | Default |
|---|---|---|
| `WAVRY_CONTROL_PLANE_TAG` | image tag | `latest` |
| `WAVRY_IMAGE_REPO` | image repo base | `ghcr.io/bybrooklyn/wavry` |
| `WAVRY_GATEWAY_PORT` | published gateway port | `3000` |
| `ADMIN_PANEL_TOKEN` | admin API access token | empty |
| `WAVRY_RELAY_PORT` | published relay UDP port | `4000` |
| `WAVRY_RELAY_HEALTH_PORT` | published relay health HTTP port | `9091` |
| `WAVRY_RELAY_MASTER_URL` | relay upstream registration target | `http://host.docker.internal:8080` |
| `WAVRY_RELAY_MASTER_PUBLIC_KEY` | relay signature verification key | empty |
| `WAVRY_RELAY_ALLOW_INSECURE_DEV` | insecure relay mode (dev only) | `1` |

## Production Baseline

Before production rollout:

1. Set `WAVRY_RELAY_MASTER_PUBLIC_KEY`.
2. Set `WAVRY_RELAY_ALLOW_INSECURE_DEV=0`.
3. Set `ADMIN_PANEL_TOKEN` to a high-entropy token.
4. Pin image tags (do not use floating tags).
5. Put gateway behind TLS termination + ingress controls.
6. Keep relay health endpoint private (do not expose publicly unless required).

## Volumes and Persistence

Default volumes:

- gateway: `/var/lib/wavry`
- relay: `/var/lib/wavry-relay`

Recommendation:

- back up gateway persistent state regularly
- treat relay state as less critical but keep for diagnostics continuity

## Upgrade and Rollback

### Upgrade

1. Update `WAVRY_CONTROL_PLANE_TAG`.
2. Pull images:

```bash
docker compose -f docker/control-plane.compose.yml pull
```

3. Deploy updated containers:

```bash
docker compose -f docker/control-plane.compose.yml up -d
```

4. Validate:

```bash
curl -fsS http://127.0.0.1:3000/health
curl -fsS http://127.0.0.1:9091/ready
```

### Rollback

1. Restore previous known-good tag.
2. Re-run `docker compose ... up -d`.
3. Validate health and session creation path.

## Logs and Diagnostics

Tail logs:

```bash
docker compose -f docker/control-plane.compose.yml logs -f gateway
```

```bash
docker compose -f docker/control-plane.compose.yml logs -f relay
```

If relay registration fails:

- verify `WAVRY_RELAY_MASTER_URL`
- verify reachability to upstream master
- verify master public key configuration for secure mode

## Security Notes

- relay should not decrypt media payloads
- never place secrets directly in source-controlled compose files
- use environment injection or secret-management tooling

## Related Docs

- [Getting Started](/getting-started)
- [Configuration Reference](/configuration-reference)
- [Operations](/operations)
- [Troubleshooting](/troubleshooting)
- [Release Artifacts](/release-artifacts)
