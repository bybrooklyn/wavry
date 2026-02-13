---
title: Configuration Reference
description: Practical configuration guide for host/client runtimes and Docker control-plane services.
---

This page summarizes the most important runtime options and environment variables.

For exhaustive options, use each binary's `--help` output.

For the full source-derived environment catalog, see [Environment Variable Reference](/environment-variable-reference).

## Host Runtime (`wavry-server`)

Common flags:

- `--listen`: UDP listen address
- `--width` / `--height`: stream resolution
- `--fps`: target frame rate
- `--bitrate-kbps`: initial target bitrate
- `--display-id`: selected display
- `--gateway-url`: signaling endpoint
- `--session-token`: signaling session token
- `--enable-webrtc`: optional web bridge path
- `--audio-source`: `system`, `microphone`, `app:<name>`, `disabled`

Common env vars:

- `WAVRY_LISTEN_ADDR`
- `WAVRY_GATEWAY_URL`
- `WAVRY_SESSION_TOKEN`
- `WAVRY_AUDIO_SOURCE`
- `WAVRY_ALLOW_INSECURE_SIGNALING`
- `WAVRY_SIGNALING_TLS_PINS_SHA256`

## Client Runtime (`wavry-client`)

Common categories:

- target/discovery options
- encryption and identity settings
- relay/master endpoint options
- input and rendering behavior

Useful mode:

- `--file-control-stdin` for transfer control commands (`pause`, `resume`, `cancel`, `retry`)

## Desktop App (`wavry-desktop`)

Key configuration domains:

- connectivity mode (cloud/direct/custom)
- gateway URL
- host networking options
- resolution strategy
- input/gamepad settings
- display target

Linux diagnostics commands are exposed in desktop runtime surface:

- `linux_runtime_health`
- `linux_host_preflight(display_id)`

## Docker Control Plane (`gateway` and `relay`)

Control plane services are Docker-only.

Use [Docker Control Plane](/docker-control-plane) for deployment commands and image/tag policy.

Key runtime variables:

- `ADMIN_PANEL_TOKEN`
- `WAVRY_GLOBAL_RATE_LIMIT`
- `WAVRY_GLOBAL_RATE_WINDOW_SECS`
- `WAVRY_GLOBAL_RATE_MAX_KEYS`
- `WAVRY_TRUST_PROXY_HEADERS`
- `WAVRY_RELAY_MASTER_PUBLIC_KEY`
- `WAVRY_RELAY_ALLOW_INSECURE_DEV` (dev only)

## Recommended Configuration Workflow

1. start with defaults in local environment
2. pin addresses/ports and auth values in staging
3. enforce production signaling and TLS policy
4. validate direct vs relay behavior under realistic network conditions
5. document final environment profile per deployment tier

## Related Docs

- [Runtime and Service Reference](/runtime-and-service-reference)
- [Environment Variable Reference](/environment-variable-reference)
- [Getting Started](/getting-started)
- [Docker Control Plane](/docker-control-plane)
- [Networking and Relay](/networking-and-relay)
- [Operations](/operations)
- [Security](/security)
