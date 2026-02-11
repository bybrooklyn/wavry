# Changelog

All notable changes to the Wavry project.

## [0.0.3-canary] - 2026-02-10

### Added
- **Local Recording** (`wavry-media`): `VideoRecorder` with MP4 muxing (H.264 and HEVC tracks) and AAC audio. Integrated into `wavry-server` via `--record`, `--record-dir`, and `--record-quality` flags. Recordings split automatically on codec/resolution changes or when the file size limit is reached.
- **Clipboard Synchronization**: Bidirectional clipboard sync over RIFT protocol. `ClipboardMessage` added to the protobuf schema. `ArboardClipboard` wrapper in `wavry-platform` (gated to non-Android targets). Host and client poll the system clipboard at 500 ms intervals and broadcast changes to the peer. Incoming clipboard messages are capped at 1 MiB (`MAX_CLIPBOARD_TEXT_BYTES`) to prevent memory exhaustion.
- **Input Mapping** (`wavry-platform`): `InputMap` profile struct with key remapping and block rules. `MappedInjector<I>` wrapper applies a profile to any `InputInjector` at runtime without modifying the underlying platform backend. Supports key remapping, key blocking, and gamepad button remapping. 6 unit tests included.
- **`wavry-vr-openxr` crate**: All OpenXR / unsafe graphics interop (Vulkan, Direct3D 11, OpenGL) extracted into a dedicated crate, permanently resolving "ambiguous windows crate" CI errors and isolating the fast-moving `openxr = 0.21` dependency from the rest of the workspace.
- **Android Gradle wrapper**: `gradlew` / `gradlew.bat` and `gradle/wrapper/gradle-wrapper.properties` (Gradle 8.7) committed to `apps/android/` for deterministic, cache-friendly CI builds. Added `org.gradle.caching=true` to `gradle.properties`.

### Changed
- **Windows crate upgrade**: Entire workspace unified on `windows = 0.62.2` (from 0.58.0). `wavry-media`, `wavry-platform`, and `wavry-vr-openxr` updated to match new tuple-struct constructors and updated COM / DirectX APIs.
- **AV1 recording fallback**: `VideoRecorder` now sets a `disabled` flag on first unsupported-codec error instead of logging a warning on every frame. The stream continues uninterrupted; recording is silently skipped.
- **`set-version.sh` fix**: Replaced `sed` with a Perl one-liner that only replaces the first `version = "..."` occurrence in each `Cargo.toml`, preventing it from clobbering pinned dependency versions (e.g. `windows = "0.62.2"`). Added `tauri.conf.json` update step and `cargo update --workspace` at the end.
- **CI/CD — release artifacts**: Backend binaries now include platform suffix (`-linux`, `-macos`, `-windows`) to prevent name collisions. Desktop stage copies binary only (no `.d`/debug metadata). macOS Swift app is zipped to `Wavry-macos.zip`. Added "Delete existing release assets" step before re-uploading to avoid artifact accumulation.
- **CI/CD — Android caching**: Added `Swatinem/rust-cache@v2` and `cargo-ndk` binary caching to the Android CI job. NDK 26.3 cached by path.

### Fixed
- `arboard` dependency gated to `cfg(not(target_os = "android"))` — fixes `aarch64-linux-android` compile failure.
- `wavry-client/Cargo.toml` missing `wavry-platform` dependency — resolved 5 cascading E0432/E0282 errors.
- Broken `use wavry_vr::VrError, VrResult};` in `wavry-vr-openxr/src/android.rs` (missing `{`).
- Clippy: `width % 2 == 0` → `width.is_multiple_of(2)`, `assert_eq!(x, true)` → `assert!(x)`, `!hash.is_empty()` in gateway tests, unused import `Ordering` and `OPUS_BITRATE_BPS` in windows encoder.
- Missing `use std::sync::{Arc, Mutex}` import in `wavry-desktop/src-tauri/src/commands.rs`.

## [Unreleased] - 2026-02-10

### Fixed
- **GitHub Actions Workflows**: Fixed critical build pipeline issues:
  - Removed `already_released` gate preventing tests/builds from running on main branch
  - All test and build jobs now run on every push to main (not skipped after initial release)
  - Release job still prevents duplicate releases for same version
- **Docker Build System**: Fixed multi-platform Docker builds for gateway and relay:
  - Added missing system dependencies to cacher stage (pkg-config, protobuf-compiler, libgstreamer1.0-dev, libgstreamer-plugins-base1.0-dev, libgtk-3-dev)
  - Cacher stage now has all dependencies needed for cargo-chef to compile workspace
  - Fixes gdk-3.0.pc not found errors in Docker builds
- **GitHub API Rate Limiting**: Added authentication token to arduino/setup-protoc action:
  - Prevents "API rate limit exceeded" errors from unauthenticated requests
  - Enables higher rate limit (10,000 req/hour vs 60 req/hour)
- **Code Cleanup**: Removed redundant comments from Windows encoder, improved formatting

## [2026-02-09]

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