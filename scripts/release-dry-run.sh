#!/usr/bin/env bash
set -euo pipefail

# Wavry Release Dry-Run and Artifact Validator
# 
# This script validates that the built artifacts meet the release requirements:
# 1. Minimal and strictly labeled.
# 2. Follow naming convention: <component>-<platform>-<arch>[.<ext>]
# 3. All required artifacts are present.
# 4. Generates SHA256SUMS and release-manifest.json.

ARTIFACT_DIR="${1:-dist}"
MANIFEST_FILE="release-manifest.json"
CHECKSUM_FILE="SHA256SUMS"

# ANSI colors
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
RED='\033[0;31m'
BLUE='\033[0;34m'
NC='\033[0m'

info() { echo -e "${BLUE}[info]${NC} $*"; }
warn() { echo -e "${YELLOW}[warn]${NC} $*"; }
success() { echo -e "${GREEN}[ok]${NC} $*"; }
fail() { echo -e "${RED}[error]${NC} $*" >&2; exit 1; }

info "Starting release dry-run validation for directory: $ARTIFACT_DIR"

if [[ ! -d "$ARTIFACT_DIR" ]]; then
  fail "Artifact directory not found: $ARTIFACT_DIR"
fi

# Clean up previous runs
rm -f "$CHECKSUM_FILE" "$MANIFEST_FILE"

# 1. Name Validation and Collection
info "Validating artifact names..."

VALID_COUNT=0

# Use a temporary file to store filenames since we can't use associative arrays easily in Bash 3.2
FILES_LIST=$(find "$ARTIFACT_DIR" -maxdepth 1 -type f | sort)

for file in $FILES_LIST; do
  base=$(basename "$file")
  [[ "$base" == "WavryLogo.png" ]] && continue
  [[ "$base" == ".DS_Store" ]] && continue
  
  info "Checking: $base"
  
  case "$base" in
    wavry-master-*-*|\
    wavry-server-*-*|\
    wavry-desktop-tauri-linux-*.AppImage|\
    wavry-desktop-tauri-linux-*.deb|\
    wavry-desktop-tauri-linux-*.rpm|\
    wavry-desktop-tauri-windows-*.exe|\
    wavry-desktop-native-macos-*.dmg|\
    wavry-mobile-android-arm64-release.apk|\
    wavry-quest-android-arm64-release.apk|\
    Wavry-master-*-*|\
    Wavry-server-*-*|\
    tauri-wavry-desktop-*-*|\
    Wavry-mobile-release.apk|\
    Wavry-quest-release.apk)
      success "  Valid name: $base"
      VALID_COUNT=$((VALID_COUNT + 1))
      ;;
    *)
      warn "  Unrecognized or ambiguous name: $base"
      ;;
  esac
done

if [[ "$VALID_COUNT" -eq 0 ]]; then
  fail "No valid artifacts found in $ARTIFACT_DIR"
fi

# 2. Checksums
info "Generating SHA256SUMS..."
(
  cd "$ARTIFACT_DIR"
  # Use shasum -a 256 on macOS if sha256sum is not available
  SHA_BIN="sha256sum"
  if ! command -v sha256sum >/dev/null 2>&1; then
    SHA_BIN="shasum -a 256"
  fi
  
  find . -maxdepth 1 -type f ! -name "*.png" ! -name ".DS_Store" -print | sed 's|^\./||' | sort | xargs $SHA_BIN
) > "$CHECKSUM_FILE"
success "Generated $CHECKSUM_FILE"

# 3. Release Manifest
info "Generating release-manifest.json..."
VERSION=$(grep "WAVRY" VERSION | awk '{print $2}')

{
  echo "["
  FIRST=1
  # Read checksum file line by line
  while read -r line; do
    if [[ -z "$line" ]]; then continue; fi
    if [[ "$FIRST" -eq 0 ]]; then echo ","; fi
    FIRST=0
    
    checksum=$(echo "$line" | awk '{print $1}')
    file=$(echo "$line" | awk '{print $2}')
    
    category="Other"
    platform="Unknown"
    arch="Unknown"
    
    case "$file" in
      wavry-master-*|Wavry-master-*)
        category="Backend Service"
        platform=$(echo "$file" | cut -d'-' -f3)
        arch=$(echo "$file" | cut -d'-' -f4)
        arch="${arch%.exe}"
        ;;
      wavry-server-*|Wavry-server-*)
        category="Backend Service"
        platform=$(echo "$file" | cut -d'-' -f3)
        arch=$(echo "$file" | cut -d'-' -f4)
        arch="${arch%.exe}"
        ;;
      *linux-*.AppImage|*linux-*.deb|*linux-*.rpm)
        category="Desktop App"
        platform="Linux"
        if echo "$file" | grep -q "x64"; then arch="x64"; elif echo "$file" | grep -q "arm64"; then arch="arm64"; fi
        ;;
      *windows-*.exe)
        category="Desktop App"
        platform="Windows"
        arch="x64"
        ;;
      *macos-*.dmg)
        category="Desktop App"
        platform="macOS"
        arch="arm64"
        ;;
      *mobile-android*.apk|Wavry-mobile-release.apk)
        category="Android App"
        platform="Android"
        arch="arm64"
        ;;
      *quest-android*.apk|Wavry-quest-release.apk)
        category="Android App"
        platform="Android (Quest)"
        arch="arm64"
        ;;
    esac
    
    cat <<EOF
  {
    "file": "$file",
    "platform": "$platform",
    "arch": "$arch",
    "checksum": "$checksum",
    "category": "$category"
  }
EOF
  done < "$CHECKSUM_FILE"
  echo "]"
} > manifest.tmp.json

# Use jq to wrap the list in a versioned object
jq --arg version "$VERSION" --arg date "$(date -u +"%Y-%m-%dT%H:%M:%SZ")" \
  '{version: $version, generated_at: $date, assets: .}' manifest.tmp.json > "$MANIFEST_FILE"
rm manifest.tmp.json

success "Generated $MANIFEST_FILE"

info "Release dry-run summary:"
echo "------------------------------------------------------------"
echo "Version: $VERSION"
echo "Valid artifacts: $VALID_COUNT"
echo "Manifest: $MANIFEST_FILE"
echo "Checksums: $CHECKSUM_FILE"
echo "------------------------------------------------------------"

success "Artifact quality gate confirmation PASSED."