# Wavry Project TODOs

**Current Version**: v0.0.2-canary
**Last Updated**: 2026-02-09
**Build Status**: ✅ Clean (0 warnings, 132 tests passing)

---

## 🎯 COMPLETED WORK (Current Sprint)

### ✅ Code Quality & Infrastructure
- [x] **Resolve Clippy Warnings** - Zero compiler warnings across all crates
- [x] **Fix GitHub Actions** - Matrix variable interpolation in workflow job names
- [x] **Update Version** - v0.0.1-unstable → v0.0.2-canary across entire workspace

### ✅ Code Coverage (132 tests total)
- [x] **rift-core** - 42 tests (FEC, physical packets, DELTA congestion control)
- [x] **rift-crypto** - 21 tests (Noise handshake, session encryption, identity)
- [x] **wavry-client** - 26 tests (helpers, types, message encoding/decoding)
- [x] **wavry-common** - 4 tests (constant-time equality for security)
- [x] **wavry-gateway** - 14 tests (token hashing, auth, request validation, bans)
- [x] **wavry-media** - 25 tests (buffer pools, encoder pools)

### ✅ Performance Optimization Phase 1 (Task #5-8)

**Task #6: Network Optimization** - QUIC CC Tuning
- [x] LatencyProfile - Network classification (Local/Regional/Intercontinental/Satellite)
- [x] LinkType - Baseline RTT calibration with aggressiveness adjustment
- [x] CongestionDetector - Hybrid delay-slope + loss-rate detection
- [x] AdjustmentStrategy - Conservative/Moderate/Aggressive bitrate control
- [x] FecController - Dynamic redundancy (5-50%) based on loss/stability
- [x] 25 comprehensive tests for all scenarios

**Task #7: Memory Optimization** - Buffer Pool Management
- [x] FrameBufferPool - Zero-allocation frame reuse (triple-buffering)
- [x] ReorderBuffer - Sliding window with bounded memory (circular buffer)
- [x] Memory tracking and statistics collection
- [x] 11 comprehensive tests (sequential, out-of-order, memory bounds)

**Task #8: GPU Memory Management** - Encoder Pooling
- [x] EncoderPool - Reusable encoders by configuration (max 2 per config)
- [x] ReferenceFrameManager - Keyframe-based reference frame release
- [x] StagingBufferPool - CPU-GPU transfer buffer with memory pressure monitoring
- [x] PooledEncoder health tracking (drops corrupted/mismatched encoders)
- [x] 14 comprehensive tests (pooling, reuse, health, memory pressure)

### ✅ Documentation
- [x] **docs/NETWORK_OPTIMIZATION.md** - QUIC tuning strategies (300+ lines)
- [x] **docs/MEMORY_OPTIMIZATION.md** - Buffer pool optimization (350+ lines)
- [x] **docs/GPU_MEMORY_MANAGEMENT.md** - Encoder pooling (400+ lines)
- [x] **docs/AV1_VALIDATION.md** - AV1 codec validation strategy
- [x] **docs/ADMIN_DASHBOARD.md** - Admin panel API reference
- [x] **docs/CODE_COVERAGE.md** - Testing strategy and metrics

---

## 🔥 IMMEDIATE NEXT STEPS (High Priority)

### Priority 1: AV1 Performance Validation
**Status**: Documented, needs implementation validation
**Time**: 2-3 hours
- [ ] Run AV1 encoding benchmarks on M3 Mac (VideoToolbox)
- [ ] Run AV1 encoding benchmarks on Intel ARC (Media Foundation)
- [ ] Validate hardware acceleration detection
- [ ] Profile bitrate/quality at various frame rates (30/60/120fps)
- [ ] Document results and any codec-specific tuning needed

### Priority 2: Admin Dashboard Testing
**Status**: Dashboard API implemented, needs database population
**Time**: 1 hour
- [ ] Create test admin users in SQLite database
- [ ] Validate admin panel endpoints (list users, bans, analytics)
- [ ] Test token authentication flow
- [ ] Verify permission enforcement (admin-only actions)

### Priority 3: README Cleanup
**Status**: Identified but not started
**Time**: 30 minutes
- [ ] Remove "Research Notes" section from README.md
- [ ] Update feature list with v0.0.2 features
- [ ] Add architecture diagram reference
- [ ] Link to completed documentation files

---

## 📋 BACKLOG (Future Releases)

