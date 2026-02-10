# Engineering Handover & Cleanup Plan

**Date:** 2026-02-10
**Context:** Massive architectural refactor (Windows 0.62.2 upgrade), VR subsystem isolation, and v0.0.3 feature implementation (Recording, Clipboard).

---

## 🚨 Executive Summary

We have successfully executed a high-risk upgrade of the Windows stack and refactored the VR architecture to prevent dependency hell. Simultaneously, we implemented two major features for v0.0.3. The codebase is compiling, but specific cleanup and verification tasks remain to ensure production stability.

### 🏆 Key Achievements
1.  **Windows Crate Upgrade (0.58.0 → 0.62.2):** Unified the entire workspace to the latest `windows` crate. This involved heavy refactoring of `wavry-media` (Direct3D/MediaFoundation), `wavry-platform` (Input Injection), and VR subsystems.
2.  **VR Architecture Decoupling:** Created `crates/wavry-vr-openxr`. This crate now encapsulates **all** OpenXR dependencies and unsafe Windows/Linux/Android interop. `wavry-vr-alvr` is now a pure logic adapter, depending on this new crate. **This permanently fixes the "ambiguous windows crate" CI errors.**
3.  **Local Recording:** Implemented `VideoRecorder` in `wavry-media` using the `mp4` crate. Fully integrated into `wavry-server` (host-side) and `wavry-client` (client-side) with AAC audio muxing.
4.  **Clipboard Synchronization:** Added `ClipboardMessage` to RIFT protocol. Implemented `ArboardClipboard` in `wavry-platform` and integrated bi-directional syncing in `wavry-server` and `wavry-client`.
5.  **CI/CD Optimization:** Parallelized Android ABI builds and added aggressive caching (NDK, Rust, Gradle), significantly reducing build times.

---

## 🛠️ Immediate Action Items (The "Fix It" List)

### 1. 🧹 Code Cleanup & Warnings
The refactor left some debris. Run `cargo check --workspace --all-targets` and address the following:

*   **`wavry-media` (macOS/Audio):**
    *   The `MacAudioCapturer` struct has unused fields `tx` and `frame_duration_us` in `AudioContext`.
    *   **Action:** If these are truly dead, remove them. If they are needed for logic that was commented out, uncomment or implement it.
*   **`wavry-media` (Windows):**
    *   Unused imports in `src/windows.rs` (e.g., `OPUS_BITRATE_BPS` if `opus-support` is off, `Ordering`).
*   **`wavry-desktop`:**
    *   Unused `Mutex` import in `commands.rs`.
*   **General:**
    *   Run `cargo fmt --all` to ensure the new files (`recorder.rs`, `wavry-vr-openxr/*`) match style guidelines.
    *   Run `cargo clippy --workspace --all-targets -- -D warnings` to enforce strict quality.

### 2. 📋 Clipboard Synchronization Verification
We implemented the plumbing, but it needs runtime verification.

*   **Logic Check:** In `wavry-client/src/client.rs` and `wavry-server/src/main.rs`, we added polling loops.
    *   *Risk:* Infinite loop of clipboard updates if `set_text` triggers a change that `get_text` picks up immediately as "new".
    *   *Fix:* Ensure `last_clipboard_text` is updated *immediately* after `set_text` to prevent echoing the same text back to the network. (This was implemented, but double-check the logic flow).
*   **Security:** Ensure `ClipboardMessage` size is capped in `RIFT.proto` or the handler to prevent memory exhaustion attacks (e.g., trying to paste 100MB of text).

### 3. 📹 Recording Feature Polish
*   **AV1 Limitation:** The `mp4` crate (v0.14.0) **does not support AV1**.
    *   *Current Behavior:* The code errors out/logs a warning if AV1 is selected.
    *   *Action:* Verify the fallback logic. If a user selects AV1 for streaming + Recording enabled, does the stream fail? Or does it just disable recording? It *should* ideally fallback to HEVC/H264 for the recording track (transcoding) or disable recording gracefully.
*   **Audio Sync:** Verify that `timestamp_us` conversion to MP4 timescales (48kHz for Audio, 1000 for Video) results in synchronized playback. Drifting is common in V1 implementations.

### 4. 🤖 Android CI & Gradle
*   **Gradle Wrapper:** The CI still downloads Gradle manually in some paths.
    *   *Task:* Run `gradle wrapper` inside `apps/android` locally and commit the `gradlew` and `gradle/wrapper/*` files. This allows the CI (and `dev-android.sh`) to use a deterministic Gradle version without downloading the distribution zip every time.

---

## 🏗️ Architecture Notes for Future Devs

### The `wavry-vr-openxr` Crate
**DO NOT** move OpenXR logic back into `wavry-vr-alvr`.
*   **Why?** `openxr` (the crate) has strict dependencies on specific `windows` crate versions. The rest of the Wavry workspace needs to move fast (e.g., `windows` 0.62+).
*   **Rule:** If you need to touch OpenXR code, do it in `wavry-vr-openxr`. Expose a clean, high-level Rust trait/struct to the rest of the workspace.

### Windows API (0.62.2)
We are now on `windows` 0.62.2.
*   **Constructors:** Many structs (like `MFT_ENUM_FLAG`) are now tuple structs wrapping primitives. Use `MFT_ENUM_FLAG(value)` instead of casting.
*   **Integers:** DirectX constants often use `u32` vs `i32` differently than 0.58. Be prepared for explicit casts (`.try_into().unwrap()` or `as i32`).
*   **Com Interfaces:** `cast()` and `Activate()` signatures have changed. Look at `wavry-media/src/windows.rs` for examples of the new patterns.

---

## 🧪 Testing Strategy

Before shipping v0.0.3:

1.  **Unit Tests:**
    ```bash
    cargo test --workspace
    ```
2.  **Windows Build (Cross-compile check):**
    ```bash
    cargo check --workspace --target x86_64-pc-windows-msvc
    ```
    *(Note: We fixed the compilation errors, but runtime testing on actual Windows hardware is required for Input Injection and DXGI Capture).*
3.  **Linux Build:**
    ```bash
    cargo check --workspace --target x86_64-unknown-linux-gnu
    ```
4.  **End-to-End Recording:**
    *   Start Server: `cargo run --bin wavry-server -- --record`
    *   Connect Client.
    *   Perform actions.
    *   Stop.
    *   Check `recordings/` folder for valid `.mp4`.

---

## 📦 Dependency Tracking

| Crate | Old Version | New Version | Reason |
|-------|-------------|-------------|--------|
| `windows` | 0.58.0 | **0.62.2** | Modern API features, unifying deps |
| `openxr` | 0.16.0 | **0.21.1** | Support for newer runtimes |
| `mp4` | N/A | **0.14.0** | New Recording feature |
| `arboard` | N/A | **3.4.0** | New Clipboard feature |

*End of Report.*