---
title: Runtime and Service Reference
description: Commands, binaries, and HTTP/WebSocket surfaces across the Wavry stack.
---

This page is the operational reference for what runs, how to start it, and which network/API surfaces it exposes.

## Binaries at a Glance

| Component | Binary / Entrypoint | Purpose |
|---|---|---|
| Host runtime | `wavry-server` (`crates/wavry-server/src/main.rs`) | capture, encode, encrypted media stream, input/file transfer handling |
| Client runtime | `wavry-client` (`crates/wavry-client/src/bin/wavry-client.rs`) | connect, decrypt/decode/render, send input, optional local record |
| Gateway | `wavry-gateway` (`crates/wavry-gateway/src/main.rs`) | auth APIs, signaling WS, runtime metrics, relay-session broker |
| Master | `wavry-master` (`crates/wavry-master/src/main.rs`) | relay registry, lease token issuance, relay selection |
| Relay | `wavry-relay` (`crates/wavry-relay/src/main.rs`) | encrypted UDP forwarding with lease validation |
| CLI tooling | `wavry` (`crates/wavry-cli/src/main.rs`) | key generation, ID inspection, connectivity ping |

## CLI Reference

### `wavry` (CLI)

- `wavry keygen --output <prefix>`
- `wavry show-id --key <public_key_path>`
- `wavry ping --server <host:port>`
- `wavry version`

### `wavry-client`

Key flags:

- `--connect <host:port>`
- `--name <string>`
- `--no-encrypt`
- `--vr`
- `--record --record-dir <path>`
- `--send-file <path>` (repeatable)
- `--file-out-dir <path>`
- `--file-max-bytes <n>`
- `--file-control-stdin`

### `wavry-server`

Key flags:

- `--listen <host:port>`
- `--width <px> --height <px> --fps <n>`
- `--bitrate-kbps <n> --keyframe-interval-ms <n>`
- `--display-id <id>`
- `--gateway-url <ws://...>`
- `--session-token <token>`
- `--enable-webrtc`
- `--record --record-dir <path> --record-quality <high|standard|low>`
- `--send-file <path>` (repeatable)
- `--file-out-dir <path> --file-max-bytes <n>`
- `--file-transfer-share-percent <1..100>`
- `--file-transfer-min-kbps <n> --file-transfer-max-kbps <n>`
- `--audio-source <system|microphone|app:<name>|disabled>`

### `wavry-relay`

Key flags:

- `--listen <host:port>`
- `--master-url <http://...>`
- `--max-sessions <n>`
- `--idle-timeout <seconds>`
- `--master-public-key <hex>`
- `--allow-insecure-dev`
- `--ip-rate-limit-pps <n>`
- `--cleanup-interval-secs <n>`
- `--lease-duration-secs <n>`
- `--stats-log-interval-secs <n>`
- `--region <region> --asn <asn> --max-bitrate-kbps <n>`

### `wavry-master`

Key flags:

- `--listen <host:port>`
- `--log-level <level>`
- `--insecure-dev`

## Local Service Startup

Typical local stack:

```bash
cargo run --bin wavry-gateway
cargo run --bin wavry-master -- --listen 127.0.0.1:8080
cargo run --bin wavry-relay -- --master-url http://127.0.0.1:8080
cargo run --bin wavry-server -- --gateway-url ws://127.0.0.1:3000/ws
cargo run --bin wavry-client -- --connect 127.0.0.1:0
```

Use Docker for gateway/relay in production.

## Gateway API Surface

From `crates/wavry-gateway/src/main.rs`:

### Health and Metrics

- `GET /`
- `GET /health`
- `GET /metrics/runtime`
- `GET /metrics/auth`

### Auth

- `POST /auth/register`
- `POST /auth/login`
- `POST /auth/logout`
- `POST /auth/2fa/setup`
- `POST /auth/2fa/enable`

### Signaling and Relay

- `GET /ws` (WebSocket signaling)
- `POST /v1/relays/report`
- `GET /v1/relays/reputation`

### WebRTC Bridge APIs

- `GET /webrtc/config`
- `POST /webrtc/offer`
- `POST /webrtc/answer`
- `POST /webrtc/candidate`

### Admin Surface

- `GET /admin`
- `GET /admin/api/overview`
- `GET /admin/api/audit`
- `POST /admin/api/sessions/revoke`
- `POST /admin/api/ban`
- `POST /admin/api/unban`

## Master API Surface

From `crates/wavry-master/src/main.rs`:

- `GET /health`
- `GET /ready`
- `GET /.well-known/wavry-id`
- `POST /v1/relays/register`
- `POST /v1/relays/heartbeat`
- `GET /v1/relays`
- `POST /v1/feedback`
- `POST /admin/api/sessions/revoke`
- `POST /v1/auth/register`
- `POST /v1/auth/register/verify`
- `POST /v1/auth/login`
- `GET /ws` (WebSocket signaling channel)

`/health` now reports readiness inputs (relay assignability, signing key status), while `/ready` returns `200` only when master is capable of safely issuing leases.

## Relay Health Surface

From `crates/wavry-relay/src/main.rs`:

- `GET /health`
- `GET /ready`
- `GET /metrics`

Default bind: `127.0.0.1:9091` (configurable via `WAVRY_RELAY_HEALTH_LISTEN`).

## Runtime Safety Gates (Important)

The codebase enforces explicit opt-in for risky runtime modes.

Examples:

- public bind guards (`WAVRY_*_ALLOW_PUBLIC_BIND=1`)
- insecure signaling overrides (`WAVRY_ALLOW_INSECURE_SIGNALING=1`)
- insecure relay mode override (`WAVRY_ALLOW_INSECURE_RELAY=1`)
- insecure no-encrypt mode override (`WAVRY_ALLOW_INSECURE_NO_ENCRYPT=1`)

Treat these flags as development-only unless a security review approves their use.

## Related Docs

- [Configuration Reference](/configuration-reference)
- [Environment Variable Reference](/environment-variable-reference)
- [Network Ports and Firewall](/network-ports-and-firewall)
- [Security](/security)
