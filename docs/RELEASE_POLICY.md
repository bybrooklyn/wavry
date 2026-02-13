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

- `unstable` (internal only)
  - Allowed in branch/mainline development metadata only.
  - Not allowed for release tags.
  - Not allowed for published prerelease channels.

## Enforcement

- `scripts/set-version.sh` only accepts stable or `-canary` versions.
- `.github/workflows/platform-builds.yml` rejects unsupported prerelease tags.
- `.github/workflows/docker-images.yml` rejects unsupported prerelease tags.
- Release publishing from tags is gated by `scripts/verify-release-checklist.sh`.

## Publishing Rules

- Use stable tags for production releases.
- Use canary tags for prereleases.
- Never create `-unstable` tags.
