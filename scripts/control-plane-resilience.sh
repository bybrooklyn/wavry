#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
source "${SCRIPT_DIR}/lib/port-utils.sh"

require_cmd() {
  if ! command -v "$1" >/dev/null 2>&1; then
    echo "missing required command: $1" >&2
    exit 1
  fi
}

require_cmd cargo
require_cmd curl
require_cmd python3

TMP_DIR="$(mktemp -d)"
MASTER_LOG="${TMP_DIR}/master.log"
RELAY_LOG="${TMP_DIR}/relay.log"
PROXY_LOG="${TMP_DIR}/proxy.log"

MASTER_PORT="${WAVRY_MASTER_RESILIENCE_PORT:-$(find_free_tcp_port)}"
MASTER_URL="http://127.0.0.1:${MASTER_PORT}"

RELAY_HEALTH_PORT=""
MASTER_PID=""
RELAY_PID=""
PROXY_PID=""
PROXY_PORT=""

CHAOS_MASTER_OUTAGE_SECS="${WAVRY_MASTER_OUTAGE_SECS:-8}"
CHAOS_PROXY_DELAY_MS="${WAVRY_CHAOS_PROXY_DELAY_MS:-700}"

SOAK_RELAY_COUNT="${WAVRY_SOAK_RELAY_COUNT:-30}"
SOAK_SECONDS="${WAVRY_SOAK_SECONDS:-25}"
SOAK_MIN_SUCCESS_RATE="${WAVRY_SOAK_MIN_SUCCESS_RATE:-0.98}"
SOAK_MAX_REGISTER_P95_MS="${WAVRY_SOAK_MAX_REGISTER_P95_MS:-400}"
SOAK_MAX_HEARTBEAT_P95_MS="${WAVRY_SOAK_MAX_HEARTBEAT_P95_MS:-450}"

stop_process_var() {
  local var_name="$1"
  local pid="${!var_name:-}"
  if [[ -n "${pid}" ]]; then
    kill "${pid}" >/dev/null 2>&1 || true
    wait "${pid}" >/dev/null 2>&1 || true
    unset "${var_name}"
  fi
}

dump_logs() {
  echo "--- master log (tail) ---" >&2
  tail -n 200 "${MASTER_LOG}" >&2 || true
  echo "--- relay log (tail) ---" >&2
  tail -n 200 "${RELAY_LOG}" >&2 || true
  echo "--- proxy log (tail) ---" >&2
  tail -n 200 "${PROXY_LOG}" >&2 || true
}

cleanup() {
  local exit_code=$?
  stop_process_var RELAY_PID
  stop_process_var PROXY_PID
  stop_process_var MASTER_PID
  if [[ ${exit_code} -ne 0 ]]; then
    dump_logs
  fi
  rm -rf "${TMP_DIR}"
  exit ${exit_code}
}
trap cleanup EXIT

wait_for_http() {
  local url="$1"
  local timeout_secs="$2"
  local attempts=$((timeout_secs * 2))
  local i
  for ((i = 0; i < attempts; i++)); do
    if curl --silent --fail "${url}" >/dev/null 2>&1; then
      return 0
    fi
    sleep 0.5
  done
  return 1
}

relay_health_field() {
  local field="$1"
  curl --silent --fail "http://127.0.0.1:${RELAY_HEALTH_PORT}/health" | \
    python3 -c '
import json
import sys

obj = json.load(sys.stdin)
field = sys.argv[1]
value = obj.get(field)
if isinstance(value, bool):
    print("true" if value else "false")
elif value is None:
    print("")
else:
    print(value)
' "${field}"
}

wait_for_relay_registration() {
  local relay_id="$1"
  local max_last_seen_ms="$2"
  local timeout_secs="$3"
  local attempts=$((timeout_secs))
  local i
  for ((i = 0; i < attempts; i++)); do
    if curl --silent --fail "${MASTER_URL}/v1/relays" | \
      python3 -c '
import json
import sys

relay_id = sys.argv[1]
max_last_seen = int(sys.argv[2])
relays = json.load(sys.stdin)
for relay in relays:
    if relay.get("relay_id") == relay_id and int(relay.get("last_seen_ms_ago", 9999999)) <= max_last_seen:
        raise SystemExit(0)
raise SystemExit(1)
' "${relay_id}" "${max_last_seen_ms}"
    then
      return 0
    fi
    sleep 1
  done
  return 1
}

wait_for_relay_registered_flag() {
  local expected="$1"
  local timeout_secs="$2"
  local attempts=$((timeout_secs * 2))
  local i
  for ((i = 0; i < attempts; i++)); do
    local current
    current="$(relay_health_field registered_with_master || true)"
    if [[ "${current}" == "${expected}" ]]; then
      return 0
    fi
    sleep 0.5
  done
  return 1
}

