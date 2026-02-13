---
title: Runbooks and Checklists
description: Operator checklists for daily health, release, incident response, and Linux validation.
---

Use this page as a practical checklist hub.

## Daily Operations Checklist

1. control-plane containers healthy (`docker compose ... ps`)
2. gateway health endpoint responding
3. no unusual auth or handshake failure spikes
4. direct/relay ratio within expected baseline
5. no unexplained host runtime crash patterns

## Weekly Reliability Checklist

1. review top recurring error signatures
2. validate alert routing and on-call ownership
3. verify backup integrity for gateway persistent data
4. compare latency distribution against prior week
5. confirm Linux preflight remains green on current host image

## Release Checklist

1. `cargo fmt --all`
2. `cargo clippy --workspace --all-targets -- -D warnings`
3. `cargo test --workspace --locked`
4. desktop checks and website docs build
5. verify artifact names and checksums
6. verify Docker image tags/manifests
7. canary deploy and metric comparison before full rollout

## Incident Response Checklist

1. classify affected layer (control plane, runtime, network)
2. stabilize user impact first
3. capture logs and timestamps before restarts when possible
4. apply smallest safe corrective action
5. verify recovery and monitor for regression
6. record post-incident actions with owner and due date

## Linux/Wayland Validation Checklist

1. run `./scripts/linux-display-smoke.sh`
2. verify portal backend availability
3. verify PipeWire health
4. verify runtime backend behavior on KDE and GNOME lanes
5. capture and track failure matrix outcomes

If recurring Linux failures occur, use [Linux Production Playbook](/linux-production-playbook) as the primary escalation document.

## Change Approval Checklist

Before rolling out high-impact changes:

1. rollback plan documented
2. compatibility assumptions verified
3. canary scope and success criteria defined
4. observability and alert thresholds prepared
5. owner assigned for live rollout window

## Relay Drain and Recover Runbook

Drain a relay safely:

1. set relay state to `Draining` using master admin API
2. confirm no new relay assignments are issued for that relay
3. wait until active sessions drop to zero (or maintenance threshold)
4. stop or restart the relay instance
5. return relay to `Active` or `Probation` once health checks pass

Recover a quarantined relay:

1. verify heartbeat freshness and load behavior
2. validate relay key configuration (`WAVRY_RELAY_MASTER_PUBLIC_KEY`)
3. run packet-path smoke checks (lease + forward)
4. set relay state back to `Probation` first, then `Active` after stability window

## Release Channel Checklist

1. confirm target version follows policy (`stable` or `-canary` prerelease for public tags)
2. ensure `-unstable` builds are not pushed as public release tags
3. validate release artifact names are platform/arch labeled
4. verify `SHA256SUMS.txt` and release manifest are present

## Master Signing Key Rotation Runbook

1. provision new signing key material and choose a new `WAVRY_MASTER_KEY_ID`
2. restart master with new key and key id
3. verify `/.well-known/wavry-id` publishes the expected key id
4. verify relay registration response includes `master_key_id`
5. validate new lease issuance and relay acceptance (`/ready` on both master and relay)
6. monitor reject rates (`key id mismatch`, `wrong relay`, `invalid signature`) during rollout window

## Related Docs

- [Operations](/operations)
- [Control Plane Deep Dive](/control-plane-deep-dive)
- [Troubleshooting](/troubleshooting)
- [Linux and Wayland Support](/linux-wayland-support)
- [Linux Production Playbook](/linux-production-playbook)
- [Docker Control Plane](/docker-control-plane)
