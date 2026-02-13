#!/usr/bin/env python3
"""Synthetic control-plane load + soak driver for wavry-master.

Exercises relay register + heartbeat APIs with explicit SLO thresholds.
"""

from __future__ import annotations

import argparse
import concurrent.futures
import json
import random
import statistics
import sys
import time
import urllib.error
import urllib.request
import uuid
from typing import Any


def percentile(values: list[float], pct: float) -> float:
    if not values:
        return 0.0
    ordered = sorted(values)
    idx = int((len(ordered) - 1) * pct)
    return ordered[idx]


def post_json(url: str, payload: dict[str, Any], timeout: float) -> tuple[int, float, str]:
    body = json.dumps(payload).encode("utf-8")
    req = urllib.request.Request(url, data=body, headers={"Content-Type": "application/json"})
    start = time.perf_counter()
    try:
        with urllib.request.urlopen(req, timeout=timeout) as resp:
            elapsed_ms = (time.perf_counter() - start) * 1000.0
            return resp.status, elapsed_ms, resp.read().decode("utf-8")
    except urllib.error.HTTPError as exc:
        elapsed_ms = (time.perf_counter() - start) * 1000.0
        return exc.code, elapsed_ms, exc.read().decode("utf-8", errors="replace")
    except Exception:
        elapsed_ms = (time.perf_counter() - start) * 1000.0
        return 0, elapsed_ms, ""


def get_json(url: str, timeout: float) -> Any:
    with urllib.request.urlopen(url, timeout=timeout) as resp:
        return json.loads(resp.read().decode("utf-8"))


def generate_endpoint(index: int) -> str:
    # Produce deterministic unique IPv4 endpoints without prefix collisions.
    # Master's sybil check currently compares endpoint IPs using string startswith(),
    # so avoid values like 10.0.0.1 and 10.0.0.10 in the same run.
    a = 10
    b = (index % 200) + 1
    c = ((index // 200) % 200) + 1
    d = 1
    port = 20000 + (index % 20000)
    return f"{a}.{b}.{c}.{d}:{port}"


def register_one(master_url: str, relay_id: str, endpoint: str, timeout: float) -> tuple[bool, float, int]:
    payload = {
        "relay_id": relay_id,
        "endpoints": [endpoint],
        "region": "loadtest-us",
        "asn": 64512,
        "max_sessions": 256,
        "max_bitrate_kbps": 20000,
        "features": ["ipv4"],
    }
    status, elapsed_ms, _ = post_json(
        f"{master_url}/v1/relays/register", payload, timeout=timeout
    )
    return (200 <= status < 300), elapsed_ms, status


def heartbeat_one(master_url: str, relay_id: str, timeout: float) -> tuple[bool, float, int]:
    payload = {
        "relay_id": relay_id,
        "load_pct": float(random.randint(5, 90)),
    }
    status, elapsed_ms, _ = post_json(
        f"{master_url}/v1/relays/heartbeat", payload, timeout=timeout
    )
    return (200 <= status < 300), elapsed_ms, status


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--master-url", required=True)
    parser.add_argument("--relay-count", type=int, default=30)
    parser.add_argument("--soak-seconds", type=int, default=25)
    parser.add_argument("--interval-seconds", type=float, default=1.0)
    parser.add_argument("--timeout-seconds", type=float, default=3.0)
    parser.add_argument("--workers", type=int, default=16)
    parser.add_argument("--min-success-rate", type=float, default=0.98)
    parser.add_argument("--max-register-p95-ms", type=float, default=400.0)
    parser.add_argument("--max-heartbeat-p95-ms", type=float, default=450.0)
    args = parser.parse_args()

    relay_count = max(1, args.relay_count)
    workers = max(1, args.workers)

    token = uuid.uuid4().hex[:8]
    relay_ids = [f"load-{token}-{idx:03d}" for idx in range(relay_count)]

    register_latencies: list[float] = []
    register_success = 0

    with concurrent.futures.ThreadPoolExecutor(max_workers=workers) as pool:
        futs = [
            pool.submit(
                register_one,
                args.master_url,
                relay_ids[idx],
                generate_endpoint(idx),
                args.timeout_seconds,
            )
            for idx in range(relay_count)
        ]
        for fut in concurrent.futures.as_completed(futs):
            ok, latency_ms, _ = fut.result()
            register_latencies.append(latency_ms)
            if ok:
                register_success += 1

    register_success_rate = register_success / relay_count
    register_p95_ms = percentile(register_latencies, 0.95)

    heartbeat_total = 0
    heartbeat_success = 0
    heartbeat_latencies: list[float] = []

    start = time.monotonic()
    while time.monotonic() - start < args.soak_seconds:
        wave_start = time.monotonic()
        with concurrent.futures.ThreadPoolExecutor(max_workers=workers) as pool:
            futs = [
                pool.submit(
                    heartbeat_one,
                    args.master_url,
                    relay_id,
                    args.timeout_seconds,
                )
                for relay_id in relay_ids
            ]
            for fut in concurrent.futures.as_completed(futs):
                ok, latency_ms, _ = fut.result()
                heartbeat_total += 1
                heartbeat_latencies.append(latency_ms)
                if ok:
                    heartbeat_success += 1

        elapsed = time.monotonic() - wave_start
        sleep_for = args.interval_seconds - elapsed
        if sleep_for > 0:
            time.sleep(sleep_for)

    heartbeat_success_rate = (heartbeat_success / heartbeat_total) if heartbeat_total else 0.0
    heartbeat_p95_ms = percentile(heartbeat_latencies, 0.95)

    registered_relays = get_json(f"{args.master_url}/v1/relays", timeout=args.timeout_seconds)
    registered_ids = {item.get("relay_id") for item in registered_relays if isinstance(item, dict)}
    missing = [relay_id for relay_id in relay_ids if relay_id not in registered_ids]

    summary = {
        "relay_count": relay_count,
        "register_success_rate": round(register_success_rate, 4),
        "register_p95_ms": round(register_p95_ms, 2),
        "heartbeat_total": heartbeat_total,
        "heartbeat_success_rate": round(heartbeat_success_rate, 4),
        "heartbeat_p95_ms": round(heartbeat_p95_ms, 2),
        "missing_registered_relays": len(missing),
        "mean_heartbeat_ms": round(statistics.mean(heartbeat_latencies), 2)
        if heartbeat_latencies
        else 0.0,
    }

    print(json.dumps(summary, indent=2, sort_keys=True))

    if register_success_rate < args.min_success_rate:
        print(
            f"register success rate {register_success_rate:.3f} below threshold {args.min_success_rate:.3f}",
            file=sys.stderr,
        )
        return 1
    if heartbeat_success_rate < args.min_success_rate:
        print(
            f"heartbeat success rate {heartbeat_success_rate:.3f} below threshold {args.min_success_rate:.3f}",
            file=sys.stderr,
        )
        return 1
    if register_p95_ms > args.max_register_p95_ms:
        print(
            f"register p95 {register_p95_ms:.2f}ms above threshold {args.max_register_p95_ms:.2f}ms",
            file=sys.stderr,
        )
        return 1
    if heartbeat_p95_ms > args.max_heartbeat_p95_ms:
        print(
            f"heartbeat p95 {heartbeat_p95_ms:.2f}ms above threshold {args.max_heartbeat_p95_ms:.2f}ms",
            file=sys.stderr,
        )
        return 1
    if missing:
        print(
            f"missing registered relays after soak: {len(missing)} (example={missing[0]})",
            file=sys.stderr,
        )
        return 1

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
