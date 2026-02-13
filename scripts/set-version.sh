#!/bin/bash
# scripts/set-version.sh
# Usage: ./scripts/set-version.sh <wavry_version> <rift_version> <delta_version>
# Example: ./scripts/set-version.sh 0.0.3-canary 1.2 1.1.0

set -euo pipefail

if [ "$#" -ne 3 ]; then
    echo "Usage: $0 <wavry_version> <rift_version> <delta_version>"
    echo "Example: $0 0.0.3-canary 1.2 1.1.0"
    exit 1
fi

WAVRY_VER=$1
RIFT_VER=$2
DELTA_VER=$3

# Remove 'v' prefix if present for programmatic use
WAVRY_RAW=$(echo "$WAVRY_VER" | sed 's/^v//')
RIFT_RAW=$(echo "$RIFT_VER" | sed 's/^v//')
DELTA_RAW=$(echo "$DELTA_VER" | sed 's/^v//')

# Enforce release policy:
# - stable versions are allowed (e.g. 0.1.0)
# - prereleases are allowed only as -canary (optionally with dot suffixes)
#   examples: 0.1.0-canary, 0.1.0-canary.1
if ! [[ "$WAVRY_RAW" =~ ^[0-9]+\.[0-9]+\.[0-9]+(-canary([.][0-9A-Za-z-]+)*)?$ ]]; then
    echo "Error: invalid Wavry version '$WAVRY_VER'"
    echo "Only stable versions or -canary prereleases are allowed."
    echo "Examples: 0.1.0, 0.1.0-canary, 0.1.0-canary.1"
    exit 1
fi

echo "Updating Wavry to v$WAVRY_RAW"
echo "Updating RIFT to v$RIFT_RAW"
echo "Updating DELTA to v$DELTA_RAW"

# Get script directory and project root
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
cd "$PROJECT_ROOT"

# Helper for portable sed -i (for simple whole-file substitutions)
portable_sed() {
  local pattern=$1
  local file=$2
  if [[ "$OSTYPE" == "darwin"* ]]; then
    sed -i '' "$pattern" "$file"
  else
    sed -i "$pattern" "$file"
  fi
}

# Replace only the FIRST occurrence of ^version = "..." in a file.
# Needed for Cargo.toml files where dependency sections can also contain
# bare `version = "x.y.z"` lines (e.g. [dependencies.windows]).
replace_first_version() {
  local new_ver=$1
  local file=$2
  perl -i -pe '!$done && s/^version = "[^"]*"/version = "'"$new_ver"'"/ && ($done=1)' "$file"
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
replace_first_version "$WAVRY_RAW" "$PROJECT_ROOT/Cargo.toml"
echo "  Updated workspace version in Cargo.toml"

# 3. Update all crate Cargo.toml files that have an explicit version
#    No -maxdepth so wavry-desktop/src-tauri/Cargo.toml (3 levels deep) is included.
#    Uses replace_first_version so only [package].version is touched, not
#    bare `version = "x.y.z"` lines inside dependency sections.
echo "Updating crate Cargo.toml files..."
while IFS= read -r toml; do
  if grep -q "^version = \"" "$toml" 2>/dev/null && ! grep -q "^version\.workspace = true" "$toml" 2>/dev/null; then
    replace_first_version "$WAVRY_RAW" "$toml"
    rel="${toml#$PROJECT_ROOT/}"
    echo "  Updated $rel"
  fi
done < <(find "$PROJECT_ROOT/crates" -name "Cargo.toml")

# 4. Update tauri.conf.json
TAURI_CONF="$PROJECT_ROOT/crates/wavry-desktop/src-tauri/tauri.conf.json"
if [ -f "$TAURI_CONF" ]; then
  echo "Updating tauri.conf.json..."
  portable_sed "s/\"version\": \"[^\"]*\"/\"version\": \"$WAVRY_RAW\"/" "$TAURI_CONF"
  echo "  Updated crates/wavry-desktop/src-tauri/tauri.conf.json"
fi

# 5. Update Android build.gradle.kts
ANDROID_BUILD_FILE="$PROJECT_ROOT/apps/android/app/build.gradle.kts"
if [ -f "$ANDROID_BUILD_FILE" ]; then
  echo "Updating Android build.gradle.kts..."
  portable_sed "s/versionName = \"[^\"]*\"/versionName = \"$WAVRY_RAW\"/" "$ANDROID_BUILD_FILE"
  echo "  Updated apps/android/app/build.gradle.kts"
fi

# 6. Update package.json files
echo "Updating package.json files..."
found_any=0
while IFS= read -r pjson; do
  if [ -f "$pjson" ]; then
    portable_sed "s/\"version\": \"[^\"]*\"/\"version\": \"$WAVRY_RAW\"/" "$pjson"
    rel="${pjson#$PROJECT_ROOT/}"
    echo "  Updated $rel"
    found_any=1
  fi
done < <(find "$PROJECT_ROOT" -maxdepth 4 -name "package.json" -not -path "*/node_modules/*")
if [ "$found_any" -eq 0 ]; then
  echo "  (none found)"
fi

# 7. Update documentation
echo "Updating documentation..."

if [ -f "$PROJECT_ROOT/docs/RIFT_SPEC_V1.md" ]; then
  portable_sed "s/RIFT Protocol Specification v[0-9][0-9.]*/RIFT Protocol Specification v$RIFT_RAW/" "$PROJECT_ROOT/docs/RIFT_SPEC_V1.md"
  echo "  Updated docs/RIFT_SPEC_V1.md"
fi

if [ -f "$PROJECT_ROOT/docs/DELTA_CC_SPEC.md" ]; then
  portable_sed "s/DELTA Congestion Control Specification v[0-9][0-9.]*/DELTA Congestion Control Specification v$DELTA_RAW/" "$PROJECT_ROOT/docs/DELTA_CC_SPEC.md"
  echo "  Updated docs/DELTA_CC_SPEC.md"
fi

# 8. Regenerate Cargo.lock
echo "Regenerating Cargo.lock..."
cargo update --workspace --quiet
echo "  Updated Cargo.lock"

echo ""
echo "Version update complete."
echo "Wavry: v$WAVRY_RAW"
echo "RIFT:  v$RIFT_RAW"
echo "DELTA: v$DELTA_RAW"
