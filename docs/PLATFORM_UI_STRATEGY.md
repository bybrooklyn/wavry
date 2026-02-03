# Wavry Platform Scope & UI Strategy

**Status:** Locked — This specification is final.

---

## Platform Scope

### Implement Now

| Platform | Notes |
|----------|-------|
| **Linux** | Primary platform |
| **Windows** | Desktop parity |
| **macOS** | Desktop only, no App Store |
| **Android / Quest** | Mobile + VR |

### Deferred (Do NOT implement)

- ❌ iOS
- ❌ iPadOS
- ❌ App Store / TestFlight workflows

> The architecture must not block future iOS support, but no iOS code is written at this stage.

---

## Definition of "Native App"

A Wavry app is **native** if it uses:

- Native capture APIs
- Native input APIs
- Native graphics paths
- Native networking
- Native OS integration
- Native binaries

**Using a web-based UI for the control plane does not make the app non-native.**

---

## UI Strategy

### Global Rule

**UI is control plane only.**

UI must **never**:
- Handle video or audio frames
- Sit in the UDP hot path
- Own protocol logic
- Make session decisions
- Perform crypto or networking logic

**All latency-sensitive logic lives in Rust core crates.**

---

### Desktop UI (Linux + Windows)

| Technology | Purpose |
|------------|---------|
| **Tauri** | Native shell |
| **SvelteKit** | UI framework |

One shared UI codebase for:
- Connection management
- Settings
- Diagnostics
- Stats display

Communicates with Rust via IPC (commands + events).

---

### macOS UI

| Technology | Purpose |
|------------|---------|
| **SwiftUI** | Native macOS UI |

- Desktop-only
- Local distribution (no App Store assumptions)
- Rust core exposed via static lib or dylib
- SwiftUI renders state and sends commands only

---

### Android / Quest UI

| Technology | Purpose |
|------------|---------|
| **Kotlin** | Android platform |
| **Jetpack Compose** | UI framework |

- Rust core via NDK
- Compose UI is control plane only
- Designed for fullscreen / immersive modes
- VR-safe layouts on Quest
- Streaming, OpenXR, timing, and input paths remain entirely in Rust

---

## UI Parity Rules

All UIs must follow the same **UI state contract**:
- Same fields
- Same ranges
- Same semantics
- Same terminology

**UI code is platform-specific.**
**UI behavior is platform-consistent.**

---

## Non-Goals (UI)

Do **NOT**:
- Use WebView UI on Android/Quest
- Use Tauri on mobile
- Use native UI toolkits for desktop Linux/Windows
- Embed UI logic into Rust core crates

---

## Summary

| Platform | UI Technology |
|----------|---------------|
| Linux / Windows | Tauri + SvelteKit |
| macOS | SwiftUI |
| Android / Quest | Kotlin + Jetpack Compose |
| iOS | Deferred |

**Rust owns all core logic. UI is thin, native, and replaceable.**
