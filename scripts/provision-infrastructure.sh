#!/bin/bash
set -e

# Wavry Infrastructure Provisioning Script
# This script automates the creation of keys and certificates for a Wavry deployment.
# Usage: ./scripts/provision-infrastructure.sh <output_dir>

OUT_DIR=${1:-"./secrets"}
mkdir -p "$OUT_DIR"

echo "--- Wavry Provisioning Pipeline ---"
echo "Target directory: $OUT_DIR"

# 1. Generate Master Signing Key (Ed25519 Seed)
if [ ! -f "$OUT_DIR/master.key" ]; then
    echo "[1/3] Generating Master Signing Key (Seed)..."
    # Ed25519 seeds are just 32 random bytes. 
    # We generate them as hex for easy consumption by Master.
    openssl rand -hex 32 > "$OUT_DIR/master.key"
    echo "      Master key saved to $OUT_DIR/master.key"
else
    echo "[1/3] Master Signing Key already exists. Skipping."
fi

# 2. Generate Gateway TLS (ECDSA) for WebTransport
echo "[2/3] Generating Gateway TLS (localhost)..."
openssl ecparam -name prime256v1 -genkey -noout -out "$OUT_DIR/gateway-key.pem"

# Create a temporary config to ensure non-interactive run
cat <<EOF > "$OUT_DIR/openssl.cnf"
[req]
distinguished_name = req_distinguished_name
x509_extensions = v3_req
prompt = no

[req_distinguished_name]
CN = localhost

[v3_req]
subjectAltName = DNS:localhost,IP:127.0.0.1
EOF

openssl req -new -x509 -key "$OUT_DIR/gateway-key.pem" -out "$OUT_DIR/gateway-cert.pem" \
    -days 13 -config "$OUT_DIR/openssl.cnf"

rm "$OUT_DIR/openssl.cnf"

FINGERPRINT=$(openssl x509 -in "$OUT_DIR/gateway-cert.pem" -outform DER | openssl dgst -sha256 -binary | xxd -p -c 256)
echo "$FINGERPRINT" > "$OUT_DIR/gateway-cert.sha256"
echo "      Gateway TLS generated (Fingerprint: $FINGERPRINT)"

# 3. Create environment template
echo "[3/3] Generating environment template..."
cat <<EOF > "$OUT_DIR/wavry.env"
# Wavry Provisioned Environment
WAVRY_MASTER_KEY_FILE=$OUT_DIR/master.key
WAVRY_WT_CERT=$OUT_DIR/gateway-cert.pem
WAVRY_WT_KEY=$OUT_DIR/gateway-key.pem
WAVRY_GATEWAY_CERT_HASH=$FINGERPRINT
EOF

echo "-----------------------------------"
echo "Provisioning complete. Audit log:"
date +"%Y-%m-%d %H:%M:%S - Issued Master Key and Gateway TLS to $OUT_DIR" >> "$OUT_DIR/audit.log"
echo "See $OUT_DIR/audit.log for details."
