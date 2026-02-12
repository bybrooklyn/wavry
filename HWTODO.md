# Hardware Testing TODO - M4 MacBook Air ↔ Arch PC (RTX 3070 Ti)

**Tester**: You (Local Hardware)
**Target Codecs**: H.264, HEVC, AV1
**Setup**: M4 MacBook Air (VideoToolbox) ↔ Arch PC (NVIDIA 3070 Ti, NVENC)

---

## 🎯 Testing Matrix

### macOS (M4 MacBook Air)

| Codec | Encoder | Res | FPS | Target Bitrate | Hardware | Status |
|-------|---------|-----|-----|----------------|----------|--------|
| H.264 | VideoToolbox | 1080p | 30 | 2.5 Mbps | M4 Apple Silicon | [ ] Test |
| H.264 | VideoToolbox | 1080p | 60 | 5.0 Mbps | M4 Apple Silicon | [ ] Test |
| HEVC | VideoToolbox | 1080p | 30 | 1.8 Mbps | M4 Apple Silicon | [ ] Test |
| HEVC | VideoToolbox | 1080p | 60 | 3.5 Mbps | M4 Apple Silicon | [ ] Test |
| AV1 | VideoToolbox | 1080p | 30 | 1.2 Mbps | M4 Apple Silicon | [ ] Test |
| AV1 | VideoToolbox | 1080p | 60 | 2.5 Mbps | M4 Apple Silicon | [ ] Test |

### Linux (Arch PC - RTX 3070 Ti)

| Codec | Encoder | Res | FPS | Target Bitrate | Hardware | Status |
|-------|---------|-----|-----|----------------|----------|--------|
| H.264 | NVENC | 1080p | 30 | 2.5 Mbps | RTX 3070 Ti | [ ] Test |
| H.264 | NVENC | 1080p | 60 | 5.0 Mbps | RTX 3070 Ti | [ ] Test |
| HEVC | NVENC | 1080p | 30 | 1.8 Mbps | RTX 3070 Ti | [ ] Test |
| HEVC | NVENC | 1080p | 60 | 3.5 Mbps | RTX 3070 Ti | [ ] Test |
| AV1 | NVENC | 1080p | 30 | 1.2 Mbps | RTX 3070 Ti | [ ] Test |
| AV1 | NVENC | 1080p | 60 | 2.5 Mbps | RTX 3070 Ti | [ ] Test |

---

## 📋 Testing Procedure

### Setup Phase (One-time)

1. **Build wavry for both platforms**
   ```bash
   # On M4 MacBook
   cargo build --release --workspace

   # On Arch PC
   cargo build --release --workspace
   ```

2. **Generate test certificates** (for WebTransport)
   ```bash
   ./scripts/gen-wt-cert.sh
   ```

3. **Verify hardware acceleration detection**
   ```bash
   # Check what codecs each platform detects
   cargo run --bin wavry-server --release 2>&1 | grep -i "codec\|videoToolbox\|NVENC\|hardware"
   ```

### Local Progress Snapshot (2026-02-12, Apple M4)

- [x] Ran `./scripts/av1-hardware-smoke.sh`
- [x] Ran macOS probe tests (`cargo test -p wavry-media mac_probe -- --nocapture`)
- [x] Captured host startup capability output (`Local encoder candidates: [Hevc, H264]`)
- [ ] Full AV1 quality/latency benchmarking (requires end-to-end stream session workload)

---

## 🧪 Test Case 1: H.264 Baseline (30fps, 1080p)

### macOS (M4) - Server
```bash
RUST_LOG=info cargo run --bin wavry-server --release -- \
  --width 1920 \
  --height 1080 \
  --fps 30 \
  --codec h264 \
  --bitrate 2500
```

**Monitor Output:**
- [ ] Encoder initialized (VideoToolbox)
- [ ] Capture FPS: ~30
- [ ] Bitrate maintaining ~2.5 Mbps
- [ ] CPU usage: <30%
- [ ] GPU memory: <200MB

