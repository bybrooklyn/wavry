---
title: Upgrade and Rollback
description: Safe upgrade process for Wavry runtimes and Docker control-plane services.
---

This page describes a low-risk upgrade approach.

## Scope

Upgrade surface usually includes:

- control plane images (`gateway`, `relay`)
- host/client runtime binaries
- desktop app packages

## Pre-Upgrade Checklist

1. capture current version inventory
2. confirm backups for persistent control-plane data
3. define rollback trigger thresholds
4. ensure release artifact checksums are verified
5. validate staging rollout first

## Control Plane Upgrade (Docker)

1. Set target tag:

```bash
export WAVRY_CONTROL_PLANE_TAG=vX.Y.Z
```

2. Pull and deploy:

```bash
docker compose -f docker/control-plane.compose.yml pull
docker compose -f docker/control-plane.compose.yml up -d
```

3. Validate:

```bash
curl -fsS http://127.0.0.1:3000/health
docker compose -f docker/control-plane.compose.yml ps
```

## Runtime Upgrade

1. deploy host/runtime changes to canary set
2. compare session quality metrics before/after
3. expand rollout gradually

## Rollback Process

Trigger rollback when thresholds are exceeded (for example: session failures, latency spikes, handshake regressions).

Rollback steps:

1. restore previous known-good image tag/artifact version
2. redeploy previous version
3. verify health and session setup path
4. keep incident notes for root-cause follow-up

## Post-Upgrade Review

1. confirm stability over agreed observation window
2. compare direct/relay ratio and latency distribution
3. close rollout only after metrics remain within expected bounds

## Related Docs

- [Release Artifacts](/release-artifacts)
- [Docker Control Plane](/docker-control-plane)
- [Operations](/operations)
- [Observability and Alerting](/observability-and-alerting)
