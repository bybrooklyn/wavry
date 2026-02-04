#!/bin/bash
set -e

# Cross-compilation build script for Wavry
# Builds for multiple platforms and architectures

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

# Colors
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
RED='\033[0;31m'
BLUE='\033[0;34m'
NC='\033[0m'

# Supported targets
MACOS_TARGETS=(
    "aarch64-apple-darwin"    # macOS Apple Silicon (M1/M2/M3)
    "x86_64-apple-darwin"     # macOS Intel
)

LINUX_TARGETS=(
    "x86_64-unknown-linux-gnu"    # Linux x86-64
    "i686-unknown-linux-gnu"      # Linux x86 (32-bit)
)

WINDOWS_TARGETS=(
    "x86_64-pc-windows-gnu"       # Windows x86-64 (MinGW)
)

usage() {
    echo "Usage: $0 [OPTIONS] [TARGET...]"
    echo ""
    echo "Options:"
    echo "  --all         Build for all supported targets"
    echo "  --macos       Build for all macOS targets"
    echo "  --linux       Build for all Linux targets"
    echo "  --windows     Build for all Windows targets"
    echo "  --release     Build in release mode (default)"
    echo "  --debug       Build in debug mode"
    echo "  --list        List all supported targets"
    echo "  --install     Install required Rust targets"
    echo "  -h, --help    Show this help message"
    echo ""
    echo "Targets:"
    echo "  aarch64-apple-darwin      macOS Apple Silicon"
    echo "  x86_64-apple-darwin       macOS Intel"
    echo "  x86_64-unknown-linux-gnu  Linux x86-64"
    echo "  i686-unknown-linux-gnu    Linux x86 (32-bit)"
    echo "  x86_64-pc-windows-gnu     Windows x86-64"
    echo ""
    echo "Examples:"
    echo "  $0 --all                           # Build all targets"
    echo "  $0 --macos                         # Build all macOS targets"
    echo "  $0 x86_64-unknown-linux-gnu        # Build specific target"
    echo "  $0 --install                       # Install all Rust targets"
}

list_targets() {
    echo -e "${BLUE}Supported targets:${NC}"
    echo ""
    echo -e "${GREEN}macOS:${NC}"
    for t in "${MACOS_TARGETS[@]}"; do
        echo "  - $t"
    done
    echo ""
    echo -e "${GREEN}Linux:${NC}"
    for t in "${LINUX_TARGETS[@]}"; do
        echo "  - $t"
    done
    echo ""
    echo -e "${GREEN}Windows:${NC}"
    for t in "${WINDOWS_TARGETS[@]}"; do
        echo "  - $t"
    done
    echo ""
    echo -e "${YELLOW}Note: macOS Swift app can only be built on macOS.${NC}"
    echo -e "${YELLOW}The targets above are for the Rust core library.${NC}"
}

install_targets() {
    echo -e "${YELLOW}Installing Rust targets...${NC}"
    
    ALL_TARGETS=("${MACOS_TARGETS[@]}" "${LINUX_TARGETS[@]}" "${WINDOWS_TARGETS[@]}")
    
    for target in "${ALL_TARGETS[@]}"; do
        echo -e "  Installing ${BLUE}$target${NC}..."
        rustup target add "$target" 2>/dev/null || echo -e "    ${YELLOW}Already installed or unavailable${NC}"
    done
    
    echo ""
    echo -e "${YELLOW}For cross-compilation, you may also need:${NC}"
    echo ""
    echo -e "${GREEN}Linux cross-compilation (from macOS):${NC}"
    echo "  brew install FiloSottile/musl-cross/musl-cross"
    echo "  brew install mingw-w64"
    echo ""
    echo -e "${GREEN}Windows cross-compilation (from macOS):${NC}"
    echo "  brew install mingw-w64"
    echo ""
    echo -e "${GREEN}Or use Docker for Linux builds:${NC}"
    echo "  docker run --rm -v \$(pwd):/app -w /app rust:latest cargo build --target x86_64-unknown-linux-gnu"
}

