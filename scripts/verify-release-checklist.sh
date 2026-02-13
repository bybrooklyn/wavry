#!/usr/bin/env bash
set -euo pipefail

CHECKLIST_FILE="docs/RELEASE_CHECKLIST.md"

if [[ ! -f "$CHECKLIST_FILE" ]]; then
  echo "Release checklist file missing: $CHECKLIST_FILE" >&2
  exit 1
fi

if grep -Eq '^- \[ \]' "$CHECKLIST_FILE"; then
  echo "Release checklist is incomplete. Resolve unchecked items before publishing." >&2
  echo >&2
  grep -nE '^- \[ \]' "$CHECKLIST_FILE" >&2 || true
  exit 1
fi

echo "Release checklist is complete."
