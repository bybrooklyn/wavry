# Wavry Release Policy

This policy defines version channels and allowed release tags.

## Channels

- `stable`
  - Version format: `X.Y.Z`
  - Tag format: `vX.Y.Z`
  - Intended for public production use.

- `canary`
  - Version format: `X.Y.Z-canary` or `X.Y.Z-canary.N`
  - Tag format: `vX.Y.Z-canary` or `vX.Y.Z-canary.N`
  - Intended for prerelease validation and early adopters.

- `unstable`
  - Version format: `X.Y.Z-unstable` or `X.Y.Z-unstableN`
  - Tag format: `vX.Y.Z-unstable` or `vX.Y.Z-unstableN`
  - Intended for fast-moving prerelease snapshots and internal/external validation.

## Enforcement

- `scripts/set-version.sh` accepts stable, `-canary`, and `-unstable` versions.
- `.github/workflows/platform-builds.yml` rejects unsupported prerelease tags.
- `.github/workflows/docker-images.yml` rejects unsupported prerelease tags.
- Release publishing from tags is gated by `scripts/verify-release-checklist.sh`.

## Publishing Rules

- Use stable tags for production releases.
- Use canary or unstable tags for prereleases.
