#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "${ROOT_DIR}"

PROBE_SECONDS="${WAVRY_AV1_PROBE_SECONDS:-6}"
REQUIRE_AV1="${WAVRY_REQUIRE_AV1:-0}"

echo "== Wavry AV1 Hardware Smoke =="
echo "Date: $(date -u +"%Y-%m-%dT%H:%M:%SZ")"

if [[ "${OSTYPE:-}" == darwin* ]]; then
  echo "OS: $(sw_vers -productName) $(sw_vers -productVersion)"
  echo "Build: $(sw_vers -buildVersion)"
  CHIP="$(sysctl -n machdep.cpu.brand_string 2>/dev/null || true)"
  if [[ -n "${CHIP}" ]]; then
    echo "CPU: ${CHIP}"
  fi
fi

echo
echo "-- Running macOS codec probe tests --"
cargo test -p wavry-media mac_probe -- --nocapture

echo
echo "-- Sampling wavry-server startup capabilities (${PROBE_SECONDS}s) --"
LOG_FILE="$(mktemp)"
RUST_LOG=info cargo run -p wavry-server -- --listen 127.0.0.1:0 --disable-mdns >"${LOG_FILE}" 2>&1 &
PID=$!
sleep "${PROBE_SECONDS}"
kill "${PID}" >/dev/null 2>&1 || true
wait "${PID}" >/dev/null 2>&1 || true

if ! grep -q "Local encoder candidates" "${LOG_FILE}"; then
  echo "ERROR: no local encoder candidates line found in probe logs"
  echo "Probe log: ${LOG_FILE}"
  exit 1
fi

CANDIDATES="$(grep "Local encoder candidates" "${LOG_FILE}" | tail -n 1 | sed -E 's/.*Local encoder candidates: //')"
echo "Local encoder candidates: ${CANDIDATES}"
echo "Probe log: ${LOG_FILE}"

if grep -q "Av1" <<<"${CANDIDATES}"; then
  echo "Result: AV1 hardware path appears available to wavry-server."
else
  echo "Result: AV1 not available in realtime encoder candidates on this host."
  if [[ "${REQUIRE_AV1}" == "1" ]]; then
    echo "ERROR: WAVRY_REQUIRE_AV1=1 set but AV1 candidate missing."
    exit 2
  fi
fi
