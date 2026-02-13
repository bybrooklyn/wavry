---
title: Codebase Reference
description: Crate-by-crate map of the Wavry workspace, including responsibilities, entry points, and key modules.
---

This page maps the full Wavry workspace so engineers can quickly locate ownership, runtime boundaries, and extension points.

## Workspace Snapshot

- Rust crates in `crates/`: 17
- Rust source files in `crates/`: 82
- Strongest platform surface area: `wavry-media`, `wavry-server`, `wavry-client`, `wavry-gateway`
- Primary protocol/crypto core: `rift-core`, `rift-crypto`

## Layered Component Map

| Layer | Crates | Primary Responsibility |
|---|---|---|
| Protocol | `rift-core` | RIFT packet formats, relay wire protocol, congestion-control primitives, STUN helpers |
| Crypto | `rift-crypto` | identity keys, Noise XX handshake/session, replay windows |
| Shared runtime | `wavry-common` | shared protocol structs, tracing helpers, file-transfer utility types |
| Session runtimes | `wavry-server`, `wavry-client` | host capture/encode/send, client receive/decode/render/input |
| Control plane | `wavry-gateway`, `wavry-master`, `wavry-relay` | auth/signaling, relay registry/selection, encrypted UDP forwarding |
| Platform/media | `wavry-media`, `wavry-platform` | capture, encode/decode/render, input injection, clipboard |
| Product/API surfaces | `wavry-desktop`, `wavry-cli`, `wavry-ffi`, `wavry-web`, `wavry-vr*` | desktop UX, CLI tooling, FFI embedding, web bridge, VR adapters |

## Crate-by-Crate Guide

### Protocol and Crypto

| Crate | Key Files | What You Change Here |
|---|---|---|
| `rift-core` | `crates/rift-core/src/lib.rs`, `crates/rift-core/src/cc.rs`, `crates/rift-core/src/relay.rs`, `crates/rift-core/src/stun.rs` | packet schemas, framing logic, congestion behavior, relay wire headers |
| `rift-crypto` | `crates/rift-crypto/src/identity.rs`, `crates/rift-crypto/src/noise.rs`, `crates/rift-crypto/src/session.rs`, `crates/rift-crypto/src/connection.rs` | handshake flow, key material rules, replay protection windows, secure packet wrapping |

### Session Runtime

| Crate | Entrypoint | Key Modules | Notes |
|---|---|---|---|
| `wavry-server` | `crates/wavry-server/src/main.rs` | `webrtc_bridge.rs` | host runtime; capture + encode + encrypted transport + input/file transfer ingress |
| `wavry-client` | `crates/wavry-client/src/bin/wavry-client.rs` | `client.rs`, `signaling.rs`, `media.rs`, `input.rs`, `types.rs` | client runtime; signaling + receive/decode/render + input uplink |
| `wavry-common` | `crates/wavry-common/src/lib.rs` | `protocol.rs`, `helpers.rs`, `file_transfer.rs`, `error.rs` | shared message and helper layer used by most services |

### Control Plane and Relay

| Crate | Entrypoint | Key Modules | Notes |
|---|---|---|---|
| `wavry-gateway` | `crates/wavry-gateway/src/main.rs` | `auth.rs`, `signal.rs`, `security.rs`, `relay.rs`, `db.rs`, `admin.rs`, `web.rs` | internet-facing auth/signaling server and gateway-local relay session management |
| `wavry-master` | `crates/wavry-master/src/main.rs` | `selection.rs` | relay registry, lease issuance, relay scoring/selection |
| `wavry-relay` | `crates/wavry-relay/src/main.rs` | `session.rs` | encrypted UDP forwarding with lease validation and rate limiting |

### Platform and Media

| Crate | Key Files | Notes |
|---|---|---|
| `wavry-media` | `crates/wavry-media/src/lib.rs`, `crates/wavry-media/src/linux.rs`, `crates/wavry-media/src/windows.rs`, `crates/wavry-media/src/mac_*.rs`, `crates/wavry-media/src/android/*.rs`, `crates/wavry-media/src/recorder.rs` | largest platform-specific surface: encoders, renderers, capture backends, runtime diagnostics |
| `wavry-platform` | `crates/wavry-platform/src/lib.rs`, `crates/wavry-platform/src/linux/mod.rs`, `crates/wavry-platform/src/windows_input_injector.rs`, `crates/wavry-platform/src/clipboard.rs` | platform input injection, capture abstraction, clipboard plumbing |

### Product and Integration Surfaces

| Crate | Entrypoint / Core File | Notes |
|---|---|---|
| `wavry-desktop` | `crates/wavry-desktop/src-tauri/src/lib.rs` | Tauri desktop shell, auth/session commands, Linux runtime diagnostics commands |
| `wavry-cli` | `crates/wavry-cli/src/main.rs` | CLI subcommands for keygen, ID inspection, connectivity ping, version |
| `wavry-ffi` | `crates/wavry-ffi/src/lib.rs` | C ABI surface for host/client embedding |
| `wavry-web` | `crates/wavry-web/src/lib.rs` | WebTransport/WebRTC integration layer (runtime-feature-gated) |
| `wavry-vr` | `crates/wavry-vr/src/lib.rs` | shared VR traits/types/status |
| `wavry-vr-alvr` | `crates/wavry-vr-alvr/src/lib.rs` | ALVR adapter or stub fallback |
| `wavry-vr-openxr` | `crates/wavry-vr-openxr/src/lib.rs` | OpenXR runtime glue per platform |

## Runtime Entry Points

Primary process entry points in this repository:

- `crates/wavry-server/src/main.rs`
- `crates/wavry-client/src/bin/wavry-client.rs`
- `crates/wavry-gateway/src/main.rs`
- `crates/wavry-master/src/main.rs`
- `crates/wavry-relay/src/main.rs`
- `crates/wavry-cli/src/main.rs`
- `crates/wavry-desktop/src-tauri/src/main.rs`

## Test Coverage Snapshot (By `#[test]` / `#[tokio::test]` Count)

| Crate | Approx. Test Count |
|---|---|
| `rift-core` | 45 |
| `wavry-media` | 34 |
| `wavry-client` | 29 |
| `wavry-gateway` | 28 |
| `rift-crypto` | 24 |
| `wavry-common` | 14 |
| `wavry-server` | 6 |
| `wavry-platform` | 5 |
| `wavry-master` | 3 |
| `wavry-desktop` | 3 |
| `wavry-relay` | 0 |
| `wavry-ffi` | 0 |
| `wavry-vr`, `wavry-vr-alvr`, `wavry-vr-openxr`, `wavry-web`, `wavry-cli` | 0 |

Use this as a prioritization map for additional hardening tests.

## Where To Extend Safely

- New transport behavior: start in `rift-core` and `rift-crypto`, then thread into `wavry-server` and `wavry-client`.
- New auth/policy behavior: start in `wavry-gateway/src/security.rs` and `wavry-gateway/src/auth.rs`.
- Relay selection behavior: `wavry-master/src/selection.rs`.
- Linux capture/input behavior: `wavry-media/src/linux.rs` and `wavry-platform/src/linux/mod.rs`.
- Desktop workflow integration: `wavry-desktop/src-tauri/src/commands.rs` and `crates/wavry-desktop/src/routes/+page.svelte`.

## Related Docs

- [Architecture](/architecture)
- [Runtime and Service Reference](/runtime-and-service-reference)
- [Environment Variable Reference](/environment-variable-reference)
- [Internal Design Docs](/internal-design-docs)
