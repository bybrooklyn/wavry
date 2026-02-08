#!/bin/bash
set -e

# Generate Windows Self-Signed Authenticode Certificate
mkdir -p secrets

echo "Enter a password for the Windows Certificate:"
read -s PASSWORD

# 1. Create private key
openssl genrsa -out secrets/windows-key.pem 4096

# 2. Create certificate signing request
openssl req -new -key secrets/windows-key.pem -out secrets/windows-csr.pem 
    -subj "/CN=Wavry Community/O=Wavry/C=US"

# 3. Create self-signed certificate
openssl x509 -req -days 3650 -in secrets/windows-csr.pem 
    -signkey secrets/windows-key.pem -out secrets/windows-cert.pem

# 4. Export to PFX (PKCS#12)
openssl pkcs12 -export -out secrets/windows-release.pfx 
    -inkey secrets/windows-key.pem -in secrets/windows-cert.pem 
    -passout pass:"$PASSWORD"

# Export as Base64 for GitHub Secrets
base64 < secrets/windows-release.pfx > secrets/windows-release.pfx.base64

rm secrets/windows-key.pem secrets/windows-csr.pem secrets/windows-cert.pem

echo "Success!"
echo "1. Your PFX is at: secrets/windows-release.pfx"
echo "2. COPY the contents of secrets/windows-release.pfx.base64 to the GitHub Secret: WINDOWS_CERTIFICATE_P12"
