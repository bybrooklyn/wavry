---
title: Getting Started
description: Run Wavry locally, verify an end-to-end encrypted session, and understand each moving part.
---

This guide gets you from clone to first working session and explains what each process does.

## What You Will Run

A minimal local Wavry stack uses four runtime processes:

1. `wavry-gateway`: signaling/control plane entrypoint
2. `wavry-relay`: encrypted UDP fallback forwarder
3. `wavry-server`: host runtime (capture + encode + stream)
4. `wavry-client`: client runtime (receive + decode + render + input)

You can also run the desktop app for UI-driven host/client workflows.

## Prerequisites

- Rust 1.75+
- `protobuf-compiler`
- `pkg-config`
- Bun (for desktop/web tooling)

Optional (platform-specific):

- Android SDK/NDK for Android builds
- Xcode for macOS packaging
- PipeWire for Linux capture paths

## 1. Clone and Build

```bash
git clone https://github.com/bybrooklyn/wavry.git
cd wavry
cargo build --workspace
```

## 2. Start Control-Plane Services

Open two terminals from repo root.

Terminal 1:

```bash
cargo run --bin wavry-gateway
```

Terminal 2:

```bash
cargo run --bin wavry-relay -- --master-url http://localhost:8080
```

## 3. Start Host and Client Runtimes

Open two additional terminals.

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

- Verify gateway and relay are running
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

## Next Steps

1. Read [Deployment Modes](/docs/deployment-modes) to pick OSS/commercial/hosted usage.
2. Read [Architecture](/docs/architecture) to understand boundaries and extension points.
3. Read [Session Lifecycle](/docs/lifecycle) and [Networking and Relay](/docs/networking-and-relay) to understand behavior under real network conditions.
4. Read [Security](/docs/security) before internet-facing deployment.
5. Use [Operations](/docs/operations) and [Troubleshooting](/docs/troubleshooting) to define your production runbook.
