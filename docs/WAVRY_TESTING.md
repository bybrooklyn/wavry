# Wavry — Testing

This document defines reproducible test setups for validating Wavry's performance and security across platforms.

---

## Hardware Support

- **Linux Host**: Wayland with PipeWire and xdg-desktop-portal.
- **macOS Host**: Apple Silicon preferred, ScreenCaptureKit access required.
- **Client**: Any supported platform on the same LAN or reachable via relay.
- **Wired Connection**: Heavily preferred for baseline metrics; Wi-Fi for secondary stability tests.

---

## Software Prerequisites

- **macOS**: Xcode build tools, permission for Screen Recording.
- **Linux**: PipeWire + GStreamer plugins (VA-API/NVENC).
- **Network**: mDNS (Avahi/Bonjour) enabled for local discovery.
- **Credentials**: Valid identity keys (generated on first run or via `wavry-cli`).

---

## Runbook: macOS Native UI

1. **Launch App**:
   - `./scripts/run-interactive.sh`
2. **Setup**:
   - Complete the Setup Wizard (Display Name + Connectivity Mode).
3. **Connectivity**:
   - Use the **Sessions** tab to host or find a peer.
4. **Settings Verification**:
   - Toggle **Automatic Hostname** in Account tab.
   - Verify **Public Key** appears in settings.

---

## Runbook: Linux Display Selection Smoke Test

1. **Run Preflight**:
   - `./scripts/linux-display-smoke.sh`
2. **Launch Desktop with Logs**:
   - `cd crates/wavry-desktop`
   - `RUST_LOG=info npm run tauri dev`
3. **Validate Monitor Selection**:
   - In **Sessions -> Local Host**, confirm monitor dropdown is populated.
   - Start host with monitor A, stop, switch to monitor B, start again.
4. **Validate Resilience**:
   - Change connected monitors while app is open (disable/unplug one).
   - Refresh monitor list and confirm selection auto-clamps to a valid monitor.
   - Start host again and confirm capture still starts.
5. **Expected Logs**:
   - Wayland capture path should log `Selected Wayland display stream`.
   - Invalid/stale monitor ID should log a fallback warning and continue.

---

## Runbook: Encrypted Handshake Verification

To verify the Noise-based secure transport without a GUI:

1. **Build Components**:
   - `cargo build -p wavry-server -p wavry-client`
2. **Start Server**:
   - `./target/debug/wavry-server --listen 127.0.0.1:5000`
3. **Start Client**:
   - `./target/debug/wavry-client --connect 127.0.0.1:5000`
4. **Check Logs**:
   - Look for `crypto established` and `Msg3 received`.
   - Verify `Session established` log appears on both ends.

---

## Metrics to Capture

- **RTT**: Measured via RIFT Control channel (Ping/Pong).
- **Jitter**: Tracked in depacketization buffers.
- **ENC Overhead**: Measure throughput with and without encryption.
- **Handshake Latency**: Target < 50ms for secure establishment on LAN.
- **Input Responsiveness**: CGEvent injection latency on macOS.

---

## Failure Checks

- **Handshake Timeout**: Encryption must fail loudly if handshake stalls.
- **Version Mismatch**: RIFT version must match or connection is rejected.
- **Identity Spree**: Multiple connection attempts with invalid keys must be blocked.
- **Degradation**: Video should drop frames under loss, but input must never lag.