start_master() {
  echo "[resilience] starting master on ${MASTER_URL}"
  (
    cd "${REPO_ROOT}"
    exec env \
      RUST_LOG="wavry_master=warn" \
      ./target/debug/wavry-master --listen "127.0.0.1:${MASTER_PORT}" >"${MASTER_LOG}" 2>&1
  ) &
  MASTER_PID=$!
  if ! wait_for_http "${MASTER_URL}/health" 45; then
    echo "master failed to become healthy" >&2
    return 1
  fi
}

start_relay() {
  local relay_master_url="$1"
  RELAY_HEALTH_PORT="$(find_free_tcp_port)"
  echo "[resilience] starting relay against ${relay_master_url}"
  (
    cd "${REPO_ROOT}"
    exec env \
      WAVRY_ALLOW_INSECURE_RELAY=1 \
      RUST_LOG="wavry_relay=warn" \
      ./target/debug/wavry-relay \
      --listen "127.0.0.1:0" \
      --master-url "${relay_master_url}" \
      --allow-insecure-dev \
      --health-listen "127.0.0.1:${RELAY_HEALTH_PORT}" >"${RELAY_LOG}" 2>&1
  ) &
  RELAY_PID=$!
  if ! wait_for_http "http://127.0.0.1:${RELAY_HEALTH_PORT}/health" 60; then
    echo "relay failed to become healthy" >&2
    return 1
  fi
}

start_proxy() {
  local delay_ms="$1"
  local drop_every="${2:-0}"
  PROXY_PORT="$(find_free_tcp_port)"
  echo "[resilience] starting chaos proxy on 127.0.0.1:${PROXY_PORT} (delay=${delay_ms}ms drop_every=${drop_every})"
  (
    cd "${REPO_ROOT}"
    exec python3 ./scripts/lib/tcp-chaos-proxy.py \
      --listen-host 127.0.0.1 \
      --listen-port "${PROXY_PORT}" \
      --target-host 127.0.0.1 \
      --target-port "${MASTER_PORT}" \
      --delay-ms "${delay_ms}" \
      --drop-every "${drop_every}" >"${PROXY_LOG}" 2>&1
  ) &
  PROXY_PID=$!
  sleep 1
}

echo "[resilience] building master + relay binaries"
(
  cd "${REPO_ROOT}"
  cargo build --quiet -p wavry-master -p wavry-relay
)

start_master
start_relay "${MASTER_URL}"

relay_id="$(relay_health_field relay_id)"
if [[ -z "${relay_id}" ]]; then
  echo "relay_id missing from health endpoint" >&2
  exit 1
fi

if ! wait_for_relay_registration "${relay_id}" 15000 40; then
  echo "initial relay registration was not observed in master" >&2
  exit 1
fi

echo "[resilience] phase: relay restart"
stop_process_var RELAY_PID
start_relay "${MASTER_URL}"
relay_id="$(relay_health_field relay_id)"
if ! wait_for_relay_registration "${relay_id}" 15000 40; then
  echo "relay did not re-register after relay restart" >&2
  exit 1
fi

echo "[resilience] phase: master restart"
stop_process_var MASTER_PID
start_master
if ! wait_for_relay_registration "${relay_id}" 20000 70; then
  echo "relay did not recover registration after master restart" >&2
  exit 1
fi

echo "[resilience] phase: packet-loss/outage simulation"
stop_process_var MASTER_PID
if ! wait_for_relay_registered_flag "false" 25; then
  echo "relay did not report unregistered during master outage" >&2
  exit 1
fi
sleep "${CHAOS_MASTER_OUTAGE_SECS}"
start_master
if ! wait_for_relay_registration "${relay_id}" 20000 80; then
  echo "relay did not recover registration after outage" >&2
  exit 1
fi

echo "[resilience] phase: high-latency simulation"
start_proxy "${CHAOS_PROXY_DELAY_MS}" 0
stop_process_var RELAY_PID
start_relay "http://127.0.0.1:${PROXY_PORT}"
relay_id="$(relay_health_field relay_id)"
if ! wait_for_relay_registration "${relay_id}" 20000 90; then
  echo "relay did not register through delayed proxy" >&2
  exit 1
fi
if ! wait_for_relay_registered_flag "true" 20; then
  echo "relay never reached registered_with_master=true during latency phase" >&2
  exit 1
fi

echo "[resilience] phase: load + soak SLO gate"
python3 "${SCRIPT_DIR}/lib/control-plane-load-driver.py" \
  --master-url "${MASTER_URL}" \
  --relay-count "${SOAK_RELAY_COUNT}" \
  --soak-seconds "${SOAK_SECONDS}" \
  --min-success-rate "${SOAK_MIN_SUCCESS_RATE}" \
  --max-register-p95-ms "${SOAK_MAX_REGISTER_P95_MS}" \
  --max-heartbeat-p95-ms "${SOAK_MAX_HEARTBEAT_P95_MS}"

echo "[resilience] control-plane resilience checks passed"
