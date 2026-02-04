# SF Symbol SVG Export Guide

## Problem
SF Symbols cannot be programmatically exported as vector SVG using public macOS APIs. The symbols are stored in a proprietary font format, and NSImage rendering produces rasterized output.

## Solution: Manual Export via SF Symbols App

### Prerequisites
1. Install the **SF Symbols app** from Apple Developer downloads
2. Ensure you have the latest version (6.0+)

### Export Process

#### Step 1: Open SF Symbols App
```bash
open -a "SF Symbols"
```

#### Step 2: Export Each Symbol
For each symbol in `design/icons.json`:

1. Search for the symbol name (e.g., "cloud.fill")
2. Select the symbol
3. Click **File > Export Symbol as Template Image**
4. Choose **SVG** as the format
5. Set size to **24pt**
6. Set weight to **Regular**
7. Set scale to **Medium**
8. Save to: `crates/wavry-desktop/src/assets/icons/{semantic_name}.svg`

**Naming Convention**: Use the semantic name from the manifest as the filename.

Example: `cloud.fill` → export as `connectivityService.svg`

#### Step 3: Verify Exports
Run the verification script:
```bash
./scripts/verify_icons.sh
```

### Alternative: Community SVG Libraries

If manual export is prohibitive, consider these community-maintained SF Symbol SVG sets:
- [SF-Symbols-SVG](https://github.com/microsoft/fluentui-system-icons) (Microsoft Fluent, SF Symbol style)
- [SF Pro Icons](https://github.com/sag1v/react-native-sf-symbols) (community exports)

**Note**: These may not have exact parity with the latest SF Symbols release.

### Automation Opportunity

The SF Symbols app **does** support AppleScript for batch operations. A future enhancement could:
1. Read `design/icons.json`
2. Open SF Symbols app
3. Use AppleScript to automate search and export
4. Save files with correct semantic names

This would make the process semi-automated while staying within Apple's supported workflows.
