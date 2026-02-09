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
- [ ] **Code Coverage**: Target >70% coverage for core crates using `cargo-tarpaulin`.
- [ ] **Cleanup**: Remove "Research Notes" section from `README.md`.
- [x] **Clippy/Fmt**: Maintain zero warnings (Verified).

---

## 📋 Backlog (Future Features)

### Performance & Optimization
- [ ] **Network Optimization**: Implement QUIC congestion control tuning for high-latency networks
- [ ] **Memory Optimization**: Profile and reduce memory usage in media capture pipelines
- [ ] **GPU Memory**: Implement proper GPU memory management for long streaming sessions

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