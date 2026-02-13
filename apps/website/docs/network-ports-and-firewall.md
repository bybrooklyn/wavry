---
title: Network Ports and Firewall
description: Port planning, firewall policy, and network troubleshooting guidance for Wavry deployments.
---

This page defines the common network surfaces used by Wavry.

## Port Overview

| Component | Protocol | Default | Purpose |
|---|---|---|---|
| gateway | TCP | auto-allocated host port (`3000` in-container) | auth/signaling/control APIs |
| relay | UDP | auto-allocated host port (`4000` in-container) | encrypted relay forwarding path |
| host runtime (`wavry-server`) | UDP | dynamic/configured | encrypted media + input transport |

## Recommended Firewall Baseline

1. Allow inbound TCP to gateway from trusted client ranges.
2. Allow inbound UDP to relay only if relay fallback is enabled.
3. Restrict admin/control paths to trusted networks.
4. Keep least-privilege egress policies for runtime nodes.

## NAT and Connectivity Notes

Wavry is direct-path first.

If direct connectivity fails, session may fall back to relay.

High relay usage usually indicates:

- restrictive NAT/firewall policy
- invalid candidate addresses
- asymmetric routing constraints

## Validation Commands

Gateway health (discover mapped port first):

```bash
docker compose -f docker/control-plane.compose.yml port gateway 3000
```

Control-plane state:

```bash
docker compose -f docker/control-plane.compose.yml ps
```

Basic UDP listener check (relay host):

```bash
docker compose -f docker/control-plane.compose.yml port relay 4000/udp
```

## Production Recommendations

1. Put gateway behind TLS termination and ingress controls.
2. Use region-aware relay placement for latency-sensitive users.
3. Monitor direct/relay ratio continuously.
4. Alert on sudden relay ratio shifts after network changes.

## Incident Triage for Network Faults

1. confirm control-plane health
2. identify direct vs relay session path
3. verify firewall/NAT changes in incident window
4. rollback recent network policy changes if correlation is strong

## Related Docs

- [Networking and Relay](/networking-and-relay)
- [Troubleshooting](/troubleshooting)
- [Operations](/operations)
