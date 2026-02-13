#!/usr/bin/env python3
"""Tiny TCP proxy for chaos testing (delay and deterministic drop)."""

from __future__ import annotations

import argparse
import asyncio
import contextlib


async def pipe(reader: asyncio.StreamReader, writer: asyncio.StreamWriter) -> None:
    try:
        while True:
            chunk = await reader.read(65536)
            if not chunk:
                break
            writer.write(chunk)
            await writer.drain()
    except Exception:
        pass
    finally:
        with contextlib.suppress(Exception):
            writer.close()
            await writer.wait_closed()


async def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--listen-host", default="127.0.0.1")
    parser.add_argument("--listen-port", type=int, required=True)
    parser.add_argument("--target-host", default="127.0.0.1")
    parser.add_argument("--target-port", type=int, required=True)
    parser.add_argument("--delay-ms", type=int, default=0)
    parser.add_argument("--drop-every", type=int, default=0)
    args = parser.parse_args()

    state = {"conn_count": 0}

    async def handle(client_reader: asyncio.StreamReader, client_writer: asyncio.StreamWriter) -> None:
        state["conn_count"] += 1
        conn_idx = state["conn_count"]

        if args.drop_every > 0 and conn_idx % args.drop_every == 0:
            client_writer.close()
            await client_writer.wait_closed()
            return

        if args.delay_ms > 0:
            await asyncio.sleep(args.delay_ms / 1000.0)

        try:
            upstream_reader, upstream_writer = await asyncio.open_connection(
                args.target_host, args.target_port
            )
        except Exception:
            client_writer.close()
            await client_writer.wait_closed()
            return

        await asyncio.gather(
            pipe(client_reader, upstream_writer),
            pipe(upstream_reader, client_writer),
            return_exceptions=True,
        )

    server = await asyncio.start_server(handle, args.listen_host, args.listen_port)
    async with server:
        await server.serve_forever()


if __name__ == "__main__":
    asyncio.run(main())
