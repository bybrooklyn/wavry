#!/usr/bin/env bash
set -euo pipefail

failed=0

while IFS= read -r match; do
  file="${match%%:*}"
  rest="${match#*:}"
  line="${rest%%:*}"
  content="${match#*:*:}"

  # Extract the value after `uses:`.
  value="$(printf '%s' "$content" | sed -E 's/^[[:space:]]*uses:[[:space:]]*//; s/[[:space:]]+#.*$//')"
  if [[ -z "$value" ]]; then
    continue
  fi

  # Local actions are pinned by repository state and do not need external refs.
  if [[ "$value" == ./* ]]; then
    continue
  fi

  if [[ "$value" != *"@"* ]]; then
    echo "${file}:${line}: action '${value}' is missing an explicit ref after '@'." >&2
    failed=1
    continue
  fi

  ref="${value##*@}"
  if [[ -z "$ref" ]]; then
    echo "${file}:${line}: action '${value}' has an empty ref." >&2
    failed=1
    continue
  fi

  case "$ref" in
    main|master|latest|HEAD)
      echo "${file}:${line}: action '${value}' uses floating ref '${ref}'." >&2
      failed=1
      ;;
  esac
done < <(rg -n '^[[:space:]]*uses:[[:space:]]*[^[:space:]]+' .github/workflows/*.yml)

if (( failed != 0 )); then
  echo "Workflow action pinning check failed." >&2
  exit 1
fi

echo "Workflow action pinning check passed."
