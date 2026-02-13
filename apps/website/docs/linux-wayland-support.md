---
title: Linux and Wayland Support
description: Deep technical guide for Linux prerequisites, Wayland-first capture/audio, compatibility, and production operations.
---

Wavry is built to deliver **the best Wayland support on the market** for latency-first remote sessions.

This guide documents exactly how Linux support works today, what is required on host systems, and how to validate production readiness.

## Linux Support Scope

Wavry Linux support is designed around:

1. Native Wayland capture through the XDG Desktop Portal + PipeWire path.
2. X11 capture fallback for classic X11 sessions.
3. GStreamer-based encode/decode pipelines with hardware acceleration when available.
4. Practical operations tooling so Linux deployments are observable and repeatable.

## Runtime Architecture on Linux

For host capture, Wavry chooses a capture path by session type:

| Session Type | Primary Video Path | Audio Path | Notes |
|---|---|---|---|
| Wayland | `xdg-desktop-portal` screencast -> `pipewiresrc` | Portal/PipeWire first, PulseAudio fallback where needed | Native Wayland path, no X11 dependency required |
| X11 | `ximagesrc` (with optional monitor crop) | PulseAudio/auto source | Legacy path for non-Wayland sessions |

For desktop UI rendering, Wavry enforces Wayland runtime defaults on Wayland sessions:

- `GDK_BACKEND=wayland`
- `WINIT_UNIX_BACKEND=wayland`
- `WEBKIT_DISABLE_DMABUF_RENDERER=1`
- `WEBKIT_DISABLE_COMPOSITING_MODE=1`

This avoids mixed GTK/Winit backend behavior and reduces compositor protocol errors on KDE Plasma Wayland.

Host startup on Linux now performs runtime preflight before capture starts:

- Verifies required Linux capture backend/plugin availability.
- Verifies at least one H264 encoder path is available.
- Resolves capture resolution from the selected monitor instead of forcing a fixed 1080p default.
- Exposes diagnostics/preflight commands (`linux_runtime_health`, `linux_host_preflight`) for desktop UX and support tooling.

## Prerequisites by Distribution

### Ubuntu / Debian

```bash
sudo apt-get update
sudo apt-get install -y \
  pkg-config protobuf-compiler \
  libgstreamer1.0-dev libgstreamer-plugins-base1.0-dev \
  libasound2-dev libx11-dev libxtst-dev libxrandr-dev libxi-dev libevdev-dev libudev-dev \
  libgtk-3-dev libwebkit2gtk-4.1-dev libsoup-3.0-dev libayatana-appindicator3-dev librsvg2-dev libgl-dev \
  pipewire pipewire-pulse xdg-desktop-portal
```

Install your desktop-specific portal backend:

- GNOME: `xdg-desktop-portal-gnome`
- KDE Plasma: `xdg-desktop-portal-kde`
- wlroots compositors: `xdg-desktop-portal-wlr`

### Fedora

```bash
sudo dnf install -y \
  pkg-config protobuf-compiler \
  gstreamer1-devel gstreamer1-plugins-base-devel \
  alsa-lib-devel libX11-devel libXtst-devel libXrandr-devel libXi-devel libevdev-devel systemd-devel \
  gtk3-devel webkit2gtk4.1-devel libsoup3-devel libappindicator-gtk3-devel librsvg2-devel mesa-libGL-devel \
  pipewire pipewire-pulseaudio xdg-desktop-portal
```

Then add the matching portal backend package for your desktop environment.

### Arch Linux

```bash
sudo pacman -S --needed \
  base-devel pkgconf protobuf \
  gst-plugins-base-libs gstreamer \
  alsa-lib libx11 libxtst libxrandr libxi libevdev libudev0-shim \
  gtk3 webkit2gtk libsoup3 libappindicator-gtk3 librsvg mesa \
  pipewire pipewire-pulse xdg-desktop-portal
```

Add one backend package:

- `xdg-desktop-portal-gnome`
- `xdg-desktop-portal-kde`
- `xdg-desktop-portal-wlr`

## Distro Runbooks (Operations)

Use these when Linux hosts fail capture/startup and you need deterministic recovery.

### Ubuntu / Debian runbook

1. Verify portal + PipeWire processes:

```bash
pgrep -a xdg-desktop-portal || true
pgrep -a xdg-desktop-portal-gnome || true
pgrep -a xdg-desktop-portal-kde || true
pgrep -a xdg-desktop-portal-gtk || true
pgrep -a pipewire || true
```

2. Verify required GStreamer components:

```bash
gst-inspect-1.0 pipewiresrc
gst-inspect-1.0 x264enc
gst-inspect-1.0 opusenc
```

3. Run Wavry preflight:

```bash
./scripts/linux-display-smoke.sh
```

### Fedora runbook

