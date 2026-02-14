# Adaptive Streaming Roadmap

**Version:** 0.0.5-unstable2  
**Last Updated:** 2026-02-13

This roadmap defines concrete milestones for adaptive resolution and bandwidth control.

## Goals

- Keep sessions stable under network volatility.
- Reduce visible quality oscillation during congestion.
- Recover quality quickly when network headroom returns.

## Milestones

### M1: Bitrate Adaptation Foundation (Target: v0.0.6)

Scope:
- Continuous throughput + RTT sampling from transport/control channel.
- Unified target bitrate controller with floor/ceiling clamps.
- Rate-change guardrails (max step-up/down per second).

Acceptance:
- Congestion tests show bitrate reduction within 500ms of sustained queue growth.
- Recovery tests show no overshoot-induced oscillation for 30s windows.

### M2: Resolution Ladder + Hysteresis (Target: v0.0.7)

Scope:
- Resolution ladder profiles (`720p`, `900p`, `1080p`, etc.).
- Hysteresis policy for up/down transitions to avoid flapping.
- Coupled FPS policy when bitrate floor is reached.

Acceptance:
- Under synthetic packet loss/latency, resolution transitions occur no more than 1 step per decision window.
- Sessions avoid repeated up/down toggles inside a 10s period.

### M3: Receiver-Aware Adaptation (Target: v0.0.8)

Scope:
- Decoder-side feedback integration (decode latency, frame drops).
- End-to-end adaptation loop that accounts for encode + network + decode budget.
- Content-aware hints (motion/complexity) for bitrate allocation.

Acceptance:
- Decode backlog events trigger adaptation within 1 second.
- End-to-end latency p95 remains within target budget for baseline test matrix.

### M4: Production Hardening (Target: v0.0.9)

Scope:
- Per-platform policy tuning (Linux/Windows/mobile clients).
- Operator controls for adaptation aggressiveness.
- Exported adaptation metrics and alert recommendations.

Acceptance:
- Soak runs complete with no adaptation deadlocks/regressions.
- Adaptation metrics available in runtime telemetry and docs.

## Risk Controls

- Feature-flag each milestone path for staged rollout.
- Keep static profile fallback to bypass adaptation quickly.
- Gate milestone promotion on reproducible test artifacts.

## Reporting

Track per-run adaptation report fields:
- average bitrate by phase
- resolution transition count
- convergence time after induced congestion
- dropped-frame rate and p95 end-to-end latency
