# Wavry Project TODOs

## 🔥 IMMEDIATE NEXT STEPS

### Priority 1: Media Enhancements
1. **AV1 Performance Hardening** (Est: 2 hours)
   - Validate hardware acceleration on M3 and Intel ARC.

### Priority 2: Testing & Hardening
1. **Admin Dashboard Validation** (Est: 1 hour)
   - Hardcode some "Admin" users in the DB to test the new dashboard buttons.

---

## Maintenance & Technical Debt
- [x] **Code Coverage**: 132 tests across 6 crates (42 rift-core, 21 rift-crypto, 26 wavry-client, 4 wavry-common, 14 wavry-gateway, 25 wavry-media).
- [ ] **Cleanup**: Remove "Research Notes" section from `README.md`.
- [x] **Clippy/Fmt**: Maintain zero warnings (Verified).

---

## 📋 Backlog (Future Features)

### Performance & Optimization (COMPLETED)
- [x] **Network Optimization**: DELTA CC tuning with LatencyProfile, LinkType, CongestionDetector, AdjustmentStrategy, FecController (25 tests)
- [x] **Memory Optimization**: FrameBufferPool and ReorderBuffer for bounded memory (11 tests)
- [x] **GPU Memory**: EncoderPool, ReferenceFrameManager, StagingBufferPool (14 tests)

### Features
- [ ] **Recording**: Add local recording capability with configurable quality
- [ ] **Clipboard Sync**: Bidirectional clipboard sharing between host and client
- [ ] **File Transfer**: Secure file transfer over RIFT protocol
- [ ] **Audio Routing**: Per-application audio capture and routing

### Platform Support
- [ ] **iOS**: Initial iOS client support (Web-based or native)
- [ ] **Wayland**: Full Wayland support with proper screen capture
- [ ] **HDR Capture**: Native HDR screen capture on all platforms

### Security
- [ ] **E2E Encryption**: End-to-end encryption for relayed connections
- [ ] **Audit Logging**: Comprehensive audit logging for admin actions
- [ ] **Rate Limiting**: Implement rate limiting on all public APIs

---

*See [CHANGELOG.md](./CHANGELOG.md) for completed work.*