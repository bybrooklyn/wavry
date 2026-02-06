#!/bin/bash
set -e

# Get repo root
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(dirname "$SCRIPT_DIR")"

cd "$REPO_ROOT"

# Keep Rust/C dependency deployment target aligned with the Swift package target.
export MACOSX_DEPLOYMENT_TARGET="${MACOSX_DEPLOYMENT_TARGET:-14.0}"

# 1. Build Rust FFI
echo "🦀 Building Wavry Core (Rust FFI)..."
cargo build -p wavry-ffi

# 2. Build Swift App
echo "🍏 Building Wavry macOS UI..."
cd apps/macos
swift build

# 3. Find binary and run it
BINARY_PATH=$(swift build --show-bin-path)/WavryMacOS
echo "🚀 Launching $BINARY_PATH..."
echo "✨ Wavry is launching! Interaction is enabled."
echo "Press Ctrl+C in this terminal to stop the app."

# Execute the binary in the foreground
exec "$BINARY_PATH"
