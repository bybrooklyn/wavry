---
title: Getting Started
description: Run Wavry locally, verify an end-to-end encrypted session, and understand each moving part.
---

This guide gets you from clone to first working session and explains what each process does.

## What You Will Run

A minimal local Wavry stack uses:

1. `gateway` Docker container (auth/control plane)
2. optional `relay` Docker container (encrypted UDP fallback)
3. `wavry-server` process (host runtime: capture + encode + stream)
4. `wavry-client` process (client runtime: receive + decode + render + input)

You can also run the desktop app for UI-driven host/client workflows.

## Prerequisites

- Rust 1.75+
- `protobuf-compiler`
- `pkg-config`
- Bun (for desktop/web tooling)

Optional (platform-specific):

- Android SDK/NDK for Android builds
- Xcode for macOS packaging
- PipeWire + XDG Desktop Portal backend for Linux/Wayland capture paths

For detailed Linux setup (distros, compositor support, portal packages, validation), use [Linux and Wayland Support](/linux-wayland-support).

## 1. Clone and Build

```bash
git clone https://github.com/bybrooklyn/wavry.git
cd wavry
cargo build --workspace
```

## 2. Start Control-Plane Services (Docker-Only)

From repo root, start gateway:

```bash
docker compose -f docker/control-plane.compose.yml up -d gateway
```

If you want to validate relay fallback locally, start relay too:

```bash
WAVRY_RELAY_MASTER_URL=http://host.docker.internal:8080 \
docker compose -f docker/control-plane.compose.yml --profile relay up -d relay
```

Relay requires a reachable master endpoint for registration and heartbeat. For production, set `WAVRY_RELAY_MASTER_PUBLIC_KEY` and disable insecure dev mode.

## 3. Start Host and Client Runtimes

Open two terminals.

Terminal 3 (host):

```bash
cargo run --bin wavry-server
```

Terminal 4 (client):

```bash
cargo run --bin wavry-client
```

If mDNS/direct discovery is unavailable in your environment, run the client with an explicit host address.

## 4. Run the Desktop App (Optional but Recommended)

```bash
cd crates/wavry-desktop
bun install
bun run tauri dev
```

Use the UI to start hosting or connect to a remote host.

## 5. Validate Session Health

When testing locally, confirm:

- Session is established and remains connected
- Input events are responsive (mouse + keyboard)
- No handshake/encryption errors in logs
- Bitrate/congestion state updates appear without runaway delay

## Common Local Issues

### Client cannot connect

- Verify gateway container is running
- Verify relay container is running if you are validating relay fallback
- Confirm host process is active
- Confirm address/port if using direct connect

### Desktop app fails to start

- Re-run `bun install` in `crates/wavry-desktop`
- Check Tauri prerequisites for your OS
- Run `bun run check` to validate frontend types

### Choppy session on local network

- Ensure CPU/GPU is not saturated
- Try lower stream settings temporarily
- Confirm you are not relaying unnecessarily when direct path is available

### Linux/Wayland capture fails

- Run `./scripts/linux-display-smoke.sh`
- Confirm `xdg-desktop-portal` and your desktop backend package are installed/running
- Confirm screen-capture permission was granted
- Follow [Linux and Wayland Support](/linux-wayland-support) for full remediation flow

## Next Steps

1. Read [Deployment Modes](/deployment-modes) to pick OSS/commercial/hosted usage.
2. Read [Docker Control Plane](/docker-control-plane) for gateway/relay container operation.
3. Read [Architecture](/architecture) to understand boundaries and extension points.
4. Read [Session Lifecycle](/lifecycle) and [Networking and Relay](/networking-and-relay) to understand behavior under real network conditions.
5. Read [Security](/security) before internet-facing deployment.
6. Use [Operations](/operations) and [Troubleshooting](/troubleshooting) to define your production runbook.
