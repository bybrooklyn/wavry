# Wavry Security + Architecture Hygiene Audit

Date: 2026-02-06  
Scope: current repository state (Rust services, Tauri desktop, web signaling/runtime skeletons)  
Method: static code audit with focused review of auth, signaling, relay, WebRTC/WebTransport, networking assumptions, and code organization.

## Executive Summary
The current codebase has several strong primitives (Argon2 password hashing, SQL parameter binding, optional signed relay leases), but there are multiple high-impact gaps in replay protection, abuse controls, and production-hardening defaults. The biggest risks are replayable encrypted packets in active data paths, easy DoS surfaces in signaling, and deploy-time unsafe defaults that can unintentionally disable trust boundaries.

---

## High-Risk Issues (Must Fix)

### H-01: Replay protection is not enforced in active encrypted transport paths
- Severity: High
- Evidence:
  - `crates/wavry-server/src/main.rs:526` decrypts with `phys.packet_id` but does not perform sequence-window replay checks.
  - `crates/wavry-client/src/client.rs:974` does the same on client receive path.
  - Relay path explicitly acknowledges missing sequence check: `crates/wavry-relay/src/main.rs:370`.
- Impact: Captured encrypted packets can be replayed, causing repeated control/input/media events and state perturbation despite encryption.
- Recommendation:
  - Enforce per-peer sequence window checks before processing decrypted payloads.
  - Reuse `rift_crypto::SequenceWindow`/session-level replay APIs consistently in server, client, and relay forwarding paths.

### H-02: Signaling service has strong DoS surfaces (unbounded queues + unbounded relay session growth)
- Severity: High
- Evidence:
  - Unbounded WS outbound queue: `crates/wavry-gateway/src/signal.rs:96` (`mpsc::unbounded_channel`).
  - Relay session entries inserted per request: `crates/wavry-gateway/src/signal.rs:253` with no TTL/cleanup path in gateway relay map.
  - Invalid token bind does not disconnect: `crates/wavry-gateway/src/signal.rs:155` and comment `Disconnect?` at `crates/wavry-gateway/src/signal.rs:162`.
- Impact: Low-cost traffic can force memory growth and tie up server resources.
- Recommendation:
  - Replace unbounded channels with bounded channels + drop/backpressure policy.
  - Add TTL cleanup for relay map entries and cap outstanding relay sessions per user/IP.
  - Close WS on failed bind and rate-limit bind attempts.

### H-03: Relay authentication can be effectively disabled by default configuration
- Severity: High
- Evidence:
  - Relay accepts optional `master_public_key`: `crates/wavry-relay/src/main.rs:57`.
  - If missing, lease validation falls back to `dev-peer-*` acceptance: `crates/wavry-relay/src/main.rs:261`.
- Impact: Unsigned/unauthorized peers can obtain relay service and abuse bandwidth/relay infrastructure.
- Recommendation:
  - Fail-fast startup in non-dev mode if `master_public_key` is missing.
  - Add explicit `--dev-insecure-relay` flag instead of silent insecure fallback.

### H-04: `wavry-master` is deploy-dangerous in current state (mock auth + permissive CORS)
- Severity: High
- Evidence:
  - Permissive CORS: `crates/wavry-master/src/main.rs:63`.
  - Login route points to register challenge flow: `crates/wavry-master/src/main.rs:61`.
  - Mock bearer token issuance: `crates/wavry-master/src/main.rs:127`.
  - WS bind uses token prefix to derive identity: `crates/wavry-master/src/main.rs:170`.
- Impact: If exposed in production, auth and identity boundaries are effectively bypassed.
- Recommendation:
  - Gate this binary behind explicit dev feature/flag or remove from production build paths.
  - Implement real token verification and strict CORS/origin policies before exposure.

### H-05: No brute-force/rate-limit controls on authentication and signaling endpoints
- Severity: High
- Evidence:
  - Auth endpoints mounted without request throttling/middleware: `crates/wavry-gateway/src/main.rs:116`.
  - WebRTC signaling endpoints similarly unthrottled: `crates/wavry-gateway/src/main.rs:120`.
  - WS signaling has no per-IP/per-user message limits: `crates/wavry-gateway/src/signal.rs:109`.
- Impact: Password guessing, token-guess attempts, and CPU/memory DoS are feasible.
- Recommendation:
  - Add layered rate limits: per-IP, per-account, per-token, and per-route.
  - Add login backoff/lockout and WS message-rate caps.

---

## Medium-Risk Issues

### M-01: Session tokens and TOTP secrets are stored plaintext at rest
- Evidence:
  - Session token persisted as plaintext primary key: `crates/wavry-gateway/src/db.rs:22`, `crates/wavry-gateway/src/db.rs:103`.
  - TOTP secret persisted directly: `crates/wavry-gateway/src/db.rs:118`.
