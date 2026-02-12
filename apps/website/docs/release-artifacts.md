---
title: Release Artifacts
description: Exactly what files ship in a Wavry release and how to identify each one.
---

Wavry release assets follow a strict naming format so each file is immediately identifiable.

## Naming Format

`<component>-<platform>-<arch>[.<ext>]`

Examples:

- `wavry-gateway-linux-x64`
- `wavry-server-windows-x64.exe`
- `wavry-desktop-tauri-macos-arm64.dmg`
- `wavry-mobile-android-arm64-release.apk`

## What Is Included

### Backend services

- `wavry-master-*` (coordination service)
- `wavry-gateway-*` (auth + control plane API)
- `wavry-relay-*` (traffic relay)
- `wavry-server-*` (host runtime)

### Desktop apps

- `wavry-desktop-tauri-linux-x64`
- `wavry-desktop-tauri-windows-x64.exe`
- `wavry-desktop-tauri-macos-arm64.dmg`
- `wavry-desktop-native-macos-arm64.dmg`

### Android apps

- `wavry-mobile-android-arm64-release.apk`
- `wavry-quest-android-arm64-release.apk`

### Integrity/metadata files

- `SHA256SUMS.txt`
- `RELEASE_ASSETS.md`

## What Is Not Included

- CI intermediate artifacts
- debug binaries
- local-only `dist/` helper files

If required release assets are missing, the CI release job fails before publication.
