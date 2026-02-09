# Changelog

All notable changes to the Wavry project.

## [Unreleased] - 2026-02-09

### Fixed
- **Windows API Issues**: Fixed Media Foundation API misuse (cast(), GetMixFormat, CoInitializeEx), packed struct unaligned references
- **Congestion Control**: Fixed state transition logic and epsilon calculation in DELTA algorithm
- **Code Quality**: Removed unnecessary unsafe blocks, dead code, and warnings across workspace
- **Build System**: Unified Windows crate versions to 0.58.0, fixed all compilation errors

## [2026-02-08]

### Added

#### Phase 8: Web Client & Hybrid Transport
- **WebTransport TLS Certificates**: Created `scripts/gen-wt-cert.sh` using ECDSA for secure WebTransport connections
- **WebRTC Bridge Integration**: Frame pushing and signaling wired into wavry-server
- **WebRTC DataChannel Fallback**: Implemented "input" channel handling in `webrtc_bridge.rs` and `wavry-server`
- **Web Client Input Hardening**: Fixed `InputInjector` trait to use normalized `f32` coordinates for `mouse_absolute` across all platforms
- **Database Security**: Applied security hardening and relay reputation schemas migrations
- **Login Security**: Implemented email-based and IP-based lockout logic in `auth.rs`
- **WebTransport Stability**: Fixed `wtransport 0.6` compilation and integrated certificate loading

#### Phase 7: Mobile & Android Implementation
- **Quest/OpenXR Integration**: Implemented controller action binding and pose polling in `wavry-vr-alvr`
- **Android Build Stabilization**: Fixed NDK/ash/openxr compilation errors and made Opus optional to remove `make`/`ninja` dependency
- **Android Full Build Validation**: Verified `./scripts/dev-android.sh` succeeds and links against FFI
- **VR-Safe Layouts**: Added specific padding for Quest in the Android UI

#### Phase 9: Infrastructure & Global Service
- **Relay Reputation System**: Integrated client-side feedback reporting to Master
- **Community Relay Customization**: Added `max_bitrate_kbps` support with 10Mbps minimum enforcement
- **Admin API & UI**: Implemented interactive Ban/Unban/Revoke in the Gateway dashboard
- **Secure Provisioning**: Created `scripts/provision-infrastructure.sh` and `docs/SECURE_PROVISIONING.md`
- **CI/CD**: Updated GitHub Actions to use the automated provisioning pipeline

#### Phase 10: Advanced Features
- **Multi-Monitor Support**: Implemented dynamic discovery (`MonitorList`) and switching (`SelectMonitor`)

#### Media Enhancements
- **HDR & 10-bit Implementation**:
  - macOS: Implemented `kCVPixelFormatType_420YpCbCr10BiPlanarVideoRange` capture and VT Main10 profile
  - Linux: Implemented `P010_10LE` input format and VAAPI/NVENC Main10 profiles
  - Windows: Cleaned up MF/D3D11 implementation and fixed compilation errors

### Changed
- Unified workspace dependencies for Windows crates (0.58.0)
- Improved code quality: zero warnings across workspace
- Enhanced type visibility (made private types public)

### Technical Debt
- ✅ Zero compiler warnings (maintained via `RUSTFLAGS="-D warnings"`)
- ✅ All tests passing (42 tests)
- ✅ Clean build across entire workspace