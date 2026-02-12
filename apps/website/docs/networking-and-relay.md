---
title: Networking and Relay
description: Direct path behavior, relay fallback strategy, and network planning guidance.
---

Wavry is designed to prefer direct peer connectivity, with relay as a fallback when direct transport is unavailable.

## Connectivity Model

### Direct path (preferred)

- Lower latency in most environments
- Fewer intermediaries in media path
- Better throughput predictability for interactive workloads

### Relay path (fallback)

- Used when NAT/firewall/network policies block direct path
- Helps preserve session continuity in restrictive environments
- Should remain encrypted end-to-end across relay forwarding

## Why Relay Is a Fallback

Interactive workloads are highly sensitive to latency and jitter.

Using relay by default can add:

- Extra network hop distance
- Additional queuing and contention points
- Higher variability during network stress

Direct-first with relay fallback usually provides better user experience for control-sensitive sessions.

## NAT/Firewall Considerations

When deploying Wavry, validate:

- UDP path availability between peers
- Corporate firewall policies that may block or shape UDP
- Cloud security group/network ACL settings
- Region-level latency between host and expected client populations

## Operational Relay Guidance

Relay planning should include:

- Regional placement close to user/host clusters
- Capacity headroom for burst scenarios
- Rate-limiting and abuse controls at ingress
- Monitoring for anomalous traffic spikes

## Routing Policy Suggestions

1. Attempt direct path first.
2. Fall back to relay on connectivity failure.
3. Keep relay selection region-aware where possible.
4. Track relay usage ratio to detect networking regressions.

## Network Diagnostics to Collect

During incident analysis, capture:

- RTT/jitter/loss trends over time
- Relay-selected vs direct-selected ratio
- Session failure reasons at connect time
- Per-region quality differences

## Security Expectations

- Relay should not need decrypted payload access.
- Session encryption should remain endpoint-anchored.
- Admin and operator controls for relay should be isolated from session key material.

## Related Docs

- [Session Lifecycle](/lifecycle)
- [Security](/security)
- [Operations](/operations)
