#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

SKIP_CARGO=0
SKIP_RUNTIME=0

usage() {
  cat <<'EOF'
Usage: ./scripts/linux-display-smoke.sh [--skip-cargo] [--skip-runtime]

Linux display smoke test for monitor selection and capture preflight.

Options:
  --skip-cargo    Skip cargo checks for wavry-media and wavry-desktop.
  --skip-runtime  Skip Linux runtime dependency checks.
  --help          Show this help.
EOF
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --skip-cargo)
      SKIP_CARGO=1
      shift
      ;;
    --skip-runtime)
      SKIP_RUNTIME=1
      shift
      ;;
    --help|-h)
      usage
      exit 0
      ;;
    *)
      echo "[FAIL] Unknown argument: $1" >&2
      usage
      exit 2
      ;;
  esac
done

FAILED=0

pass() { echo "[PASS] $*"; }
warn() { echo "[WARN] $*"; }
fail() { echo "[FAIL] $*"; FAILED=$((FAILED + 1)); }

has_cmd() {
  command -v "$1" >/dev/null 2>&1
}

check_cmd() {
  if has_cmd "$1"; then
    pass "Command available: $1"
  else
    fail "Missing required command: $1"
  fi
}

check_gst_element() {
  local name="$1"
  if gst-inspect-1.0 "$name" >/dev/null 2>&1; then
    pass "GStreamer element available: $name"
  else
    fail "Missing GStreamer element: $name"
  fi
}

check_any_gst_element() {
  local label="$1"
  shift

  local name
  for name in "$@"; do
    if gst-inspect-1.0 "$name" >/dev/null 2>&1; then
      pass "$label available via: $name"
      return
    fi
  done

  fail "$label unavailable. Tried: $*"
}

check_any_user_service_active() {
  local label="$1"
  shift

  if [[ "${WAVRY_PORTAL_SERVICE_MODE:-}" == "process" ]]; then
    local candidate
    for candidate in "$@"; do
      local proc_name="${candidate%.service}"
      if pgrep -x "$proc_name" >/dev/null 2>&1; then
        pass "$label process active via: $proc_name"
        return
      fi
    done
    fail "$label process inactive. Tried: $*"
    return
  fi

  if ! has_cmd "systemctl"; then
    warn "systemctl not available; skipped $label service checks"
    return
  fi

  local svc
  for svc in "$@"; do
    if systemctl --user is-active --quiet "$svc"; then
      pass "$label service active via: $svc"
      return
    fi
  done

  fail "$label service inactive. Tried: $*"
}

portal_descriptor_exists() {
  local descriptor="$1"
  local roots=()

  if [[ -n "${XDG_DATA_HOME:-}" ]]; then
    roots+=("$XDG_DATA_HOME")
  elif [[ -n "${HOME:-}" ]]; then
    roots+=("$HOME/.local/share")
  fi

  if [[ -n "${XDG_DATA_DIRS:-}" ]]; then
    IFS=':' read -r -a xdg_dirs <<<"$XDG_DATA_DIRS"
    roots+=("${xdg_dirs[@]}")
  else
    roots+=("/usr/local/share" "/usr/share")
  fi

  local root
  for root in "${roots[@]}"; do
    [[ -z "$root" ]] && continue
    if [[ -f "$root/xdg-desktop-portal/portals/$descriptor" ]]; then
      return 0
    fi
  done
  return 1
}

check_any_portal_descriptor() {
  local label="$1"
  shift

  local descriptor
  for descriptor in "$@"; do
    if portal_descriptor_exists "$descriptor"; then
      pass "$label descriptor present: $descriptor"
      return
    fi
  done

  fail "$label descriptor missing. Tried: $*"
}

expected_portal_descriptors_for_desktop() {
  local desktop="${XDG_CURRENT_DESKTOP:-}"
  desktop="$(echo "$desktop" | tr '[:upper:]' '[:lower:]')"

  if [[ "$desktop" == *"kde"* || "$desktop" == *"plasma"* ]]; then
    echo "kde.portal gtk.portal"
    return
  fi
  if [[ "$desktop" == *"gnome"* || "$desktop" == *"unity"* || "$desktop" == *"cinnamon"* || "$desktop" == *"pantheon"* ]]; then
    echo "gnome.portal gtk.portal"
    return
  fi
  if [[ "$desktop" == *"hyprland"* ]]; then
    echo "hyprland.portal wlr.portal gtk.portal"
    return
  fi
  if [[ "$desktop" == *"sway"* || "$desktop" == *"wlroots"* || "$desktop" == *"river"* || "$desktop" == *"wayfire"* ]]; then
    echo "wlr.portal gtk.portal"
    return
  fi

  echo "kde.portal gnome.portal wlr.portal gtk.portal"
}

