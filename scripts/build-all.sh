#!/bin/bash
set -e

# Build Wavry for all platforms (release mode)
# Outputs to dist/<target>/

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

GREEN='\033[0;32m'
YELLOW='\033[1;33m'
RED='\033[0;31m'
BLUE='\033[0;34m'
NC='\033[0m'

TARGETS=(
    "aarch64-apple-darwin"        # macOS Apple Silicon
    "x86_64-apple-darwin"         # macOS Intel
    "x86_64-unknown-linux-gnu"    # Linux x86-64
    "i686-unknown-linux-gnu"      # Linux x86
    "x86_64-pc-windows-gnu"       # Windows x86-64
)

echo -e "${GREEN}═══════════════════════════════════════════════════════════${NC}"
echo -e "${GREEN}  Wavry Release Build - All Platforms${NC}"
echo -e "${GREEN}═══════════════════════════════════════════════════════════${NC}"
echo ""

cd "$REPO_ROOT"
mkdir -p dist

# Ensure all targets are installed
echo -e "${YELLOW}Checking Rust targets...${NC}"
for target in "${TARGETS[@]}"; do
    rustup target add "$target" 2>/dev/null || true
done
echo ""

SUCCESS=()
FAILED=()

for target in "${TARGETS[@]}"; do
    echo -e "${BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
    echo -e "${YELLOW}Building: $target${NC}"
    echo -e "${BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
    
    # Set cross-compilation linker
    case "$target" in
        x86_64-unknown-linux-gnu)
            export CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_LINKER="x86_64-linux-gnu-gcc"
            ;;
        i686-unknown-linux-gnu)
            export CARGO_TARGET_I686_UNKNOWN_LINUX_GNU_LINKER="i686-linux-gnu-gcc"
            ;;
        x86_64-pc-windows-gnu)
            export CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER="x86_64-w64-mingw32-gcc"
            ;;
    esac
    
    # Build wavry-common (cross-platform core)
    if cargo build --target "$target" --release -p wavry-common 2>&1; then
        mkdir -p "dist/$target"
        
        # Copy artifacts
        LIB_DIR="target/$target/release"
        [ -f "$LIB_DIR/libwavry_common.a" ] && cp "$LIB_DIR/libwavry_common.a" "dist/$target/"
        [ -f "$LIB_DIR/libwavry_common.so" ] && cp "$LIB_DIR/libwavry_common.so" "dist/$target/"
        [ -f "$LIB_DIR/libwavry_common.dylib" ] && cp "$LIB_DIR/libwavry_common.dylib" "dist/$target/"
        [ -f "$LIB_DIR/wavry_common.dll" ] && cp "$LIB_DIR/wavry_common.dll" "dist/$target/"

        
        echo -e "${GREEN}✓ $target${NC}"
        SUCCESS+=("$target")
    else
        echo -e "${RED}✗ $target${NC}"
        FAILED+=("$target")
    fi
    echo ""
done

# Build macOS app if on macOS
if [ "$(uname -s)" = "Darwin" ]; then
    echo -e "${BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
    echo -e "${YELLOW}Building: macOS App Bundle${NC}"
    echo -e "${BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
    
    if "$SCRIPT_DIR/build-macos.sh" release 2>&1; then
        echo -e "${GREEN}✓ Wavry.app${NC}"
        SUCCESS+=("Wavry.app")
    else
        echo -e "${RED}✗ Wavry.app${NC}"
        FAILED+=("Wavry.app")
    fi
    echo ""
fi

# Summary
echo -e "${GREEN}═══════════════════════════════════════════════════════════${NC}"
echo -e "${GREEN}  Build Complete${NC}"
echo -e "${GREEN}═══════════════════════════════════════════════════════════${NC}"
echo ""
echo -e "${GREEN}✓ Succeeded (${#SUCCESS[@]}):${NC}"
for t in "${SUCCESS[@]}"; do echo "    $t"; done
echo ""

if [ ${#FAILED[@]} -gt 0 ]; then
    echo -e "${RED}✗ Failed (${#FAILED[@]}):${NC}"
    for t in "${FAILED[@]}"; do echo "    $t"; done
    echo ""
    echo -e "${YELLOW}Note: Cross-compilation requires toolchains:${NC}"
    echo "  brew install mingw-w64                     # Windows"
    echo "  brew install FiloSottile/musl-cross/musl-cross  # Linux"
fi

echo ""
echo -e "Output: ${YELLOW}$REPO_ROOT/dist/${NC}"
ls -la "$REPO_ROOT/dist/"
