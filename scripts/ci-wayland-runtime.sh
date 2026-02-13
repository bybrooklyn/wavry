#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

if [[ "$(uname -s)" != "Linux" ]]; then
  echo "[FAIL] Wayland runtime CI harness only supports Linux." >&2
  exit 1
fi

require_cmd() {
  local cmd="$1"
  if ! command -v "$cmd" >/dev/null 2>&1; then
    echo "[FAIL] Missing required command: $cmd" >&2
    exit 1
  fi
}

require_cmd dbus-daemon
require_cmd weston
require_cmd pipewire
require_cmd xdg-desktop-portal
require_cmd xdg-desktop-portal-gtk

TMP_DIR="$(mktemp -d -t wavry-wayland-ci.XXXXXX)"
LOG_DIR="$TMP_DIR/logs"
mkdir -p "$LOG_DIR"

export XDG_RUNTIME_DIR="$TMP_DIR/runtime"
mkdir -p "$XDG_RUNTIME_DIR"
chmod 700 "$XDG_RUNTIME_DIR"

export WAYLAND_DISPLAY="wayland-1"
export XDG_SESSION_TYPE="wayland"
export XDG_CURRENT_DESKTOP="GNOME"
export GTK_USE_PORTAL="1"
export WAVRY_PORTAL_SERVICE_MODE="process"

PIDS=()
DBUS_PID=""

start_bg() {
  local name="$1"
  shift
  "$@" >"$LOG_DIR/${name}.log" 2>&1 &
  local pid=$!
  PIDS+=("$pid")
  echo "[INFO] Started $name (pid=$pid)"
}

wait_for_socket() {
  local socket_path="$1"
  local timeout_secs="${2:-20}"
  local i
  for ((i = 0; i < timeout_secs; i++)); do
    if [[ -S "$socket_path" ]]; then
      return 0
    fi
    sleep 1
  done
  return 1
}

wait_for_process() {
  local proc_name="$1"
  local timeout_secs="${2:-20}"
  local i
  for ((i = 0; i < timeout_secs; i++)); do
    if pgrep -x "$proc_name" >/dev/null 2>&1; then
      return 0
    fi
    sleep 1
  done
  return 1
}

cleanup() {
  local code=$?

  for pid in "${PIDS[@]:-}"; do
    if kill -0 "$pid" >/dev/null 2>&1; then
      kill "$pid" >/dev/null 2>&1 || true
    fi
  done

  if [[ -n "$DBUS_PID" ]] && kill -0 "$DBUS_PID" >/dev/null 2>&1; then
    kill "$DBUS_PID" >/dev/null 2>&1 || true
  fi

  if [[ "$code" -ne 0 ]]; then
    echo "[ERROR] Wayland CI harness failed. Dumping logs..." >&2
    for log in "$LOG_DIR"/*.log; do
      [[ -f "$log" ]] || continue
      echo "===== ${log##*/} =====" >&2
      tail -n 200 "$log" >&2 || true
      echo >&2
    done
  fi

  rm -rf "$TMP_DIR"
  exit "$code"
}

trap cleanup EXIT

# Start session D-Bus daemon.
DBUS_OUTPUT="$(dbus-daemon --session --fork --print-address 1 --print-pid 1)"
export DBUS_SESSION_BUS_ADDRESS="$(echo "$DBUS_OUTPUT" | sed -n '1p')"
DBUS_PID="$(echo "$DBUS_OUTPUT" | sed -n '2p')"
echo "[INFO] D-Bus session started (pid=$DBUS_PID)"

start_bg pipewire pipewire
if command -v wireplumber >/dev/null 2>&1; then
  start_bg wireplumber wireplumber
fi

start_bg weston weston --backend=headless-backend.so --socket="$WAYLAND_DISPLAY" --idle-time=0 --xwayland

if ! wait_for_socket "$XDG_RUNTIME_DIR/$WAYLAND_DISPLAY" 25; then
  echo "[FAIL] Wayland socket did not appear: $XDG_RUNTIME_DIR/$WAYLAND_DISPLAY" >&2
  exit 1
fi

start_bg portal_gtk xdg-desktop-portal-gtk -r
start_bg portal xdg-desktop-portal -r

if ! wait_for_process "xdg-desktop-portal" 25; then
  echo "[FAIL] xdg-desktop-portal process did not start" >&2
  exit 1
fi

if ! wait_for_process "xdg-desktop-portal-gtk" 25; then
  echo "[FAIL] xdg-desktop-portal-gtk process did not start" >&2
  exit 1
fi

echo "[INFO] Running Linux display smoke runtime checks in Wayland session"
(
  cd "$REPO_ROOT"
  ./scripts/linux-display-smoke.sh --skip-cargo
)

echo "[PASS] Wayland runtime CI harness completed"