print_manual_matrix() {
  cat <<'EOF'

Manual runtime matrix (Wayland/X11):

1. Start desktop app with logs:
   cd crates/wavry-desktop
   RUST_LOG=info bun run tauri dev

2. In Sessions -> Local Host:
   - Verify monitor dropdown is populated.
   - Start host on monitor A.
   - Confirm logs include "Selected Wayland display stream" (Wayland) or no capture error (X11).

3. Stop host, switch to monitor B, start host again:
   - Confirm capture switches to the selected monitor.

4. While app is running, change connected monitors (disable/unplug one), then refresh monitor list:
   - Confirm selection auto-clamps to a valid monitor.
   - Start host again and verify capture still starts.

5. Stale-selection fallback:
   - Start host with an invalid monitor id via backend invocation or stale state.
   - Confirm host still starts and logs include
     "Requested Wayland display ... falling back to first stream".
EOF
}

echo "=== Wavry Linux Display Smoke Test ==="
echo "Repo: $REPO_ROOT"

if [[ "$SKIP_CARGO" -eq 0 ]]; then
  echo
  echo "== Cargo checks =="
  (
    cd "$REPO_ROOT"
    cargo check -p wavry-media
    cargo check -p wavry-desktop
  )
  pass "Cargo checks completed"
else
  warn "Skipping cargo checks (--skip-cargo)"
fi

if [[ "$SKIP_RUNTIME" -eq 1 ]]; then
  warn "Skipping runtime checks (--skip-runtime)"
  print_manual_matrix
  exit 0
fi

if [[ "$(uname -s)" != "Linux" ]]; then
  fail "Runtime checks require Linux (current: $(uname -s)). Use --skip-runtime on non-Linux hosts."
  print_manual_matrix
  exit 1
fi

echo
echo "== Runtime preflight =="

check_cmd "cargo"
check_cmd "gst-inspect-1.0"
check_cmd "xdg-desktop-portal"
check_cmd "pw-cli"
check_cmd "pactl"

SESSION_TYPE="${XDG_SESSION_TYPE:-unknown}"
WAYLAND_DISPLAY_VAR="${WAYLAND_DISPLAY:-}"
X11_DISPLAY_VAR="${DISPLAY:-}"

if [[ -n "$WAYLAND_DISPLAY_VAR" || "$SESSION_TYPE" == "wayland" ]]; then
  pass "Wayland session detected"
else
  warn "Wayland session not detected"
fi

if [[ -n "$X11_DISPLAY_VAR" ]]; then
  pass "X11 DISPLAY detected ($X11_DISPLAY_VAR)"
else
  warn "X11 DISPLAY not detected"
fi

if has_cmd "systemctl"; then
  if systemctl --user is-active --quiet xdg-desktop-portal.service; then
    pass "xdg-desktop-portal service is active"
  else
    warn "xdg-desktop-portal service not active (or no user systemd session)"
  fi
else
  warn "systemctl not available; skipped portal service health check"
fi

if [[ -n "$WAYLAND_DISPLAY_VAR" || "$SESSION_TYPE" == "wayland" ]]; then
  expected_descriptors="$(expected_portal_descriptors_for_desktop)"
  # shellcheck disable=SC2206
  descriptor_list=($expected_descriptors)
  check_any_portal_descriptor "Desktop portal backend" "${descriptor_list[@]}"

  check_any_user_service_active "Desktop portal backend" \
    "xdg-desktop-portal-gnome.service" \
    "xdg-desktop-portal-kde.service" \
    "xdg-desktop-portal-wlr.service" \
    "xdg-desktop-portal-hyprland.service" \
    "xdg-desktop-portal-gtk.service"
fi

check_gst_element "videoconvert"
check_gst_element "videoscale"
check_gst_element "queue"
check_gst_element "appsink"
check_gst_element "h264parse"
check_gst_element "opusenc"
check_gst_element "audioresample"
check_gst_element "audioconvert"

if [[ -n "$WAYLAND_DISPLAY_VAR" || "$SESSION_TYPE" == "wayland" ]]; then
  check_gst_element "pipewiresrc"
fi

if [[ -n "$X11_DISPLAY_VAR" ]]; then
  check_gst_element "ximagesrc"
  check_gst_element "videocrop"
fi

check_any_gst_element "Microphone source" \
  "pulsesrc" \
  "autoaudiosrc"

check_any_gst_element "H264 encoder" \
  "vaapih264enc" \
  "nvh264enc" \
  "v4l2h264enc" \
  "x264enc" \
  "openh264enc"

if [[ "$FAILED" -ne 0 ]]; then
  echo
  fail "Preflight completed with $FAILED failure(s)"
  print_manual_matrix
  exit 1
fi

echo
pass "Preflight completed successfully"
print_manual_matrix
