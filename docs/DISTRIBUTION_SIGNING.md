# Wavry Distribution & Signing Guide

Wavry uses a "Community-First" signing model that avoids expensive developer programs while maintaining security and integrity.

## Platform Signing Overview

| Platform | Method | Cost | User Experience |
|:---|:---|:---|:---|
| **Android** | Self-signed Keystore | Free | Standard install. |
| **macOS** | Ad-hoc Signing | Free | Requires "Right-click -> Open" on first run. |
| **Windows** | Self-signed PFX | Free | Shows "Unknown Publisher" warning. |
| **Linux** | GPG Manifest | Free | Verifiable via public GPG key. |

---

## 1. Setup Instructions (Run these once)

### A. Android Keystore
Run the following to generate your release key:
```bash
./scripts/gen-android-keystore.sh
```
**Secrets to add to GitHub:**
- `WAVRY_ANDROID_RELEASE_KEYSTORE_B64`: (Contents of `secrets/android-release.jks.base64`)
- `WAVRY_ANDROID_RELEASE_STORE_PASSWORD`: The password you chose.
- `WAVRY_ANDROID_RELEASE_KEY_ALIAS`: `wavry-release`
- `WAVRY_ANDROID_RELEASE_KEY_PASSWORD`: The password you chose.

### B. Windows Self-Signed Certificate
Run the following to generate a Windows signing certificate:
```bash
./scripts/gen-windows-cert.sh
```
**Secrets to add to GitHub:**
- `WINDOWS_CERTIFICATE_P12`: (Contents of `secrets/windows-release.pfx.base64`)
- `WINDOWS_CERTIFICATE_PASSWORD`: The password you chose.

### C. Linux GPG Key
Generate a new GPG key:
```bash
gpg --full-generate-key # Select EdDSA (or RSA 4096)
```
Export the keys:
```bash
# Get your Key ID from: gpg --list-secret-keys --keyid-format LONG
gpg --armor --export-secret-key <YOUR_KEY_ID> > secrets/gpg-private.key
gpg --armor --export <YOUR_KEY_ID> > wavry-public.asc
```
**Secrets to add to GitHub:**
- `GPG_PRIVATE_KEY`: (Contents of `secrets/gpg-private.key`)
- `GPG_PASSPHRASE`: The passphrase you used for the key.

---

## 2. GitHub Secrets Checklist

Go to **Settings > Secrets and variables > Actions** in your repository and add:

1. `WAVRY_ANDROID_RELEASE_KEYSTORE_B64`
2. `WAVRY_ANDROID_RELEASE_STORE_PASSWORD`
3. `WAVRY_ANDROID_RELEASE_KEY_ALIAS`
4. `WAVRY_ANDROID_RELEASE_KEY_PASSWORD`
5. `WINDOWS_CERTIFICATE_P12`
6. `WINDOWS_CERTIFICATE_PASSWORD`
7. `GPG_PRIVATE_KEY`
8. `GPG_PASSPHRASE`

---

## 3. Security Notes

- **NEVER** commit files in the `secrets/` directory. They are already added to `.gitignore`.
- Certificates generated via these scripts are valid for local and community distribution.
- If you later join an official developer program, simply replace these secrets with your official credentials.
