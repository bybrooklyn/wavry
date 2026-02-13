# Control Plane Incident Runbooks

**Version:** 0.0.5-unstable  
**Last Updated:** 2026-02-13

This runbook covers incident response and recovery for the Wavry control plane (`wavry-master` + `wavry-relay`).

## Scope

- Master availability and relay registry health
- Relay registration/heartbeat failures
- Restart recovery for master and relay
- Packet-loss and high-latency degradation handling

## Fast Triage

1. Check master health and relay registry:

```bash
curl -sf http://<master-host>:8080/health
curl -sf http://<master-host>:8080/v1/relays
```

2. Check relay health/readiness:

```bash
curl -sf http://<relay-host>:9091/health
curl -sf http://<relay-host>:9091/ready
```

3. Inspect logs for registration/heartbeat failures:

```bash
# master
journalctl -u wavry-master -n 200 --no-pager

# relay
journalctl -u wavry-relay -n 200 --no-pager
```

## Incident Types and Playbooks

### 1) Master Unavailable

Symptoms:
- `GET /health` fails on master
- Relay health shows `registered_with_master=false`

Recovery steps:
1. Restart master.
2. Confirm master `GET /health` returns 200.
3. Wait up to 60s for relay re-registration.
4. Confirm relay appears in `GET /v1/relays` with recent `last_seen_ms_ago`.

Validation:

```bash
curl -sf http://<master-host>:8080/v1/relays | jq
```

### 2) Relay Lost Registration After Master Restart

Symptoms:
- Master restarted successfully, but relay remains missing from relay list
- Relay logs show heartbeat failures (for example HTTP 404)

Recovery steps:
1. Confirm relay is running and reachable on health port.
2. Confirm relay auto re-registration is active (recent "re-registered" log line).
3. If needed, restart relay once.
4. Re-check relay list and readiness.

Validation:

```bash
curl -sf http://<relay-host>:9091/health
curl -sf http://<master-host>:8080/v1/relays
```

### 3) Packet Loss / Control-Plane Connectivity Drops

Symptoms:
- Intermittent heartbeat request failures
- Flapping `registered_with_master` value

Recovery steps:
1. Verify upstream routing/firewall from relay to master.
2. Confirm repeated heartbeat transport errors are not persistent.
3. Relay should auto re-register after repeated failures.
4. If network instability persists, fail traffic over to another relay region.

Validation:
- Heartbeat success recovers.
- Relay returns to `ready=true`.

### 4) High Latency Between Relay and Master

Symptoms:
- Slow register/heartbeat requests
- Readiness intermittently drops under severe delay

Recovery steps:
1. Check current RTT and packet-loss between relay and master networks.
2. Verify control-plane endpoints are not transiting congested links.
3. Scale master regionally or move relay placement closer to selected master.
4. Temporarily reduce churn (drain noisy relays) while latency stabilizes.

Validation:
- Register/heartbeat p95 return under SLO thresholds.

## Soak/Chaos Validation (Pre-Release and CI)

Use the unified resilience gate:

```bash
./scripts/control-plane-resilience.sh
```

It validates:
- relay restart recovery
- master restart recovery
- outage/packet-loss style recovery
- high-latency control-plane behavior
- load/soak success-rate and latency SLOs

Current default local thresholds:
- Success rate: `>= 98%`
- Register p95: `<= 400ms`
- Heartbeat p95: `<= 450ms`

CI uses slightly relaxed thresholds to account for shared runner variance.

## Escalation and Evidence

Collect before escalation:

```bash
# master and relay logs
journalctl -u wavry-master --since "15 minutes ago" > master.log
journalctl -u wavry-relay --since "15 minutes ago" > relay.log

# point-in-time state
curl -sf http://<master-host>:8080/health > master-health.json
curl -sf http://<master-host>:8080/v1/relays > master-relays.json
curl -sf http://<relay-host>:9091/health > relay-health.json
```

Escalate with:
- incident timeline
- command outputs above
- deploy/environment metadata (region, commit SHA, image tag)
