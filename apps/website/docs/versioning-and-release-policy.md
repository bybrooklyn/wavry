---
title: Versioning and Release Policy
description: Release channel rules, artifact policy, and pre-release tag constraints.
---

This page defines how Wavry versions are interpreted and which versions are allowed to ship publicly.

## Release Channels

Wavry uses three practical channels:

| Channel | Example | Intended use |
|---|---|---|
| Stable | `v0.1.0` | public releases and production pinning |
| Canary | `v0.1.1-canary.1` | public prerelease validation with explicit opt-in |
| Internal unstable | `v0.1.2-unstable` | internal development snapshots only |

## Hard Rule: Public Prerelease Tags Are Canary-Only

For public release tags:

- Allowed prerelease suffix: `-canary` (optionally with `.N` or additional dot suffix segments)
- Disallowed for release tags: `-unstable`, `-alpha`, `-beta`, and other custom prerelease suffixes

This is enforced in version tooling and CI policy gates.

## Internal `-unstable` Versions

`-unstable` may exist in development branches/workspaces for internal iteration.

Policy for `-unstable`:

- do not publish as release tags
- do not treat as public compatibility commitments
- use for short-lived dev/test coordination only

## Release Artifact Policy

Public release assets must be:

1. clearly named and platform/arch labeled
2. minimal (no unnecessary build intermediates)
3. checksummed (`SHA256SUMS.txt`)
4. listed in a machine-readable/structured release manifest

Control-plane services are distributed as Docker images (gateway/relay), not raw release binaries.

## Version Validation in Tooling

`scripts/set-version.sh` enforces:

- stable versions (`X.Y.Z`) or canary prereleases (`X.Y.Z-canary...`)
- rejection of unsupported prerelease formats for release-oriented version updates

CI release policy enforces:

- release generation is skipped or blocked when version policy is not satisfied
- canary prereleases are marked as prerelease artifacts

## Operational Guidance

1. Use stable tags for production rollout pinning.
2. Use canary tags for controlled rollout cohorts.
3. Keep internal unstable versions off public release channels.
4. Document compatibility implications when bumping major/minor protocol behavior.

## Related Docs

- [Release Artifacts](/release-artifacts)
- [Upgrade and Rollback](/upgrade-and-rollback)
- [Docker Control Plane](/docker-control-plane)
- [Operations](/operations)