- Impact: DB compromise immediately enables session hijack and 2FA seed recovery.
- Recommendation:
  - Store only salted/peppered hash of session tokens (show token once at issuance).
  - Encrypt TOTP secrets at rest with key management.

### M-02: Session lifecycle is incomplete (limited revocation paths, weak context binding)
- Evidence:
  - Session created with `ip_address: None` (TODO): `crates/wavry-gateway/src/auth.rs:109`.
  - No user logout endpoint in gateway routes: `crates/wavry-gateway/src/main.rs:116`.
  - Sessions are time-based but no visible periodic purge job in gateway.
- Impact: More stale active tokens, weaker anomaly detection, broader hijack window.
- Recommendation:
  - Add explicit logout/logout-all, session rotation, inactivity expiry, and periodic expired-session cleanup.

### M-03: WebSocket origin hardening is missing
- Evidence:
  - Gateway WS upgrade path has no `Origin` validation: `crates/wavry-gateway/src/signal.rs:77`.
  - Master WS path same pattern: `crates/wavry-master/src/main.rs:136`.
- Impact: Cross-origin WS use is easier if tokens leak via another vector.
- Recommendation:
  - Validate `Origin` against allowlist on WS upgrades.
  - Add connection auth timeout and immediate close for unauthenticated sockets.

### M-04: WebTransport runtime path is unauthenticated and binds publicly when enabled
- Evidence:
  - Dev runtime binds UDP socket and treats peer address as session identity: `crates/wavry-web/src/webtransport.rs:62`, `crates/wavry-web/src/webtransport.rs:68`.
  - Gateway runtime bind default `0.0.0.0:4444`: `crates/wavry-gateway/src/main.rs:99`.
- Impact: If enabled outside controlled dev environments, allows unauthorized traffic ingestion.
- Recommendation:
  - Keep feature strictly dev-only and refuse startup without explicit insecure-dev flag.
  - Require authenticated session binding before accepting control/input frames.

### M-05: Relay protocol mismatch between gateway relay and client relay path
- Evidence:
  - Client uses RIFT relay header + lease payload: `crates/wavry-client/src/client.rs:264`.
  - Gateway relay expects custom 17-byte handshake (`0x01 + UUID`): `crates/wavry-gateway/src/relay.rs:45` and `crates/wavry-gateway/src/relay.rs:49`.
  - Gateway signaling returns placeholder relay address: `crates/wavry-gateway/src/signal.rs:270`.
- Impact: Relay fallback can break, and operators may assume protection paths that are not actually active.
- Recommendation:
  - Standardize on one relay protocol implementation and remove/deprecate the divergent one.

### M-06: NAT traversal assumptions are brittle
- Evidence:
  - Single hardcoded STUN endpoint: `crates/wavry-client/src/client.rs:243`.
  - Fixed short timeout: `crates/wavry-client/src/client.rs:250`.
- Impact: Connectivity failure in restricted networks; increased forced relay usage.
- Recommendation:
  - Use multiple STUN servers and configurable ICE/STUN lists.

### M-07: Desktop token persistence + disabled CSP in Tauri UI
- Evidence:
  - Session token stored in browser localStorage: `crates/wavry-desktop/src/lib/appState.svelte.ts:98`.
  - Token rehydrated from localStorage: `crates/wavry-desktop/src/lib/appState.svelte.ts:76`.
  - Tauri CSP disabled: `crates/wavry-desktop/src-tauri/tauri.conf.json:21`.
- Impact: Any renderer-side XSS can exfiltrate signaling token.
- Recommendation:
  - Prefer OS keychain/secure storage for tokens.
  - Reinstate restrictive CSP and reduce script injection surface.

### M-08: Potential secret leakage via operational logs
- Evidence:
  - Gateway logs full `DATABASE_URL`: `crates/wavry-gateway/src/main.rs:70`.
- Impact: Credentials in DSNs can leak via logs/aggregators.
- Recommendation:
  - Redact credentials before logging DSNs.

### M-09: Challenge map in master can grow without TTL cleanup
- Evidence:
  - Unbounded in-memory map: `crates/wavry-master/src/main.rs:43`.
  - Insert on every register challenge: `crates/wavry-master/src/main.rs:90`.
- Impact: Memory DoS via repeated challenge requests.
- Recommendation:
  - Add challenge TTL eviction and max outstanding challenges per key/IP.

### M-10: Token slicing panic risk in master WS bind
- Evidence:
  - `&token[..4]` without length check: `crates/wavry-master/src/main.rs:171`.
- Impact: Malformed short token can crash connection task and destabilize service under load.
- Recommendation:
  - Validate token length before slicing and return structured error.

---

## Low-Risk / Cleanup

### L-01: Admin token comparison is direct string equality
- Evidence: `crates/wavry-gateway/src/admin.rs:74`.
- Improvement: Use constant-time compare for bearer secret checks.

