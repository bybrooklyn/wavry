# Wavry Gateway — Design Specification

**Status:** Current  
**Last Updated:** 2026-02-09

Wavry Gateway is the central coordination service for authentication, session signaling, relay coordination, and WebRTC bridge support. It is a **signaling + auth service**, not a transport—Gateway never sees encrypted stream data.

---

## Production Endpoint

The production instance of Wavry Gateway is available at:  
**`https://auth.wavry.dev`**

Clients should default to this URL for all signaling, unless manually overridden for development.

---

## Design Principles

| Principle | Rationale |
|-----------|-----------|
| **Gateway is blind** | Never proxies or inspects media; only coordinates connections |
| **P2P by default** | Relays are optional fallbacks, not the primary path |
| **Email-based auth** | Simple email/password with optional TOTP 2FA |
| **Ephemeral sessions** | Sessions expire after 24 hours; no long-lived tokens |
| **SQLite simplicity** | Single-file database for easy deployment |

---

## 1. Identity System

### 1.1 User Registration

New users register with email, password, and profile information:

```
Client                                  Gateway
   |                                       |
   |  POST /auth/register                  |
   |  { email, password, display_name,     |
   |    username, public_key }             |
   |-------------------------------------->|
   |                                       |
   |  201 { user, session, token }         |
   |<--------------------------------------|
```

**Password Security:**
- Passwords are hashed using **Argon2id** (memory-hard password hashing)
- Minimum password length enforced
- Rate limiting on registration attempts

### 1.2 Login Flow

Returning users authenticate with email/password and optional TOTP:

```
Client                                  Gateway
   |                                       |
   |  POST /auth/login                     |
   |  { email, password, totp_code? }      |
   |-------------------------------------->|
   |                                       |
   |  200 { user, session, token }         |
   |  OR 403 { totp_required: true }       |
   |<--------------------------------------|
```

### 1.3 Session Tokens

Sessions are identified by high-entropy random tokens (256-bit):

- **Token Format:** Hex-encoded 32-byte random value
- **Storage:** Only SHA-256 hash stored in database (actual token returned to client)
- **Expiry:** 24 hours from creation
- **Transport:** Sent via `Authorization: Bearer <token>` header or `X-Session-Token` header

**Session Claims:**

```json
{
  "token": "<hex-encoded-random>",
  "user_id": "<uuid>",
  "expires_at": "2026-02-10T12:00:00Z",
  "ip_address": "203.0.113.42",
  "created_at": "2026-02-09T12:00:00Z"
}
```

### 1.4 TOTP Two-Factor Authentication

Users can optionally enable TOTP-based 2FA:

1. **Setup:** `POST /auth/2fa/setup` → Returns secret + QR code PNG
2. **Enable:** `POST /auth/2fa/enable` → Verifies code and activates 2FA
3. **Login:** If 2FA enabled, `totp_code` required in login request

TOTP secrets are encrypted at rest using XChaCha20-Poly1305.

---

## 2. WebSocket Signaling

Clients establish WebSocket connections for real-time session coordination:

```
Client                                  Gateway
   |                                       |
   |  GET /ws (upgrade)                    |
   |  Headers: Authorization: Bearer <tok> |
   |-------------------------------------->|
   |                                       |
   |  101 Switching Protocols              |
   |<--------------------------------------|
```

**WebSocket Features:**
- Bidirectional JSON message protocol
- Heartbeat/ping-pong handling
- Connection multiplexing (one WS per user, multiple sessions)
- Automatic cleanup on disconnect

### 2.1 Message Types

**Client → Gateway:**

| Message | Purpose |
|:--------|:--------|
| `RegisterHost` | Announce host availability with endpoints |
| `UnregisterHost` | Remove host from discovery |
| `ConnectRequest` | Request connection to a host |
| `IceCandidate` | WebRTC ICE candidate exchange |
| `Disconnect` | Close session gracefully |

**Gateway → Client:**

| Message | Purpose |
|:--------|:--------|
| `HostRegistered` | Confirmation of host registration |
| `ConnectOffer` | Incoming connection request from client |
| `ConnectAccept` | Host accepted connection |
| `ConnectReject` | Host rejected connection |
| `IceCandidate` | Forwarded ICE candidate |
| `SessionEnded` | Session terminated |
| `Error` | Protocol or session error |

---

## 3. WebRTC Bridge

Gateway provides WebRTC configuration and signaling for browser-based clients:

### 3.1 WebRTC Endpoints

| Method | Path | Description |
|:-------|:-----|:------------|
| GET | `/webrtc/config` | Get ICE servers and config |
| POST | `/webrtc/offer` | Submit SDP offer |
| POST | `/webrtc/answer` | Submit SDP answer |
| POST | `/webrtc/candidate` | Exchange ICE candidates |

