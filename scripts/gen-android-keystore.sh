#!/bin/bash
set -e

# Generate Android Release Keystore
mkdir -p secrets

echo "Enter a password for the Android Keystore:"
read -s PASSWORD

keytool -genkey -v -keystore secrets/android-release.jks 
    -alias wavry-release -keyalg RSA -keysize 2048 -validity 10000 
    -storepass "$PASSWORD" -keypass "$PASSWORD" 
    -dname "CN=Wavry Community, O=Wavry, L=Internet, ST=Global, C=US"

# Export as Base64 for GitHub Secrets
base64 < secrets/android-release.jks > secrets/android-release.jks.base64

echo "Success!"
echo "1. Your keystore is at: secrets/android-release.jks"
echo "2. COPY the contents of secrets/android-release.jks.base64 to the GitHub Secret: WAVRY_ANDROID_RELEASE_KEYSTORE_B64"
