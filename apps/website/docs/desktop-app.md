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

## Linux Runtime Behavior (Wayland and X11)

Wavry desktop treats Linux as a first-class runtime target.

- On Wayland sessions, Wavry enforces native runtime backends (`GDK_BACKEND=wayland`, `WINIT_UNIX_BACKEND=wayland`) and conservative WebKit flags for stability.
- On X11 sessions, Wavry uses the X11 desktop path.
- Host display capture on Wayland uses the portal + PipeWire flow.
- Linux host startup runs preflight checks (capture backend, encoder availability) and aligns encode resolution to the chosen monitor.
- The host card surfaces Linux runtime diagnostics and first-action recommendations directly in the UI.

For full setup and compatibility detail, see [Linux and Wayland Support](/linux-wayland-support).

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

- [Getting Started](/getting-started)
- [Linux and Wayland Support](/linux-wayland-support)
- [Troubleshooting](/troubleshooting)
- [Operations](/operations)
