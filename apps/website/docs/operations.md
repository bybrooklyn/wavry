---
title: Operations
description: Production operations runbook for reliability, performance, and release control.
---

This page is the practical operator baseline for running Wavry in production-like environments.

## Operational Objectives

1. Keep interactive latency stable.
2. Detect regressions quickly.
3. Deploy safely with fast rollback.
4. Maintain secure control-plane posture.

## Deployment Model

Typical topology:

- Docker-only control plane (`gateway`, `relay`)
- host runtime pools (`wavry-server`)
- user-facing clients (desktop/mobile/web integrations)

Use separate scaling strategies for:

- control plane (API and registration behavior)
- data plane (session/media load)

## Reliability Baseline (SLIs)

Track these minimum service indicators:

- session setup success rate
- handshake failure rate
- direct vs relay ratio
- p95/p99 input-to-present latency
- session drop rate

Set SLO targets per environment tier (dev/stage/prod).

## Monitoring Baseline

Collect at minimum:

- control-plane health and request rates
- auth failures and rate-limit triggers
- relay registration/heartbeat health
- host runtime CPU/GPU pressure
- RTT/loss/jitter trend series

Alert examples:

- gateway health endpoint failing for > 2 minutes
- relay registrations drop below expected region baseline
- handshake failures spike above normal envelope
- direct-path ratio sharply drops after rollout

## Control-Plane Operating Discipline

For gateway/master/relay operations:

1. Keep lease/signing key status visible in dashboards.
2. Track relay state distribution (`Active`, `Draining`, `Probation`) over time.
3. Alert on relay rejection reason spikes (signature mismatch, wrong relay id, expired lease).
4. Keep an explicit drain-and-recover runbook for unstable relays.
5. Treat gateway auth/signal errors as user-impacting indicators.

## Runbook: Daily

1. Confirm control-plane container health.
2. Review top error classes in gateway and relay logs.
3. Check direct/relay ratio trend for anomalies.
4. Verify no unexplained session failure burst.

## Runbook: Release Day

1. `cargo fmt --all`
2. `cargo clippy --workspace --all-targets -- -D warnings`
3. `cargo test --workspace --locked`
4. Desktop checks (`bun run check`)
5. Website docs build (`bun run build`)
6. Validate release artifact naming and checksums
7. Confirm docker image tags and manifests

## Linux Fleet Operations

For Linux host fleets (especially Wayland):

1. Pin compositor and portal backend package versions.
2. Pin required GStreamer plugin set.
3. Run `./scripts/linux-display-smoke.sh` on candidate images.
4. Keep at least one KDE Wayland lane and one GNOME Wayland lane in verification.
5. Track portal and PipeWire failure rates as first-class signals.

## Change Management

For non-trivial changes:

1. stage in representative network conditions
2. deploy to canary cohort
3. compare pre/post latency and failure metrics
4. expand rollout only if indicators remain within target bounds

Always define rollback threshold before rollout begins.

## Capacity Planning

Plan capacity for:

- peak concurrent sessions
- relay burst conditions
- host CPU/GPU saturation behavior
- region failover traffic shifts

Keep headroom and avoid running near steady-state capacity ceilings.

## Incident Response Flow

1. Classify impact (control-plane, data-plane, client runtime).
2. Stabilize by limiting blast radius.
3. Restore availability first, then optimize quality.
4. Capture root cause with precise timeline.
5. Assign follow-up fixes with owners and due dates.

## Monthly Reliability Drills

Run at least one rehearsal per month:

1. relay drain/recover drill in non-production
2. master restart and readiness validation
3. gateway restart and auth/signal continuity validation
4. rollback drill using previous known-good release tags/images

## Backup and Recovery

Minimum recommendations:

- backup gateway persistent state on a schedule
- keep versioned environment configuration
- validate restore path in staging periodically

## Related Docs

- [Docker Control Plane](/docker-control-plane)
- [Control Plane Deep Dive](/control-plane-deep-dive)
- [Security](/security)
- [Troubleshooting](/troubleshooting)
- [Runbooks and Checklists](/runbooks-and-checklists)
- [Linux and Wayland Support](/linux-wayland-support)
