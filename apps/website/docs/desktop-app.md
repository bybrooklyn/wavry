---
title: Desktop App (Tauri)
description: Development, runtime behavior, and packaging guidance for the Wavry desktop application.
---

Wavry Desktop provides a native application surface for host/client workflows, built with Tauri and Svelte.

## Stack

- Rust backend: `crates/wavry-desktop/src-tauri`
- Frontend: Svelte + TypeScript
- Runtime: Tauri

## Local Development

```bash
cd crates/wavry-desktop
bun install
bun run tauri dev
```

Recommended additional validation:

```bash
bun run check
```

## What the Desktop App Controls

The desktop UI drives runtime config for:

- Connectivity mode (cloud/direct/custom)
- Session start/stop behavior
- Resolution and display target
- Input settings (including gamepad controls)
- Session status and runtime feedback

## Packaging

```bash
cd crates/wavry-desktop
bun run tauri build
```

Typical outputs by platform:

- Windows: executable/installer
- Linux: distro-appropriate bundles
- macOS: app bundle and disk image workflow

## Production Validation Checklist

1. Launch packaged app on a clean environment.
2. Verify app startup without local dev servers.
3. Verify session start/stop behavior and persistence.
4. Verify upgrade path from previous app builds.

## Troubleshooting Quick Notes

If packaged app loads a localhost error, confirm:

- Production build points to `frontendDist`
- `devUrl` is used only during dev mode
- Tauri feature flags/config match Cargo configuration

## Related Docs

- [Getting Started](/docs/getting-started)
- [Troubleshooting](/docs/troubleshooting)
- [Operations](/docs/operations)