### Linux (Arch) - Client
```bash
RUST_LOG=info WAVRY_GATEWAY_URL=ws://192.168.x.x:3000/ws \
cargo run --bin wavry-client --release -- \
  --connect 192.168.x.x:5000
```

**Monitor Output:**
- [ ] Decoder initialized (NVIDIA hardware)
- [ ] Receiving ~30fps
- [ ] RTT: <10ms (local network)
- [ ] Jitter: <5ms
- [ ] Frame drops: 0
- [ ] CPU: <20%

---

## 🧪 Test Case 2: HEVC 60fps (1080p)

### macOS (M4) - Server
```bash
RUST_LOG=info cargo run --bin wavry-server --release -- \
  --width 1920 \
  --height 1080 \
  --fps 60 \
  --codec hevc \
  --bitrate 3500
```

**Monitor Output:**
- [ ] Encoder: HEVC/H.265 (VideoToolbox)
- [ ] Encode latency: <16ms (for 60fps)
- [ ] Bitrate: ±10% variance from 3.5 Mbps
- [ ] GPU memory: <250MB
- [ ] Thermal: Check if throttling

### Linux (Arch) - Client
```bash
RUST_LOG=info WAVRY_GATEWAY_URL=ws://192.168.x.x:3000/ws \
cargo run --bin wavry-client --release -- \
  --connect 192.168.x.x:5000
```

**Monitor Output:**
- [ ] Decoding 60 fps smoothly
- [ ] Frame timing stability: ±2ms variance
- [ ] No visible artifacts
- [ ] GPU utilization: 40-60%

---

## 🎬 Test Case 3: AV1 Codec Validation

### macOS (M4) - Server
```bash
RUST_LOG=info cargo run --bin wavry-server --release -- \
  --width 1920 \
  --height 1080 \
  --fps 30 \
  --codec av1 \
  --bitrate 1200
```

**Verify:**
- [ ] AV1 hardware acceleration detected
- [ ] Encoding latency: <33ms per frame (30fps)
- [ ] Bitrate efficiency: 1.2 Mbps for comparable quality to H.264@2.5Mbps
- [ ] Quality: Compare subjective quality vs H.264
- [ ] Supported: Yes/No on M4

### Linux (Arch) - Client
```bash
RUST_LOG=info WAVRY_GATEWAY_URL=ws://192.168.x.x:3000/ws \
cargo run --bin wavry-client --release -- \
  --connect 192.168.x.x:5000
```

**Verify:**
- [ ] AV1 decoding works
- [ ] Frame drops vs H.264: Compare
- [ ] Visual quality: Assess artifacts
- [ ] Supported: Yes/No on RTX 3070 Ti

---

## 📊 Performance Metrics to Capture

For each codec/FPS combination, record:

```
Codec: [H.264/HEVC/AV1]
FPS: [30/60/120]
Bitrate Target: [Mbps]
Resolution: [1920x1080]

=== ENCODER (macOS M4) ===
Actual Bitrate: _____ Mbps (variance: ±_____%)
Encode Latency: _____ ms
GPU Memory: _____ MB
CPU Load: _____ %
Thermal: OK / Throttling

=== NETWORK ===
RTT: _____ ms
Jitter: _____ ms
Loss: _____ %

=== DECODER (Arch/3070Ti) ===
Decode Latency: _____ ms
GPU Memory: _____ MB
CPU Load: _____ %
Frame Drops: _____
Visual Quality: Excellent / Good / Acceptable / Poor
Artifacts: None / [describe]
```

---

## 🔍 Quality Assessment (Subjective)

For each codec, evaluate:

| Metric | H.264 | HEVC | AV1 | Notes |
|--------|-------|------|-----|-------|
| Detail preservation | [ ] | [ ] | [ ] | Text sharpness, edges |
| Motion smoothness | [ ] | [ ] | [ ] | Pan/scroll artifacts |
| Color accuracy | [ ] | [ ] | [ ] | Gradients, skin tones |
| Compression artifacts | [ ] | [ ] | [ ] | Banding, blockiness |