build_target() {
    local target=$1
    local build_mode=$2
    local cargo_flag=""
    local output_dir=""
    
    if [ "$build_mode" = "release" ]; then
        cargo_flag="--release"
        output_dir="release"
    else
        output_dir="debug"
    fi
    
    echo -e "${BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
    echo -e "${GREEN}Building for: ${YELLOW}$target${NC}"
    echo -e "${BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
    
    cd "$REPO_ROOT"
    
    # Check if target is installed
    if ! rustup target list --installed | grep -q "^$target$"; then
        echo -e "${YELLOW}Target $target not installed. Installing...${NC}"
        rustup target add "$target" || {
            echo -e "${RED}Failed to install target $target${NC}"
            return 1
        }
    fi
    
    # Build the Rust library
    echo -e "${YELLOW}🦀 Building wavry-core for $target...${NC}"
    
    # Set linker for cross-compilation
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
    
    # Build core library (skip platform-specific crates for cross-compilation)
    if cargo build --target "$target" $cargo_flag -p wavry-core 2>&1; then
        echo -e "${GREEN}✓ wavry-core built for $target${NC}"
    else
        echo -e "${RED}✗ Failed to build wavry-core for $target${NC}"
        return 1
    fi
    
    # For native macOS targets, also build the FFI and Swift app
    local current_arch=$(uname -m)
    local current_os=$(uname -s)
    
    if [ "$current_os" = "Darwin" ]; then
        if [ "$target" = "aarch64-apple-darwin" ] && [ "$current_arch" = "arm64" ]; then
            echo -e "${YELLOW}🍏 Building Swift app (native Apple Silicon)...${NC}"
            build_macos_app "$target" "$build_mode"
        elif [ "$target" = "x86_64-apple-darwin" ] && [ "$current_arch" = "x86_64" ]; then
            echo -e "${YELLOW}🍏 Building Swift app (native Intel)...${NC}"
            build_macos_app "$target" "$build_mode"
        elif [ "$target" = "aarch64-apple-darwin" ] || [ "$target" = "x86_64-apple-darwin" ]; then
            # Cross-compile FFI for other macOS arch
            echo -e "${YELLOW}🍏 Cross-compiling FFI for $target...${NC}"
            cargo build --target "$target" $cargo_flag -p wavry-ffi 2>&1 || true
        fi
    fi
    
    # Copy artifacts to dist folder
    local dist_target_dir="$REPO_ROOT/dist/$target"
    mkdir -p "$dist_target_dir"
    
    # Copy library files
    local lib_dir="$REPO_ROOT/target/$target/$output_dir"
    if [ -f "$lib_dir/libwavry_core.a" ]; then
        cp "$lib_dir/libwavry_core.a" "$dist_target_dir/"
    fi
    if [ -f "$lib_dir/libwavry_core.so" ]; then
        cp "$lib_dir/libwavry_core.so" "$dist_target_dir/"
    fi
    if [ -f "$lib_dir/wavry_core.dll" ]; then
        cp "$lib_dir/wavry_core.dll" "$dist_target_dir/"
    fi
    
    echo -e "${GREEN}✓ Artifacts copied to dist/$target/${NC}"
    echo ""
}

build_macos_app() {
    local target=$1
    local build_mode=$2
    
    # Build FFI
    if [ "$build_mode" = "release" ]; then
        cargo build --target "$target" --release -p wavry-ffi
    else
        cargo build --target "$target" -p wavry-ffi
    fi
    
    # Build Swift app
    cd "$REPO_ROOT/apps/macos"
    if [ "$build_mode" = "release" ]; then
        swift build -c release
    else
        swift build
    fi
    
    # Create app bundle
    "$SCRIPT_DIR/build-macos.sh" "$build_mode"
    
    cd "$REPO_ROOT"
}

# Parse arguments
BUILD_MODE="release"
TARGETS=()

while [[ $# -gt 0 ]]; do
    case $1 in
        --all)
            TARGETS+=("${MACOS_TARGETS[@]}" "${LINUX_TARGETS[@]}" "${WINDOWS_TARGETS[@]}")
            shift
            ;;
        --macos)
            TARGETS+=("${MACOS_TARGETS[@]}")
            shift
            ;;
        --linux)
            TARGETS+=("${LINUX_TARGETS[@]}")
            shift
            ;;
        --windows)
            TARGETS+=("${WINDOWS_TARGETS[@]}")
            shift
            ;;
        --release)
            BUILD_MODE="release"
            shift
            ;;
        --debug)
            BUILD_MODE="debug"
            shift
            ;;
        --list)
            list_targets
            exit 0
            ;;
        --install)
            install_targets
            exit 0
            ;;
        -h|--help)
            usage
            exit 0
            ;;
        -*)
            echo -e "${RED}Unknown option: $1${NC}"
            usage
            exit 1
            ;;
        *)
            TARGETS+=("$1")
            shift
            ;;
    esac
done

# If no targets specified, show usage
if [ ${#TARGETS[@]} -eq 0 ]; then
    usage
    exit 1
fi

# Remove duplicates
TARGETS=($(echo "${TARGETS[@]}" | tr ' ' '\n' | sort -u | tr '\n' ' '))

echo -e "${GREEN}=========================================${NC}"
echo -e "${GREEN}  Wavry Cross-Compilation Build${NC}"
echo -e "${GREEN}  Mode: $BUILD_MODE${NC}"
echo -e "${GREEN}  Targets: ${#TARGETS[@]}${NC}"
echo -e "${GREEN}=========================================${NC}"
echo ""

# Build each target
FAILED_TARGETS=()
SUCCESS_TARGETS=()

for target in "${TARGETS[@]}"; do
    if build_target "$target" "$BUILD_MODE"; then
        SUCCESS_TARGETS+=("$target")
    else
        FAILED_TARGETS+=("$target")
    fi
done

# Summary
echo -e "${GREEN}=========================================${NC}"
echo -e "${GREEN}  Build Summary${NC}"
echo -e "${GREEN}=========================================${NC}"
echo ""

if [ ${#SUCCESS_TARGETS[@]} -gt 0 ]; then
    echo -e "${GREEN}✓ Successful builds:${NC}"
    for t in "${SUCCESS_TARGETS[@]}"; do
        echo -e "  - $t"
    done
    echo ""
fi

if [ ${#FAILED_TARGETS[@]} -gt 0 ]; then
    echo -e "${RED}✗ Failed builds:${NC}"
    for t in "${FAILED_TARGETS[@]}"; do
        echo -e "  - $t"
    done
    echo ""
    exit 1
fi

echo -e "${GREEN}All builds completed successfully!${NC}"
echo ""
echo -e "Artifacts are in: ${YELLOW}$REPO_ROOT/dist/${NC}"
ls -la "$REPO_ROOT/dist/"