### L-02: Public bind defaults expose services on all interfaces
- Evidence:
  - Gateway: `crates/wavry-gateway/src/main.rs:128`
  - Master: `crates/wavry-master/src/main.rs:31`
  - Relay: `crates/wavry-relay/src/main.rs:42`
  - Server: `crates/wavry-server/src/main.rs:54`
- Improvement: Default to loopback in dev and make external bind explicit.

### L-03: TODO bombs in critical flows
- Evidence:
  - Missing IP capture in session creation: `crates/wavry-gateway/src/auth.rs:110`
  - Missing relay sequence check: `crates/wavry-relay/src/main.rs:371`
  - Non-random session IDs in desktop signaling path: `crates/wavry-desktop/src-tauri/src/lib.rs:635`
- Improvement: Convert to tracked issues with deadlines and owner.

---

## Networking Correctness Findings

### N-01: WebTransport + WebRTC integration is still skeletal
- `wavry-web` provides skeleton interfaces but no full authenticated production runtime behavior.
- Gateway runtime handler currently only logs input/control frames (`crates/wavry-gateway/src/web.rs:223`).
- Fallback logic documented in `docs/WEB_CLIENT.md` is not fully implemented end-to-end in production paths.

### N-02: Input channel reliability choices are directionally correct but enforcement is incomplete
- Datagrams for high-rate input and stream frames for control are sensible.
- Missing anti-replay enforcement in active decrypt paths undermines integrity of “unreliable input” assumptions.

### N-03: Relay bypass and endpoint trust assumptions
- Signaling can deliver relay endpoint/address data to clients; clients accept and use it (`crates/wavry-desktop/src-tauri/src/lib.rs:456`, `crates/wavry-client/src/client.rs:279`).
- Session IDs in desktop host signaling are currently static zeros (`crates/wavry-desktop/src-tauri/src/lib.rs:635`, `crates/wavry-desktop/src-tauri/src/lib.rs:1116`), increasing collision/confusion risk.

---

## Code Organization / Architecture Hygiene

### Observations
- Oversized modules with mixed concerns:
  - `crates/wavry-client/src/client.rs` (~1666 LOC)
  - `crates/wavry-desktop/src-tauri/src/lib.rs` (~1385 LOC)
  - `crates/wavry-media/src/linux.rs` (~915 LOC)
  - `crates/wavry-server/src/main.rs` (~819 LOC)
- Duplicated signaling/offer handling logic across platform-specific host paths in desktop code.
- Divergent relay implementations (`crates/wavry-gateway/src/relay.rs` vs `crates/wavry-relay/src/main.rs`) create trust-boundary ambiguity.

### Suggested modular splits
- `wavry-gateway`
  - `auth/handlers.rs`, `auth/session_store.rs`, `auth/rate_limit.rs`
  - `signaling/ws.rs`, `signaling/webrtc_http.rs`, `signaling/relay_credentials.rs`
  - shared `middleware/` for auth extraction, origin checks, and request limits.
- `wavry-desktop/src-tauri`
  - split per-role commands (`auth_commands.rs`, `host_commands.rs`, `session_commands.rs`) plus shared signaling client wrapper.
- `wavry-client`
  - isolate crypto/session transport, NAT traversal, and input pipelines into separate modules with explicit interfaces.

---

## Container / Deployment Hygiene

### What is visible
- No Dockerfile / compose / K8s manifests are present in repo root paths scanned.
- Services default to listening on public interfaces (`0.0.0.0`).

### Implication
- Production-safe defaults (non-root user, minimal capabilities, explicit port exposure, secret injection strategy, read-only FS, seccomp/apparmor, volume constraints) are not codified here.

### Recommendation
- Add reference deployment manifests with least-privilege defaults and explicit secret/env handling.

---

## Concrete Action List (Prioritized)

1. Implement and enforce replay windows in server/client/relay receive paths.
2. Add request and message rate limiting (auth routes, WS bind, WS message throughput, WebRTC signaling endpoints).
3. Replace unbounded WS channels with bounded queues and backpressure/drop policy.
4. Enforce signed relay leases in non-dev mode; fail startup if `master_public_key` missing.
5. Remove/guard deploy-unsafe master behavior (mock auth, permissive CORS) behind explicit dev-only feature flags.
6. Unify relay protocol implementation (client/gateway/relay) and remove placeholder relay endpoint values.
7. Move session tokens and TOTP secrets to safer storage models (hashed tokens, encrypted TOTP seeds).
8. Complete session lifecycle APIs: logout/logout-all, rotation, idle expiry, scheduled cleanup.
9. Add WS origin allowlist checks and explicit CORS policy (not permissive defaults) for HTTP browser endpoints.
10. Redact DB DSN logs and define production logging policy for sensitive values.
11. Move desktop signaling token storage from `localStorage` to secure storage and re-enable CSP in Tauri config.
12. Break up oversized modules and consolidate duplicated signaling flows.

