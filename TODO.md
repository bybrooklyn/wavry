# Wavry Project TODOs

## ✅ COMPLETED TODAY (2026-02-08)

### Phase 8: Web Client & Hybrid Transport
- [x] **Generate TLS certificates for WebTransport**: Created `scripts/gen-wt-cert.sh` using ECDSA.
- [x] **Integrate WebRTC bridge into wavry-server**: Frame pushing and signaling wired.
- [x] **WebRTC DataChannel Fallback**: Implemented "input" channel handling in `webrtc_bridge.rs` and `wavry-server`.
- [x] **Web Client Input Hardening**: Fixed `InputInjector` trait to use normalized `f32` coordinates for `mouse_absolute` across all platforms.
- [x] **Apply database migrations**: Applied security hardening and relay reputation schemas.
- [x] **Integrate login lockouts**: Implemented email-based and IP-based lockout logic in `auth.rs`.
- [x] **WebTransport Stability**: Fixed `wtransport 0.6` compilation and integrated certificate loading.

### Phase 7: Mobile & Android Implementation
- [x] **Quest/OpenXR Integration**: Implemented controller action binding and pose polling in `wavry-vr-alvr`.
- [x] **Android Build Stabilization**: Fixed NDK/ash/openxr compilation errors and made Opus optional to remove `make`/`ninja` dependency.
- [x] **Android Full Build Validation**: Verified `./scripts/dev-android.sh` succeeds and links against FFI.
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

### Priority 1: Media Enhancements
1. **HDR & 10-bit Implementation**
   - [x] macOS: Implemented `kCVPixelFormatType_420YpCbCr10BiPlanarVideoRange` capture and VT Main10 profile.
   - [x] Linux: Implemented `P010_10LE` input format and VAAPI/NVENC Main10 profiles.
   - [x] Windows: Cleaned up MF/D3D11 implementation and fixed compilation errors. (Pending native capture integration for HDR).

2. **AV1 Performance Hardening** (Est: 2 hours)
   - Validate hardware acceleration on M3 and Intel ARC.

### Priority 2: Testing & Hardening
1. **Admin Dashboard Validation** (Est: 1 hour)
   - Hardcode some "Admin" users in the DB to test the new dashboard buttons.

---

## Maintenance & Technical Debt
- [ ] **Code Coverage**: Target >70% coverage for core crates using `cargo-tarpaulin`.
- [ ] **Cleanup**: Remove "Research Notes" section from `README.md`.
- [x] **Clippy/Fmt**: Maintain zero warnings (Verified).