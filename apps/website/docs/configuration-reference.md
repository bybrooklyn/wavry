---
title: Configuration Reference
description: Key runtime flags and environment variables used when operating Wavry components.
---

This page summarizes commonly used runtime configuration knobs.

For complete and current option sets, always check each binary's `--help` output.

## Host Runtime (`wavry-server`)

Common options:

- `--listen`: UDP listen address (`0.0.0.0:0` by default)
- `--no-encrypt`: disable transport encryption (testing only)
- `--width` / `--height`: default stream resolution
- `--fps`: target frame rate
- `--bitrate-kbps`: initial target bitrate
- `--keyframe-interval-ms`: keyframe cadence
- `--display-id`: select capture display
- `--gateway-url`: signaling gateway URL
- `--session-token`: signaling session token
- `--enable-webrtc`: optional web bridge path
- `--record`: enable local recording
- `--record-dir`: recording output directory
- `--send-file`: queue file transfer(s) to client
- `--file-out-dir`: incoming transfer directory
- `--file-transfer-share-percent`: max transfer share of current video bitrate
- `--file-transfer-min-kbps`: transfer floor budget
- `--file-transfer-max-kbps`: transfer ceiling budget
- `--audio-source`: `system`, `microphone`, `app:<name>`, `disabled`
  - `app:<name>` is supported on Linux/macOS/Windows; on startup failure it falls back safely to `system`.

Related env vars include:

- `WAVRY_LISTEN_ADDR`
- `WAVRY_GATEWAY_URL`
- `WAVRY_SESSION_TOKEN`
- `WAVRY_FILE_OUT_DIR`
- `WAVRY_FILE_MAX_BYTES`
- `WAVRY_AUDIO_SOURCE`
- `WAVRY_FILE_TRANSFER_SHARE_PERCENT`
- `WAVRY_FILE_TRANSFER_MIN_KBPS`
- `WAVRY_FILE_TRANSFER_MAX_KBPS`
- `WAVRY_ALLOW_INSECURE_SIGNALING` (production override for `ws://`)
- `WAVRY_SIGNALING_TLS_PINS_SHA256` (comma/semicolon-separated SHA-256 cert fingerprints for `wss://` signaling pinning)

## Client Runtime (`wavry-client`)

Common options:

- connect target (explicit host address or discovery-driven)
- encryption and identity options
- relay/master endpoint options
- resolution and input behavior options
- file transfer receive/output options

For interactive transfer controls in CLI mode:

- `--file-control-stdin`
  - accepts `"<file_id> <pause|resume|cancel|retry>"`

## Desktop App (`wavry-desktop`)

Desktop app config spans:

- Connectivity mode (cloud/direct/custom)
- Gateway URL
- Host port and UPnP behavior
- Resolution strategy (native/client/custom)
- Gamepad enable + deadzone tuning
- Selected monitor/display target

The desktop UI maps these choices to runtime config passed into host/client processes.

Linux desktop builds also expose runtime diagnostics commands:

- `linux_runtime_health`: returns Linux session/backend/plugin diagnostics.
- `linux_host_preflight(display_id)`: validates Linux host readiness and resolves the selected display + capture resolution before host start.

## Gateway and Relay

Gateway and relay are distributed as Docker containers. Use [Docker Control Plane](/docker-control-plane) for deployment commands and image/tag policy.

Deployment configuration should define:

- Binding/listen addresses
- Auth/session token policies
- Rate-limiting/security controls
- Database/migration paths (gateway)
- Upstream coordination settings

Useful security-focused env vars:

- `WAVRY_GLOBAL_RATE_LIMIT`: global gateway request cap per window
- `WAVRY_GLOBAL_RATE_WINDOW_SECS`: global limiter window size
- `WAVRY_GLOBAL_RATE_MAX_KEYS`: bounded key cardinality for limiter map
- `WAVRY_TRUST_PROXY_HEADERS`: whether to trust `X-Forwarded-For` / `X-Real-IP`

## Suggested Configuration Workflow

1. Start with defaults for local validation.
2. Pin critical addresses/ports for production.
3. Set explicit security and logging policies.
4. Validate direct/relay behavior in a staging network.
5. Document your final environment profile per deployment tier.

## Related Docs

- [Getting Started](/getting-started)
- [Networking and Relay](/networking-and-relay)
- [Operations](/operations)
