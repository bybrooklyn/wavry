---
title: Desktop App (Tauri)
description: Build, run, and package the desktop app for Linux, Windows, and macOS.
---

## Stack

- Rust backend (`crates/wavry-desktop/src-tauri`)
- Frontend build with Bun
- Tauri runtime for native distribution

## Local Development

```bash
cd crates/wavry-desktop
bun install
bun run tauri dev
```

## Packaging

```bash
cd crates/wavry-desktop
bun run tauri build
```

Expected packaging targets by platform:

- Windows: `.exe` and/or installer bundle
- Linux: distro-friendly binary formats
- macOS: signed app bundle and `.dmg` for user distribution

## Windows Localhost Error Troubleshooting

If a packaged build opens and shows `localhost cannot be found`, verify:

1. Production config points to built static assets (`frontendDist`), not a dev server URL.
2. `devUrl` is only used for `tauri dev`.
3. You are not enabling undeclared Cargo features (for example `custom-protocol`) unless that feature exists in `Cargo.toml`.

Typical Tauri config shape:

```json
{
  "build": {
    "beforeDevCommand": "bun run dev",
    "devUrl": "http://localhost:1420",
    "beforeBuildCommand": "bun run build",
    "frontendDist": "../dist"
  }
}
```

## Release Validation

- Launch packaged app on a clean VM.
- Confirm the app starts without local dev services.
- Confirm upgrade path and settings persistence.