### 3.2 WebRTC Flow

```
Browser Client                          Gateway
     |                                     |
     |  GET /webrtc/config                 |
     |------------------------------------>|
     |  { ice_servers: [...] }             |
     |<------------------------------------|
     |                                     |
     |  POST /webrtc/offer                 |
     |  { sdp, session_id }                |
     |------------------------------------>|
     |                                     |
     |  (Signaling via WebSocket to Host)  |
     |                                     |
     |  POST /webrtc/answer (from Host)    |
     |<------------------------------------|
```

---

## 4. Relay Coordination

Gateway coordinates with external relay nodes for NAT traversal:

### 4.1 Relay Reporting

Relays report their status and client feedback:

| Method | Path | Auth | Description |
|:-------|:-----|:-----|:------------|
| POST | `/v1/relays/report` | None | Report relay session quality |
| GET | `/v1/relays/reputation` | None | Get relay reputation scores |

### 4.2 Relay Report Format

```json
{
  "relay_id": "<relay-identifier>",
  "session_id": "<session-uuid>",
  "quality_score": 95,
  "bytes_transferred": 10485760,
  "duration_secs": 300,
  "issues": ["packet_loss", "latency_spike"]
}
```

---

## 5. Admin Operations

Admin panel available at `/admin` for system management:

### 5.1 Admin Endpoints

| Method | Path | Description |
|:-------|:-----|:------------|
| GET | `/admin` | Admin panel HTML interface |
| GET | `/admin/api/overview` | System overview stats |
| POST | `/admin/api/sessions/revoke` | Revoke user session |
| POST | `/admin/api/ban` | Ban user by email |
| POST | `/admin/api/unban` | Remove user ban |

### 5.2 Metrics Endpoints

| Method | Path | Description |
|:-------|:-----|:------------|
| GET | `/metrics/runtime` | Active connections and sessions |
| GET | `/metrics/auth` | Authentication attempt metrics |

---

## 6. Data Model

### 6.1 SQLite Schema

#### `users` Table

| Column | Type | Constraints |
|:-------|:-----|:------------|
| `id` | TEXT | PRIMARY KEY (UUID) |
| `email` | TEXT | UNIQUE, NOT NULL |
| `username` | TEXT | UNIQUE, NOT NULL |
| `password_hash` | TEXT | NOT NULL (Argon2) |
| `display_name` | TEXT | NOT NULL |
| `public_key` | TEXT | Ed25519 public key |
| `totp_secret` | TEXT | Encrypted TOTP secret |
| `created_at` | DATETIME | DEFAULT CURRENT_TIMESTAMP |
| `is_banned` | INTEGER | DEFAULT 0 |

#### `sessions` Table

| Column | Type | Constraints |
|:-------|:-----|:------------|
| `token` | TEXT | PRIMARY KEY (hashed) |
| `user_id` | TEXT | FOREIGN KEY → users |
| `expires_at` | DATETIME | NOT NULL |
| `ip_address` | TEXT | Client IP |
| `created_at` | DATETIME | DEFAULT CURRENT_TIMESTAMP |

### 6.2 Entity Relationships

```
┌─────────┐       ┌──────────┐
│  users  │◄──────┤ sessions │
└─────────┘ 1:M   └──────────┘
```

---

## 7. Security Considerations

### 7.1 Password Security

- **Algorithm:** Argon2id (winner of Password Hashing Competition)
- **Parameters:** Configurable memory, iterations, parallelism
- **Storage:** Never store plaintext passwords

### 7.2 Session Security

- **Token Entropy:** 256-bit random values
- **Hash Storage:** SHA-256 hash stored, original token only in client memory
- **Expiry:** 24-hour maximum lifetime
- **Revocation:** Immediate via admin API
- **Binding:** Optional IP address binding for additional security

### 7.3 Rate Limiting

Fixed-window rate limiting on authentication endpoints:

- **Registration:** 5 attempts per 15 minutes per IP
- **Login:** 10 attempts per 5 minutes per IP
- **WebRTC:** 20 requests per minute per IP

### 7.4 CORS Policy

Configurable allowed origins via environment:
- Default allows localhost development origins
- Production requires explicit origin whitelist
- Credentials (cookies/auth headers) allowed

### 7.5 TOTP Security

- **Algorithm:** SHA-1 (30-second windows, 6 digits)
- **Storage:** Encrypted with XChaCha20-Poly1305
- **Backup:** No backup codes (account recovery via admin)

---

## 8. API Reference

### 8.1 Authentication Endpoints

