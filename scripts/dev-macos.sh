#!/bin/bash
set -e

# Get repo root (assume script is in scripts/)
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(dirname "$SCRIPT_DIR")"

cd "$REPO_ROOT"

# 1. Build Rust FFI (Static Lib)
echo "Building Wavry Core (Rust FFI)..."
cargo build -p wavry-ffi

# 2. Run macOS App (Swift)
echo "Running Wavry macOS App..."
cd apps/macos
swift run
