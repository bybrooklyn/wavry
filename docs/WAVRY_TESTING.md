# Wavry — Step Two Testing

This document defines a reproducible test setup for Step Two.

---

## Hardware

- Linux host (Wayland) with PipeWire and xdg-desktop-portal.
- Linux client on the same LAN.
- Wired Ethernet preferred; Wi-Fi for secondary tests.

---

## Software Prerequisites

- PipeWire + xdg-desktop-portal running.
- GStreamer plugins for VA-API (or NVENC if available).
- uinput access on host.
- evdev access on client.
- Avahi/mDNS working on LAN.
- Root or udev rules may be required for uinput/evdev.

---

## Runbook

1. Start host:
   - `cargo run -p wavry-server`
2. Start client:
   - `cargo run -p wavry-client`
3. Confirm discovery:
   - Client should auto-connect via mDNS.
4. Confirm video:
   - Frame display should start immediately after HELLO_ACK.
5. Confirm input:
   - Mouse/keyboard events should affect the host.

---

## Metrics to Capture

- RTT from `RIFT_PING`/`RIFT_PONG`.
- Packet loss from `RIFT_STATS`.
- Frame pacing stability (visual + logs).
- Motion-to-photon latency using a high-speed camera (target < 25 ms).

---

## Failure Checks

- No freezes under packet loss (video should degrade).
- Encoder must not stall capture (drop frames instead of queueing).
- Input must remain responsive even with video loss.
