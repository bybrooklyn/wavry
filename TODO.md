# Wavry Project TODOs

**Current Version**: v0.0.3-rc1
**Last Updated**: 2026-02-10
**Build Status**: ✅ Clean (0 warnings, 155+ tests passing)

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

## 🔥 COMPLETED (v0.0.2-canary)

### ✅ Priority 1: AV1 Performance Validation
**Status**: Documentation & hardware testing procedure complete
**Completion**: 🎯 Hardware testing procedure created (HWTODO.md)
- [x] AV1 validation strategy documented (docs/AV1_VALIDATION.md)
- [x] Hardware testing matrix created (HWTODO.md)
- [x] M4 MacBook & RTX 3070 Ti test procedures documented
- [ ] Actual benchmarks TBD (user hardware testing)

### ✅ Priority 2: Admin Dashboard Testing
**Status**: Complete with 10 new integration tests
**Completion**: 🎯 Admin dashboard fully tested
- [x] 10 comprehensive integration tests created
- [x] User creation & management tested
- [x] Session revocation workflow tested
- [x] User banning/unbanning tested
- [x] Token hashing & authentication tested
- [x] Admin overview data structure tested
- [x] All tests passing (150 total)

### ✅ Priority 3: README Cleanup
**Status**: Complete with v0.0.2 feature documentation
**Completion**: 🎯 README refreshed with new features
- [x] Removed "skeleton" terminology
- [x] Added v0.0.2 Features & Improvements section
- [x] Documented new input support (scroll, gamepad)
- [x] Enhanced Admin Dashboard setup instructions
- [x] Added memory optimization highlights
- [x] Referenced optimization & testing documentation

### ✅ BONUS: Code Improvements
**Status**: Complete with 3 critical fixes
- [x] Feedback signing with Ed25519 identity key
- [x] InputInjector scroll & gamepad support (all platforms)
- [x] WebTransport unidirectional frame streaming fixed

### ✅ CI/CD & Build Infrastructure Fixes (2026-02-10)
**Status**: Complete
- [x] Fixed Docker build system for multi-platform (gateway, relay)
- [x] Fixed Platform Builds workflow logic (release job, artifact naming, asset deletion)
- [x] Fixed GitHub API rate limiting (authenticated protoc action)
- [x] Android caching: Rust cache, NDK cache, Gradle wrapper, build cache
- [x] `set-version.sh`: perl one-liner for safe first-occurrence version replace
- [x] Code cleanup: Windows encoder, all Clippy warnings resolved

### ✅ Code Cleanup & Warning Elimination (FIXING.md complete)
**Status**: All items resolved
- [x] `mac_audio_capturer.rs`: `tx` / `frame_duration_us` dead-code warnings suppressed with `#[cfg_attr(not(feature = "opus-support"), allow(dead_code))]`; opus-only constants moved to `#[cfg(feature = "opus-support")]` import
- [x] `wavry-desktop/commands.rs`: added missing `Mutex` import
- [x] `wavry-vr-openxr/common.rs`: `width % 2 == 0` → `width.is_multiple_of(2)`
- [x] `wavry-gateway` tests: `assert!(hash.len() > 0)` → `assert!(!hash.is_empty())`
- [x] `wavry-vr-openxr/android.rs`: broken `use` brace, unused imports, spurious `mut`
- [x] `wavry-vr-openxr/linux.rs`: unused `Instant` import
- [x] Zero compiler warnings across all crates

---

## 🔄 IMMEDIATE NEXT STEPS

### Current Status: v0.0.2-canary Ready
All infrastructure and core functionality is complete. Next phase focuses on new features for v0.0.3.

### Android CI Build Speed Improvements
- [x] **Add Rust cache to Android CI job** - `Swatinem/rust-cache@v2` added to Android job
- [x] **Cache Android NDK/SDK** - NDK 26.3 cached by path in CI
- [x] **Add `gradlew` wrapper to repo** - Gradle 8.7 wrapper committed to `apps/android/`
- [x] **Enable Gradle build cache** - `org.gradle.caching=true` added to `gradle.properties`
- [x] **Parallelize ABI builds** - `build-android-ffi.sh` already launches arm64-v8a and x86_64 as background jobs and waits (implemented)
- [x] **`--no-daemon` removal** - `dev-android.sh` never had `--no-daemon`; uses `gradlew` wrapper directly (already correct)

### Windows Platform Stabilization (v0.0.2+)
- [x] **Workspace-wide `windows` crate upgrade** - Upgrade from `windows` 0.58 to 0.62+ and refactor media/server/client modules to match new API.
- [x] **Architectural Refactor: `wavry-vr-openxr`** - Separate OpenXR implementation from `wavry-vr-alvr` into its own crate for better dependency isolation and modularity.
- [x] **Remove `openxr` 0.16 patch** - Successfully upgraded to upstream `openxr` 0.21.1 and isolated it in its own crate.

### Outstanding Items (Optional, not blocking)
- [ ] **Hardware AV1 Validation**: Run benchmarks on M4 MacBook Air ↔ RTX 3070 Ti
  - Use HWTODO.md procedures
  - Validates codec performance on actual hardware
  - Updates docs/AV1_VALIDATION.md with results

