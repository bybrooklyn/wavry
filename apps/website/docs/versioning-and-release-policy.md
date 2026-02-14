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
| Unstable | `v0.1.2-unstable2` | fast-moving prerelease snapshots for internal/external validation |

## Allowed Public Prerelease Tags

For public release tags:

- Allowed prerelease suffixes:
  - `-canary` (optionally with `.N` or additional dot suffix segments)
  - `-unstable` (for example `-unstable`, `-unstable2`, `-unstable.2`)
- Disallowed for release tags: `-alpha`, `-beta`, and other custom prerelease suffixes

This is enforced in version tooling and CI policy gates.

## Release Artifact Policy

Public release assets must be:

1. clearly named and platform/arch labeled
2. minimal (no unnecessary build intermediates)
3. checksummed (`SHA256SUMS.txt`)
4. listed in a machine-readable/structured release manifest

Control-plane services are distributed as Docker images (gateway/relay), not raw release binaries.

## Version Validation in Tooling

`scripts/set-version.sh` enforces:

- stable versions (`X.Y.Z`) or allowed prereleases (`X.Y.Z-canary...`, `X.Y.Z-unstable...`)
- rejection of unsupported prerelease formats for release-oriented version updates

CI release policy enforces:

- release generation is skipped or blocked when version policy is not satisfied
- canary and unstable prereleases are marked as prerelease artifacts

## Operational Guidance

1. Use stable tags for production rollout pinning.
2. Use canary or unstable tags for controlled rollout cohorts.
3. Use unstable tags for high-velocity snapshot releases where policy flexibility is required.
4. Document compatibility implications when bumping major/minor protocol behavior.

## Related Docs

- [Release Artifacts](/release-artifacts)
- [Upgrade and Rollback](/upgrade-and-rollback)
- [Docker Control Plane](/docker-control-plane)
- [Operations](/operations)
