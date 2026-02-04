#!/bin/bash
set -e

# Get repo root
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(dirname "$SCRIPT_DIR")"

cd "$REPO_ROOT/crates/wavry-desktop"

# 1. Install dependencies
echo "Installing frontend dependencies..."
npm install

# 2. Run Tauri
echo "Running Wavry Desktop (Tauri)..."
npm run tauri dev
