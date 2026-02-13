---
title: Getting Started
description: End-to-end local setup with verification steps, expected results, and teardown.
---

This guide gets you from clone to a verified local session quickly.

## Goal

By the end of this page, you should have:

1. gateway container running and healthy
2. optional relay container running
3. host/client runtimes started
4. a basic session validated

## Prerequisites

Required:

- Rust 1.75+
- `protobuf-compiler`
- `pkg-config`

Optional:

- Bun (desktop app)
- Android SDK/NDK (Android builds)
- Xcode (macOS packaging)

Linux note:

- For Wayland hosts, ensure PipeWire + XDG portal stack is installed.
- Use [Linux and Wayland Support](/linux-wayland-support) for distro-specific setup.

## 1. Clone and Build

```bash
git clone https://github.com/bybrooklyn/wavry.git
cd wavry
cargo build --workspace --locked
```

Expected outcome:

- Workspace compiles successfully with no build errors.

## 2. Start Control Plane (Docker-Only)

Start gateway:

```bash
docker compose -f docker/control-plane.compose.yml up -d gateway
```

Verify gateway health:

```bash
curl -fsS http://127.0.0.1:3000/health
```

If you want relay fallback validation, start relay profile:

```bash
WAVRY_RELAY_MASTER_URL=http://host.docker.internal:8080 \
docker compose -f docker/control-plane.compose.yml --profile relay up -d relay
```

Check container status:

```bash
docker compose -f docker/control-plane.compose.yml ps
```

## 3. Start Host and Client Runtimes

Terminal A (host):

```bash
RUST_LOG=info cargo run --bin wavry-server
```

Terminal B (client):

```bash
RUST_LOG=info cargo run --bin wavry-client
```

Expected outcome:

- Host and client start without panic.
- Client reaches connected/active state when target is resolved.

## 4. Validate Basic Session Quality

Confirm:

- input feels responsive
- no repeated handshake failures in logs
- no runaway delay growth under short interaction bursts

Useful checks:

```bash
# control plane health
curl -fsS http://127.0.0.1:3000/health

# control plane container logs
docker compose -f docker/control-plane.compose.yml logs --tail=100 gateway
```

## 5. Optional Desktop App Run

```bash
cd crates/wavry-desktop
bun install
bun run tauri dev
```

Then use desktop UI for host/client workflow testing.

## Linux/Wayland Validation (Recommended)

Run preflight:

```bash
./scripts/linux-display-smoke.sh
```

If preflight fails, follow [Linux and Wayland Support](/linux-wayland-support).

## Common First-Run Problems

### Gateway is unhealthy

- Check container logs: `docker compose -f docker/control-plane.compose.yml logs gateway`
- Confirm `3000` is not already in use.

### Client cannot connect

- Confirm host runtime is active.
- Confirm target/session settings are correct.
- Confirm firewall policy permits required UDP path.

### Session is unexpectedly relayed

- Recheck NAT/firewall behavior.
- Confirm direct candidate path availability.

### Linux capture fails

- Run `./scripts/linux-display-smoke.sh`.
- Verify portal backend packages and PipeWire availability.

## Teardown

Stop local control-plane containers:

```bash
docker compose -f docker/control-plane.compose.yml down
```

Stop host/client runtimes with `Ctrl+C` in their terminals.

## Next Steps

1. [Architecture](/architecture)
2. [Session Lifecycle](/lifecycle)
3. [Docker Control Plane](/docker-control-plane)
4. [Security](/security)
5. [Operations](/operations)
6. [Troubleshooting](/troubleshooting)
