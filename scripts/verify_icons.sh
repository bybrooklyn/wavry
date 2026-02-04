#!/bin/bash
set -e

MANIFEST="design/icons.json"
ICON_DIR="crates/wavry-desktop/src/assets/icons"

echo "Verifying icon parity..."
echo ""

# Check if manifest exists
if [ ! -f "$MANIFEST" ]; then
    echo "❌ Error: Manifest not found at $MANIFEST"
    exit 1
fi

# Check if icon directory exists
if [ ! -d "$ICON_DIR" ]; then
    echo "❌ Error: Icon directory not found at $ICON_DIR"
    exit 1
fi

# Extract semantic names from manifest
semantic_names=$(cat "$MANIFEST" | python3 -c "import sys, json; data = json.load(sys.stdin); print('\n'.join(data['icons'].keys()))")

missing_icons=()
found_icons=()
orphaned_svgs=()

# Check each semantic name has a corresponding SVG
for name in $semantic_names; do
    if [ -f "$ICON_DIR/$name.svg" ]; then
        found_icons+=("$name")
        echo "✓ $name.svg"
    else
        missing_icons+=("$name")
        echo "✗ $name.svg (MISSING)"
    fi
done

echo ""

# Check for orphaned SVGs (SVGs without manifest entry)
for svg in "$ICON_DIR"/*.svg; do
    if [ -f "$svg" ]; then
        basename=$(basename "$svg" .svg)
        if ! echo "$semantic_names" | grep -q "^$basename$"; then
            orphaned_svgs+=("$basename")
        fi
    fi
done

# Report orphaned icons
if [ ${#orphaned_svgs[@]} -gt 0 ]; then
    echo "⚠️  Orphaned SVGs (not in manifest):"
    for orphan in "${orphaned_svgs[@]}"; do
        echo "    - $orphan.svg"
    done
    echo ""
fi

# Summary
echo "========================================="
echo "Summary:"
echo "  Found: ${#found_icons[@]}"
echo "  Missing: ${#missing_icons[@]}"
echo "  Orphaned: ${#orphaned_svgs[@]}"
echo "========================================="

if [ ${#missing_icons[@]} -gt 0 ]; then
    echo ""
    echo "❌ Icon parity check FAILED"
    echo "Missing icons must be exported before deployment."
    exit 1
elif [ ${#orphaned_svgs[@]} -gt 0 ]; then
    echo ""
    echo "⚠️  Warning: Orphaned icons detected"
    echo "Consider removing unused SVGs or adding them to the manifest."
    exit 0
else
    echo ""
    echo "✅ Icon parity check PASSED"
    exit 0
fi
