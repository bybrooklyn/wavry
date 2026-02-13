# Control Plane Hardening (v0.0.4)

This document records the relay/master hardening changes introduced for v0.0.4.

## Goals

1. Tighten lease validation and session lifecycle handling.
2. Improve relay selection safety under degraded conditions.
3. Add explicit readiness and health endpoints for operations.
4. Make signing key rotation practical with key identifiers.

## Master Hardening

### Lease signing and claims

- Added signing key identifier (`kid`) support in lease claims.
- Added relay binding (`rid`) in lease claims to prevent token reuse on wrong relays.
- Added explicit lease metadata timestamps (`iat_rfc3339`, `nbf_rfc3339`, `exp_rfc3339`).
- Added configurable lease TTL via `WAVRY_MASTER_LEASE_TTL_SECS` (clamped to 60..3600).

### Key identity

- Added `WAVRY_MASTER_KEY_ID` override.
- If unset, key id is derived from the active public key.
- `/.well-known/wavry-id` now includes key id metadata.
- Relay register response now returns `master_key_id`.

### Health/readiness

- `/health` now returns structured JSON including:
  - relay counts and assignability
  - signing key metadata
  - lease TTL and uptime
- `/ready` is `200` only when:
  - a provisioned signing key is present
  - at least one relay is assignable

### Relay state safety

- Added `Draining` relay state.
- Relay assignment excludes `Draining`, `Quarantined`, and `Banned` relays.
- Stale relay lifecycle:
  - stale relays are marked `Quarantined`
  - long-stale relays are purged

## Relay Hardening

### Lease validation

Relay now validates the following before accepting `LEASE_PRESENT`:

- session id match and non-nil
- role consistency (`payload.peer_role` vs signed role)
- relay binding (`rid` equals local relay id)
- key id match (`kid` equals expected active key id when provided)
- timestamp windows (`nbf_rfc3339`, `iat_rfc3339`, `exp_rfc3339`)
- max token size bounds

### Session state behavior

- Session peer registration supports same-identity re-registration for NAT rebinding.
- Session lease renew rejects expired sessions explicitly.
- Session cleanup now reports idle vs expired removals.

### Overload and observability

- Added load-shed threshold for new sessions (`--load-shed-threshold-pct`).
- Added relay HTTP endpoints:
  - `/health`
  - `/ready`
  - `/metrics`
- Added detailed packet/drop counters:
  - session-not-found
  - unknown-peer
  - replay drops
  - session-full rejects
  - wrong-relay rejects
  - overload shed counts
  - NAT rebind events

## Container and CI hardening

- Relay and gateway Docker builds now cache `/app/target` to reduce rebuild time.
- Relay image now exposes health endpoint port and defaults to `--health-listen 0.0.0.0:9091`.
- Control-plane compose enables stronger container security defaults:
  - `read_only: true`
  - `cap_drop: [ALL]`
  - `no-new-privileges:true`
  - `tmpfs: /tmp`
- Docker image CI skips unchanged component builds in PRs.

## Validation

- `cargo test -p wavry-master -p wavry-relay`
- `cargo clippy -p wavry-master -p wavry-relay --all-targets -- -D warnings`

