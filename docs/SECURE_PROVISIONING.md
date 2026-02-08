# Wavry Secure Provisioning & Secret Management

This document defines the requirements and architecture for the automated, auditable provisioning of credentials within the Wavry ecosystem.

## 1. Credential Types

| Credential | Scope | Type | Rotation |
|:---|:---|:---|:---|
| **Master Signing Key** | Global | Ed25519 (PASETO) | Yearly / On-Compromise |
| **Gateway TLS** | Public | ECDSA (WebTransport) | 90 Days (ACME) |
| **Relay Identity** | Internal | Ed25519 | Monthly |
| **Database Credentials** | Internal | AES-256-GCM / Password | Continuous |

## 2. The Provisioning Pipeline

### Level 1: Development (scripts/bootstrap-dev.sh)
- Uses `step` CLI or `openssl` to create a local Root CA.
- Generates short-lived (14-day) ECDSA certs for WebTransport.
- Injected via environment variables.

### Level 2: Continuous Integration (GitHub Actions)
- Generates ephemeral identities for each test run.
- Secrets are never stored; they are piped directly into the runner environment.
- **Audit:** GitHub Action logs provide an execution trace.

### Level 3: Production (Vault + ACME)
- **Master Keys:** Stored in HashiCorp Vault or AWS KMS.
- **TLS:** Gateway implements the ACME protocol to handle `http-01` or `dns-01` challenges.
- **Relays:** Provisioned with PASETO tokens that authorize them to register with the Master.

## 3. Implementation Rules

1.  **No On-the-Fly Randomness:** Production binaries must fail if a required persistent key (like the Master signing key) is missing. They must NOT generate a temporary one.
2.  **Filesystem Isolation:** Keys should be read from `/run/secrets/` (Docker/K8s standard) rather than arbitrary paths.
3.  **No Secrets in Logs:** Application logs must be scrubbed of any key material.
4.  **Environment Parity:** The mechanism for reading a key must be identical across Dev, CI, and Prod (e.g., always looking for `WAVRY_MASTER_KEY_FILE`).

## 4. Audit Trail

All provisioning actions must log:
- **Timestamp**
- **Action** (Issue, Rotate, Revoke)
- **Subject** (Which component/user)
- **Actor** (Provisioning script, Admin user, or Auto-scaler)