- [ ] **Version Bump** (when starting v0.0.3 work)
  - Update VERSION file to v0.0.3-canary
  - Create new CHANGELOG section for v0.0.3
  - Update workflow version checks

---

## 🎯 v0.0.3 FEATURE SELECTION & IMPLEMENTATION

**Status**: Design & Prioritization Complete
**Reference**: See [docs/BACKLOG_ROADMAP.md](docs/BACKLOG_ROADMAP.md)

### ✅ v0.0.3 Feature: Recording
**Status**: Complete
- [x] Core RecorderConfig + Quality structs
- [x] VideoRecorder (MP4 muxing with AAC audio)
- [x] Server-side (`--record`, `--record-dir`, `--record-quality`)
- [x] Client-side integration
- [x] AV1 fallback: logs once, disables recording gracefully (stream unaffected)

### ✅ v0.0.3 Feature: Clipboard Sync
**Status**: Complete
- [x] `ClipboardMessage` in RIFT proto, 1 MiB size cap
- [x] `ArboardClipboard` in `wavry-platform` (Android-gated)
- [x] Bidirectional polling in server and client (500 ms interval)
- [x] Echo prevention via `last_clipboard_text`

### ✅ v0.0.3 Feature: Input Mapping
**Status**: Complete
- [x] `InputMap` profile struct (key remapping, key blocking, button remapping)
- [x] `MappedInjector<I>` wrapper — applies map to any `InputInjector` at runtime
- [x] 6 unit tests covering passthrough, remap, block, and gamepad button mapping

### v0.0.4+ Candidates
- **Audio Routing** (Medium complexity, 6-8 hours)
- **File Transfer** (High complexity, 12-16 hours)

---

## 📋 BACKLOG (Future Releases)

### Features (v0.0.4+)
- [x] ~~**Recording**~~ - Shipped in v0.0.3-canary
- [x] ~~**Clipboard Sync**~~ - Shipped in v0.0.3-canary
- [x] ~~**Input Mapping**~~ - Shipped in v0.0.3-canary
- [ ] **File Transfer** - Secure file transfer over RIFT protocol
- [ ] **Audio Routing** - Per-application audio capture and routing

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
rift-core        42 tests ✅
rift-crypto      21 tests + 1 doctest ✅
wavry-client     26 tests ✅
wavry-common      4 tests ✅
wavry-gateway    14 tests ✅
wavry-gateway    10 integration tests (admin_dashboard) ✅
wavry-master      3 tests ✅
wavry-media      25 tests ✅
wavry-server      1 test ✅
─────────────────────────────
TOTAL          147+ tests ✅
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
- **Test Coverage**: 147+ passing tests across 9 crates ✅
- **Build Time**: ~10 seconds (dev), ~2 minutes (release)
- **Binary Size**: ~30 MB (server), ~25 MB (client)
- **CI/CD Status**: ✅ All workflows operational (Docker builds, platform builds, tests, rate limiting fixed)

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
- [x] Admin dashboard with 10 integration tests
- [x] Extended input support (scroll, gamepad)
- [x] Ed25519 feedback signing
- [x] WebTransport frame streaming fixed
- [x] GitHub Actions CI/CD fully operational
- [x] Docker multi-platform builds (gateway, relay)
- [ ] AV1 validation on M4 MacBook Air & RTX 3070 Ti (hardware testing via HWTODO.md)
- [ ] Optional: Record actual hardware benchmark results

### v0.0.3-rc1 (Current)
- [x] Recording capability (VideoRecorder, MP4 muxing)
- [x] Clipboard sync (bidirectional, 1 MiB cap)
- [x] Input mapping (key/button remap + block profiles)
- [x] Android Gradle wrapper (deterministic builds)
- [x] Windows crate upgrade to 0.62.2
- [x] VR architecture decoupled (wavry-vr-openxr crate)
- [x] Zero compiler warnings across entire workspace
- [ ] AV1 validation on M4 MacBook Air & RTX 3070 Ti (hardware testing via HWTODO.md)

### v0.0.4 (Proposed)
- [ ] File transfer
- [ ] iOS support
- [ ] Performance benchmarking suite
- [ ] Audio routing

---

## 📝 Notes

- **Git Commits**: Latest on origin/main (b679677, 38588e8, 0215145, f963de5, 8ac0a89)
- **Workspace**: 16 crates, all building successfully (0 warnings)
- **Dependencies**: Up-to-date with security patches
- **CI/CD**: ✅ GitHub Actions workflows fully operational
  - Platform Builds: Tests and builds run on every push
  - Docker Images: Multi-platform builds (linux/amd64, linux/arm64)
  - Rate limiting: Fixed (authenticated GitHub API tokens)
- **Platform Support**: macOS, Linux, Windows, Android (ready); iOS (planned)
- **Next Phase**: v0.0.3 feature implementation (Recording recommended as first feature)

See [CHANGELOG.md](./CHANGELOG.md) for detailed version history.