1. Verify portal backend package selection:

```bash
rpm -qa | rg "xdg-desktop-portal|pipewire|gstreamer1"
```

2. Verify active user services:

```bash
systemctl --user status xdg-desktop-portal.service --no-pager
systemctl --user status xdg-desktop-portal-gnome.service --no-pager || true
systemctl --user status xdg-desktop-portal-kde.service --no-pager || true
systemctl --user status xdg-desktop-portal-wlr.service --no-pager || true
```

3. Run Wavry preflight:

```bash
./scripts/linux-display-smoke.sh
```

### Arch Linux runbook

1. Verify portal descriptor files:

```bash
ls /usr/share/xdg-desktop-portal/portals
```

2. Verify session environment:

```bash
echo "XDG_SESSION_TYPE=$XDG_SESSION_TYPE"
echo "XDG_CURRENT_DESKTOP=$XDG_CURRENT_DESKTOP"
echo "WAYLAND_DISPLAY=$WAYLAND_DISPLAY"
```

3. Run Wavry preflight:

```bash
./scripts/linux-display-smoke.sh
```

## Session and Compositor Compatibility

| Platform Stack | Status | Notes |
|---|---|---|
| KDE Plasma + Wayland | First-class | Native portal/PipeWire capture path |
| GNOME + Wayland | First-class | Native portal/PipeWire capture path |
| Sway/Hyprland/wlroots + Wayland | Supported with correct portal backend | Requires `xdg-desktop-portal-wlr` and PipeWire |
| X11 desktops | Supported | Uses X11 capture pipeline |

## Validation Workflow

### 1. Build-time checks

```bash
cargo check -p wavry-media
cargo check -p wavry-desktop
```

### 2. Linux display smoke test

```bash
./scripts/linux-display-smoke.sh
```

This script validates command availability, key GStreamer elements, expected portal backend descriptors, portal process health, and session context before runtime testing.

### 3. Runtime confirmation

```bash
cd crates/wavry-desktop
RUST_LOG=info bun run tauri dev
```

For Wayland sessions, confirm logs include native stream selection and no protocol-dispatch errors.
The app also logs Linux runtime diagnostics on startup (session type, backend choice, missing plugin hints).

## Troubleshooting Checklist (Wayland)

If capture or startup fails on Wayland:

1. Confirm `xdg-desktop-portal` is running in the user session.
2. Confirm the desktop-specific portal backend package is installed and active.
3. Confirm PipeWire is running and healthy.
4. Confirm capture permission prompt was granted by the portal.
5. Re-run `./scripts/linux-display-smoke.sh` and fix all failing checks.

If the app reports that Wayland portal probing failed, treat this as a host configuration issue first (portal backend, permissions, or PipeWire state).

## Failure Matrix

| Symptom | Likely Cause | Where to Confirm | Corrective Action |
|---|---|---|---|
| `Gdk-Message ... Error 71 (Protocol Error) dispatching to wayland display` | Mixed GTK/WebKit backend mode | Desktop startup logs | Ensure Wavry runtime env overrides are active; relaunch app |
| `Wayland session detected but portal monitor probe failed` | Missing/inactive portal backend | `linux_runtime_health` + `linux-display-smoke.sh` | Install correct backend package and restart portal services |
| No monitors in host card | Portal permission denied or stale | Desktop logs + portal prompt history | Re-grant screencast permission, then refresh monitors |
| Host start fails with missing `pipewiresrc` | Missing GStreamer plugin package | `gst-inspect-1.0 pipewiresrc` | Install PipeWire GStreamer plugin set |
| Host start fails with no H264 encoder | Missing encoder plugin | `gst-inspect-1.0 x264enc` (or VAAPI/NVENC plugin) | Install software/hardware encoder plugin package |
| Linux package installs but app fails to launch | Missing runtime libs on target distro | `ldd` / distro package manager logs | Install required WebKitGTK/GTK/GStreamer runtime deps |

## Production Guidance for Linux Hosts

1. Pin compositor and portal backend versions in production images.
2. Track and alert on portal failures and relay/direct ratio changes.
3. Keep GStreamer plugin sets consistent across host fleets.
4. Validate every release on at least one GNOME Wayland host and one KDE Wayland host.
5. Keep an X11 validation lane only for legacy desktops, not as the primary path.

## Security and Privacy Notes

- Wavry relies on portal-mediated capture on Wayland, which enforces user permission boundaries.
- Session media transport remains end-to-end encrypted through the RIFT stack.
- Do not run production hosts with ad-hoc desktop permissions or unknown portal backends.

## Related Docs

- [Desktop App](/desktop-app)
- [Getting Started](/getting-started)
- [Operations](/operations)
- [Troubleshooting](/troubleshooting)
- [Architecture](/architecture)
