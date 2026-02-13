#!/usr/bin/env bash
set -euo pipefail

# Wavry Linux Package Validator
# 
# Verifies that generated packages (.AppImage, .deb, .rpm) are valid.

ARTIFACT_DIR="${1:-dist}"

info() { echo -e "\033[0;34m[info]\033[0m $*"; }
success() { echo -e "\033[0;32m[ok]\033[0m $*"; }
fail() { echo -e "\033[0;31m[error]\033[0m $*" >&2; exit 1; }

if [[ ! -d "$ARTIFACT_DIR" ]]; then
  fail "Artifact directory not found: $ARTIFACT_DIR"
fi

# 1. AppImage Validation
info "Validating AppImage..."
APPIMAGE=$(find "$ARTIFACT_DIR" -name "*.AppImage" | head -n 1)
if [[ -n "$APPIMAGE" ]]; then
  info "Checking $APPIMAGE"
  chmod +x "$APPIMAGE"
  # Try to run with --appimage-extract-and-run if available, or just check version
  "$APPIMAGE" --appimage-version >/dev/null
  success "AppImage version check passed."
else
  warn "No AppImage found in $ARTIFACT_DIR"
fi

# 2. DEB Validation
info "Validating DEB..."
DEB=$(find "$ARTIFACT_DIR" -name "*.deb" | head -n 1)
if [[ -n "$DEB" ]]; then
  if command -v dpkg-deb >/dev/null 2>&1; then
    info "Checking $DEB"
    dpkg-deb --info "$DEB" >/dev/null
    dpkg-deb --contents "$DEB" | grep -q "usr/bin/wavry-desktop"
    success "DEB metadata and content check passed."
  else
    warn "dpkg-deb not found; skipped DEB validation."
  fi
else
  warn "No DEB found in $ARTIFACT_DIR"
fi

# 3. RPM Validation
info "Validating RPM..."
RPM=$(find "$ARTIFACT_DIR" -name "*.rpm" | head -n 1)
if [[ -n "$RPM" ]]; then
  if command -v rpm >/dev/null 2>&1; then
    info "Checking $RPM"
    rpm -qip "$RPM" >/dev/null
    rpm -qlp "$RPM" | grep -q "usr/bin/wavry-desktop"
    success "RPM metadata and content check passed."
  else
    warn "rpm not found; skipped RPM validation."
  fi
else
  warn "No RPM found in $ARTIFACT_DIR"
fi

success "Linux package validation completed."
