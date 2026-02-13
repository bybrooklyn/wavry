# Key Rotation and Secret Management Procedures

**Last Updated:** 2026-02-13  
**Audience:** Operations team, security engineers  
**Scope:** Production deployment key management

---

## Table of Contents

1. [Overview](#overview)
2. [Key Inventory](#key-inventory)
3. [Rotation Schedules](#rotation-schedules)
4. [Rotation Procedures](#rotation-procedures)
5. [Emergency Rotation](#emergency-rotation)
6. [Secret Storage](#secret-storage)
7. [Audit Trail](#audit-trail)

---

## Overview

This document defines procedures for rotating cryptographic keys and secrets used in Wavry's production infrastructure. Proper key rotation limits the impact of key compromise and ensures cryptographic hygiene.

**Key principles:**
- Regular scheduled rotation reduces compromise risk
- Emergency rotation procedures must be tested
- All rotations must be audited and logged
- Zero-downtime rotation is required for user-facing services
- Old keys must have a deprecation period before deletion

---

## Key Inventory

### Gateway/Authentication Keys

| Key/Secret | Purpose | Storage Location | Rotation Frequency | Criticality |
|:-----------|:--------|:----------------|:-------------------|:------------|
| DATABASE_ENCRYPTION_KEY | Encrypt 2FA secrets at rest | Environment variable | 90 days | High |
| JWT_SECRET (if used) | Session token signing | Environment variable | 90 days | High |
| ADMIN_API_KEY | Admin dashboard access | Secret manager | 30 days | Critical |
| DATABASE_PASSWORD | PostgreSQL/SQLite access | Secret manager | 90 days | High |

### Master/Relay Keys

| Key/Secret | Purpose | Storage Location | Rotation Frequency | Criticality |
|:-----------|:--------|:----------------|:-------------------|:------------|
| LEASE_SIGNING_KEY | PASETO lease signatures | HSM or Secret manager | 180 days | Critical |
| RELAY_REGISTRATION_SECRET | Relay authentication | Secret manager | 90 days | High |
| PROMETHEUS_AUTH_TOKEN | Metrics endpoint access | Secret manager | 90 days | Medium |

### TLS/Transport Keys

| Key/Secret | Purpose | Storage Location | Rotation Frequency | Criticality |
|:-----------|:--------|:----------------|:-------------------|:------------|
| HTTPS TLS Certificate | Gateway HTTPS | Let's Encrypt/ACME | 90 days (auto) | High |
| Internal CA Certificate | Service-to-service TLS | Secret manager | 1 year | High |

---

## Rotation Schedules

### Scheduled Maintenance Windows

| Service | Window | Timezone | Frequency |
|:--------|:-------|:---------|:----------|
| Gateway | Tuesday 02:00-04:00 UTC | UTC | Weekly |
| Master | Tuesday 03:00-05:00 UTC | UTC | Weekly |
| Relay Pool | Rolling, 24/7 | UTC | Continuous |

### Rotation Triggers

**Scheduled rotation:**
- 30 days: Admin API keys
- 90 days: Database encryption keys, authentication secrets
- 180 days: Lease signing keys
- 1 year: Internal CA certificates

**Emergency rotation:**
- Suspected key compromise
- Employee departure with key access
- Security audit finding
- Breach notification from upstream provider

---

## Rotation Procedures

### 1. Database Encryption Key Rotation

**Impact:** Requires re-encryption of all 2FA secrets  
**Downtime:** Zero (dual-key transition period)  
**Frequency:** 90 days

#### Procedure

1. **Pre-rotation validation**
   ```bash
   # Verify current key is working
   ./scripts/test-encryption-key.sh
   ```

2. **Generate new key**
   ```bash
   # Generate cryptographically random 256-bit key
   NEW_KEY=$(openssl rand -hex 32)
   echo "New key generated: ${NEW_KEY:0:8}..." # Log prefix only
   ```

3. **Deploy new key as secondary**
   ```bash
   # Add new key to secret manager
   aws secretsmanager create-secret \
     --name wavry/gateway/encryption-key-new \
     --secret-string "$NEW_KEY"
   
   # Update deployment to use both keys (decrypt with old, encrypt with new)
   kubectl set env deployment/wavry-gateway \
     ENCRYPTION_KEY_OLD="$OLD_KEY" \
     ENCRYPTION_KEY_NEW="$NEW_KEY"
   ```

4. **Re-encrypt all secrets**
   ```bash
   # Migration script re-encrypts all 2FA secrets with new key
   ./scripts/migrate-encryption-key.sh
   ```

5. **Verify migration**
   ```bash
   # Ensure all 2FA authentications still work
   ./scripts/test-2fa-auth.sh
   ```

6. **Promote new key to primary**
   ```bash
   # Use only new key going forward
   kubectl set env deployment/wavry-gateway \
     ENCRYPTION_KEY="$NEW_KEY"
   kubectl rollout status deployment/wavry-gateway
   ```

7. **Deprecate old key**
   ```bash
   # Keep old key for 7 days for rollback, then delete
   # Schedule deletion
   aws secretsmanager delete-secret \
     --secret-id wavry/gateway/encryption-key-old \
     --recovery-window-in-days 7
   ```

8. **Audit and document**
   ```bash
   # Log rotation in security audit log
   echo "Encryption key rotated at $(date -u +%Y-%m-%dT%H:%M:%SZ)" >> /var/log/wavry/key-rotation.log
   ```

---

### 2. Admin API Key Rotation

**Impact:** Requires updating admin dashboard clients  
**Downtime:** Zero (dual-key acceptance period)  
**Frequency:** 30 days

#### Procedure

1. **Generate new admin key**
   ```bash
   NEW_ADMIN_KEY=$(openssl rand -base64 32)
   ```

2. **Add new key to gateway**
   ```bash
   # Gateway accepts both old and new keys temporarily
   kubectl create secret generic wavry-admin-keys \
     --from-literal=primary="$NEW_ADMIN_KEY" \
     --from-literal=secondary="$OLD_ADMIN_KEY"
   ```

3. **Update admin dashboard config**
   ```bash
   # Update admin dashboard to use new key
   ./scripts/update-admin-dashboard-key.sh "$NEW_ADMIN_KEY"
   ```

4. **Verify admin access**
   ```bash
   # Test admin API with new key
   curl -H "Authorization: Bearer $NEW_ADMIN_KEY" \
     https://gateway.wavry.dev/v1/admin/overview
   ```

5. **Revoke old key**
   ```bash
   # After 24-hour transition period, revoke old key
   kubectl create secret generic wavry-admin-keys \
     --from-literal=primary="$NEW_ADMIN_KEY"
   ```

---

### 3. Lease Signing Key Rotation

**Impact:** All active relay leases become invalid  
**Downtime:** Minimal (clients automatically re-request leases)  
**Frequency:** 180 days

#### Procedure

1. **Generate new Ed25519 keypair**
   ```bash
   # Generate new signing keypair
   ./scripts/generate-lease-keypair.sh
   # Output: lease-signing-key-new.pem, lease-signing-key-new.pub
   ```

2. **Deploy new public key to all relays**
   ```bash
   # Relays need new public key to verify signatures
   ansible-playbook -i inventory/production \
     playbooks/update-relay-pubkey.yml \
     -e "new_pubkey=$(cat lease-signing-key-new.pub)"
   ```

3. **Wait for relay fleet update**
   ```bash
   # Ensure 95%+ of relays have new public key
   ./scripts/check-relay-pubkey-deployment.sh
   ```

4. **Switch master to new signing key**
   ```bash
   # Master starts signing with new key
   kubectl set env deployment/wavry-master \
     LEASE_SIGNING_KEY="$(cat lease-signing-key-new.pem)"
   ```

5. **Invalidate old leases (optional)**
   ```bash
   # Force all clients to request new leases immediately
   curl -X POST https://master.wavry.dev/v1/admin/invalidate-all-leases \
     -H "Authorization: Bearer $ADMIN_KEY"
   ```

6. **Verify lease issuance**
   ```bash
   # Ensure new leases are being issued and validated
   ./scripts/test-lease-flow.sh
   ```

7. **Retire old key**
   ```bash
   # After 7 days, remove old public key from relays
   ansible-playbook -i inventory/production \
     playbooks/remove-old-relay-pubkey.yml
   ```

---

### 4. TLS Certificate Rotation

**Impact:** Brief connection interruption during reload  
**Downtime:** ~1 second (graceful reload)  
**Frequency:** Automated via Let's Encrypt (90 days)

#### Procedure (Manual Renewal)

1. **Request new certificate**
   ```bash
   # Use certbot for Let's Encrypt
   sudo certbot renew --nginx --deploy-hook "/usr/local/bin/reload-gateway.sh"
   ```

2. **Verify certificate**
   ```bash
   # Check certificate validity
   openssl s_client -connect gateway.wavry.dev:443 </dev/null 2>/dev/null | \
     openssl x509 -noout -dates
   ```

3. **Test HTTPS access**
   ```bash
   # Ensure no certificate warnings
   curl -v https://gateway.wavry.dev/health 2>&1 | grep -i "certificate"
   ```

---

## Emergency Rotation

### Compromise Response Procedure

If a key compromise is suspected:

1. **Assess scope**
   - Identify which keys are compromised
   - Determine if active exploitation is occurring
   - Estimate number of affected users/sessions

2. **Immediate containment**
   ```bash
   # Revoke compromised key immediately (accept downtime if necessary)
   ./scripts/emergency-revoke-key.sh <key-id>
   
   # For gateway: Invalidate all sessions
   curl -X POST https://gateway.wavry.dev/v1/admin/invalidate-all-sessions \
     -H "Authorization: Bearer $EMERGENCY_ADMIN_KEY"
   ```

3. **Generate and deploy new key**
   - Follow standard rotation procedure but expedite timeline
   - Accept service disruption if compromise is active

4. **Notify affected users**
   ```bash
   # Send security notification
   ./scripts/send-security-notification.sh \
     --template security-incident \
     --severity high
   ```

5. **Incident report**
   - Document timeline of compromise
   - Root cause analysis
   - Preventive measures

---

## Secret Storage

### Production Secret Manager

**Primary:** AWS Secrets Manager (or equivalent)

**Backup:** Encrypted vault in version control (for disaster recovery)

### Access Control

| Role | Gateway Secrets | Master Secrets | Relay Secrets | Admin Keys |
|:-----|:---------------|:---------------|:--------------|:-----------|
| Engineer | Read | No | No | No |
| SRE | Read/Write | Read/Write | Read/Write | Read |
| Security Admin | Read/Write | Read/Write | Read/Write | Read/Write |
| CI/CD System | Read | No | No | No |

### Secret Encryption at Rest

All secrets stored in secret manager must be encrypted with AES-256.

For manual storage (emergency only):
```bash
# Encrypt secret
echo -n "$SECRET" | openssl enc -aes-256-cbc -pbkdf2 -out secret.enc

# Decrypt secret
openssl enc -d -aes-256-cbc -pbkdf2 -in secret.enc
```

---

## Audit Trail

### Required Logging

Every key rotation must log:
- Timestamp (UTC)
- Key identifier (name, not value)
- Operator identity (who performed rotation)
- Rotation reason (scheduled, emergency, or breach response)
- Verification status (success/failure)

### Audit Log Format

```json
{
  "timestamp": "2026-02-13T10:30:00Z",
  "event_type": "KEY_ROTATION",
  "key_id": "gateway/encryption-key",
  "operator": "alice@wavry.dev",
  "reason": "scheduled_maintenance",
  "verification": "success",
  "notes": "90-day scheduled rotation"
}
```

### Log Retention

- Key rotation events: 3 years
- Emergency rotations: 5 years
- Access logs to secret manager: 1 year

---

## Testing Rotation Procedures

### Quarterly Drill

Test key rotation procedures in staging environment every quarter:

1. **Schedule drill**
   - Announce to team 1 week in advance
   - Block 2-hour window for execution

2. **Execute rotation**
   - Follow documented procedures exactly
   - Time each step
   - Document any issues

3. **Post-drill review**
   - Update procedures based on findings
   - Identify improvements
   - Train new team members

### Rotation Success Metrics

- Zero unplanned downtime during rotation
- < 5 minutes per rotation (excluding waiting periods)
- All verification tests pass
- Audit log complete and accurate

---

## Contact and Escalation

**Security Team:** security@wavry.dev  
**On-call SRE:** PagerDuty escalation  
**Emergency Rotation:** Contact security team immediately via Slack #security-incidents

---

**Document Version:** 1.0  
**Next Review:** 2026-05-13
