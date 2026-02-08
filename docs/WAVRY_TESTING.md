# Wavry Testing Guide

**Version:** 1.0  
**Last Updated:** 2026-02-07

This document defines reproducible test setups for validating Wavry's performance and security across platforms.

---

## Table of Contents

1. [Hardware Requirements](#1-hardware-requirements)
2. [Software Prerequisites](#2-software-prerequisites)
3. [Test Runbooks](#3-test-runbooks)
4. [Performance Metrics](#4-performance-metrics)
5. [Failure Checks](#5-failure-checks)

---

## 1. Hardware Requirements

### Host Machine

| Component | Minimum | Recommended |
|:----------|:--------|:------------|
| **OS** | Linux (Wayland), Windows 10+, macOS 13+ | Linux (Wayland) |
| **CPU** | 4 cores | 6+ cores with QuickSync/NVENC |
| **GPU** | Intel HD 630 / GTX 1050 | RTX 3060 / M1 Pro+ |
| **RAM** | 8 GB | 16 GB |
| **Network** | Ethernet 1 Gbps | Ethernet 1 Gbps |

### Client Machine

| Component | Minimum | Recommended |
|:----------|:--------|:------------|
| **OS** | Linux, Windows 10+, macOS 13+ | Linux (Wayland) |
| **CPU** | 4 cores | 4+ cores |
| **GPU** | Hardware decode support | Dedicated GPU |
| **Network** | Wi-Fi 5 or Ethernet | Ethernet preferred |

### Network Setup

- **Wired Connection**: Heavily preferred for baseline metrics
- **Wi-Fi**: Use only for secondary stability tests
- **Latency Target**: < 2ms on LAN between host and client

---

## 2. Software Prerequisites

### Linux Host

```bash
# Ubuntu/Debian
sudo apt install pipewire xdg-desktop-portal gstreamer1.0-vaapi

# Verify PipeWire is running
systemctl --user status pipewire
```

### macOS Host

- Xcode build tools
- Screen Recording permission (System Preferences → Security & Privacy)
- Camera/microphone permissions if testing audio

### Windows Host

- Windows 10 1809+ or Windows 11
- Graphics Capture permission enabled
- Latest GPU drivers with encode/decode support

### Network

- mDNS (Avahi on Linux, Bonjour on macOS/Windows) enabled for local discovery
- Valid identity keys (generated on first run or via `wavry-cli`)

---

## 3. Test Runbooks

### 3.1 Encrypted Handshake Verification

Verify the Noise-based secure transport without a GUI:

**Prerequisites:**
```bash
cargo build -p wavry-server -p wavry-client
```

**Steps:**
```bash
# Terminal 1: Start Server
./target/debug/wavry-server --listen 127.0.0.1:5000

# Terminal 2: Start Client
./target/debug/wavry-client --connect 127.0.0.1:5000
```

**Validation:**
- [ ] Look for `crypto established` in logs
- [ ] Verify `Msg3 received` appears on both ends
- [ ] Confirm `Session established` log appears on both ends
- [ ] Check for successful encrypted ping/pong exchange

### 3.2 Linux Display Selection Smoke Test

**Automated Script:**
```bash
./scripts/linux-display-smoke.sh
```

**Manual Validation:**
```bash
# Launch Desktop with Logs
cd crates/wavry-desktop
RUST_LOG=info npm run tauri dev
```

**Test Cases:**

1. **Monitor Enumeration**
   - Navigate to **Sessions → Local Host**
   - Confirm monitor dropdown is populated with available displays
   - Expected: At least one monitor listed

2. **Monitor Switching**
   - Start host with monitor A
   - Stop host
   - Switch to monitor B
   - Start host again
   - Expected: Capture starts successfully on both monitors

3. **Hot-Plug Resilience**
   - Change connected monitors while app is open (disable/unplug one)
   - Refresh monitor list
   - Confirm selection auto-clamps to a valid monitor
   - Start host again
   - Expected: Capture still starts despite hardware changes

**Expected Logs:**
- Wayland capture path: `Selected Wayland display stream`
- Invalid monitor ID: Fallback warning and continuation
- No panics or unhandled errors

### 3.3 macOS Native UI Runbook

**Launch:**
```bash
./scripts/run-interactive.sh
```

**Setup Verification:**
1. Complete the Setup Wizard
   - Set Display Name
   - Choose Connectivity Mode (LAN or Global)

2. Connectivity Test
   - Use the **Sessions** tab to host or find a peer
   - Verify connection can be established

3. Settings Validation
   - Toggle **Automatic Hostname** in Account tab
   - Verify **Public Key** appears in settings
   - Check hostname is auto-detected correctly

**Performance Checks:**
- UI responds within 100ms to all interactions
- No dropped frames during normal operation
- Settings persist across restarts

### 3.4 DELTA Congestion Control Validation

**Setup:**
```bash
# Start server with verbose logging
RUST_LOG=debug ./target/debug/wavry-server --listen 0.0.0.0:5000

# Connect client
RUST_LOG=debug ./target/debug/wavry-client --connect <host-ip>:5000
```

**Test Scenarios:**

1. **Stable Network**
   - Monitor `DELTA state: STABLE` in logs
   - Verify bitrate increases gradually
   - Confirm FPS remains at target

2. **Congestion Simulation**
   ```bash
   # On host, introduce artificial delay
   tc qdisc add dev eth0 root netem delay 50ms 10ms
   ```
   - Watch for `DELTA state: RISING` then `CONGESTED`
   - Verify bitrate reduction occurs
   - Confirm FPS stepping if congestion persists >1s

3. **Recovery**
   ```bash
   # Remove congestion
   tc qdisc del dev eth0 root
   ```
   - Verify return to `STABLE` state
   - Confirm gradual bitrate increase

### 3.5 FEC Recovery Test

**Setup:** Same as DELTA test

**Test:**
```bash
# Introduce packet loss
sudo tc qdisc add dev eth0 root netem loss 1%
```

**Validation:**
- [ ] Monitor `FEC recovery successful` logs
- [ ] Video stream continues without significant artifacts
- [ ] NACK count increases appropriately
- [ ] Session does not drop

---

## 4. Performance Metrics

### 4.1 Latency Budget

| Stage | Target | Maximum |
|:------|:-------|:--------|
| Capture + Encode | ≤ 8 ms | 12 ms |
| Network (LAN) | ≤ 2 ms | 5 ms |
| Decode + Present | ≤ 5 ms | 8 ms |
| **Total** | **~15 ms** | **25 ms** |

### 4.2 Metrics to Capture

| Metric | Measurement Method | Target |
|:-------|:-------------------|:-------|
| **RTT** | RIFT Control channel (Ping/Pong) | < 5ms LAN, < 50ms WAN |
| **Jitter** | Depacketization buffer variance | < 2ms |
| **Encode Overhead** | Throughput with/without encryption | < 5% overhead |
| **Handshake Latency** | Noise XX completion time | < 50ms on LAN |
| **Input Responsiveness** | Click-to-photon latency | < 20ms |

### 4.3 Continuous Monitoring

Enable detailed telemetry:
```bash
RUST_LOG=wavry=trace,info ./target/debug/wavry-server
```

Key log patterns:
- `frame_id=X encode_time=Yμs` - Per-frame encode timing
- `rtt=Xms jitter=Yms` - Network statistics
- `delta_state=X bitrate=Y` - Congestion control decisions
- `fec_recovered=X lost=Y` - Error correction stats

---

## 5. Failure Checks

### 5.1 Handshake Failures

| Scenario | Expected Behavior |
|:---------|:------------------|
| Version mismatch | Connection rejected with clear error |
| Invalid signature | Immediate disconnect, no retry |
| Timeout (5s) | Connection attempt aborted |
| Replay attack | Sequence window rejection |

### 5.2 Network Degradation

| Scenario | Expected Behavior |
|:---------|:------------------|
| 1% packet loss | FEC recovery, minimal visual impact |
| 5% packet loss | Graceful quality degradation (bitrate/FPS reduction) |
| 20% packet loss | Session timeout with clear error message |
| RTT spike (+50ms) | Encoder skip mode triggered, buffer drain |

### 5.3 Security Failures

| Scenario | Expected Behavior |
|:---------|:------------------|
| Invalid lease signature | Immediate rejection, ban consideration |
| Expired lease | Session termination, renewal request |
| Replay (old sequence) | Packet dropped, counter incremented |
| Rate limit exceeded | Graceful backoff with retry-after |

### 5.4 Recovery Validation

All failure scenarios must:
- [ ] Fail loudly with clear error messages
- [ ] Not leave zombie sessions
- [ ] Allow clean restart without manual cleanup
- [ ] Log diagnostic information for debugging

---

## Related Documents

- [WAVRY_ARCHITECTURE.md](WAVRY_ARCHITECTURE.md) - System architecture overview
- [RIFT_SPEC_V1.md](RIFT_SPEC_V1.md) - Protocol specification
- [DELTA_CC_SPEC.md](DELTA_CC_SPEC.md) - Congestion control algorithm
- [WAVRY_SECURITY.md](WAVRY_SECURITY.md) - Security testing scenarios
