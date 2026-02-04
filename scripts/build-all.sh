#!/bin/bash
set -e

# Wavry Unified Build Script
# Handles building Master Server, Desktop (Tauri), and Native macOS apps.

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
DIST_DIR="$REPO_ROOT/dist"

GREEN='\033[0;32m'
YELLOW='\033[1;33m'
RED='\033[0;31m'
BLUE='\033[0;34m'
NC='\033[0m'

echo -e "${BLUE}═══════════════════════════════════════════════════════════${NC}"
echo -e "${BLUE}         Wavry Unified Build Pipeline${NC}"
echo -e "${BLUE}═══════════════════════════════════════════════════════════${NC}"

mkdir -p "$DIST_DIR"

# 1. Build Wavry Master
echo -e "\n${YELLOW}🏗️  Building Wavry Master Server...${NC}"
cd "$REPO_ROOT"
cargo build -p wavry-master --release
mkdir -p "$DIST_DIR/master"
cp "target/release/wavry-master" "$DIST_DIR/master/"
echo -e "${GREEN}✓ Master Server built at $DIST_DIR/master/wavry-master${NC}"

# 2. Build Wavry Desktop (Tauri)
echo -e "\n${YELLOW}🏗️  Building Wavry Desktop (Tauri)...${NC}"
cd "$REPO_ROOT/crates/wavry-desktop"
# Ensure dependencies are installed
if [ ! -d "node_modules" ]; then
    echo -e "${YELLOW}Installing Node dependencies...${NC}"
    bun install
fi

# Run Tauri build
bun tauri build

# Collect Tauri artifacts
mkdir -p "$DIST_DIR/desktop"
if [ "$(uname -s)" = "Darwin" ]; then
    # macOS artifacts
    cp -r "src-tauri/target/release/bundle/dmg" "$DIST_DIR/desktop/" || true
    cp -r "src-tauri/target/release/bundle/macos" "$DIST_DIR/desktop/" || true
else
    # Linux artifacts
    cp -r "src-tauri/target/release/bundle/deb" "$DIST_DIR/desktop/" || true
    cp -r "src-tauri/target/release/bundle/appimage" "$DIST_DIR/desktop/" || true
fi
echo -e "${GREEN}✓ Desktop App built at $DIST_DIR/desktop/${NC}"

# 3. Build Wavry Native (macOS Swift)
if [ "$(uname -s)" = "Darwin" ]; then
    echo -e "\n${YELLOW}🏗️  Building Wavry Native (macOS Swift)...${NC}"
    "$SCRIPT_DIR/build-macos.sh" release
    echo -e "${GREEN}✓ Native macOS App built at $DIST_DIR/Wavry.app${NC}"
fi

# 4. Build Wavry Relay
echo -e "\n${YELLOW}🏗️  Building Wavry Relay...${NC}"
cd "$REPO_ROOT"
cargo build -p wavry-relay --release
mkdir -p "$DIST_DIR/relay"
cp "target/release/wavry-relay" "$DIST_DIR/relay/"
echo -e "${GREEN}✓ Relay built at $DIST_DIR/relay/wavry-relay${NC}"

# 5. AUR Packaging Placeholder
if [ "$(uname -s)" = "Linux" ]; then
    echo -e "\n${BLUE}💡 To generate AUR package, use:${NC}"
    echo -e "   ${YELLOW}cd distribution/aur && makepkg -si${NC}"
fi

echo -e "\n${GREEN}═══════════════════════════════════════════════════════════${NC}"
echo -e "${GREEN}              All Builds Completed Successfully!${NC}"
echo -e "${GREEN}═══════════════════════════════════════════════════════════${NC}"
echo -e "Artifacts available in: ${YELLOW}$DIST_DIR${NC}"
