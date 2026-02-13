# Gateway Server Operations Guide

**Version:** 0.0.5-unstable  
**Last Updated:** 2026-02-13

This document provides operational guidance for deploying and managing Wavry gateway servers (authentication and signaling).

---

## Table of Contents

1. [Deployment](#deployment)
2. [Database Management](#database-management)
3. [Monitoring](#monitoring)
4. [User Management](#user-management)
5. [Troubleshooting](#troubleshooting)
6. [Security](#security)
7. [Backup and Recovery](#backup-and-recovery)

---

## Deployment

### Prerequisites

- Linux server (or Docker)
- Minimum 1 vCPU, 1GB RAM
- SQLite (bundled) or PostgreSQL
- TLS certificate (for production)

### Docker Deployment (Recommended)

```bash
# Pull the latest gateway image
docker pull ghcr.io/bybrooklyn/wavry-gateway:latest

# Create persistent volume for database
docker volume create wavry-gateway-data

# Run with environment configuration
docker run -d \
  --name wavry-gateway \
  --restart unless-stopped \
  -p 8080:8080/tcp \
  -p 4001:4001/udp \
  -v wavry-gateway-data:/data \
  -e DATABASE_URL=sqlite:/data/gateway.db \
  -e WAVRY_GATEWAY_ALLOW_PUBLIC_BIND=1 \
  -e WAVRY_ADMIN_TOKEN=<secure_random_token> \
  -e WAVRY_TOTP_ENCRYPTION_KEY=<base64_32byte_key> \
  -e ALLOWED_ORIGINS=https://app.wavry.dev \
  ghcr.io/bybrooklyn/wavry-gateway:latest
```

### Environment Variables

| Variable | Default | Description |
|----------|---------|-------------|
| `DATABASE_URL` | `sqlite:gateway.db` | Database connection string |
| `WAVRY_GATEWAY_ALLOW_PUBLIC_BIND` | `0` | Allow binding to public IPs |
| `WAVRY_ADMIN_TOKEN` | None | Admin dashboard authentication token |
| `WAVRY_TOTP_ENCRYPTION_KEY` | Generated | Base64-encoded 32-byte key for TOTP secret encryption |
| `ALLOWED_ORIGINS` | Localhost only | Comma-separated CORS origins |
| `WAVRY_GATEWAY_RELAY_PORT` | `0` (random) | Internal relay server port |
| `WAVRY_RELAY_SESSION_TTL_SECS` | `300` | Relay session timeout |
| `WAVRY_RELAY_SESSION_LIMIT` | `4096` | Maximum concurrent relay sessions |
| `RUST_LOG` | `wavry_gateway=info` | Logging level |

### Generate Secure Keys

```bash
# Generate admin token (32 random bytes, base64-encoded)
openssl rand -base64 32

# Generate TOTP encryption key (32 random bytes, base64-encoded)
openssl rand -base64 32
```

---

## Database Management

### Schema Migrations

The gateway automatically runs migrations on startup using `sqlx::migrate!`.

Current schema includes:
- `users` - User accounts with credentials and public keys
- `sessions` - Active authentication sessions
- `user_bans` - Permanent and temporary bans
- `login_failures` - Brute-force tracking
- `relay_stats` - Relay performance metrics
- `admin_audit_log` - Admin action audit trail

### Database Backup (SQLite)

```bash
# Online backup using SQLite .backup command
sqlite3 /data/gateway.db ".backup /backup/gateway-$(date +%Y%m%d-%H%M%S).db"

# Copy database file (offline only)
docker stop wavry-gateway
docker cp wavry-gateway:/data/gateway.db ./gateway-backup.db
docker start wavry-gateway
```

### Database Restore

```bash
# Stop gateway
docker stop wavry-gateway

# Restore from backup
docker cp gateway-backup.db wavry-gateway:/data/gateway.db

# Start gateway
docker start wavry-gateway
```

### PostgreSQL Migration (Optional)

To use PostgreSQL instead of SQLite:

1. Update `DATABASE_URL`:
   ```
   DATABASE_URL=postgres://user:pass@localhost/wavry_gateway
   ```

2. Rebuild with PostgreSQL support:
   ```bash
   cargo build --release --features postgres
   ```

3. Note: Migrations are managed via `sqlx::migrate!` and should run automatically.

---

## Monitoring

### Health Endpoints

#### `/health`
Returns active connection and session counts.

```bash
curl http://localhost:8080/health
```

Response:
```json
{
  "active_ws_connections": 23,
  "active_relay_sessions": 15
}
```

#### `/metrics/runtime`
Alias for `/health` with runtime statistics.

#### `/metrics/auth`
Authentication-specific metrics (login attempts, 2FA usage, etc.).

```bash
curl http://localhost:8080/metrics/auth
```

#### `/metrics/prometheus` (New!)
Prometheus-compatible metrics.

```bash
curl http://localhost:8080/metrics/prometheus
```

Example output:
```
# HELP wavry_gateway_websocket_connections Active WebSocket connections
# TYPE wavry_gateway_websocket_connections gauge
wavry_gateway_websocket_connections 23
# HELP wavry_gateway_relay_sessions Active relay sessions
# TYPE wavry_gateway_relay_sessions gauge
wavry_gateway_relay_sessions 15
```

### Key Metrics to Monitor

| Metric | Description | Alert Threshold |
|--------|-------------|-----------------|
| `active_ws_connections` | Active WebSocket signaling connections | Monitor for anomalies |
| `active_relay_sessions` | Active UDP relay sessions | Monitor for anomalies |
| Session cleanup rate | Expired sessions cleaned per interval | Should be > 0 periodically |
| Database size | SQLite file size | Plan migration if > 1GB |
| Login failure rate | Failed authentication attempts | > 10/min may indicate attack |

### Prometheus Integration

```yaml
scrape_configs:
  - job_name: 'wavry-gateway'
    static_configs:
      - targets: ['gateway.example.com:8080']
    metrics_path: '/metrics/prometheus'
    scrape_interval: 15s
```

---

## User Management

### Admin Dashboard

Access the admin dashboard at: `http://localhost:8080/admin`

Authenticate using the `WAVRY_ADMIN_TOKEN` environment variable.

**Features:**
- User overview (total users, banned users, active sessions)
- Session revocation
- User banning (permanent or temporary with expiration)
- Audit log viewing

### Manual User Operations (SQL)

```sql
-- List all users
SELECT id, username, email, public_key, created_at, totp_enabled FROM users;

-- Find user by email
SELECT * FROM users WHERE email = 'user@example.com';

-- Revoke all sessions for a user
DELETE FROM sessions WHERE user_id = (SELECT id FROM users WHERE email = 'user@example.com');

-- Check active sessions
SELECT COUNT(*) FROM sessions WHERE expires_at > datetime('now');

-- View login failures
SELECT * FROM login_failures WHERE ip_address = '203.0.113.1' ORDER BY attempted_at DESC;

-- Check user bans
SELECT u.username, u.email, ub.reason, ub.banned_at, ub.expires_at 
FROM user_bans ub 
JOIN users u ON ub.user_id = u.id 
WHERE ub.expires_at IS NULL OR ub.expires_at > datetime('now');
```

### Password Reset (Manual)

There is no built-in password reset. Users must:
1. Contact admin for account access
2. Admin can manually delete user from database
3. User re-registers with same identity (public key)

Future enhancement: Add password reset flow with email verification.

---

## Troubleshooting

### Issue: Admin dashboard not accessible

**Symptoms:**
- 401 Unauthorized on `/admin` endpoint

**Resolution:**
1. Verify `WAVRY_ADMIN_TOKEN` is set correctly
2. Check browser sends correct token in request
3. Restart gateway after changing token:
   ```bash
   docker restart wavry-gateway
   ```

---

### Issue: Users cannot register/login

**Symptoms:**
- Registration/login requests return errors
- `/auth/register` or `/auth/login` failing

**Diagnosis:**
```bash
# Check gateway logs
docker logs wavry-gateway | grep -i "auth"

# Test registration endpoint
curl -X POST http://localhost:8080/auth/register \
  -H "Content-Type: application/json" \
  -d '{"username":"test","email":"test@example.com","password":"SecurePass123!","public_key":"..."}'
```

**Common Causes:**
1. Database connection failure
2. Rate limiting (429 Too Many Requests)
3. Validation errors (weak password, duplicate email/username)
4. CORS issues (browser blocks request)

**Resolution:**
- Check database connectivity
- Verify rate limits aren't too restrictive
- Ensure password meets requirements (min 8 chars)
- Add client origin to `ALLOWED_ORIGINS`

---

### Issue: TOTP/2FA setup failing

**Symptoms:**
- `/auth/2fa/setup` returns 500 Internal Server Error
- QR code generation fails

**Diagnosis:**
```bash
# Check if TOTP encryption key is valid
docker exec wavry-gateway printenv WAVRY_TOTP_ENCRYPTION_KEY

# Check logs for encryption errors
docker logs wavry-gateway | grep -i "totp"
```

**Resolution:**
- Ensure `WAVRY_TOTP_ENCRYPTION_KEY` is valid base64-encoded 32-byte key
- Regenerate key if corrupted:
  ```bash
  openssl rand -base64 32
  ```
- Restart gateway with new key (existing TOTP secrets will be invalid)

---

### Issue: WebSocket connections failing

**Symptoms:**
- `/signal` WebSocket upgrade fails
- Clients cannot establish signaling connection

**Diagnosis:**
```bash
# Test WebSocket endpoint
wscat -c ws://localhost:8080/signal

# Check for firewall/proxy issues
curl -i http://localhost:8080/signal
```

**Common Causes:**
1. Reverse proxy not configured for WebSocket upgrade
2. CORS blocking WebSocket handshake
3. Rate limiting

**Resolution:**
- Configure proxy for WebSocket support (Nginx example):
  ```nginx
  location /signal {
      proxy_pass http://gateway:8080;
      proxy_http_version 1.1;
      proxy_set_header Upgrade $http_upgrade;
      proxy_set_header Connection "upgrade";
  }
  ```
- Add client origin to `ALLOWED_ORIGINS`
- Check rate limiter thresholds

---

### Issue: High memory usage

**Symptoms:**
- Gateway memory steadily increasing
- OOM killer terminates process

**Diagnosis:**
```bash
# Check active sessions and connections
curl http://localhost:8080/health

# Check database size
docker exec wavry-gateway du -h /data/gateway.db
```

**Common Causes:**
1. Expired sessions not cleaned up
2. Database growth (audit logs, etc.)
3. WebSocket connection leaks

**Resolution:**
- Verify session cleanup task is running (check logs)
- Prune old audit logs manually:
  ```sql
  DELETE FROM admin_audit_log WHERE timestamp < datetime('now', '-30 days');
  ```
- Restart gateway to clear in-memory state
- Consider PostgreSQL for better memory management with large datasets

---

## Security

### Rate Limiting

The gateway implements multiple rate limiters:

| Limiter | Default Limit | Endpoint Coverage |
|---------|---------------|-------------------|
| AUTH_LIMITER | 5 req/min | `/auth/register`, `/auth/login` |
| POST_AUTH_LIMITER | 20 req/min | `/auth/logout`, `/auth/2fa/*` |
| WEBRTC_LIMITER | 30 req/min | `/webrtc/*` |
| WS_BIND_LIMITER | 10 req/min | `/signal` WebSocket |
| GLOBAL_API_LIMITER | 100 req/min | All other API endpoints |

Rate limits are per-IP and use a fixed-window algorithm.

### TOTP Secret Encryption

TOTP secrets are encrypted at rest using XChaCha20-Poly1305 AEAD.

**Format:** `enc:v1:<base64_ciphertext>`

**Key Management:**
- Key stored in `WAVRY_TOTP_ENCRYPTION_KEY` environment variable
- Key rotation requires re-encrypting all secrets (not currently supported)
- Backup key securely; loss means all users must reset 2FA

### Session Token Hashing

Session tokens are hashed with SHA-256 before storage.

**Format:** `h1:<hex_hash>`

### CORS Configuration

By default, only localhost origins are allowed:
- `http://localhost:1420`
- `http://127.0.0.1:1420`
- `http://localhost:3000`
- `http://127.0.0.1:3000`
- `tauri://localhost`

**Production:** Set `ALLOWED_ORIGINS` to your application's domain:
```bash
ALLOWED_ORIGINS=https://app.wavry.dev,https://www.wavry.dev
```

### Audit Logging

All admin actions are logged to `admin_audit_log` table:
- Session revocations
- User bans/unbans
- Admin login attempts

Query recent admin actions:
```sql
SELECT * FROM admin_audit_log ORDER BY timestamp DESC LIMIT 50;
```

---

## Backup and Recovery

### What to Backup

1. **Database** (`gateway.db` or PostgreSQL dump)
   - Contains all user accounts, sessions, and audit logs
   - Backup frequency: Daily minimum, hourly recommended

2. **TOTP Encryption Key**
   - Environment variable `WAVRY_TOTP_ENCRYPTION_KEY`
   - Store securely (e.g., HashiCorp Vault, AWS Secrets Manager)
   - Loss requires all users to reset 2FA

3. **Admin Token**
   - Environment variable `WAVRY_ADMIN_TOKEN`
   - Can be regenerated if lost (no data impact)

### Backup Script Example

```bash
#!/bin/bash
# Daily gateway database backup

BACKUP_DIR=/backup/wavry-gateway
DATE=$(date +%Y%m%d-%H%M%S)
DB_PATH=/data/gateway.db

# Create backup directory
mkdir -p $BACKUP_DIR

# Backup database
docker exec wavry-gateway sqlite3 $DB_PATH ".backup /tmp/gateway-$DATE.db"
docker cp wavry-gateway:/tmp/gateway-$DATE.db $BACKUP_DIR/

# Compress and encrypt backup
tar -czf $BACKUP_DIR/gateway-$DATE.tar.gz -C $BACKUP_DIR gateway-$DATE.db
gpg --encrypt --recipient admin@wavry.dev $BACKUP_DIR/gateway-$DATE.tar.gz

# Clean up unencrypted files
rm $BACKUP_DIR/gateway-$DATE.db $BACKUP_DIR/gateway-$DATE.tar.gz

# Retain last 30 days of backups
find $BACKUP_DIR -name "gateway-*.gpg" -mtime +30 -delete

echo "Backup completed: gateway-$DATE.tar.gz.gpg"
```

### Disaster Recovery

1. **Restore from backup:**
   ```bash
   # Decrypt backup
   gpg --decrypt gateway-backup.tar.gz.gpg | tar -xzf -
   
   # Stop gateway
   docker stop wavry-gateway
   
   # Restore database
   docker cp gateway-backup.db wavry-gateway:/data/gateway.db
   
   # Start gateway
   docker start wavry-gateway
   ```

2. **Verify restored state:**
   ```bash
   # Check user count
   curl http://localhost:8080/admin/api/overview
   
   # Test login
   curl -X POST http://localhost:8080/auth/login \
     -H "Content-Type: application/json" \
     -d '{"email":"admin@example.com","password":"..."}'
   ```

3. **Notify users if necessary:**
   - If sessions were lost, users must log in again
   - If TOTP key was lost, all users must re-enroll 2FA

---

## Support

- **Documentation:** https://github.com/bybrooklyn/wavry/tree/main/docs
- **Issues:** https://github.com/bybrooklyn/wavry/issues
- **Email:** support@wavry.dev

