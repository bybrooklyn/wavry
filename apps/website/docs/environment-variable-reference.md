---
title: Environment Variable Reference
description: Source-driven environment variable catalog for host, client, gateway, master, and relay components.
---

This reference is derived from active variable reads in the codebase.

Use it as the canonical starting point for deployment config.

## Server (`wavry-server`)

| Variable | Default | Purpose |
|---|---|---|
| `WAVRY_LISTEN_ADDR` | `0.0.0.0:0` | UDP bind address for host runtime |
| `WAVRY_NO_ENCRYPT` | `false` | disable encryption (development/debug only) |
| `WAVRY_DISPLAY_ID` | unset | force capture display ID |
| `WAVRY_GATEWAY_URL` | `ws://127.0.0.1:3000/ws` | signaling gateway URL |
| `WAVRY_SESSION_TOKEN` | unset | signaling auth/session token |
| `WAVRY_ENABLE_WEBRTC` | `false` | enable WebRTC bridge path |
| `WAVRY_RECORD` | `false` | enable local recording |
| `WAVRY_RECORD_DIR` | `recordings` | recording output directory |
| `WAVRY_RECORD_QUALITY` | `standard` | recording quality preset |
| `WAVRY_FILE_OUT_DIR` | `received-files` | incoming file transfer output directory |
| `WAVRY_FILE_MAX_BYTES` | code default (`DEFAULT_MAX_FILE_BYTES`) | max inbound file size |
| `WAVRY_FILE_TRANSFER_SHARE_PERCENT` | `15.0` | max video bitrate share for file transfer |
| `WAVRY_FILE_TRANSFER_MIN_KBPS` | `256` | file-transfer bandwidth floor |
| `WAVRY_FILE_TRANSFER_MAX_KBPS` | `4096` | file-transfer bandwidth cap |
| `WAVRY_AUDIO_SOURCE` | `system` | audio route (`system`, `microphone`, `app:<name>`, `disabled`) |
| `WAVRY_SERVER_ALLOW_PUBLIC_BIND` | `false` | allow non-loopback host bind |

## Client Runtime (`wavry-client` and shared signaling/client paths)

| Variable | Default | Purpose |
|---|---|---|
| `WAVRY_ALLOW_INSECURE_NO_ENCRYPT` | `false` | allow `--no-encrypt` mode |
| `WAVRY_CLIENT_ALLOW_PUBLIC_CONNECT` | `false` | allow non-loopback targets in insecure mode |
| `WAVRY_ALLOW_INSECURE_SIGNALING` | `false` | permit insecure signaling URLs in guarded paths |
| `WAVRY_SIGNALING_TLS_PINS_SHA256` | unset | optional cert pin hashes for signaling |
| `WAVRY_ENVIRONMENT_PRODUCTION` | `false` | enforce production signaling/TOTP guardrails |
| `WAVRY_ENVIRONMENT` | unset | if set to `production`, enables production guardrails |

## Gateway (`wavry-gateway`)

### Core runtime

| Variable | Default | Purpose |
|---|---|---|
| `WAVRY_GATEWAY_BIND_ADDR` | `0.0.0.0:3000` | HTTP bind address |
| `WAVRY_ALLOW_PUBLIC_BIND` | `false` | allow non-loopback HTTP bind |
| `DATABASE_URL` | `sqlite:gateway.db` | SQLx database DSN |
| `RUST_LOG` | `wavry_gateway=info,tower_http=info` | logging filter |
| `WAVRY_GATEWAY_RELAY_PORT` | `0` | UDP relay helper bind port |
| `WAVRY_GATEWAY_RELAY_BIND_ADDR` | `127.0.0.1:<relay_port>` | gateway-local relay bind addr |
| `WAVRY_GATEWAY_RELAY_ALLOW_PUBLIC_BIND` | `false` | allow non-loopback gateway relay bind |
| `WAVRY_RELAY_SESSION_TTL_SECS` | `300` (min `30`) | gateway relay session TTL |
| `WAVRY_RELAY_SESSION_LIMIT` | `4096` | max in-memory relay sessions |
| `WAVRY_RELAY_PUBLIC_ADDR` | `127.0.0.1:3478` | relay address advertised to clients |
| `WAVRY_WS_MAX_CONNECTIONS` | `4096` | max concurrent WS signaling connections |
| `WAVRY_WS_MAX_PER_IP` | `16` | per-IP WS connection cap |
| `WAVRY_ENABLE_INSECURE_WEBTRANSPORT_RUNTIME` | `false` | enable runtime-gated WebTransport server |
| `WEBTRANSPORT_BIND_ADDR` | `0.0.0.0:0` | WebTransport bind address when enabled |
| `ADMIN_PANEL_TOKEN` | unset | bearer token for admin routes (required to enable admin panel) |

### CORS / Origin policy

| Variable | Default | Purpose |
|---|---|---|
| `WAVRY_ALLOWED_ORIGINS` | built-in localhost set | comma-separated allowed browser origins |
| `WAVRY_CORS_ALLOW_ANY` | `false` | permissive CORS (development only) |
| `WAVRY_WS_REQUIRE_ORIGIN` | `true` | require Origin for WS upgrade |
| `WAVRY_WS_ALLOW_MISSING_ORIGIN` | `false` | allow missing Origin header on WS |

### Rate limiting and edge hardening

