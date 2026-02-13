---
title: Observability and Alerting
description: Metrics, logs, and alert strategies for operating Wavry with confidence.
---

Observability determines whether you catch latency and reliability regressions early.

## Signals to Track

Minimum baseline:

- session setup success/failure
- handshake error rate
- direct vs relay ratio
- p95/p99 latency indicators
- session drop/disconnect rates

Linux host focus:

- portal preflight failures
- PipeWire/session backend failures
- host encoder/capture startup failures

## Log Strategy

Use structured logs and include:

- timestamp with timezone
- component (`gateway`, `relay`, `host`, `client`)
- session/correlation identifiers when available
- clear error category and action context

Do not log:

- secrets/tokens/private key material
- sensitive payload data

## Alert Baseline

Create alerts for:

1. gateway health endpoint failure
2. auth failure surge
3. handshake failure surge
4. relay registration/heartbeat anomalies
5. sudden direct-to-relay ratio shift

Tune thresholds per environment tier.

## Dashboard Baseline

At minimum, maintain dashboards for:

- control-plane health and request volume
- session quality and adaptation trends
- relay utilization and error states
- Linux host runtime health markers

## Release Observability Checklist

Before rollout:

1. baseline current metrics
2. define rollback thresholds
3. enable focused alerting during rollout window

After rollout:

1. compare latency and failure deltas
2. verify no hidden increase in relay usage
3. confirm error distribution stability

## Troubleshooting Correlation Tips

When an issue appears:

1. align client, host, and control-plane timestamps
2. compare before/after deployment windows
3. isolate whether change is network, runtime, or control-plane induced

## Related Docs

- [Operations](/operations)
- [Troubleshooting](/troubleshooting)
- [Runbooks and Checklists](/runbooks-and-checklists)
