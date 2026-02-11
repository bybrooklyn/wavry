---
title: Operations
description: CI/CD guidance, caching strategy, and release packaging expectations.
---

## CI/CD Baseline

Production pipelines should optimize for repeatability and speed.

### Cache What Actually Hurts Build Time

- Rust crates + target metadata (`~/.cargo`, workspace target cache strategy)
- Bun dependency cache (keyed by `bun.lock`)
- Toolchain layers (Rust toolchain, Bun runtime, system packages)

Example approach in GitHub Actions:

```yaml
- uses: dtolnay/rust-toolchain@stable
- uses: Swatinem/rust-cache@v2
  with:
    workspaces: |
      . -> target

- uses: oven-sh/setup-bun@v2
- uses: actions/cache@v4
  with:
    path: |
      ~/.bun/install/cache
      apps/website/node_modules
    key: ${{ runner.os }}-bun-${{ hashFiles('apps/website/bun.lock') }}
```

## Docker Build Reliability

- Pin base images by digest where possible.
- Keep Docker context small and deterministic.
- Move dependency resolution earlier in the Dockerfile so layers are reusable.
- Fail fast on missing toolchain prerequisites.

## Release Artifact Strategy

Only publish artifacts users can install directly.

Recommended desktop distribution outputs:

- Windows: installer and/or signed executable
- Linux: package + checksum
- macOS: signed `.app` bundle packaged as `.dmg` (plus checksums)

Avoid publishing internal-only intermediates unless explicitly needed.

## Release Checklist

1. Build matrix passes for Linux/macOS/Windows.
2. Checksums are generated and verified.
3. macOS bundles are signed/notarized where required.
4. Release notes map artifacts to target platforms.
5. Smoke test each platform artifact on a clean machine.
