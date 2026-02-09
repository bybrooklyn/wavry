# Wavry Admin Dashboard

**Status**: Implementation Complete, Testing Phase
**Components**: Gateway Admin Panel + SQLite Database
**Access Control**: Token-based authentication

---

## Overview

The Wavry Admin Dashboard provides gateway operators with tools to:
- View system overview (users, sessions, activity)
- Manage active sessions (revoke, monitor)
- Manage user accounts (ban/unban, view details)
- Monitor gateway health and relay usage

## Architecture

### Backend
- **Service**: `wavry-gateway` (Axum HTTP server)
- **Authentication**: Token-based via `ADMIN_PANEL_TOKEN` environment variable
- **Database**: SQLite (wavry.db)
- **Endpoints**: HTTP REST API at `/admin/api/*`

### Frontend
- **Tech**: HTML/CSS/JavaScript (served from `/admin` route)
- **Framework**: Vanilla JS (no external dependencies)
- **Features**: Real-time updates, session management, user controls

### Data Models

#### User
```rust
pub struct User {
    pub id: String,                  // UUID
    pub email: String,               // Email address
    pub username: String,            // Display username
    pub display_name: String,        // Full name
    pub password_hash: String,       // Argon2id hash
    pub totp_secret: Option<String>, // 2FA secret
    pub created_at: DateTime<Utc>,   // Registration timestamp
}
```

#### Session
```rust
pub struct Session {
    pub token: String,              // Bearer token (SHA-256 hash stored)
    pub user_id: String,            // Foreign key to users
    pub expires_at: DateTime<Utc>,  // 24-hour expiry
    pub ip_address: Option<String>, // Client IP
    pub created_at: DateTime<Utc>,  // Creation timestamp
}
```

#### Banned User
```rust
pub struct BannedUser {
    pub user_id: String,
    pub reason: String,
    pub created_at: DateTime<Utc>,
    pub expires_at: Option<DateTime<Utc>>, // NULL = permanent ban
}
```

---

## Setup & Configuration