| Method | Path | Auth | Description |
|:-------|:-----|:-----|:------------|
| POST | `/auth/register` | None | Create new account |
| POST | `/auth/login` | None | Authenticate |
| POST | `/auth/logout` | Bearer | Revoke session |
| POST | `/auth/2fa/setup` | Bearer | Generate TOTP secret |
| POST | `/auth/2fa/enable` | Bearer | Activate TOTP |

### 8.2 Signaling Endpoints

| Method | Path | Auth | Description |
|:-------|:-----|:-----|:------------|
| GET | `/ws` | Bearer | WebSocket upgrade |

### 8.3 WebRTC Endpoints

| Method | Path | Auth | Description |
|:-------|:-----|:-----|:------------|
| GET | `/webrtc/config` | None | ICE server configuration |
| POST | `/webrtc/offer` | Bearer | Submit SDP offer |
| POST | `/webrtc/answer` | Bearer | Submit SDP answer |
| POST | `/webrtc/candidate` | Bearer | ICE candidate exchange |

### 8.4 Health & Monitoring

| Method | Path | Auth | Description |
|:-------|:-----|:-----|:------------|
| GET | `/` | None | Health check (returns "Wavry Gateway Online") |
| GET | `/health` | None | Runtime metrics |

---

## 9. Error Response Format

```json
{
  "error": "invalid_credentials",
  "message": "Email or password is incorrect"
}
```

### 9.1 Standard Error Codes

| Code | HTTP Status | Description |
|:-----|:------------|:------------|
| `invalid_request` | 400 | Malformed request body |
| `invalid_credentials` | 401 | Wrong email/password |
| `totp_required` | 403 | 2FA code required |
| `invalid_totp` | 401 | Wrong TOTP code |
| `unauthorized` | 401 | Missing or invalid token |
| `forbidden` | 403 | Insufficient permissions |
| `user_banned` | 403 | Account suspended |
| `rate_limited` | 429 | Too many requests |
| `internal_error` | 500 | Server error |

---

## 10. Environment Configuration

| Variable | Default | Description |
|:---------|:--------|:------------|
| `WAVRY_GATEWAY_BIND_ADDR` | `0.0.0.0:3000` | HTTP server bind address |
| `WAVRY_GATEWAY_RELAY_PORT` | `0` | UDP relay port (0 = random) |
| `DATABASE_URL` | `sqlite:gateway.db` | SQLite database path |
| `RUST_LOG` | `wavry_gateway=info` | Logging level |
| `WAVRY_ALLOW_PUBLIC_BIND` | `false` | Allow non-loopback binding |
| `WAVRY_RELAY_SESSION_TTL_SECS` | `300` | Relay session lifetime |
| `WAVRY_RELAY_SESSION_LIMIT` | `4096` | Max relay sessions |
| `WAVRY_ENABLE_INSECURE_WEBTRANSPORT_RUNTIME` | `false` | Enable WebTransport (dev only) |
| `CORS_ORIGINS` | (localhost defaults) | Allowed CORS origins |

---

## 11. Rust Implementation

### 11.1 Crate Structure

```
crates/wavry-gateway/
├── Cargo.toml
├── migrations/
│   └── *.sql
└── src/
    ├── main.rs           # Entry point, axum router
    ├── lib.rs            # Module exports
    ├── auth.rs           # /auth/* endpoints
    ├── db.rs             # Database operations
    ├── security.rs       # Crypto, rate limiting, CORS
    ├── signal.rs         # WebSocket signaling
    ├── web.rs            # WebRTC bridge endpoints
    ├── relay.rs          # UDP relay coordination
    └── admin.rs          # Admin panel and API
```

### 11.2 Dependencies

```toml
[dependencies]
axum = "0.7"                    # HTTP framework
tokio = { version = "1", features = ["full"] }
sqlx = { version = "0.8", features = ["sqlite", "runtime-tokio"] }
argon2 = "0.5"                  # Password hashing
totp-rs = "5"                   # TOTP generation/verification
chacha20poly1305 = "0.10"       # TOTP secret encryption
uuid = { version = "1", features = ["v4"] }
serde = { version = "1", features = ["derive"] }
tracing = "0.1"
```

---

## Related Documents

- [RIFT_SPEC_V1.md](RIFT_SPEC_V1.md) — RIFT protocol specification
- [WAVRY_ARCHITECTURE.md](WAVRY_ARCHITECTURE.md) — System architecture overview
- [WAVRY_SECURITY.md](WAVRY_SECURITY.md) — Security model and threat mitigations
- [WAVRY_RELAY.md](WAVRY_RELAY.md) — Relay node specification
- [WEB_CLIENT.md](WEB_CLIENT.md) — WebTransport/WebRTC client details
