#!/bin/bash
set -e

MANIFEST="design/icons.json"
ICON_DIR="crates/wavry-desktop/src/assets/icons"

echo "========================================="
echo "SF Symbol Export Helper"
echo "========================================="
echo ""

# Open SF Symbols app
echo "Opening SF Symbols app..."
open -a "SF Symbols" 2>/dev/null || {
    echo "⚠️  SF Symbols app not found!"
    echo ""
    echo "Please download it from:"
    echo "https://developer.apple.com/sf-symbols/"
    echo ""
    exit 1
}

sleep 2

# Extract icon mappings
echo ""
echo "========================================="
echo "Export Checklist"
echo "========================================="
echo ""
echo "For each icon below:"
echo "  1. Search for the SF Symbol name in SF Symbols app"
echo "  2. Select the symbol"
echo "  3. File > Export Symbol > SVG"
echo "  4. Set: 24pt, Regular weight, Medium scale"
echo "  5. Save as the filename shown"
echo ""
echo "Save location: $(pwd)/$ICON_DIR"
echo ""
echo "========================================="
echo ""

# Parse and display the checklist
python3 << 'EOF'
import json

with open('design/icons.json', 'r') as f:
    manifest = json.load(f)

icons = manifest['icons']
for i, (semantic_name, symbol_name) in enumerate(icons.items(), 1):
    checkbox = "☐"
    print(f"{checkbox} {i:2d}. Search: \"{symbol_name}\"")
    print(f"       Export as: \"{semantic_name}.svg\"")
    print()
EOF

echo "========================================="
echo ""
echo "When finished, run:"
echo "  ./scripts/verify_icons.sh"
echo ""
