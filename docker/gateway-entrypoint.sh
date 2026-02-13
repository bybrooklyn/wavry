#!/usr/bin/env sh
set -eu

db_url="${DATABASE_URL:-sqlite:gateway.db}"

case "$db_url" in
  sqlite:///*)
    db_path="${db_url#sqlite://}"
    ;;
  sqlite:*)
    db_path="${db_url#sqlite:}"
    ;;
  *)
    db_path=""
    ;;
esac

if [ -n "$db_path" ]; then
  db_dir="$(dirname "$db_path")"
  mkdir -p "$db_dir"
  touch "$db_path"
fi

exec /usr/local/bin/wavry-gateway "$@"
