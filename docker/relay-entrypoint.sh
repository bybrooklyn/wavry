#!/usr/bin/env sh
set -eu

if [ "${WAVRY_RELAY_ALLOW_INSECURE_DEV:-0}" = "1" ]; then
  export WAVRY_ALLOW_INSECURE_RELAY=1
  exec /usr/local/bin/wavry-relay --allow-insecure-dev "$@"
fi

if [ -z "${WAVRY_RELAY_MASTER_PUBLIC_KEY:-}" ]; then
  cat >&2 <<'ERR'
Missing WAVRY_RELAY_MASTER_PUBLIC_KEY.
Set WAVRY_RELAY_MASTER_PUBLIC_KEY (hex Ed25519 public key),
or set WAVRY_RELAY_ALLOW_INSECURE_DEV=1 for local development only.
ERR
  exit 64
fi

exec /usr/local/bin/wavry-relay --master-public-key "$WAVRY_RELAY_MASTER_PUBLIC_KEY" "$@"