### Features (v0.0.3+)
- [ ] **Recording** - Local recording with configurable quality and compression
- [ ] **Clipboard Sync** - Bidirectional clipboard sharing (host ↔ client)
- [ ] **File Transfer** - Secure file transfer over RIFT protocol
- [ ] **Audio Routing** - Per-application audio capture and routing
- [ ] **Input Mapping** - Custom input device profiles

### Platform Support
- [ ] **iOS Client** - WebTransport or native app
- [ ] **Full Wayland Support** - Proper xdg-desktop-portal integration
- [ ] **HDR Capture** - Native HDR (HDR10, Dolby Vision)
- [ ] **60fps+ Streaming** - Optimizations for 120fps+ displays

### Security Enhancements
- [ ] **E2E Encryption** - End-to-end encryption for relayed connections
- [ ] **Audit Logging** - Comprehensive audit trail for admin actions
- [ ] **Rate Limiting** - Per-IP and per-user rate limits on all APIs
- [ ] **Certificate Pinning** - TLS certificate validation for relay servers

### Advanced Networking
- [ ] **Adaptive Resolution** - Dynamic resolution adjustment based on bandwidth
- [ ] **MultiPath Transport** - Split traffic across multiple network paths
- [ ] **Connection Migration** - Seamless handoff between networks (WiFi ↔ cellular)
- [ ] **Custom Congestion Control Profiles** - User-definable CC algorithms

### Performance Tuning
- [ ] **Profile-Guided Optimization** - PGO builds for release binaries
- [ ] **SIMD Optimizations** - Hand-tuned SIMD for FEC/crypto
- [ ] **Lock-Free Data Structures** - Reduce contention in hot paths
- [ ] **Jemalloc Integration** - Better memory allocator for long sessions

---

## 🧪 Testing & Quality Assurance

### Current Test Coverage
```
rift-core       42 tests ✅
rift-crypto     21 tests ✅
wavry-client    26 tests ✅
wavry-common     4 tests ✅
wavry-gateway   14 tests ✅
wavry-media     25 tests ✅
─────────────────────────────
TOTAL         132 tests ✅
```

### Testing Gaps (Future)
- [ ] Integration tests (end-to-end client-server)
- [ ] Network simulation tests (high-loss, high-latency scenarios)
- [ ] Stress tests (long-duration streaming, memory stability)
- [ ] Fuzz testing (malformed packet handling)
- [ ] Performance benchmarks (throughput, latency, CPU/GPU usage)

---

## 📊 Metrics & Goals

### Code Quality
- **Clippy Warnings**: 0/0 ✅
- **Test Coverage**: 132 passing tests across 6 crates ✅
- **Build Time**: ~10 seconds (dev), ~2 minutes (release)
- **Binary Size**: ~30 MB (server), ~25 MB (client)

### Performance Targets (v0.0.2)
- **Local Network**: <10ms latency, ±20% bitrate variance
- **Regional Network**: <50ms latency, ±10% bitrate variance
- **Intercontinental**: <150ms latency, ±5% bitrate variance
- **Satellite**: <500ms latency, ±3% bitrate variance
- **Memory Baseline**: <150 MB RSS (stable for 24+ hours)
- **GPU Memory**: <500 MB peak, 85%+ encoder reuse

---

## 🚀 Release Checklist

### v0.0.2-canary (Current)
- [x] Core RIFT protocol implementation
- [x] DELTA congestion control
- [x] Hardware video encoding (H.264, HEVC, AV1 on all platforms)
- [x] Noise XX encryption
- [x] Memory optimization (buffer pools, reorder buffers)
- [x] GPU memory management (encoder pooling)
- [x] Admin dashboard (basic functionality)
- [ ] AV1 validation on M3/Intel ARC
- [ ] Admin dashboard testing
- [ ] README refresh

### v0.0.3 (Proposed)
- [ ] Recording capability
- [ ] Clipboard sync
- [ ] File transfer
- [ ] iOS support
- [ ] Performance benchmarking suite

---

## 📝 Notes

- **Git Commits**: 5 recent commits (4 ahead of origin/main)
- **Workspace**: 16 crates, all building successfully
- **Dependencies**: Up-to-date with security patches
- **CI/CD**: GitHub Actions workflows validated and fixed
- **Platform Support**: macOS, Linux, Windows, Android (ready); iOS (planned)

See [CHANGELOG.md](./CHANGELOG.md) for detailed version history.
