---
title: Operations
description: Production operations guidance for deploying, observing, and releasing Wavry reliably.
---

This page outlines practical operations guidance for teams running Wavry in production-like environments.

## Operational Priorities

1. Predictable session quality
2. Fast failure detection
3. Repeatable release process
4. Security and compliance alignment

## Deployment Topology (Typical)

A common deployment layout includes:

- Gateway/control-plane services
- Relay services in one or more regions
- Host pools with capture/encode workloads
- Client apps or integrated product frontends

For resilience, separate control-plane and data-plane scaling decisions.

## Observability Baseline

At minimum, collect:

- Session creation/teardown events
- Handshake success/failure rates
- Relay/direct path usage ratios
- RTT/loss/jitter trends
- Bitrate adaptation and quality state transitions

Recommended output targets:

- Structured logs
- Metrics backend with dashboards
- Alerting integrated into on-call workflow

## CI/CD Baseline

### Build speed and reproducibility

- Cache Rust dependencies/toolchain artifacts
- Cache Bun dependencies keyed by lockfile
- Keep build environments deterministic

### Quality gates

- Formatting and lint checks
- Unit/integration test suites
- Platform packaging validation where applicable

## Release Pipeline Checklist

1. `cargo fmt --all`
2. `cargo clippy --workspace --all-targets -- -D warnings`
3. `cargo test --workspace`
4. Desktop/frontend checks where relevant
5. Artifact checksum generation and verification
6. Signed/notarized packaging where required by platform

## Capacity and Reliability Planning

Plan for:

- Burst relay load during network events
- Host pool saturation behavior
- Backpressure and admission strategy
- Regional failover/runbook procedures

## Incident Management Suggestions

- Maintain a severity rubric tied to user impact
- Keep runbooks for connect failures, relay overload, and auth outages
- Capture post-incident action items with owners and deadlines

## Change Management

Before major rollout:

- Test in staging with realistic network conditions
- Validate backward compatibility assumptions
- Roll out with canary phases and rollback plan

## Related Docs

- [Troubleshooting](/docs/troubleshooting)
- [Security](/docs/security)
- [Configuration Reference](/docs/configuration-reference)
