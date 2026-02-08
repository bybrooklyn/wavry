# Wavry Project TODOs

## ✅ COMPLETED TODAY (2026-02-08)

### Phase 8: Web Client & Hybrid Transport
- [x] **Generate TLS certificates for WebTransport**: Created `scripts/gen-wt-cert.sh` using ECDSA.
- [x] **Integrate WebRTC bridge into wavry-server**: Frame pushing and signaling wired.
- [x] **WebRTC DataChannel Fallback**: Implemented "input" channel handling in `webrtc_bridge.rs` and `wavry-server`.
- [x] **Apply database migrations**: Applied security hardening and relay reputation schemas.
- [x] **Integrate login lockouts**: Implemented email-based and IP-based lockout logic in `auth.rs`.
- [x] **WebTransport Stability**: Fixed `wtransport 0.6` compilation and integrated certificate loading.

### Phase 7: Mobile & Android Implementation
- [x] **Quest/OpenXR Integration**: Implemented controller action binding and pose polling in `wavry-vr-alvr`.
- [x] **Android Build Stabilization**: Fixed NDK/ash/openxr compilation errors and made Opus optional to remove `make`/`ninja` dependency.
- [x] **VR-Safe Layouts**: Added specific padding for Quest in the Android UI.

### Phase 9: Infrastructure & Global Service
- [x] **Relay Reputation System**: Integrated client-side feedback reporting to Master.
- [x] **Community Relay Customization**: Added `max_bitrate_kbps` support with 10Mbps minimum enforcement.
- [x] **Admin API & UI**: Implemented interactive Ban/Unban/Revoke in the Gateway dashboard.
- [x] **Secure Provisioning**: Created `scripts/provision-infrastructure.sh` and `docs/SECURE_PROVISIONING.md`.
- [x] **GitHub Actions Integration**: Updated CI to use the automated provisioning pipeline.

### Phase 10: Advanced Features
- [x] **Multi-Monitor Support**: Implemented dynamic discovery (`MonitorList`) and switching (`SelectMonitor`).

---

## 🔥 IMMEDIATE NEXT STEPS

### Priority 1: Hardening & Testing
1. **Android Full Build Validation** (Est: 1 hour)
   - Run `./scripts/dev-android.sh` to ensure Kotlin side builds and links correctly.
   - Test on Quest headset to verify controller tracking.

2. **Web Client End-to-End Hardening** (Est: 2 hours)
   - Verify input injection from browser via WebRTC DataChannel.
   - Hardcode some "Admin" users in the DB to test the new dashboard buttons.

### Priority 2: Media Enhancements
1. **HDR & 10-bit Implementation** (Est: 3-4 hours)
   - Implement `MTLStorageMode` and color space conversion for macOS HDR.
   - Update VA-API/NVENC logic for HEVC Main10.

2. **AV1 Performance Hardening** (Est: 2 hours)
   - Validate hardware acceleration on M3 and Intel ARC.

---

## Maintenance & Technical Debt
- [ ] **Code Coverage**: Target >70% coverage for core crates using `cargo-tarpaulin`.
- [ ] **Cleanup**: Remove "Research Notes" section from `README.md`.
- [x] **Clippy/Fmt**: Maintain zero warnings (Verified).