---

## 🚀 Testing Sequence

### Day 1: Baseline (H.264)
1. [ ] M4 Server: H.264 30fps
2. [ ] Arch Client: Receives & decodes
3. [ ] Document baseline metrics
4. [ ] Record: Bitrate, latency, quality

### Day 2: High FPS (H.264 60fps + HEVC)
1. [ ] M4 Server: H.264 60fps
2. [ ] Arch Client: Receives 60fps
3. [ ] M4 Server: HEVC 30fps
4. [ ] Arch Client: HEVC decode

### Day 3: AV1 & Reverse Test
1. [ ] M4 Server: AV1 30fps
2. [ ] Arch Client: AV1 decode
3. [ ] **Reverse: Arch PC as Server, M4 as Client**
   - Run `wavry-server` on Arch (NVIDIA NVENC)
   - Run `wavry-client` on M4 (VideoToolbox decode)
4. [ ] Test H.264/HEVC/AV1 from Arch → M4

---

## 📝 Results Template

```markdown
## Test Run: [DATE] [CODEC] [FPS]fps [RESOLUTION]

### Server (macOS M4)
- Hardware Acceleration: [Yes/No]
- Encoder: [VideoToolbox]
- Target Bitrate: 2500 kbps
- Actual Bitrate: _______ kbps
- Bitrate Variance: ±_____%
- Encode Latency (min/avg/max): ___ / ___ / ___ ms
- GPU Memory: _______ MB
- CPU: _______ %
- Thermal: [OK/Throttling]

### Network
- RTT: _______ ms
- Jitter: _______ ms
- Loss: _______ %
- Bitrate Stability: ±_____%

### Client (Arch RTX 3070 Ti)
- Hardware Acceleration: [Yes/No]
- Decoder: [NVIDIA/CPU]
- Decode Latency (min/avg/max): ___ / ___ / ___ ms
- GPU Memory: _______ MB
- CPU: _______ %
- Frame Drops: _____
- Quality: [Excellent/Good/Acceptable/Poor]
- Issues: [None/describe]

### Analysis
- Hardware acceleration working: [Yes/No]
- Bitrate efficiency vs H.264: [Better/Same/Worse]
- Recommended bitrate: _______ kbps
- Ready for production: [Yes/No/Needs tuning]
```

---

## 🐛 Troubleshooting

### M4 VideoToolbox Issues
```bash
# Check VideoToolbox availability
cargo run --bin wavry-server --release -- --list-codecs

# If AV1 not available:
# - Check macOS version (15.1+)
# - May require VideoToolbox API update

# GPU memory issues:
# - Check Activity Monitor for leaks
# - Reduce resolution/bitrate
```

### Arch NVIDIA Issues
```bash
# Check NVIDIA driver
nvidia-smi

# Check NVENC availability
nvidia-smi -q | grep -i "nvenc\|video"

# If AV1 not available:
# - Driver version too old (need 525+)
# - GPU generation doesn't support AV1 (3070 Ti should support)
```

---

## 📤 Reporting Results

Once testing is complete:

1. **Update docs/AV1_VALIDATION.md** with results
2. **Create HWTEST_RESULTS.md** with full metrics
3. **Update TODO.md**: Mark "AV1 Performance Validation" complete
4. **Note any codec recommendations** in codebase

---

## 🎯 Success Criteria

- [x] H.264: Works 30fps & 60fps (baseline)
- [ ] HEVC: Encoding confirmed, client decodes
- [ ] AV1: Detects codec, encode/decode works
- [ ] Bitrate variance: ±10% max
- [ ] Frame drops: Zero on stable network
- [ ] Latency: <50ms total (encode + network + decode)
- [ ] Reverse test: Arch → M4 also works
