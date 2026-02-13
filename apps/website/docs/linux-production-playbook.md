---
title: Linux Production Playbook
description: End-to-end Linux operations guide for Wayland capture stability, packaging, and runtime diagnostics.
---

Wavry is built with Linux-first runtime behavior and a native Wayland capture path.

This playbook focuses on making Linux deployments stable in real operator environments, not just local demos.

## What "Production-Ready Linux" Means

A Linux host lane is production-ready when:

1. Wayland screencast starts reliably through portal + PipeWire.
2. Monitor selection and stale-monitor fallback are correct.
3. Audio capture routes behave predictably for system/mic/app paths.
4. Runtime diagnostics and runbooks are sufficient for first-response triage.
5. Packaging/install flows are repeatable for your target distro family.

## Runtime Architecture (Linux)

Video capture path:

1. Wayland session requests screencast via XDG desktop portal.
2. Portal returns stream metadata and PipeWire node.
3. GStreamer `pipewiresrc` reads frames.
4. Encoder path emits low-latency H.264/HEVC/AV1 payloads.

Fallback path:

- If Wayland capture is unavailable and X11 is available, Wavry falls back to X11 capture.

Audio routing:

- System mix: portal/PipeWire path with fallback where supported
- Microphone: PulseAudio or auto audio source paths
- Application route: per-app resolution via Pulse/PipeWire metadata

## Distro Baselines

### Debian / Ubuntu

Install at minimum:

```bash
sudo apt-get update
sudo apt-get install -y \
  pipewire wireplumber \
  xdg-desktop-portal xdg-desktop-portal-gtk \
  gstreamer1.0-tools gstreamer1.0-pipewire \
  gstreamer1.0-plugins-base gstreamer1.0-plugins-good gstreamer1.0-plugins-bad \
  pulseaudio-utils
```

Install desktop-specific backend for your environment when available:

- KDE Plasma: `xdg-desktop-portal-kde`
- GNOME: `xdg-desktop-portal-gnome`
- wlroots compositors: `xdg-desktop-portal-wlr`

### Fedora / RHEL-family

Install the matching packages for:

- PipeWire + WirePlumber
- XDG desktop portal + desktop backend
- GStreamer tools + base/good/bad plugins + PipeWire source plugin

## Compositor Matrix

| Compositor family | Support status | Key requirements |
|---|---|---|
| KDE Plasma (Wayland) | First-class | `xdg-desktop-portal-kde`, PipeWire health |
| GNOME (Wayland) | First-class | `xdg-desktop-portal-gnome`, PipeWire health |
| Sway/Hyprland/wlroots | Supported | `xdg-desktop-portal-wlr` or compositor-specific backend |
| X11 desktops | Supported fallback | `ximagesrc` capture path |

## Preflight and CI Validation

Run Linux smoke checks on candidate host images:

```bash
./scripts/linux-display-smoke.sh
```

Wayland session harness (CI lane):

```bash
./scripts/ci-wayland-runtime.sh
```

Expected outcomes:

- Required capture elements are present.
- Portal backend descriptors are present for the active desktop family.
- Portal process/services are available in session context.
- Monitor selection and fallback behavior are validated.

## Known Error Signatures

### `Gdk-Message ... Error 71 (Protocol Error) dispatching to wayland display`

Likely causes:

1. portal backend mismatch for compositor
2. stale/invalid portal session state
3. PipeWire startup race or user-session service issue

Response:

1. restart user portal and PipeWire services
2. verify backend package and descriptor availability
3. re-run `linux-display-smoke.sh`
4. retry host start and confirm monitor stream selection logs

### Missing `pipewiresrc`

Likely cause:

- GStreamer PipeWire plugin package not installed on host/runner image

Response:

1. install distro package providing `pipewiresrc`
2. verify with `gst-inspect-1.0 pipewiresrc`
3. re-run smoke checks

### No monitors shown in host UI

Likely causes:

1. portal permission denied
2. backend unavailable for active desktop
3. stale portal cache/session

Response:

1. grant/re-grant screencast permission
2. restart portal services and session
3. refresh monitor list and confirm fallback logs when needed

## Performance and Stability Checklist

1. Pin compositor + portal package versions in production images.
2. Pin GStreamer plugin set used by runtime capture paths.
3. Track CPU/GPU pressure and encoder fallback behavior.
4. Track portal failure rates and direct/relay ratio changes.
5. Keep at least KDE and GNOME Wayland validation lanes in regular testing.

## Packaging and Distribution Notes

Linux desktop distribution artifacts should remain clearly named and reproducible:

- AppImage: portable execution
- DEB: Debian/Ubuntu packaging path
- RPM: Fedora/RHEL packaging path

Run install/uninstall checks in CI for each packaging path used by your release.

## Operational Readiness Gates

Before accepting a Linux image for rollout:

1. smoke checks pass on target compositor family
2. runtime capture starts successfully on at least two monitor layouts
3. audio route checks pass for required capture mode
4. rollback image is available and tested
5. incident runbook owner is assigned

## Related Docs

- [Linux and Wayland Support](/linux-wayland-support)
- [Troubleshooting](/troubleshooting)
- [Operations](/operations)
- [Runbooks and Checklists](/runbooks-and-checklists)
- [Runtime and Service Reference](/runtime-and-service-reference)

