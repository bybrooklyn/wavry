---
title: Release Artifacts
description: Human-readable catalog for every file in a Wavry release, with clear labels and install intent.
---

This page is the canonical release file catalog.

If you are looking at a GitHub release and unsure what to download, start here.

## Quick Download Guide

| If You Need | Download This File | Why |
|---|---|---|
| Linux desktop (portable) | `wavry-desktop-tauri-linux-x64.AppImage` | Runs without system package install |
| Linux desktop (Debian/Ubuntu) | `wavry-desktop-tauri-linux-x64.deb` | Native package install via APT/dpkg |
| Linux desktop (Fedora/RHEL) | `wavry-desktop-tauri-linux-x64.rpm` | Native package install via DNF/RPM |
| Windows desktop | `wavry-desktop-tauri-windows-x64.exe` | Desktop app executable |
| macOS desktop (Native Swift) | `wavry-desktop-native-macos-arm64.dmg` | Native Swift app DMG package |
| Gateway auth/control plane (Docker) | `ghcr.io/<owner>/<repo>/gateway:<tag>` | Control plane service container |
| Relay service (Docker) | `ghcr.io/<owner>/<repo>/relay:<tag>` | UDP relay service container |
| Host runtime service | `wavry-server-<platform>-<arch>[.exe]` | Host runtime for capture/stream |
| Android mobile app | `wavry-mobile-android-arm64-release.apk` | Mobile Android client |
| Android Quest app | `wavry-quest-android-arm64-release.apk` | Quest client |

## Naming Rules

Every release file uses:

`<component>-<variant>-<platform>-<arch>[.<ext>]`

Examples:

- `wavry-server-windows-x64.exe`
- `wavry-desktop-tauri-linux-x64.deb`
- `wavry-desktop-native-macos-arm64.dmg`

## Full Asset Catalog

| File Pattern | Category | Platform | Architecture | Purpose |
|---|---|---|---|---|
| `wavry-master-<platform>-<arch>[.exe]` | Backend Service | Linux/macOS/Windows | x64/arm64 | Master coordination service binary |
| `wavry-server-<platform>-<arch>[.exe]` | Backend Service | Linux/macOS/Windows | x64/arm64 | Host runtime service |
| `wavry-desktop-tauri-linux-x64.AppImage` | Desktop App | Linux | x64 | Portable Linux desktop package |
| `wavry-desktop-tauri-linux-x64.deb` | Desktop App | Linux | x64 | Debian/Ubuntu package |
| `wavry-desktop-tauri-linux-x64.rpm` | Desktop App | Linux | x64 | Fedora/RHEL package |
| `wavry-desktop-tauri-windows-x64.exe` | Desktop App | Windows | x64 | Windows desktop executable |
| `wavry-desktop-native-macos-arm64.dmg` | Desktop App | macOS | arm64 | Native Swift desktop DMG |
| `wavry-mobile-android-arm64-release.apk` | Android App | Android | arm64 | Mobile client APK |
| `wavry-quest-android-arm64-release.apk` | Android App | Android (Quest) | arm64 | Quest client APK |
| `SHA256SUMS` | Integrity | All | n/a | SHA-256 checksums for all shipped files |
| `release-manifest.json` | Metadata | All | n/a | Machine-readable file/platform/arch/checksum manifest |
| `release-signatures/<artifact>.sig` | Signature | All | n/a | Sigstore signature for each shipped artifact |
| `release-signatures/<artifact>.pem` | Signature | All | n/a | Sigstore signing certificate for each artifact |

## Docker-Only Control Plane Components

Wavry distributes gateway and relay as container images, not release binaries:

- `ghcr.io/<owner>/<repo>/gateway:<tag>`
- `ghcr.io/<owner>/<repo>/relay:<tag>`

Tag guidance:

- Use `vX.Y.Z` (or `vX.Y.Z-canary...` / `vX.Y.Z-unstable...`) for release-pinned deployments
- Use `main`/`latest` only for fast-moving development environments

## Linux Package Install Notes

### AppImage

```bash
chmod +x wavry-desktop-tauri-linux-x64.AppImage
./wavry-desktop-tauri-linux-x64.AppImage
```

### Debian/Ubuntu

```bash
sudo dpkg -i wavry-desktop-tauri-linux-x64.deb
sudo apt-get install -f
```

### Fedora/RHEL

```bash
sudo dnf install ./wavry-desktop-tauri-linux-x64.rpm
```

## What Should Never Be in a Release

- CI intermediary directories
- `target/` tree dumps
- Debug binaries
- Build cache content
- Temporary helper files

## Integrity Verification

```bash
sha256sum -c SHA256SUMS
```

If any file does not verify, discard the artifact and redownload.

## Signature Verification (Sigstore)

Each release artifact is signed with keyless Sigstore (`cosign`) and shipped with:

- `release-signatures/<artifact>.sig`
- `release-signatures/<artifact>.pem`

Example verification:

```bash
cosign verify-blob \
  --signature release-signatures/wavry-server-linux-x64.sig \
  --certificate release-signatures/wavry-server-linux-x64.pem \
  wavry-server-linux-x64
```

## Related Docs

- [Getting Started](/getting-started)
- [Desktop App](/desktop-app)
- [Versioning and Release Policy](/versioning-and-release-policy)
- [Docker Control Plane](/docker-control-plane)
- [Linux and Wayland Support](/linux-wayland-support)
- [Operations](/operations)
