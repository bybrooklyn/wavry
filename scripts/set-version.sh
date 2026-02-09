#!/bin/bash
# scripts/set-version.sh
# Usage: ./scripts/set-version.sh <wavry_version> <rift_version> <delta_version>
# Example: ./scripts/set-version.sh 0.0.1-unstable 1.2 1.1.0

set -euo pipefail

if [ "$#" -ne 3 ]; then
    echo "Usage: $0 <wavry_version> <rift_version> <delta_version>"
    echo "Example: $0 0.0.1-unstable 1.2 1.1.0"
    exit 1
fi

WAVRY_VER=$1
RIFT_VER=$2
DELTA_VER=$3

# Remove 'v' prefix if present for programmatic use
WAVRY_RAW=$(echo "$WAVRY_VER" | sed 's/^v//')
RIFT_RAW=$(echo "$RIFT_VER" | sed 's/^v//')
DELTA_RAW=$(echo "$DELTA_VER" | sed 's/^v//')

echo "Updating Wavry to v$WAVRY_RAW"
echo "Updating RIFT to v$RIFT_RAW"
echo "Updating DELTA to v$DELTA_RAW"

# Get script directory and project root
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
cd "$PROJECT_ROOT"

# Helper for portable sed -i
portable_sed() {
  local pattern=$1
  local file=$2
  if [[ "$OSTYPE" == "darwin"* ]]; then
    sed -i '' "$pattern" "$file"
  else
    sed -i "$pattern" "$file"
  fi
}

# 1. Update VERSION file
echo "Updating VERSION file..."
cat <<EOF > "$PROJECT_ROOT/VERSION"
WAVRY v$WAVRY_RAW
RIFT (Remote Interactive Frame Transport) v$RIFT_RAW
DELTA (Differential Latency Estimation and Tuning Algorithm) v$DELTA_RAW
ALVR (Commit 1a4194ff137937c0a4f416ad2d6d1acedb851e8a)
EOF
echo "  Updated VERSION"

# 2. Update workspace.package version in root Cargo.toml
echo "Updating root Cargo.toml..."
# Update the version in [workspace.package] section
portable_sed "s/^version = \"[^\"]*\"/version = \"$WAVRY_RAW\"/" "$PROJECT_ROOT/Cargo.toml"
echo "  Updated workspace version in Cargo.toml"

# 3. Update Cargo.toml files in crates that have explicit version (not workspace = true)
echo "Updating crate Cargo.toml files..."
find "$PROJECT_ROOT/crates" -maxdepth 2 -name "Cargo.toml" | while read -r toml; do
  # Check if this crate has an explicit version (not version.workspace = true)
  if grep -q "^version = \"" "$toml" 2>/dev/null && ! grep -q "^version.workspace = true" "$toml" 2>/dev/null; then
    portable_sed "s/^version = \"[^\"]*\"/version = \"$WAVRY_RAW\"/" "$toml"
    echo "  Updated $(basename "$(dirname "$toml")")"
  fi
done

# 4. Update Android build.gradle.kts if it exists
ANDROID_BUILD_FILE="$PROJECT_ROOT/apps/android/app/build.gradle.kts"
if [ -f "$ANDROID_BUILD_FILE" ]; then
  echo "Updating Android build.gradle.kts..."
  portable_sed "s/versionName = \"[^\"]*\"/versionName = \"$WAVRY_RAW\"/" "$ANDROID_BUILD_FILE"
  echo "  Updated apps/android/app/build.gradle.kts"
fi

# 5. Update package.json files
PACKAGE_JSON_COUNT=$(find "$PROJECT_ROOT" -maxdepth 3 -name "package.json" -not -path "*/node_modules/*" | wc -l)
if [ "$PACKAGE_JSON_COUNT" -gt 0 ]; then
  echo "Updating package.json files..."
  find "$PROJECT_ROOT" -maxdepth 3 -name "package.json" -not -path "*/node_modules/*" | while read -r pjson; do
    if [ -f "$pjson" ]; then
      # Update the version field in package.json
      portable_sed "s/\"version\": \"[^\"]*\"/\"version\": \"$WAVRY_RAW\"/" "$pjson"
      echo "  Updated $(basename "$(dirname "$pjson")")/package.json"
    fi
  done
fi

# 6. Update Documentation
echo "Updating documentation..."

# RIFT Spec
if [ -f "$PROJECT_ROOT/docs/RIFT_SPEC_V1.md" ]; then
  portable_sed "s/RIFT Protocol Specification v[0-9][0-9.]*/RIFT Protocol Specification v$RIFT_RAW/" "$PROJECT_ROOT/docs/RIFT_SPEC_V1.md"
  echo "  Updated docs/RIFT_SPEC_V1.md"
fi

# DELTA Spec
if [ -f "$PROJECT_ROOT/docs/DELTA_CC_SPEC.md" ]; then
  portable_sed "s/DELTA Congestion Control Specification v[0-9][0-9.]*/DELTA Congestion Control Specification v$DELTA_RAW/" "$PROJECT_ROOT/docs/DELTA_CC_SPEC.md"
  echo "  Updated docs/DELTA_CC_SPEC.md"
fi

echo ""
echo "Version update complete."
echo "Wavry: v$WAVRY_RAW"
echo "RIFT: v$RIFT_RAW"
echo "DELTA: v$DELTA_RAW"
