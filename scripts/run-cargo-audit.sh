#!/usr/bin/env bash
set -euo pipefail

if [[ ! -f audit.toml ]]; then
  echo "audit.toml not found at repository root." >&2
  exit 1
fi

cmd=(cargo audit --deny warnings)
while IFS= read -r advisory; do
  [[ -z "$advisory" ]] && continue
  cmd+=(--ignore "$advisory")
done < <(grep -oE 'RUSTSEC-[0-9]{4}-[0-9]{4}' audit.toml | sort -u || true)

"${cmd[@]}"
