#!/usr/bin/env bash
set -euo pipefail

# Wavry Rollback Utility
# 
# Usage: ./scripts/rollback.sh <component> <previous_version>
# Example: ./scripts/rollback.sh gateway v0.0.4

COMPONENT="${1:-}"
PREV_VERSION="${2:-}"

if [[ -z "$COMPONENT" || -z "$PREV_VERSION" ]]; then
  echo "Usage: $0 <component> <previous_version>"
  echo "Components: gateway, relay, master, server"
  exit 1
fi

GREEN='\033[0;32m'
RED='\033[0;31m'
BLUE='\033[0;34m'
NC='\033[0m'

info() { echo -e "${BLUE}[info]${NC} $*"; }
success() { echo -e "${GREEN}[ok]${NC} $*"; }
fail() { echo -e "${RED}[error]${NC} $*" >&2; exit 1; }

info "Initiating rollback for $COMPONENT to version $PREV_VERSION"

case "$COMPONENT" in
  gateway|relay)
    info "Rolling back Docker-based service: $COMPONENT"
    IMAGE="ghcr.io/bybrooklyn/wavry/$COMPONENT:$PREV_VERSION"
    info "Target image: $IMAGE"
    
    # Check if image exists
    if ! docker manifest inspect "$IMAGE" >/dev/null 2>&1; then
      warn "Image $IMAGE not found in registry. Ensure the version is correct."
    fi
    
    echo "Steps to manually rollback $COMPONENT:"
    echo "1. Update docker-compose.yml or deployment spec with image: $IMAGE"
    echo "2. Run: docker-compose up -d $COMPONENT"
    ;;
    
  master|server)
    info "Rolling back binary-based service: $COMPONENT"
    echo "Steps to rollback $COMPONENT:"
    echo "1. Download binary for $PREV_VERSION from GitHub Releases."
    echo "2. Replace current binary with downloaded version."
    echo "3. Restart service: systemctl restart wavry-$COMPONENT"
    ;;
    
  *)
    fail "Unknown component: $COMPONENT"
    ;;
esac

# Database Migration Check
if [[ "$COMPONENT" == "gateway" ]]; then
  info "Database migration rollback may be required."
  echo "1. Identify migrations introduced since $PREV_VERSION."
  echo "2. Run SQL revert scripts if available."
fi

success "Rollback plan generated for $COMPONENT $PREV_VERSION"
