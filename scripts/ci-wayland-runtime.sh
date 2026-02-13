#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

if [[ "$(uname -s)" != "Linux" ]]; then
  echo "[FAIL] Wayland runtime CI harness only supports Linux." >&2
  exit 1
fi

resolve_cmd() {
  local cmd="$1"
  if command -v "$cmd" >/dev/null 2>&1; then
    command -v "$cmd"
    return 0
  fi

  local candidate
  for candidate in \
    "/usr/libexec/$cmd" \
    "/usr/lib/$cmd" \
    "/usr/lib/xdg-desktop-portal/$cmd"; do
    if [[ -x "$candidate" ]]; then
      echo "$candidate"
      return 0
    fi
  done

  return 1
}

require_cmd() {
  local cmd="$1"
  if ! resolve_cmd "$cmd" >/dev/null; then
    echo "[FAIL] Missing required command: $cmd" >&2
    exit 1
  fi
}

require_cmd dbus-daemon
require_cmd weston
require_cmd pipewire
require_cmd xdg-desktop-portal
require_cmd xdg-desktop-portal-gtk

DBUS_DAEMON_BIN="$(resolve_cmd dbus-daemon)"
WESTON_BIN="$(resolve_cmd weston)"
PIPEWIRE_BIN="$(resolve_cmd pipewire)"
XDG_PORTAL_BIN="$(resolve_cmd xdg-desktop-portal)"
XDG_PORTAL_GTK_BIN="$(resolve_cmd xdg-desktop-portal-gtk)"
export PATH="$(dirname "$XDG_PORTAL_BIN"):$(dirname "$XDG_PORTAL_GTK_BIN"):$PATH"

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
declare -A NAMED_PIDS=()

start_bg() {
  local name="$1"
  shift
  "$@" >"$LOG_DIR/${name}.log" 2>&1 &
  local pid=$!
  PIDS+=("$pid")
  NAMED_PIDS["$name"]="$pid"
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

wait_for_pid() {
  local pid="$1"
  local name="$2"
  local timeout_secs="${3:-20}"
  local i
  for ((i = 0; i < timeout_secs; i++)); do
    if kill -0 "$pid" >/dev/null 2>&1; then
      return 0
    fi
    sleep 1
  done
  echo "[FAIL] ${name} process is not running (pid=${pid})" >&2
  return 1
}

cleanup() {
  local code=$?
  set +e

  # Stop named processes in reverse startup order so portal-backed sockets
  # release cleanly before deleting the runtime directory.
  for name in portal portal_gtk weston wireplumber pipewire; do
    local pid="${NAMED_PIDS[$name]:-}"
    if [[ -n "$pid" ]] && kill -0 "$pid" >/dev/null 2>&1; then
      kill "$pid" >/dev/null 2>&1 || true
      wait "$pid" >/dev/null 2>&1 || true
    fi
  done

  for pid in "${PIDS[@]:-}"; do
    if kill -0 "$pid" >/dev/null 2>&1; then
      kill "$pid" >/dev/null 2>&1 || true
      wait "$pid" >/dev/null 2>&1 || true
    fi
  done

  if [[ -n "$DBUS_PID" ]] && kill -0 "$DBUS_PID" >/dev/null 2>&1; then
    kill "$DBUS_PID" >/dev/null 2>&1 || true
    wait "$DBUS_PID" >/dev/null 2>&1 || true
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

  # xdg-desktop-portal may keep the document portal mount busy briefly.
  if [[ -n "${XDG_RUNTIME_DIR:-}" && -d "$XDG_RUNTIME_DIR/doc" ]]; then
    if command -v mountpoint >/dev/null 2>&1 && mountpoint -q "$XDG_RUNTIME_DIR/doc"; then
      if command -v fusermount >/dev/null 2>&1; then
        fusermount -u "$XDG_RUNTIME_DIR/doc" >/dev/null 2>&1 || true
      fi
      umount -l "$XDG_RUNTIME_DIR/doc" >/dev/null 2>&1 || true
    fi
    rm -rf "$XDG_RUNTIME_DIR/doc" >/dev/null 2>&1 || true
  fi

  if ! rm -rf "$TMP_DIR" >/dev/null 2>&1; then
    echo "[WARN] Failed to fully remove temporary runtime dir: $TMP_DIR" >&2
  fi
  exit "$code"
}

trap cleanup EXIT

# Start session D-Bus daemon.
DBUS_OUTPUT="$("$DBUS_DAEMON_BIN" --session --fork --print-address 1 --print-pid 1)"
export DBUS_SESSION_BUS_ADDRESS="$(echo "$DBUS_OUTPUT" | sed -n '1p')"
DBUS_PID="$(echo "$DBUS_OUTPUT" | sed -n '2p')"
echo "[INFO] D-Bus session started (pid=$DBUS_PID)"

start_bg pipewire "$PIPEWIRE_BIN"
if command -v wireplumber >/dev/null 2>&1; then
  start_bg wireplumber wireplumber
fi

start_bg weston "$WESTON_BIN" --backend=headless-backend.so --socket="$WAYLAND_DISPLAY" --idle-time=0 --xwayland

if ! wait_for_socket "$XDG_RUNTIME_DIR/$WAYLAND_DISPLAY" 25; then
  echo "[FAIL] Wayland socket did not appear: $XDG_RUNTIME_DIR/$WAYLAND_DISPLAY" >&2
  exit 1
fi

start_bg portal_gtk "$XDG_PORTAL_GTK_BIN" -r
start_bg portal "$XDG_PORTAL_BIN" -r

PORTAL_PID="${NAMED_PIDS[portal]:-}"
PORTAL_GTK_PID="${NAMED_PIDS[portal_gtk]:-}"

if [[ -z "$PORTAL_PID" || -z "$PORTAL_GTK_PID" ]]; then
  echo "[FAIL] failed to track portal process IDs" >&2
  exit 1
fi

wait_for_pid "$PORTAL_PID" "xdg-desktop-portal" 25
wait_for_pid "$PORTAL_GTK_PID" "xdg-desktop-portal-gtk" 25

echo "[INFO] Running Linux display smoke runtime checks in Wayland session"
(
  cd "$REPO_ROOT"
  ./scripts/linux-display-smoke.sh --skip-cargo
)

echo "[INFO] Running Wayland capture smoke test"
(
  cd "$REPO_ROOT"
  export WAVRY_CI_WAYLAND_CAPTURE_TEST=1
  cargo test -p wavry-media --lib linux::tests::test_wayland_capture_smoke -- --nocapture
)

echo "[PASS] Wayland runtime CI harness completed"
