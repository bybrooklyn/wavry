#!/usr/bin/env bash

# Shared helpers for dynamic local port allocation in scripts.

require_python3() {
  if ! command -v python3 >/dev/null 2>&1; then
    echo "missing required command: python3" >&2
    exit 1
  fi
}

find_free_tcp_port() {
  require_python3
  python3 - <<'PY'
import socket

with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as sock:
    sock.bind(("127.0.0.1", 0))
    print(sock.getsockname()[1])
PY
}

find_free_udp_port() {
  require_python3
  python3 - <<'PY'
import socket

with socket.socket(socket.AF_INET, socket.SOCK_DGRAM) as sock:
    sock.bind(("127.0.0.1", 0))
    print(sock.getsockname()[1])
PY
}
