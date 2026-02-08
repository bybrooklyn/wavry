#!/bin/bash
set -e

# Wavry Infrastructure Provisioning Script
# This script automates the creation of keys and certificates for a Wavry deployment.
# Usage: ./scripts/provision-infrastructure.sh <output_dir>

OUT_DIR=${1:-"./secrets"}
mkdir -p "$OUT_DIR"

echo "--- Wavry Provisioning Pipeline ---"
echo "Target directory: $OUT_DIR"

# 1. Generate Master Signing Key (Ed25519)
if [ ! -f "$OUT_DIR/master.key" ]; then
    echo "[1/3] Generating Master Signing Key..."
    # We use openssl to generate a raw Ed25519 key, then extract the seed
    openssl genpkey -algorithm ed25519 -out "$OUT_DIR/master_openssl.pem"
    # Convert to hex for Master consumption
    openssl pkey -in "$OUT_DIR/master_openssl.pem" -noout -text | grep priv: -A 3 | grep -v priv: | tr -d ' 
:' > "$OUT_DIR/master.key"
    rm "$OUT_DIR/master_openssl.pem"
    echo "      Master key saved to $OUT_DIR/master.key"
else
    echo "[1/3] Master Signing Key already exists. Skipping."
fi

# 2. Generate Gateway TLS (ECDSA) for WebTransport
echo "[2/3] Generating Gateway TLS (localhost)..."
openssl ecparam -name prime256v1 -genkey -noout -out "$OUT_DIR/gateway-key.pem"
openssl req -new -x509 -key "$OUT_DIR/gateway-key.pem" -out "$OUT_DIR/gateway-cert.pem" -days 13 
    -subj "/CN=localhost" 
    -addext "subjectAltName=DNS:localhost,IP:127.0.0.1"

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
