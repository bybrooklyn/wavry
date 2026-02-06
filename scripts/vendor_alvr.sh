#!/usr/bin/env bash
set -euo pipefail

ALVR_REPO="https://github.com/alvr-org/ALVR.git"
ALVR_COMMIT="${1:-1a4194ff137937c0a4f416ad2d6d1acedb851e8a}"

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
DEST="$ROOT_DIR/third_party/alvr"
TMPDIR="$(mktemp -d /tmp/alvr_vendor.XXXXXX)"

cleanup() { rm -rf "$TMPDIR"; }
trap cleanup EXIT

git init -q "$TMPDIR"
cd "$TMPDIR"
git remote add origin "$ALVR_REPO"
git fetch -q --depth 1 origin "$ALVR_COMMIT"
git checkout -q FETCH_HEAD

mkdir -p "$DEST"
cp LICENSE "$DEST/LICENSE"

cat > "$DEST/COMMIT" <<EOF2
ALVR commit: $ALVR_COMMIT
Repo: $ALVR_REPO
Pinned: YES (do not track HEAD)
EOF2

# Extract minimal required directories
rm -rf "$DEST/alvr" "$DEST/openvr"
mkdir -p "$DEST/alvr"

copy_dir() {
  local src="$1"
  local dst="$DEST/$1"
  mkdir -p "$(dirname "$dst")"
  rsync -a --delete "$src" "$dst"
}

copy_dir "alvr/common"
copy_dir "alvr/graphics"
copy_dir "alvr/client_openxr"
copy_dir "alvr/server_openvr"
copy_dir "alvr/vrcompositor_wrapper"
copy_dir "alvr/system_info"
copy_dir "alvr/session"
copy_dir "openvr"

find "$DEST" -type f \( -name "*.rs" -o -name "*.c" -o -name "*.cpp" -o -name "*.h" -o -name "*.hpp" \) | while read -r f; do
  if ! grep -q "Derived from ALVR" "$f"; then
    tmp="$f.tmp"
    printf "// Derived from ALVR (MIT)\n// Original copyright preserved\n\n" > "$tmp"
    cat "$f" >> "$tmp"
    mv "$tmp" "$f"
  fi
done

echo "Vendored ALVR subset at commit $ALVR_COMMIT"
