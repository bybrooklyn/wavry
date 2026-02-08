#!/bin/bash
set -e

# Generate a self-signed certificate for WebTransport (ECDSA)
# WebTransport in Chrome requires:
# 1. ECDSA key (RSA is not supported for WebTransport hashes)
# 2. Validity < 14 days for self-signed certificates used by hash

mkdir -p certs

echo "Generating ECDSA private key..."
openssl ecparam -name prime256v1 -genkey -noout -out certs/wt-key.pem

echo "Generating self-signed certificate..."
openssl req -new -x509 -key certs/wt-key.pem -out certs/wt-cert.pem -days 14 -subj "/CN=localhost" -addext "subjectAltName=DNS:localhost,IP:127.0.0.1"

echo "Certificate generation complete."
echo "Key: certs/wt-key.pem"
echo "Cert: certs/wt-cert.pem"

# Calculate the certificate hash for WebTransport client verification
# This is needed for the client to trust the self-signed cert
FINGERPRINT=$(openssl x509 -in certs/wt-cert.pem -outform DER | openssl dgst -sha256 -binary | xxd -p -c 256)
echo "Certificate Hash (SHA-256): $FINGERPRINT"