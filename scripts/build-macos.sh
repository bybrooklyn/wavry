#!/bin/bash
set -e

# Build script for Wavry macOS application
# Creates a complete, distributable .app bundle

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

# Configuration
APP_NAME="Wavry"
BUNDLE_ID="dev.wavry.Wavry"
BUILD_TYPE="${1:-release}"  # 'debug' or 'release' (default: release)

# Colors for output
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
RED='\033[0;31m'
NC='\033[0m' # No Color

echo -e "${GREEN}=========================================${NC}"
echo -e "${GREEN}  Building $APP_NAME for macOS${NC}"
echo -e "${GREEN}  Build type: $BUILD_TYPE${NC}"
echo -e "${GREEN}=========================================${NC}"
echo ""

# Step 1: Build Rust FFI library
echo -e "${YELLOW}🦀 Building Rust FFI library...${NC}"
cd "$REPO_ROOT"
if [ "$BUILD_TYPE" = "release" ]; then
    cargo build -p wavry-ffi --release
    RUST_LIB_DIR="$REPO_ROOT/target/release"
else
    cargo build -p wavry-ffi
    RUST_LIB_DIR="$REPO_ROOT/target/debug"
fi

# Verify the library was built
if [ ! -f "$RUST_LIB_DIR/libwavry_ffi.a" ]; then
    echo -e "${RED}❌ Error: libwavry_ffi.a not found in $RUST_LIB_DIR${NC}"
    exit 1
fi
echo -e "${GREEN}✓ Rust FFI library built${NC}"
echo ""

# Step 2: Build Swift application
echo -e "${YELLOW}🍏 Building Swift application...${NC}"
cd "$REPO_ROOT/apps/macos"

if [ "$BUILD_TYPE" = "release" ]; then
    swift build -c release
    SWIFT_BUILD_DIR="$REPO_ROOT/apps/macos/.build/release"
else
    swift build
    SWIFT_BUILD_DIR="$REPO_ROOT/apps/macos/.build/debug"
fi

# Verify the executable was built
if [ ! -f "$SWIFT_BUILD_DIR/WavryMacOS" ]; then
    echo -e "${RED}❌ Error: WavryMacOS executable not found${NC}"
    exit 1
fi
echo -e "${GREEN}✓ Swift application built${NC}"
echo ""

# Step 3: Create .app bundle
echo -e "${YELLOW}📦 Creating application bundle...${NC}"
OUTPUT_DIR="$REPO_ROOT/dist"
APP_BUNDLE="$OUTPUT_DIR/$APP_NAME.app"
CONTENTS_DIR="$APP_BUNDLE/Contents"
MACOS_DIR="$CONTENTS_DIR/MacOS"
RESOURCES_DIR="$CONTENTS_DIR/Resources"

# Clean and create directory structure
rm -rf "$APP_BUNDLE"
mkdir -p "$MACOS_DIR"
mkdir -p "$RESOURCES_DIR"

# Copy executable (Rust library is statically linked)
cp "$SWIFT_BUILD_DIR/WavryMacOS" "$MACOS_DIR/$APP_NAME"


# Create Info.plist
cat > "$CONTENTS_DIR/Info.plist" << EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleDevelopmentRegion</key>
    <string>en</string>
    <key>CFBundleExecutable</key>
    <string>$APP_NAME</string>
    <key>CFBundleIconFile</key>
    <string>AppIcon</string>
    <key>CFBundleIconName</key>
    <string>AppIcon</string>
    <key>CFBundleIdentifier</key>
    <string>$BUNDLE_ID</string>
    <key>CFBundleInfoDictionaryVersion</key>
    <string>6.0</string>
    <key>CFBundleName</key>
    <string>$APP_NAME</string>
    <key>CFBundlePackageType</key>
    <string>APPL</string>
    <key>CFBundleShortVersionString</key>
    <string>1.0.0</string>
    <key>CFBundleVersion</key>
    <string>1</string>
    <key>LSMinimumSystemVersion</key>
    <string>14.0</string>
    <key>NSHighResolutionCapable</key>
    <true/>
    <key>NSPrincipalClass</key>
    <string>NSApplication</string>
    <key>NSScreenCaptureUsageDescription</key>
    <string>Wavry needs screen recording access to share your display.</string>
    <key>NSCameraUsageDescription</key>
    <string>Wavry uses the camera for video streaming.</string>
    <key>NSMicrophoneUsageDescription</key>
    <string>Wavry uses the microphone for audio streaming.</string>
</dict>
</plist>
EOF

# Create PkgInfo
echo -n "APPL????" > "$CONTENTS_DIR/PkgInfo"

echo -e "${GREEN}✓ Application bundle created${NC}"
echo ""

# Step 4: Sign the application (ad-hoc for local testing)
echo -e "${YELLOW}🔐 Signing application (ad-hoc)...${NC}"
codesign --force --deep --sign - "$APP_BUNDLE"
echo -e "${GREEN}✓ Application signed${NC}"
echo ""

# Summary
echo -e "${GREEN}=========================================${NC}"
echo -e "${GREEN}  Build Complete!${NC}"
echo -e "${GREEN}=========================================${NC}"
echo ""
echo -e "  📍 Location: ${YELLOW}$APP_BUNDLE${NC}"
echo ""
echo -e "  To run:"
echo -e "    ${YELLOW}open \"$APP_BUNDLE\"${NC}"
echo ""
echo -e "  To inspect:"
echo -e "    ${YELLOW}ls -la \"$APP_BUNDLE/Contents/\"${NC}"
echo ""

# Show bundle size
BUNDLE_SIZE=$(du -sh "$APP_BUNDLE" | cut -f1)
echo -e "  📦 Bundle size: ${YELLOW}$BUNDLE_SIZE${NC}"
echo ""

# Note about code signing for distribution
if [ "$BUILD_TYPE" = "release" ]; then
    echo -e "${YELLOW}⚠️  Note: For distribution, you'll need to:${NC}"
    echo -e "     1. Sign with a valid Developer ID"
    echo -e "     2. Notarize with Apple"
    echo -e "     3. Create a DMG or distribute via App Store"
    echo ""
fi
