# Wavry Release Checklist

Mark every item complete before creating a release tag.
Tag-triggered publishing is blocked while any unchecked box remains.

## Release Metadata

- [ ] `VERSION` is set to the intended release (stable or `-canary` only).
- [ ] `CHANGELOG.md` includes release notes for this version.
- [ ] Release channel matches policy in `docs/RELEASE_POLICY.md`.

## CI + Test Gates

- [ ] `Quality Gates` workflow is green on the release commit.
- [ ] `Platform Builds` workflow is green on the release commit.
- [ ] `Docker Images` workflow is green on the release commit.
- [ ] Linux Wayland runtime lane completed without regressions.

## Artifact Validation

- [ ] Release assets are explicitly named and policy-compliant.
- [ ] `SHA256SUMS` is generated and verified.
- [ ] `release-manifest.json` is generated and verified.
- [ ] Native macOS DMG artifact is present (or intentionally skipped by policy).

## Control Plane + Operations

- [ ] Gateway and relay container images are published for the target tag.
- [ ] Operational runbooks referenced in release notes are current.
- [ ] Security-sensitive config changes (if any) are documented.

## Sign-off

- [ ] Release owner approval
- [ ] Secondary reviewer approval