| Variable | Default |
|---|---|
| `WAVRY_AUTH_RATE_LIMIT` | `20` |
| `WAVRY_AUTH_RATE_WINDOW_SECS` | `60` |
| `WAVRY_AUTH_RATE_MAX_KEYS` | `10000` |
| `WAVRY_POST_AUTH_RATE_LIMIT` | `60` |
| `WAVRY_POST_AUTH_RATE_WINDOW_SECS` | `60` |
| `WAVRY_POST_AUTH_RATE_MAX_KEYS` | `50000` |
| `WAVRY_WEBRTC_RATE_LIMIT` | `120` |
| `WAVRY_WEBRTC_RATE_WINDOW_SECS` | `60` |
| `WAVRY_WEBRTC_RATE_MAX_KEYS` | `50000` |
| `WAVRY_WS_BIND_RATE_LIMIT` | `10` |
| `WAVRY_WS_BIND_RATE_WINDOW_SECS` | `60` |
| `WAVRY_WS_BIND_RATE_MAX_KEYS` | `50000` |
| `WAVRY_GLOBAL_RATE_LIMIT` | `600` |
| `WAVRY_GLOBAL_RATE_WINDOW_SECS` | `60` |
| `WAVRY_GLOBAL_RATE_MAX_KEYS` | `200000` |
| `WAVRY_TRUST_PROXY_HEADERS` | `false` |

### TOTP key management

| Variable | Default | Purpose |
|---|---|---|
| `WAVRY_TOTP_KEY_B64` | unset | base64 32-byte key for encrypted stored TOTP secret |
| `WAVRY_ALLOW_INSECURE_TOTP` | `false` | allow plaintext TOTP secret mode in non-production |

## Master (`wavry-master`)

| Variable | Default | Purpose |
|---|---|---|
| `WAVRY_MASTER_ALLOWED_ORIGINS` | localhost/tauri defaults | allowed origins for CORS + WS checks |
| `WAVRY_MASTER_CORS_ALLOW_ANY` | `false` | permissive CORS mode |
| `WAVRY_MASTER_WS_REQUIRE_ORIGIN` | `true` | require Origin on master WS |
| `WAVRY_MASTER_WS_ALLOW_MISSING_ORIGIN` | `false` | allow missing Origin on master WS |
| `WAVRY_MASTER_ALLOW_PUBLIC_BIND` | `false` | allow non-loopback bind |
| `WAVRY_MASTER_INSECURE_DEV` | `false` | insecure dev auth mode toggle (feature-gated) |
| `WAVRY_MASTER_SIGNING_KEY` | unset | signing key (hex) for relay lease tokens |
| `WAVRY_MASTER_KEY_FILE` | unset | path to signing key file (hex) |
| `WAVRY_MASTER_KEY_ID` | derived from public key | active signing key identifier embedded in lease claims |
| `WAVRY_MASTER_LEASE_TTL_SECS` | `900` (clamped `60..3600`) | relay lease token lifetime in seconds |
| `ADMIN_PANEL_TOKEN` | unset | bearer token for admin endpoints |

## Relay (`wavry-relay`)

| Variable | Default | Purpose |
|---|---|---|
| `WAVRY_RELAY_LISTEN` | `0.0.0.0:4000` | UDP listen address |
| `WAVRY_MASTER_URL` | `http://localhost:8080` | master server URL |
| `WAVRY_RELAY_MASTER_PUBLIC_KEY` | unset | relay-side verification key |
| `WAVRY_RELAY_ALLOW_INSECURE_DEV` | `false` | allow missing verification key in dev mode |
| `WAVRY_ALLOW_INSECURE_RELAY` | `false` | hard override required to run insecure relay mode |
| `WAVRY_RELAY_REGION` | unset | relay metadata region |
| `WAVRY_RELAY_ASN` | unset | relay metadata ASN |
| `WAVRY_RELAY_MAX_BITRATE` | `20000` | relay advertised max bitrate (kbps) |
| `WAVRY_RELAY_HEALTH_LISTEN` | `127.0.0.1:9091` | relay HTTP health/readiness/metrics bind |
| `WAVRY_RELAY_ALLOW_PUBLIC_BIND` | `false` | allow non-loopback relay bind |

## Web / VR / Platform-Specific

| Variable | Default | Component |
|---|---|---|
| `WAVRY_WT_CERT` | `cert.pem` | web transport runtime cert path (`wavry-web`) |
| `WAVRY_WT_KEY` | `key.pem` | web transport runtime key path (`wavry-web`) |
| `WAVRY_USE_VULKAN` | unset (presence enables) | Linux OpenXR path toggle (`wavry-vr-openxr`) |

## Security Guidance

- Prefer explicit allowlists (`WAVRY_ALLOWED_ORIGINS`, `WAVRY_MASTER_ALLOWED_ORIGINS`) over wildcard mode.
- Keep all `ALLOW_INSECURE*` and `*_ALLOW_PUBLIC_BIND` flags disabled in production by default.
- Set `WAVRY_TOTP_KEY_B64` for production to avoid insecure secret handling paths.
- Always set `ADMIN_PANEL_TOKEN` to a high-entropy value if admin APIs are exposed.

## Related Docs

- [Configuration Reference](/configuration-reference)
- [Runtime and Service Reference](/runtime-and-service-reference)
- [Security](/security)
- [Operations](/operations)