### Prerequisites
1. Wavry Gateway running with SQLite database initialized
2. Admin panel token (32+ character random string)
3. HTTP access to gateway (default: http://localhost:3000)

### Initialization

#### Step 1: Generate Admin Token
```bash
ADMIN_TOKEN=$(openssl rand -hex 32)
echo "Save this token: $ADMIN_TOKEN"
```

#### Step 2: Start Gateway with Admin Token
```bash
export ADMIN_PANEL_TOKEN="$ADMIN_TOKEN"
cargo run --bin wavry-gateway
```

#### Step 3: Create Test Users (Optional)
```bash
./scripts/setup-admin-users.sh
```

### Environment Variables
```bash
# Required for admin panel access
ADMIN_PANEL_TOKEN=<32+ character hex string>

# Optional gateway config
WAVRY_DB_PATH=.wavry/gateway.db    # Default database location
WAVRY_GATEWAY_BIND=0.0.0.0:3000    # Default bind address
WAVRY_LOG_LEVEL=info               # Default log level
```

---

## API Reference

### Authentication
All admin API requests require one of:

**Header Method 1: Token Header**
```bash
curl -H "x-admin-token: YOUR_TOKEN_HERE" http://localhost:3000/admin/api/overview
```

**Header Method 2: Bearer Token**
```bash
curl -H "Authorization: Bearer YOUR_TOKEN_HERE" http://localhost:3000/admin/api/overview
```

### Endpoints

#### 1. GET /admin/api/overview
Returns system statistics and recent activity.

**Response (200 OK)**
```json
{
    "users_total": 42,
    "sessions_total": 128,
    "sessions_active": 15,
    "recent_users": [
        {
            "id": "uuid-1",
            "email": "user@example.com",
            "username": "john_doe",
            "display_name": "John Doe",
            "has_totp": false,
            "created_at": "2026-02-09T10:30:00Z"
        }
    ],
    "recent_sessions": [
        {
            "token": "xxxxxxx...xxx",
            "user_id": "uuid-1",
            "email": "user@example.com",
            "username": "john_doe",
            "expires_at": "2026-02-10T10:30:00Z",
            "ip_address": "192.168.1.100",
            "created_at": "2026-02-09T10:30:00Z"
        }
    ],
    "active_bans": [
        {
            "user_id": "uuid-2",
            "reason": "Violation of ToS",
            "created_at": "2026-02-08T15:00:00Z",
            "expires_at": null
        }
    ]
}
```

**Errors**
- `401 Unauthorized` - Missing or invalid admin token
- `500 Internal Server Error` - Database error

---

#### 2. POST /admin/api/sessions/revoke
Revokes an active session, forcing re-authentication.

**Request**
```json
{
    "token": "the_session_token_to_revoke"
}
```

**Response (200 OK)**
```json
{
    "revoked": true
}
```

**Errors**
- `401 Unauthorized` - Invalid token
- `404 Not Found` - Session not found
- `500 Internal Server Error` - Database error

---

#### 3. POST /admin/api/users/ban
Temporarily or permanently ban a user.

**Request**
```json
{
    "user_id": "uuid-of-user-to-ban",
    "reason": "Spamming relay resources",
    "duration_hours": 48
}
```

**Response (200 OK)**
```json
{
    "success": true
}
```

**Duration Handling**
- Omit `duration_hours` for permanent ban
- Set `duration_hours: 0` for immediate unban
- Set `duration_hours: 24` for 24-hour ban

**Errors**
- `401 Unauthorized` - Invalid token
- `400 Bad Request` - Invalid user_id
- `500 Internal Server Error` - Database error

---

#### 4. POST /admin/api/users/unban
Lift a ban from a user.

**Request**
```json
{
    "user_id": "uuid-of-user-to-unban"
}
```

**Response (200 OK)**
```json
{
    "success": true
}
```

**Errors**
- `401 Unauthorized` - Invalid token
- `400 Bad Request` - Invalid user_id or user not banned
- `500 Internal Server Error` - Database error

---

## Web Dashboard

### URL
```
http://localhost:3000/admin
```

### Features

#### Overview Panel
- **Users**: Total registered users + growth chart
- **Sessions**: Active vs. total sessions
- **Recent Logins**: Last 10 users with timestamps
- **Active Bans**: Current banned users list

#### Session Management
- **Table**: All active sessions with user info, IP, expiry
- **Actions**:
  - Revoke session (instant logout)
  - View user profile
  - Ban user from this session

#### User Management
- **Search**: Filter users by email/username
- **Actions**:
  - Ban user (with duration)
  - View session history
  - Delete account (if no active sessions)

#### System Health
- **Relay Status**: Connected relays + packet throughput
- **Gateway Load**: CPU, memory, network usage
- **Database**: Size, query performance, cleanup status

---

## Testing Guide

### Manual Testing

#### Test 1: Verify Admin Access
```bash
TOKEN=$(openssl rand -hex 32)
export ADMIN_PANEL_TOKEN="$TOKEN"

# Start gateway
cargo run --bin wavry-gateway &
GATEWAY_PID=$!

sleep 2

# Test API access
curl -H "x-admin-token: $TOKEN" http://localhost:3000/admin/api/overview

# Cleanup
kill $GATEWAY_PID
```

#### Test 2: Session Revocation
```bash
# Create a regular user session (via authentication endpoint)
# Then revoke it via admin API
curl -X POST \
  -H "x-admin-token: $TOKEN" \
  -H "Content-Type: application/json" \
  -d '{"token": "SESSION_TOKEN_HERE"}' \
  http://localhost:3000/admin/api/sessions/revoke
```

#### Test 3: User Ban/Unban
```bash
# Ban user
curl -X POST \
  -H "x-admin-token: $TOKEN" \
  -H "Content-Type: application/json" \
  -d '{"user_id": "user-id", "reason": "Test ban", "duration_hours": 24}' \
  http://localhost:3000/admin/api/users/ban

# Unban user
curl -X POST \
  -H "x-admin-token: $TOKEN" \
  -H "Content-Type: application/json" \
  -d '{"user_id": "user-id"}' \
  http://localhost:3000/admin/api/users/unban
```

### Automated Testing (Unit Tests)

**File**: `crates/wavry-gateway/src/admin.rs`

Tests cover:
- [x] Token validation (missing, invalid, expired)
- [x] API endpoint authorization
- [x] Session revocation
- [x] User ban/unban logic
- [x] Concurrent requests
- [x] Database transaction integrity

**Run tests**:
```bash
cargo test -p wavry-gateway --lib admin
```

---

## Security Considerations

### Token Management
- **Storage**: Only in environment variables, never in config files
- **Length**: Minimum 32 characters (recommend 64)
- **Rotation**: Change periodically (recommended: monthly)
- **Transmission**: Always over HTTPS in production

### API Security
- **Rate Limiting**: Implement on gateway (not yet)
- **Audit Logging**: All admin actions logged to gateway
- **IP Binding**: Optional IP whitelist for admin token
- **CORS**: Admin endpoints exclude CORS headers (same-origin only)

### Database Security
- **Hashing**: Token hashes stored, not plain tokens
- **Transactions**: All modifications are atomic
- **Backup**: Database backed up before admin operations

---

## Troubleshooting

### Admin Panel Not Accessible
**Problem**: `http://localhost:3000/admin` returns 404
**Solution**:
1. Verify gateway is running: `ps aux | grep wavry-gateway`
2. Check listening port: `netstat -tlnp | grep 3000`
3. Check logs for errors

### Unauthorized (401) Errors
**Problem**: API requests return 401
**Solution**:
1. Verify token is set: `echo $ADMIN_PANEL_TOKEN`
2. Check token length (minimum 32 chars)
3. Verify header format: `x-admin-token` or `Authorization: Bearer`
4. Ensure no trailing whitespace in token

### Database Locked
**Problem**: SQLite "database is locked"
**Solution**:
1. Check for other gateway instances: `pgrep -f wavry-gateway`
2. Kill conflicting process: `pkill -9 wavry-gateway`
3. Verify file permissions: `ls -la .wavry/gateway.db`

---

## Monitoring & Alerting (Future)

Planned enhancements:
- [ ] Prometheus metrics export
- [ ] Slack/email notifications for critical events
- [ ] Grafana dashboard integration
- [ ] Real-time activity feed
- [ ] Session anomaly detection

---

## Related Files

- **Implementation**: `crates/wavry-gateway/src/admin.rs`
- **Database Queries**: `crates/wavry-gateway/src/db.rs`
- **API Routes**: `crates/wavry-gateway/src/main.rs`
- **Setup Script**: `scripts/setup-admin-users.sh`
- **Tests**: `crates/wavry-gateway/tests/admin_integration.rs` (to be created)

---

## References

- [Axum HTTP Framework](https://github.com/tokio-rs/axum)
- [SQLx Query Builder](https://github.com/launchbadge/sqlx)
- [OWASP Admin Interface Security](https://owasp.org/www-community/attacks/attacks_on_business_logic)

