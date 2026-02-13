# Wavry Security & Operations — Design Specification v1.0

**Status:** Current  
**Last Updated:** 2026-02-13  
**Implementation Note:** This document describes the **target security architecture**. Current implementation status:
- ✅ End-to-end encryption via Noise XX (fully implemented)
- ✅ Basic relay forwarding with session validation
- ✅ Gateway authentication with Argon2id + 2FA (fully implemented)
- ✅ Structured security audit logging (fully implemented)
- 🚧 Advanced lease system with PASETO (partial - simpler token model in use)
- 🚧 Kill switches and automated responses (admin manual only)
- 🚧 Advanced Sybil detection (not yet implemented)

This document defines the threat model, security mitigations, operational procedures, and privacy posture for Wavry's relay network.

---

## Table of Contents

1. [Threat Model](#1-threat-model)
   - [1.1 Threat Actors](#11-threat-actors)
   - [1.2 Threat Matrix](#12-threat-matrix)
   - [1.3 Gateway/Control Plane Attack Surface](#13-gatewaycontrol-plane-attack-surface)
2. [Control Plane Security Model](#2-control-plane-security-model)
   - [2.1 Gateway Authentication Architecture](#21-gateway-authentication-architecture)
   - [2.2 Authentication Threat Mitigations](#22-authentication-threat-mitigations)
   - [2.3 Security Audit Logging](#23-security-audit-logging)
3. [Threat Mitigations](#3-threat-mitigations)
4. [End-to-End Encryption](#4-end-to-end-encryption)
5. [Lease Security](#5-lease-security)
6. [Rate Limiting](#6-rate-limiting)
7. [Audit & Observability](#7-audit--observability)
8. [Kill Switches](#8-kill-switches)
9. [Key Management](#9-key-management)
10. [Operational Procedures](#10-operational-procedures)
11. [Privacy Posture](#11-privacy-posture)
12. [Recommended Defaults](#12-recommended-defaults)
13. [Minimum Viable Safe Launch Checklist](#13-minimum-viable-safe-launch-checklist)
7. [Kill Switches](#7-kill-switches)
8. [Key Management](#8-key-management)
9. [Operational Procedures](#9-operational-procedures)
10. [Privacy Posture](#10-privacy-posture)
11. [Recommended Defaults](#11-recommended-defaults)
12. [Minimum Viable Safe Launch Checklist](#12-minimum-viable-safe-launch-checklist)

---

## 1. Threat Model

### 1.1 Threat Actors

| Actor | Capability | Motivation |
|:------|:-----------|:-----------|
| **Script kiddie** | Basic tools, limited resources | Disruption, vandalism |
| **Abusive user** | Valid credentials, moderate skill | Free relay abuse, harassment |
| **Malicious relay operator** | Controls relay, network access | Surveillance, disruption, reputation attacks |
| **Network attacker** | MITM position, traffic analysis | Surveillance, correlation |
| **Compromised Master** | Full Master access | Mass surveillance, service disruption |

### 1.2 Threat Matrix

| ID | Threat | Actor | Impact | Likelihood | Severity |
|:---|:-------|:------|:-------|:-----------|:---------|
| T1 | DDoS amplification via relay | Script kiddie | Relays used to amplify attacks | Medium | High |
| T2 | Packet dropping by malicious relay | Malicious operator | Session quality degradation | Medium | Medium |
| T3 | Jitter/latency injection | Malicious operator | Poor user experience, reputation damage | Medium | Medium |
| T4 | User fingerprinting via relay | Malicious operator | Privacy violation, tracking | High | High |
| T5 | Token theft and replay | Network attacker | Unauthorized relay access | Low | High |
| T6 | Token reuse across sessions | Abusive user | Extended unauthorized access | Low | Medium |
| T7 | Lease forgery | Network attacker | Unauthorized forwarding | Very Low | Critical |
| T8 | Relay registration spam | Abusive user | Pool pollution, resource waste | Medium | Low |
| T9 | Bypassing P2P to abuse relays | Abusive user | Unnecessary relay load | Medium | Low |
| T10 | Master server compromise | APT | Full service compromise | Low | Critical |
| T11 | IP/timing correlation | Network attacker | Session deanonymization | High | Medium |
| T12 | Metadata logging by relay | Malicious operator | Privacy violation | High | Medium |
| T13 | Gateway authentication bypass | Network attacker | Unauthorized account access | Low | Critical |
| T14 | Credential stuffing attack | Abusive user | Account takeover via leaked passwords | Medium | High |
| T15 | Brute force authentication | Script kiddie | Account compromise, service disruption | High | Medium |
| T16 | Session token theft | Network attacker | Account impersonation | Medium | High |
| T17 | Admin dashboard compromise | APT | Control plane access, service disruption | Very Low | Critical |

### 1.3 Gateway/Control Plane Attack Surface

The **Gateway** provides authentication, signaling, and session coordination services. Unlike relay forwarding, the Gateway handles sensitive credential data and session management.

**Attack Surfaces:**
- HTTP/WebSocket endpoints for authentication
- Database containing user credentials and sessions
- Admin dashboard for operational management
- Session token issuance and validation
- Rate limiting and abuse prevention systems

---

## 2. Control Plane Security Model

### 2.1 Gateway Authentication Architecture

The Gateway uses a multi-layer authentication system:

```
┌────────────────────────────────────────────────────────────┐
│ Layer 1: Password Authentication                           │
│ - Argon2id password hashing                                │
│ - Minimum 8 character password requirement                 │
│ - Rate limiting: 10 attempts/min per IP                    │
└────────────────────────────────────────────────────────────┘
                            │
                            ▼
┌────────────────────────────────────────────────────────────┐
│ Layer 2: Two-Factor Authentication (Optional)              │
│ - TOTP-based 2FA (RFC 6238)                                │
│ - Secret encrypted at rest with AES-256                    │
│ - Rolling window validation for clock skew                 │
└────────────────────────────────────────────────────────────┘
                            │
                            ▼
┌────────────────────────────────────────────────────────────┐
│ Layer 3: Session Token Management                          │
│ - Cryptographically random session tokens                  │
│ - SHA-256 hashed before database storage                   │
│ - Configurable expiration (default: 30 days)               │
│ - Revocable via logout or admin action                     │
└────────────────────────────────────────────────────────────┘
```

### 2.2 Authentication Threat Mitigations

| Threat | Mitigation | Implementation Status |
|:-------|:-----------|:---------------------|
| T13: Auth bypass | Input validation + parameterized queries | ✅ Implemented |
| T14: Credential stuffing | Rate limiting + account lockout | ✅ Implemented |
| T15: Brute force | Exponential backoff + IP bans | ✅ Implemented |
| T16: Token theft | HTTPS-only + SHA-256 hashing | ✅ Implemented |
| T17: Admin compromise | Separate admin auth + audit logs | 🚧 Partial |

### 2.3 Security Audit Logging

**Structured audit events** are logged for all authentication operations:

```rust
pub enum SecurityEventType {
    LoginSuccess,      // Successful authentication
    LoginFailure,      // Failed authentication attempt
    Registration,      // New account creation
    TotpEnabled,       // 2FA activated
    Logout,            // Session termination
    RateLimitExceeded, // Abuse prevention triggered
    AccountSuspended,  // Ban enforcement
}

pub enum FailureReason {
    UserNotFound,      // Account doesn't exist
    InvalidPassword,   // Wrong password
    TotpRequired,      // 2FA code missing
    InvalidTotp,       // Wrong 2FA code
    AccountBanned,     // Account suspended
    RateLimited,       // Too many attempts
}
```

**Logged fields:**
- Event type (LOGIN_SUCCESS, LOGIN_FAILURE, etc.)
- Client IP address (for correlation and rate limiting)
- User ID (when available)
- Email (for failed login tracking)
- Failure reason (specific cause of auth failure)
- Timestamp (for audit trail and incident response)

**Log retention:**
- Authentication events: 30 days
- Admin actions: 1 year
- Security incidents: 1 year

**Privacy considerations:**
- Passwords are NEVER logged (not even hashes)
- 2FA secrets are NEVER logged
- Session tokens are logged as SHA-256 hashes only
- IP addresses are retained per compliance requirements

---

## 3. Threat Mitigations

### 2.1 Mitigation Matrix

| Threat | Mitigation | Implementation |
|:-------|:-----------|:---------------|
| T1: DDoS amplification | Lease-gated forwarding + rate limits | Relay validates lease before any forwarding; per-session hard caps |
| T2: Packet dropping | Session success metrics + client feedback | Score drops trigger DEGRADED state; persistent dropping triggers QUARANTINE |
| T3: Jitter injection | Probe RTT + client reports | Detect via probe vs. session latency delta |
| T4: Fingerprinting | E2EE (Noise/snow protocol) | Relay sees only opaque encrypted packets |
| T5: Token theft/replay | Short-lived tokens + sequence windows | 5-minute leases; 128-packet replay window |
| T6: Token reuse | Session binding + unique JTI | Lease bound to session_id; relay tracks JTIs |
| T7: Lease forgery | PASETO v4.public signatures | Ed25519 signature verified against Master pubkey |
| T8: Relay spam | Sybil detection + probation | IP/ASN clustering detection; 7-day probation |
| T9: P2P bypass abuse | Per-user lease caps | Max 10 leases/minute; prefer P2P in selection |
| T10: Master compromise | Key hierarchy + HSM + detection | Separate signing keys; canary tokens; incident response |
| T11: Correlation | Minimal logging + padding | No payload logging; optional traffic padding |
| T12: Metadata logging | Audit requirements + reputation | Published privacy policy; negative feedback affects score |

### 2.2 Defense in Depth Layers

```
┌──────────────────────────────────────────────────────────────┐
│ Layer 1: End-to-End Encryption                               │
│ - Noise protocol between client and server                   │
│ - Relay cannot decrypt any payload                           │
└──────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌──────────────────────────────────────────────────────────────┐
│ Layer 2: Lease Authentication                                │
│ - PASETO v4.public signed by Master                          │
│ - Bound to session, peers, and relay                         │
│ - Short-lived (5-15 minutes)                                 │
└──────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌──────────────────────────────────────────────────────────────┐
│ Layer 3: Replay Protection                                   │
│ - Unique nonce per lease                                     │
│ - Sequence number window (128 packets)                       │
│ - JTI tracking at relay                                      │
└──────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌──────────────────────────────────────────────────────────────┐
│ Layer 4: Rate Limiting                                       │
│ - Per-IP packet limits (pre-validation)                      │
│ - Per-session bandwidth caps (from lease)                    │
│ - Per-user lease request caps (at Master)                    │
└──────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌──────────────────────────────────────────────────────────────┐
│ Layer 5: Reputation & Detection                              │
│ - Session success tracking                                   │
│ - Client feedback (signed)                                   │
│ - Anomaly detection triggers                                 │
└──────────────────────────────────────────────────────────────┘
```

---

## 4. End-to-End Encryption

### 3.1 Protocol: Noise XX

Wavry uses the Noise XX handshake pattern:

```
-> e
<- e, ee, s, es
-> s, se
```

This provides:
- Mutual authentication
- Forward secrecy (new ephemeral keys per session)
- Identity hiding (encrypted static keys)

### 3.2 Implementation

```rust
// Using snow crate
let builder = snow::Builder::new("Noise_XX_25519_ChaChaPoly_BLAKE2s".parse()?);

// Initiator (client)
let mut noise = builder
    .local_private_key(&client_private_key)
    .build_initiator()?;

// Responder (server)
let mut noise = builder
    .local_private_key(&server_private_key)
    .build_responder()?;
```

### 3.3 Implementation Status

The Noise XX handshake (Msg1-3) has been verified end-to-end between `wavry-server` and `wavry-client`. The implementation uses `Noise_XX_25519_ChaChaPoly_BLAKE2s` to secure all Control, Input, and Media channels.

### 3.4 Relay Blindness Guarantee

| Layer | What Relay Sees | What Relay Cannot See |
|:------|:----------------|:----------------------|
| IP Layer | Source/dest IP:port | — |
| UDP Layer | Datagram size, timing | — |
| Relay Protocol | Session ID, sequence number | — |
| RIFT Protocol | ❌ | Channel type, message type |
| Application | ❌ | Input events, video frames |

---

## 5. Lease Security

### 4.1 Lease Properties

| Property | Value | Rationale |
|:---------|:------|:----------|
| **Signature algorithm** | Ed25519 (PASETO v4.public) | No algorithm confusion |
| **Default TTL** | 5 minutes | Minimize exposure window |
| **Max TTL** | 15 minutes | Allow for clock skew |
| **Bound to session** | session_id claim | Prevents cross-session reuse |
| **Bound to peers** | peers[] claim | Prevents unauthorized peers |
| **Bound to relay** | relay_id claim | Prevents wrong-relay usage |
| **Replay protection** | nonce + seq_window | Prevents packet replay |

### 4.2 Renewal Flow

```
Time →
|----[Lease 1: 5 min]-----|
          |----[Lease 2: 5 min]-----|
                    |----[Lease 3: 5 min]-----|

Client renews at 80% (4 min):
- New lease issued with fresh nonce, JTI
- Old JTI enters denial window (30s)
- Overlap ensures continuity
```

### 4.3 Revocation

**Immediate revocation** via Master API:

```
POST /v1/admin/leases/{jti}/revoke
Authorization: Bearer <admin_token>
```

Revocation propagated to relays via:
1. **Push** (if relay has WebSocket to Master): Immediate
2. **Pull** (relay polls revocation list every 30s): Up to 30s delay

For production, recommend push-based with periodic pull as backup.

---

## 6. Rate Limiting

### 5.1 Master-Side Limits

| Resource | Limit | Window | Response |
|:---------|:------|:-------|:---------|
| Auth challenges | 10 | 1 minute | 429 + backoff |
| Lease requests | 10 | 1 minute | 429 |
| API calls (authenticated) | 100 | 1 minute | 429 |
| API calls (unauthenticated) | 20 | 1 minute | 429 |
| Relay registrations per IP | 2 | 24 hours | 403 |

### 5.2 Relay-Side Limits

| Resource | Limit | Scope | Response |
|:---------|:------|:------|:---------|
| Packets/sec (pre-session) | 1000 | Per source IP | Silent drop |
| Bytes/sec (pre-session) | 10 MB | Per source IP | Silent drop |
| Lease presentations | 10 | Per source IP per minute | LEASE_REJECT |
| Session bandwidth soft | 50 Mbps | Per session (from lease) | Log warning |
| Session bandwidth hard | 100 Mbps | Per session (from lease) | Drop excess |

### 5.3 Token Bucket Configuration

```rust
struct RateLimitConfig {
    // Pre-session (per source IP)
    ip_bucket_capacity: usize,      // 1000 packets
    ip_bucket_refill_ms: u64,       // 1ms per token
    
    // Per-session (from lease)
    session_soft_kbps: u32,         // 51200 (50 Mbps)
    session_hard_kbps: u32,         // 102400 (100 Mbps)
}
```

---

## 7. Audit & Observability

### 6.1 What We Log (Master)

| Event | Fields Logged | Retention |
|:------|:--------------|:----------|
| Auth attempt | wavry_id, IP, success/fail, timestamp | 30 days |
| Lease issued | session_id, relay_id, peer_ids (hashed), timestamp | 90 days |
| Session created | session_id, client_id (hashed), server_id (hashed) | 90 days |
| Relay state change | relay_id, old_state, new_state, reason | 1 year |
| Admin action | admin_id, action, target, timestamp | 1 year |

### 6.2 What We Log (Relay - Privacy-Preserving)

| Event | Fields Logged | NOT Logged |
|:------|:--------------|:-----------|
| Session start | session_id (truncated), timestamp | Peer IPs, Wavry IDs |
| Session end | session_id (truncated), duration, bytes | Peer IPs |
| Rate limit hit | source IP (hashed), count | Actual IP |
| Lease reject | reason, timestamp | Peer identity |

### 6.3 What We Refuse to Log

> [!CAUTION]
> The following must NEVER be logged by any component:

- Packet payloads (obviously)
- Full peer IP addresses (use hashes or truncation)
- Packet timing at sub-second granularity
- Per-packet metadata
- Correlation data between sessions

### 6.4 Metrics (Privacy-Safe)

```
# Master metrics
wavry_master_auth_attempts_total{result="success|fail"}
wavry_master_leases_issued_total{region="us-east-1"}
wavry_master_sessions_active
wavry_master_relays_by_state{state="active|degraded|..."}

# Relay metrics
wavry_relay_sessions_active
wavry_relay_bytes_forwarded_total
wavry_relay_packets_dropped_total{reason="rate_limit|replay|expired"}
wavry_relay_lease_validations_total{result="valid|invalid"}
```

---

## 8. Kill Switches

### 7.1 Available Kill Switches

| Switch | Scope | Effect | Recovery |
|:-------|:------|:-------|:---------|
| Ban user | Single user | Revoke tokens, reject all requests | Admin unban |
| Ban relay | Single relay | Remove from pool, reject heartbeats | Admin unban |
| Quarantine relay | Single relay | Remove from selection, continue heartbeats | Auto (7 days) or admin |
| Disable relay mode | Global | No new leases issued | Admin re-enable |
| Emergency lockdown | Global | Reject all non-admin requests | Admin unlock |

### 7.2 API Endpoints

```
# Ban user
POST /v1/admin/bans
{
  "target_type": "user",
  "target_id": "<wavry_id>",
  "reason": "abuse",
  "expires_at": null  // permanent
}

# Ban relay
POST /v1/admin/bans
{
  "target_type": "relay",
  "target_id": "<relay_id>",
  "reason": "malicious behavior"
}

# Disable relay mode globally
POST /v1/admin/config
{
  "relay_mode_enabled": false
}

# Emergency lockdown
POST /v1/admin/lockdown
{
  "enabled": true,
  "reason": "active attack",
  "allow_admin": true
}
```

---

## 9. Key Management

### 8.1 Key Hierarchy

```
┌──────────────────────────────────────────────────────────────┐
│ Root Key (offline, HSM)                                       │
│ - Signs intermediate keys                                     │
│ - Never touches production servers                            │
└──────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌──────────────────────────────────────────────────────────────┐
│ Signing Key (online, secrets manager)                         │
│ - Signs relay leases (PASETO v4.public)                      │
│ - Rotated quarterly                                           │
│ - Old key kept for overlap period                             │
└──────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌──────────────────────────────────────────────────────────────┐
│ Encryption Key (online, secrets manager)                      │
│ - Encrypts session tokens (PASETO v4.local)                  │
│ - Rotated quarterly                                           │
└──────────────────────────────────────────────────────────────┘
```

### 8.2 Key Rotation Schedule

| Key | Rotation | Overlap | Recovery |
|:----|:---------|:--------|:---------|
| Root key | Never (or 5 years) | — | Multi-party recovery from HSM backups |
| Signing key | Quarterly | 2 weeks | Generate new, publish, old continues |
| Encryption key | Quarterly | 1 hour | Generate new, old tokens expire naturally |

### 8.3 Public Key Publication

Master's signing public key must be verifiable:

1. **Well-known endpoint**: `https://master.wavry.io/.well-known/wavry-keys.json`
2. **DNS TXT record**: `_wavry-keys.wavry.io`
3. **Repository**: `github.com/wavry/wavry/keys/`

```json
// /.well-known/wavry-keys.json
{
  "signing_keys": [
    {
      "kid": "2024Q1",
      "public_key": "MCowBQYDK2VwAyEA...",
      "valid_from": "2024-01-01T00:00:00Z",
      "valid_until": "2024-04-15T00:00:00Z",
      "status": "active"
    },
    {
      "kid": "2024Q2",
      "public_key": "MCowBQYDK2VwAyEA...",
      "valid_from": "2024-04-01T00:00:00Z",
      "valid_until": "2024-07-15T00:00:00Z",
      "status": "active"
    }
  ],
  "signature": "<root key signature over this document>"
}
```

### 8.4 Compromise Response

| Compromise | Impact | Response |
|:-----------|:-------|:---------|
| Signing key leaked | Attacker can forge leases | Immediate rotation, revoke old kid, notify relays |
| Encryption key leaked | Attacker can forge session tokens | Immediate rotation, mass token revocation |
| Root key leaked | Total compromise | New root key ceremony, rotate all keys, audit |
| Relay key leaked | Attacker can impersonate relay | Ban relay, operator generates new key |
| User key leaked | Attacker controls identity | User generates new keypair = new identity |

---

## 10. Operational Procedures

### 9.1 Relay Updates

Relays should auto-update from signed releases:

```rust
struct SignedRelease {
    version: String,
    sha256: String,
    signature: String,  // Signed by release key
    min_supported: String,
    release_notes: String,
}
```

Update flow:
1. Relay polls `https://releases.wavry.io/relay/latest.json`
2. Verify signature against release public key
3. Download binary, verify SHA256
4. Graceful shutdown (drain sessions)
5. Replace binary, restart

### 9.2 Incident Response Playbook

#### Malicious Relay Detected

```
1. IMMEDIATE (< 5 min)
   - Quarantine relay via API
   - Check for active sessions (warn users if possible)
   
2. INVESTIGATE (< 1 hour)
   - Review session logs for affected users
   - Check for correlated relays (Sybil)
   - Assess data exposure risk
   
3. REMEDIATE (< 24 hours)
   - Ban relay permanently if confirmed
   - Notify affected users (if identifiable)
   - Update detection rules
   
4. POSTMORTEM (< 1 week)
   - Document incident
   - Update threat model
   - Improve detection
```

#### Master Compromise Suspected

```
1. IMMEDIATE (< 5 min)
   - Enable emergency lockdown
   - Rotate all online keys
   - Alert on-call team
   
2. INVESTIGATE (< 1 hour)
   - Identify compromise vector
   - Determine data exposure
   - Check for persistence
   
3. REMEDIATE (< 24 hours)
   - Clean rebuild of Master
   - Restore from verified backup
   - Force all users/relays to re-authenticate
   
4. COMMUNICATE (< 48 hours)
   - Public disclosure (if user data affected)
   - Notify relay operators
   - Update security posture
```

### 9.3 Monitoring Alerts

| Alert | Condition | Severity | Response |
|:------|:----------|:---------|:---------|
| High auth failures | > 1000/min | Warning | Check for brute force |
| Relay mass offline | > 20% offline | Critical | Network issue or attack |
| Lease issuance spike | > 10x normal | Warning | Potential abuse |
| Master latency | P99 > 500ms | Warning | Scale or investigate |
| Signing key nearing expiry | < 2 weeks | Critical | Rotate immediately |

---

## 11. Privacy Posture

### 10.1 Data Minimization

| Data | Collected | Stored | Retention |
|:-----|:----------|:-------|:----------|
| User IP (at Master) | Yes (for rate limit) | Hashed only | 24 hours |
| User IP (at relay) | Yes (for forwarding) | Never | — |
| Session content | Never | Never | — |
| Timing (per-packet) | Never | Never | — |
| Peer relationships | Yes (session records) | IDs hashed | 90 days |

### 10.2 User Rights

| Right | Implementation |
|:------|:---------------|
| Access | Export session history via API |
| Deletion | Delete account + hash all associated data |
| Portability | Export account data in JSON |

### 10.3 Relay Operator Requirements

Relay operators must agree to:
1. No packet payload logging
2. No correlation data collection
3. No traffic analysis beyond metrics
4. Compliance with local privacy laws
5. Cooperation with abuse reports

Violation = permanent ban + public disclosure.

---

## 12. Recommended Defaults

### 11.1 Security Defaults

| Setting | Default | Min | Max |
|:--------|:--------|:----|:----|
| Lease TTL | 5 min | 1 min | 15 min |
| Session token TTL | 1 hour | 15 min | 24 hours |
| Relay token TTL | 24 hours | 1 hour | 7 days |
| Challenge expiry | 60 sec | 30 sec | 5 min |
| Sequence window | 128 | 64 | 1024 |
| Max clock skew | 30 sec | 10 sec | 60 sec |

### 11.2 Rate Limit Defaults

| Limit | Default | Rationale |
|:------|:--------|:----------|
| Per-user leases/min | 10 | Prevents lease flooding |
| Per-IP auth/min | 10 | Prevents brute force |
| Per-session bandwidth | 100 Mbps hard | Reasonable 4K streaming |
| Per-relay max sessions | 100 | Memory safety |
| Global relay bandwidth | 1 Gbps | Typical server |

### 11.3 Operational Defaults

| Setting | Default | Rationale |
|:--------|:--------|:----------|
| Heartbeat interval | 30 sec | Balance responsiveness/overhead |
| Probe interval | 60 sec | Sufficient for health detection |
| Probation period | 7 days | Time for behavior observation |
| Probation sessions | 100 | Statistical significance |
| Quarantine duration | 7 days | Time for investigation/appeal |
| Log retention | 90 days | Incident investigation window |

---

## 13. Minimum Viable Safe Launch Checklist

> [!IMPORTANT]
> All items must be completed before public launch.

### 12.1 Cryptography

- [ ] Ed25519 key generation uses CSPRNG
- [ ] PASETO library is current version
- [ ] Master signing key stored in HSM or secrets manager
- [ ] Key rotation procedure documented and tested
- [ ] Public key publication mechanism deployed

### 12.2 Authentication

- [ ] Challenge expiry enforced (60s)
- [ ] Signature verification uses constant-time comparison
- [ ] Session tokens are encrypted (PASETO v4.local)
- [ ] Lease tokens are signed (PASETO v4.public)
- [ ] No plaintext secrets in logs

### 12.3 Authorization

- [ ] Lease validation checks all claims
- [ ] Lease bound to session_id, peers, relay_id
- [ ] Sequence window replay protection implemented
- [ ] Expired leases rejected (with clock skew allowance)
- [ ] JTI uniqueness verified

### 12.4 Rate Limiting

- [ ] Pre-auth rate limits at edge (per-IP)
- [ ] Post-auth rate limits implemented
- [ ] Relay pre-session rate limits implemented
- [ ] Per-session bandwidth caps enforced
- [ ] Subnet blocking for floods

### 12.5 Monitoring

- [ ] Auth failure alerts configured
- [ ] Relay health monitoring active
- [ ] Session success tracking implemented
- [ ] Admin audit logging enabled
- [ ] Metrics exported to monitoring system

### 12.6 Incident Response

- [ ] Kill switch APIs tested
- [ ] Emergency lockdown tested
- [ ] Key rotation runbook documented
- [ ] Incident response team identified
- [ ] Communication channels established

### 12.7 Privacy

- [ ] No packet payloads logged anywhere
- [ ] IP addresses hashed in logs
- [ ] Retention policies enforced
- [ ] Privacy policy published
- [ ] Relay operator agreement defined

### 12.8 Relay Network

- [ ] Sybil detection rules active
- [ ] Probation period enforced
- [ ] Client feedback system implemented
- [ ] Relay scoring function deployed
- [ ] Quarantine/ban mechanisms tested

### 12.9 Operations

- [ ] Signed release mechanism deployed
- [ ] Relay auto-update tested
- [ ] Backup and recovery tested
- [ ] Monitoring dashboards deployed
- [ ] On-call rotation established

---

## Appendix A: Threat Scenarios

### A.1 Scenario: Amplification Attack

**Attack:** Attacker obtains valid lease, sends small packets that trigger large responses.

**Why it fails:**
1. Leases are short-lived (5 min) — limited window
2. Leases bound to specific peers — can't target arbitrary IPs
3. Per-session rate limits — caps total amplification
4. Relay is UDP-to-UDP — no protocol amplification

### A.2 Scenario: Sybil Relay Network

**Attack:** Attacker registers 50 relays from different IPs to dominate selection.

**Why it fails:**
1. Sybil detection flags correlated registrations
2. All relays start in PROBATION — 7 day wait
3. ASN diversity in selection — max 2 relays per ASN
4. Low initial score weight — new relays rarely selected
5. Client feedback exposes poor-quality relays quickly

### A.3 Scenario: Session Correlation

**Attack:** Attacker controls two relays, correlates users across sessions.

**Why it fails:**
1. Random relay selection with diversity
2. Minimal metadata visible (encrypted payloads)
3. No cross-relay session data sharing
4. Short sessions (typically < 1 hour)
5. IP addresses not logged

### A.4 Scenario: Stolen Lease Replay

**Attack:** Attacker captures lease token from network, replays on different relay.

**Why it fails:**
1. Lease bound to specific relay_id
2. Relay verifies relay_id matches own identity
3. Sequence numbers prevent packet replay
4. Short TTL limits replay window

---

## Related Documents

- [WAVRY_ARCHITECTURE.md](WAVRY_ARCHITECTURE.md) - System architecture overview
- [RIFT_SPEC_V1.md](RIFT_SPEC_V1.md) - Protocol specification
- [WAVRY_MASTER.md](WAVRY_MASTER.md) - Master service API
- [WAVRY_RELAY.md](WAVRY_RELAY.md) - Relay node specification
- [WAVRY_RELAY_SELECTION.md](WAVRY_RELAY_SELECTION.md) - Relay selection algorithm
