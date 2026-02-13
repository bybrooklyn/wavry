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

PORT="${WAVRY_GATEWAY_SMOKE_PORT:-$(find_free_tcp_port)}"
RELAY_PORT="${WAVRY_GATEWAY_SMOKE_RELAY_PORT:-$(find_free_udp_port)}"
BASE_URL="http://127.0.0.1:${PORT}"
TMP_DIR="$(mktemp -d)"
LOG_FILE="${TMP_DIR}/gateway.log"
DB_FILE="${TMP_DIR}/gateway.db"
touch "${DB_FILE}"

cleanup() {
  if [[ -n "${GATEWAY_PID:-}" ]]; then
    kill "${GATEWAY_PID}" >/dev/null 2>&1 || true
    wait "${GATEWAY_PID}" >/dev/null 2>&1 || true
  fi
  rm -rf "${TMP_DIR}"
}
trap cleanup EXIT

echo "[smoke] starting gateway on ${BASE_URL}"
(
  cd "${REPO_ROOT}"
  DATABASE_URL="sqlite://${DB_FILE}" \
  WAVRY_GATEWAY_BIND_ADDR="127.0.0.1:${PORT}" \
  WAVRY_GATEWAY_RELAY_PORT="${RELAY_PORT}" \
  RUST_LOG="wavry_gateway=warn" \
  cargo run --quiet --bin wavry-gateway >"${LOG_FILE}" 2>&1
) &
GATEWAY_PID=$!

for _ in $(seq 1 360); do
  if curl --silent --fail "${BASE_URL}/health" >/dev/null 2>&1; then
    break
  fi
  sleep 0.5
done

if ! curl --silent --fail "${BASE_URL}/health" >/dev/null 2>&1; then
  echo "[smoke] gateway failed to become healthy" >&2
  cat "${LOG_FILE}" >&2 || true
  exit 1
fi

SUFFIX="$(date +%s)"
EMAIL="smoke-${SUFFIX}@example.com"
USERNAME="smoke_${SUFFIX}"
PASSWORD="SuperSecurePass123!"
PUBLIC_KEY="00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff"

echo "[smoke] register"
REGISTER_BODY="$(
  curl --silent --show-error --fail \
    -H "content-type: application/json" \
    -d "{\"email\":\"${EMAIL}\",\"password\":\"${PASSWORD}\",\"display_name\":\"Smoke User\",\"username\":\"${USERNAME}\",\"public_key\":\"${PUBLIC_KEY}\"}" \
    "${BASE_URL}/auth/register"
)"
if [[ "${REGISTER_BODY}" != *"\"token\""* ]]; then
  echo "[smoke] register response missing token" >&2
  echo "${REGISTER_BODY}" >&2
  exit 1
fi

echo "[smoke] login"
LOGIN_BODY="$(
  curl --silent --show-error --fail \
    -H "content-type: application/json" \
    -d "{\"email\":\"${EMAIL}\",\"password\":\"${PASSWORD}\"}" \
    "${BASE_URL}/auth/login"
)"
TOKEN="$(printf '%s' "${LOGIN_BODY}" | sed -n 's/.*"token":"\([^"]*\)".*/\1/p')"
if [[ -z "${TOKEN}" ]]; then
  echo "[smoke] login response missing token" >&2
  echo "${LOGIN_BODY}" >&2
  exit 1
fi

echo "[smoke] logout"
curl --silent --show-error --fail \
  -X POST \
  -H "authorization: Bearer ${TOKEN}" \
  "${BASE_URL}/auth/logout" >/dev/null

echo "[smoke] auth metrics"
METRICS_BODY="$(curl --silent --show-error --fail "${BASE_URL}/metrics/auth")"
if [[ "${METRICS_BODY}" != *"\"register_success\":"* || "${METRICS_BODY}" != *"\"login_success\":"* ]]; then
  echo "[smoke] metrics endpoint missing expected fields" >&2
  echo "${METRICS_BODY}" >&2
  exit 1
fi

echo "[smoke] gateway auth flow passed"
