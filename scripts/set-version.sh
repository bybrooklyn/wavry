#!/bin/bash
# scripts/set-version.sh
# Usage: ./scripts/set-version.sh <wavry_version> <rift_version> <delta_version>

if [ "$#" -ne 3 ]; then
    echo "Usage: $0 <wavry_version> <rift_version> <delta_version>"
    echo "Example: $0 0.0.1-canary 1.2 1.1.0"
    exit 1
fi

WAVRY_VER=$1
RIFT_VER=$2
DELTA_VER=$3

# Remove 'v' prefix if present for programmatic use
WAVRY_RAW=$(echo $WAVRY_VER | sed 's/^v//')
RIFT_RAW=$(echo $RIFT_VER | sed 's/^v//')
DELTA_RAW=$(echo $DELTA_VER | sed 's/^v//')

echo "Updating Wavry to v$WAVRY_RAW"
echo "Updating RIFT to v$RIFT_RAW"
echo "Updating DELTA to v$DELTA_RAW"

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
cat <<EOF > VERSION
WAVRY v$WAVRY_RAW
RIFT (Remote Interactive Frame Transport) v$RIFT_RAW
DELTA (Differential Latency Estimation and Tuning Algorithm) v$DELTA_RAW
ALVR (Commit 1a4194ff137937c0a4f416ad2d6d1acedb851e8a)
EOF

# 2. Update Cargo.toml files
echo "Updating Cargo.toml files..."
# Only update the first 'version =' occurrence (package version)
portable_sed "1,/^version = .*/s/^version = .*/version = \"$WAVRY_RAW\"/" Cargo.toml
echo "  Updated root Cargo.toml"
find crates -name "Cargo.toml" -maxdepth 3 | while read -r toml; do
  portable_sed "1,/^version = .*/s/^version = .*/version = \"$WAVRY_RAW\"/" "$toml"
  echo "  Updated $toml"
done

# 3. Update Android build.gradle.kts
echo "Updating Android build.gradle.kts..."
portable_sed "s/versionName = .*/versionName = \"$WAVRY_RAW\"/" apps/android/app/build.gradle.kts
echo "  Updated apps/android/app/build.gradle.kts"

# 4. Update package.json files
echo "Updating package.json files..."
find . -name "package.json" -not -path "*/node_modules/*" | while read -r pjson; do
  # Only update the first "version": occurrence
  portable_sed "1,/\"version\": \".*\"/s/\"version\": \".*\"/\"version\": \"$WAVRY_RAW\"/" "$pjson"
  echo "  Updated $pjson"
done

# 5. Update Documentation
echo "Updating documentation..."
# RIFT Spec
portable_sed "s/RIFT Protocol Specification v[0-9.]*/RIFT Protocol Specification v$RIFT_RAW/" docs/RIFT_SPEC_V1.md
echo "  Updated docs/RIFT_SPEC_V1.md"
# DELTA Spec
portable_sed "s/DELTA Congestion Control Specification v[0-9.]*/DELTA Congestion Control Specification v$DELTA_RAW/" docs/DELTA_CC_SPEC.md
echo "  Updated docs/DELTA_CC_SPEC.md"

echo "Version update complete."
