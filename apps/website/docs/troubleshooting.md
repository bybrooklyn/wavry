---
title: Troubleshooting
description: Practical troubleshooting playbook for connection, performance, and deployment issues.
---

Use this page as a first-pass runbook when sessions fail or degrade.

## 1. Cannot Establish a Session

### Symptoms

- Client never reaches connected state
- Handshake errors in logs
- Immediate disconnect after signaling

### Checks

1. Confirm `wavry-gateway` and `wavry-relay` are running when required.
2. Confirm host process is active and reachable.
3. Confirm target address/port is correct for direct connect.
4. Confirm firewall/security group policy allows required UDP path.
5. Confirm client and host builds are compatible.

## 2. Session Connects but Feels Laggy

### Symptoms

- Input delay spikes
- Jitter/stutter during interaction
- Throughput swings with visible quality collapse

### Checks

1. Verify whether session is on direct path or relay fallback.
2. Check RTT/loss/jitter logs around issue windows.
3. Inspect host CPU/GPU usage for saturation.
4. Reduce initial bitrate/resolution temporarily for isolation.
5. Validate no competing heavy background workloads.

## 3. Audio Problems

### Symptoms

- No audio output
- Wrong capture source
- Audio route mismatch on macOS

### Checks

1. Verify selected audio source configuration.
2. On macOS, validate source mode (`system`, `microphone`, `app:<name>`).
3. Confirm OS permissions for capture/input devices.
4. Confirm sample-rate/channel compatibility in logs.

## 4. Desktop App Issues

### Symptoms

- Desktop app fails in dev mode
- UI connects but runtime does not start
- Settings appear ignored

### Checks

1. Run `bun run check` in `crates/wavry-desktop`.
2. Confirm Tauri dependencies and platform prerequisites.
3. Validate that session start/stop commands are reflected in backend logs.
4. Verify monitor/audio/input permissions in OS settings.

## 5. Relay Usage Is Unexpectedly High

### Symptoms

- Most sessions relay even in expected direct environments

### Checks

1. Re-validate NAT/firewall behavior for UDP.
2. Check if clients are behind restrictive enterprise networking.
3. Confirm gateway-provided connection candidates are valid.
4. Review recent infrastructure/network policy changes.

## 6. Build/Release Pipeline Failures

### Symptoms

- CI takes too long or flakes
- Packaging succeeds on one platform but not another

### Checks

1. Verify toolchain cache keys and lockfile changes.
2. Validate platform-specific build dependencies.
3. Build from clean environment to reproduce deterministically.
4. Ensure artifact signing/notarization steps are configured correctly.

## Debugging Workflow Recommendation

1. Reproduce with minimal topology.
2. Capture precise timestamps and relevant logs.
3. Separate control-plane failures from media-plane failures.
4. Verify security configuration did not unintentionally block transport.
5. Apply one change at a time and retest.

## Related Docs

- [Getting Started](/getting-started)
- [Session Lifecycle](/lifecycle)
- [Networking and Relay](/networking-and-relay)
- [Operations](/operations)
