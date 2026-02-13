---
title: Installation and Prerequisites
description: Required toolchains, platform packages, and setup checks before running Wavry.
---

This page covers what must be installed before building or running Wavry.

## Baseline Requirements

- Rust `1.75+`
- `protobuf-compiler`
- `pkg-config`
- Git

Desktop/web docs tooling:

- Bun

Docker control plane:

- Docker Engine
- Docker Compose v2

## Linux Prerequisites

Build/runtime package baseline (common):

- `libgstreamer1.0-dev`
- `libgstreamer-plugins-base1.0-dev`
- `libasound2-dev`
- `libx11-dev`, `libxtst-dev`, `libxrandr-dev`, `libxi-dev`
- `libevdev-dev`, `libudev-dev`

Desktop/Tauri baseline:

- `libgtk-3-dev`
- `libwebkit2gtk-4.1-dev` (or distro equivalent)
- `libsoup-3.0-dev`
- `libayatana-appindicator3-dev`
- `librsvg2-dev`

Wayland host runtime baseline:

- `xdg-desktop-portal`
- desktop backend portal package (GNOME/KDE/GTK as applicable)
- PipeWire and session manager

Validation command:

```bash
./scripts/linux-display-smoke.sh
```

## macOS Prerequisites

- Xcode 15+
- Command Line Tools
- Rust toolchain

Note:

- macOS release desktop artifact is native Swift DMG.
- Tauri macOS release distribution is not used.

## Windows Prerequisites

- Rust stable toolchain
- Visual Studio Build Tools / MSVC toolchain
- platform SDK components needed by Rust crates

## Android Prerequisites (Optional)

- Android SDK + NDK
- Java 17

Convenience script:

```bash
./scripts/dev-android.sh
```

## Website Docs Tooling

```bash
cd apps/website
bun install
bun run build
```

## Quick Validation Checklist

1. `cargo build --workspace --locked` succeeds
2. `cargo clippy --workspace --all-targets -- -D warnings` succeeds
3. `cargo test --workspace --locked` succeeds
4. `cd apps/website && bun run build` succeeds

## Related Docs

- [Getting Started](/getting-started)
- [Linux and Wayland Support](/linux-wayland-support)
- [Docker Control Plane](/docker-control-plane)
- [Configuration Reference](/configuration-reference)
