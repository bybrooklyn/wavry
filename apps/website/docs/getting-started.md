---
title: Getting Started
description: Build Wavry, run the local stack, and validate end-to-end.
---

## Prerequisites

- Rust 1.75+
- `protobuf-compiler`
- `pkg-config`
- Bun (for desktop/web app tooling)

## 1. Build the Workspace

```bash
git clone https://github.com/bybrooklyn/wavry.git
cd wavry
cargo build --workspace
```

## 2. Start Core Services

Run these in separate terminals from repo root:

```bash
cargo run --bin wavry-gateway
```

```bash
cargo run --bin wavry-relay -- --master-url http://localhost:8080
```

## 3. Run Host and Client

```bash
cargo run --bin wavry-server
```

```bash
cargo run --bin wavry-client
```

## 4. Run Desktop App (Tauri)

```bash
cd crates/wavry-desktop
bun install
bun run tauri dev
```

## 5. Validate

- Confirm the gateway and relay register correctly.
- Start a host session and connect from a client.
- Check logs for stable RTT and no handshake errors.

## Next Steps

- Review [Deployment Modes](/docs/deployment-modes) before production use.
- Use [Security](/docs/security) to align with your threat model.
- Use [Operations](/docs/operations) for CI/CD and release distribution.
