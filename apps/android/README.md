# Wavry Android

Android control-plane client built with Kotlin + Jetpack Compose.

This project now has two app flavors:

- `mobile`: standard Android phone/tablet build (client controls).
- `quest`: Meta Quest build (client-focused + VR manifest capabilities).

## What is included

- Compose app shell (host/client/session controls).
- JNI bridge (`libwavry_android.so`) that calls `wavry-ffi` C API.
- CMake integration for Rust prebuilt static libs per ABI.

## Build prerequisites

- Android Studio (or Android SDK + NDK + CMake installed)
- Rust toolchain
- `cargo-ndk` (`cargo install cargo-ndk`)

## One-command build

```bash
./scripts/dev-android.sh
```

By default, this builds the `mobile` flavor (`:app:assembleMobileDebug`).
It auto-detects Java/Gradle where possible and builds Rust FFI first.

### Build Quest flavor

```bash
./scripts/dev-android.sh --quest
```

### Build both flavors

```bash
./scripts/dev-android.sh --both
```

## One-command build + install + launch

```bash
./scripts/run-android.sh
```

Useful variants:

```bash
./scripts/run-android.sh --quest
./scripts/run-android.sh --both --launch-target quest
./scripts/run-android.sh --mobile --serial <adb-device-id>
```

## FFI-only

```bash
./scripts/dev-android.sh --ffi-only
```

This places ABI-specific `libwavry_ffi.a` files under:

- `apps/android/app/src/main/cpp/prebuilt/arm64-v8a/`
- `apps/android/app/src/main/cpp/prebuilt/x86_64/`

For Quest-only FFI output:

```bash
./scripts/dev-android.sh --ffi-only --quest
```
