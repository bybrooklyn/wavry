# Icon Export Checklist

All icons are currently **placeholders**. Export the real SF Symbols using the checklist below.

## Quick Start

```bash
# Open helper script (opens SF Symbols app + shows checklist)
./scripts/export_icons_helper.sh

# After exporting, verify
./scripts/verify_icons.sh
```

## Export Settings (Apply to ALL exports)
- **Size**: 24pt
- **Weight**: Regular
- **Scale**: Medium
- **Format**: SVG (Template Image)
- **Save to**: `crates/wavry-desktop/src/assets/icons/`

## Export Checklist

| # | Search for... | Export as... |
|---|---------------|--------------|
| ☐ 1 | `desktopcomputer` | `tabSessions.svg` |
| ☐ 2 | `gearshape.fill` | `tabSettings.svg` |
| ☐ 3 | `doc.on.doc` | `copy.svg` |
| ☐ 4 | `person.circle.fill` | `identity.svg` |
| ☐ 5 | `checkmark.circle.fill` | `success.svg` |
| ☐ 6 | `network.slash` | `noSessions.svg` |
| ☐ 7 | `macpro.gen3.fill` | `hostDefault.svg` |
| ☐ 8 | `cloud.fill` | `connectivityService.svg` |
| ☐ 9 | `network` | `connectivityDirect.svg` |
| ☐ 10 | `server.rack` | `connectivityCustom.svg` |
| ☐ 11 | `lock.shield.fill` | `permissions.svg` |
| ☐ 12 | `display` | `screenRecording.svg` |
| ☐ 13 | `info.circle` | `info.svg` |

## In SF Symbols App

1. **Search**: Type the symbol name (e.g., "cloud.fill")
2. **Select**: Click the symbol
3. **Export**: 
   - **File** → **Export Symbol...**
   - Choose **SVG** format
   - Verify settings (24pt, Regular, Medium)
4. **Save**: Use the exact filename from the table above
5. **Repeat**: Move to next symbol

## Verification

After exporting all 13 icons:

```bash
./scripts/verify_icons.sh
```

You should see:
```
✓ tabSessions.svg
✓ tabSettings.svg
...
=========================================
Summary:
  Found: 13
  Missing: 0
  Orphaned: 0
=========================================
✅ Icon parity check PASSED
```

## Current Status

Run `./scripts/verify_icons.sh` to see which icons are still placeholders vs. real exports.

Placeholder icons show as dashed boxes with "?" in the UI.
