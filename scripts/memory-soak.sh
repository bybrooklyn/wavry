#!/usr/bin/env bash
set -euo pipefail

# Wavry Memory Soak and Leak Detector
# 
# Runs a long-duration session and samples memory to detect leaks.

DURATION_SECS="${1:-60}" # Default to 60s for smoke test
INTERVAL_SECS=5
THRESHOLD_MB_PER_HOUR=50

info() { echo -e "\033[0;34m[info]\033[0m $*"; }
success() { echo -e "\033[0;32m[ok]\033[0m $*"; }
fail() { echo -e "\033[0;31m[error]\033[0m $*" >&2; exit 1; }

info "Starting memory soak test for $DURATION_SECS seconds (interval: ${INTERVAL_SECS}s)..."

# Start master and relay in background
# (Simplified for this script, assumes they are already built)
./target/debug/wavry-master --listen 127.0.0.1:8080 > /dev/null 2>&1 &
MASTER_PID=$!
./target/debug/wavry-relay --listen 127.0.0.1:0 --master-url http://127.0.0.1:8080 > /dev/null 2>&1 &
RELAY_PID=$!

cleanup() {
  info "Cleaning up..."
  kill $MASTER_PID $RELAY_PID 2>/dev/null || true
}
trap cleanup EXIT

# Sampling loop
START_TIME=$(date +%s)
END_TIME=$((START_TIME + DURATION_SECS))

SAMPLES_FILE="memory_samples.csv"
echo "timestamp,master_rss_kb,relay_rss_kb" > "$SAMPLES_FILE"

while [[ $(date +%s) -lt $END_TIME ]]; do
  TIMESTAMP=$(date +%s)
  
  MASTER_RSS=$(ps -o rss= -p $MASTER_PID || echo 0)
  RELAY_RSS=$(ps -o rss= -p $RELAY_PID || echo 0)
  
  echo "$TIMESTAMP,$MASTER_RSS,$RELAY_RSS" >> "$SAMPLES_FILE"
  
  sleep $INTERVAL_SECS
done

info "Soak test completed. Analyzing results..."

# Analysis logic (simplified)
# Calculate slope using first and last samples
FIRST_MASTER=$(head -n 2 "$SAMPLES_FILE" | tail -n 1 | cut -d',' -f2)
LAST_MASTER=$(tail -n 1 "$SAMPLES_FILE" | cut -d',' -f2)
DIFF_MASTER=$((LAST_MASTER - FIRST_MASTER))
LEAK_RATE_MB_H=$(echo "scale=2; $DIFF_MASTER / 1024 / ($DURATION_SECS / 3600)" | bc -l || echo 0)

info "Master leak rate: ${LEAK_RATE_MB_H} MB/hour"

if (( $(echo "$LEAK_RATE_MB_H > $THRESHOLD_MB_PER_HOUR" | bc -l) )); then
  fail "Memory leak detected in master: ${LEAK_RATE_MB_H} MB/hour exceeds threshold ${THRESHOLD_MB_PER_HOUR} MB/hour"
fi

success "Memory stability check passed."
