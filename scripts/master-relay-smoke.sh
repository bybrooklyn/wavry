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

MASTER_PORT="${WAVRY_MASTER_SMOKE_PORT:-$(find_free_tcp_port)}"
MASTER_URL="http://127.0.0.1:${MASTER_PORT}"
TMP_DIR="$(mktemp -d)"
MASTER_LOG="${TMP_DIR}/master.log"
RELAY_LOG="${TMP_DIR}/relay.log"

cleanup() {
  if [[ -n "${RELAY_PID:-}" ]]; then
    kill "${RELAY_PID}" >/dev/null 2>&1 || true
    wait "${RELAY_PID}" >/dev/null 2>&1 || true
  fi
  if [[ -n "${MASTER_PID:-}" ]]; then
    kill "${MASTER_PID}" >/dev/null 2>&1 || true
    wait "${MASTER_PID}" >/dev/null 2>&1 || true
  fi
  rm -rf "${TMP_DIR}"
}
trap cleanup EXIT

echo "[smoke] starting master on ${MASTER_URL}"
(
  cd "${REPO_ROOT}"
  RUST_LOG="wavry_master=warn" \
  cargo run --quiet --bin wavry-master -- --listen "127.0.0.1:${MASTER_PORT}" >"${MASTER_LOG}" 2>&1
) &
MASTER_PID=$!

for _ in $(seq 1 360); do
  if curl --silent --fail "${MASTER_URL}/health" >/dev/null 2>&1; then
    break
  fi
  sleep 0.5
done

if ! curl --silent --fail "${MASTER_URL}/health" >/dev/null 2>&1; then
  echo "[smoke] master failed to become healthy" >&2
  cat "${MASTER_LOG}" >&2 || true
  exit 1
fi

echo "[smoke] starting relay with random UDP port"
(
  cd "${REPO_ROOT}"
  WAVRY_ALLOW_INSECURE_RELAY=1 \
  RUST_LOG="wavry_relay=warn" \
  cargo run --quiet --bin wavry-relay -- \
    --listen "127.0.0.1:0" \
    --master-url "${MASTER_URL}" \
    --allow-insecure-dev >"${RELAY_LOG}" 2>&1
) &
RELAY_PID=$!

for _ in $(seq 1 30); do
  RELAYS_JSON="$(curl --silent --fail "${MASTER_URL}/v1/relays" || true)"
  if [[ "${RELAYS_JSON}" == *"relay_id"* ]]; then
    break
  fi
  sleep 1
done

RELAYS_JSON="$(curl --silent --fail "${MASTER_URL}/v1/relays" || true)"
if [[ "${RELAYS_JSON}" != *"relay_id"* || "${RELAYS_JSON}" != *"load_pct"* ]]; then
  echo "[smoke] relay did not register/heartbeat through master" >&2
  echo "--- master log ---" >&2
  cat "${MASTER_LOG}" >&2 || true
  echo "--- relay log ---" >&2
  cat "${RELAY_LOG}" >&2 || true
  echo "--- relays json ---" >&2
  echo "${RELAYS_JSON}" >&2
  exit 1
fi

echo "[smoke] master/relay flow passed"
