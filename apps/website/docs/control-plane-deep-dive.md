---
title: Control Plane Deep Dive
description: Detailed architecture and operations guide for gateway, master, and relay services.
---

This document explains how Wavry control-plane services work in production, how they fail, and how to operate them safely.

## Scope

Control plane includes:

- `wavry-gateway`: authentication, session APIs, signaling WebSocket, relay-session broker
- `wavry-master`: relay registry, lease issuance, relay selection
- `wavry-relay`: encrypted UDP forwarding fallback path

Data plane includes:

- `wavry-server` and `wavry-client` encrypted media/input transport

Control-plane services should be deployed as Docker services in production. Native release binaries for gateway and relay are intentionally not shipped.

## Architectural Responsibilities

| Service | Primary responsibility | Must never do |
|---|---|---|
| Gateway | user/session auth, signaling rendezvous, relay metadata broker | decrypt media payloads |
| Master | relay registration, health state, signed lease issuance | forward media packets |
| Relay | encrypted UDP packet forwarding between peers | inspect/decrypt media/application payloads |

## Trust Model

1. Client/host authenticate through gateway session/token mechanisms.
2. Master signs relay leases with bounded validity.
3. Relay validates lease claims and forwards encrypted packets only.
4. End-to-end media/input confidentiality is maintained by RIFT crypto layers, not by relay trust.

## Session Setup Sequence

1. Client and host bind/authenticate via gateway signaling.
2. Session metadata and direct-candidate data are exchanged.
3. If direct path is unavailable, master-selected relay lease is issued.
4. Relay validates lease and session identity constraints.
5. Media/input packets flow directly or relay-forwarded.

## Relay Lease Lifecycle

Relay lease lifecycle should be treated as a strict state machine:

1. `Issued`: signed by master with bounded TTL.
2. `Presented`: peer presents lease to relay.
3. `Accepted`: relay verifies signature, key id, relay id, time window.
4. `Active`: relay forwards packets while lease is valid and session remains healthy.
5. `Renewed` or `Expired`: renewal extends valid window; expiration terminates forwarding eligibility.

Operationally important constraints:

- reject future `nbf` claims beyond allowed skew
- reject expired leases immediately
- reject leases bound to wrong relay id
- reject replayed/duplicated lease-present packets

## Failure Modes and Expected Behavior

| Failure mode | Expected behavior | Operator action |
|---|---|---|
| Gateway unhealthy | new session setup fails fast | restore gateway health, preserve DB state |
| Master unhealthy | relay assignment and lease issuance fail | fail over or restore master; verify signing key availability |
| Relay unhealthy | affected sessions degrade/disconnect | drain relay, remove from active selection, replace instance |
| Bad master key rotation | lease rejects spike | verify `kid`, relay public key config, roll forward/rollback key plan |
| High relay load | dropped/rejected sessions increase | scale relay pool, enforce load shedding and rate controls |
| NAT churn/rebind spikes | peer address changes rise | ensure NAT rebinding handling paths remain enabled and tested |

## Scaling Strategy

Control-plane scaling and runtime scaling should be decoupled:

- Scale gateway on auth/API/signaling pressure.
- Scale master on lease/registry pressure.
- Scale relay on UDP forwarding pressure and region coverage.

Recommended minimums:

- multi-instance gateway behind stable ingress
- relay pools segmented by region
- relay state/health tracked continuously by master

## Docker-Only Deployment Policy

Production policy:

1. Deploy gateway/relay via Docker images only.
2. Pin explicit image tags for production.
3. Avoid floating tags in long-lived environments.
4. Keep relay health endpoints private unless required by control tooling.

See [Docker Control Plane](/docker-control-plane) for deployment commands and base environment variables.

## Security Hardening Baseline

1. Keep insecure dev flags disabled in production:
   - `WAVRY_RELAY_ALLOW_INSECURE_DEV=0`
   - avoid insecure runtime overrides
2. Set and rotate strong admin/auth tokens.
3. Restrict public binds unless explicitly intended.
4. Keep signing key material out of repository and image layers.
5. Audit auth failures, rate-limit triggers, and admin actions.

## Observability Baseline

Track at minimum:

- gateway auth success/failure rates
- signaling bind failures and timeouts
- relay register/heartbeat freshness
- lease issue/reject rates by reason
- relay packet forward/drop/rate-limit counters

Alert examples:

- gateway health failure > 2 minutes
- relay registration inventory drops below expected baseline
- lease rejects spike by signature/key-id mismatch
- sudden direct-to-relay ratio collapse after rollout

## Incident Runbook Entry Points

When control plane is degraded:

1. Identify impacted layer: gateway, master, relay, or network perimeter.
2. Stop blast radius first: drain failing relay or isolate failing gateway instance.
3. Restore session setup path before quality tuning.
4. Validate with smoke flows and health endpoints.
5. Record root cause and enforce follow-up actions.

Primary companion docs:

- [Operations](/operations)
- [Runbooks and Checklists](/runbooks-and-checklists)
- [Troubleshooting](/troubleshooting)
- [Runtime and Service Reference](/runtime-and-service-reference)

## Deployment Validation Checklist

Before promoting a control-plane change:

1. `cargo fmt --all`
2. `cargo clippy --workspace --all-targets -- -D warnings`
3. `cargo test --workspace --locked`
4. gateway auth smoke passes
5. master/relay smoke passes
6. relay drain/recover flow verified
7. logs and metrics confirm expected behavior after canary

## Practical Notes

- Keep relay and auth flow documentation close to code changes.
- Keep runbooks executable by operators who are not the original implementers.
- Treat lease/token behavior as release-critical: small mistakes can cause wide session impact.

