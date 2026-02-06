# Wavry Project TODOs

## Immediate Next Steps (Phase 6: UI & macOS Parity)

### PCVR (Linux/Windows)
- [x] **OpenXR PCVR adapter**: Linux Wayland (Vulkan) + X11 (OpenGL), Windows D3D11.
- [x] **Stereo submission**: per-eye swapchain layout and view-specific transforms.
- [x] **Controller/hand input**: OpenXR action bindings for tracked devices.
- [x] **OpenXR hand tracking**: `XR_EXT_hand_tracking` + RIFT hand-pose control message path.
- [x] **Vulkan upload optimization**: staging reuse + async submit (avoid queue-wait per frame).
- [x] **Frame format + color space**: runtime validation of swapchain format/gamma per runtime + headset.

### macOS Audio
- [x] **Implement `MacAudioCapturer` Logic**:
    - [x] Extract PCM data from `CMSampleBuffer` using `CMSampleBufferGetAudioBufferListWithRetainedBlockBuffer`.
    - [x] Encode to Opus (48kHz stereo, 5ms frames) for low-latency transport.
- [x] **Implement `MacAudioRenderer`**:
    - [x] Decode Opus packets and play via `cpal` with a short buffer.

### Gamepad Support
- [x] **Connect Gamepad UI to Backend**:
    - [x] Update `start_session` command to accept `gamepad_enabled` and `deadzone` from `appState`.
    - [x] Pass these settings to `ClientConfig` and input capture/injector paths.
    - [x] Implement deadzone filtering in `MacInputInjector` and `WindowsInputInjector`.

### Host Integration
- [x] **Monitor Selection (macOS/Windows desktop app)**:
    - [x] Implement `enumerate_displays` in `MacProbe`.
    - [x] Expose display list to `wavry-desktop` UI.
    - [x] Allow user to select which display to capture in `start_host`.
    - [x] Linux display probe (`list_monitors`) via Wayland portal metadata + X11 RandR fallback.

## Future Phases

### Web Client (Hybrid Transport)
- [x] **WebTransport runtime binding**: feature-gated dev runtime implementation wired to the WebTransport skeleton.
- [x] **WebRTC signaling skeleton in gateway**: minimal HTTP/WS handlers for SDP + ICE exchange.
- [x] **Browser reference client stub**: minimal web app for connect/input/control signaling.

### Mobile & Native Clients
- [ ] Android Core (NDK) implementation.
    - [x] JNI bridge + Rust FFI bindings baseline (`apps/android/app/src/main/cpp`).
    - [ ] Quest/OpenXR integration and Android media/input paths.
- [ ] Android UI (Jetpack Compose).
    - [x] Compose control-plane shell (host/client controls + live stats).
    - [ ] Permissions flow, onboarding, and Quest-safe immersive layouts.
- [ ] Native macOS Client (SwiftUI) for better performance than Tauri.

### Infrastructure & Optimization
- [x] **AV1 Support**: Hardware-gated AV1 encode support added for M3/M4 Macs and supported GPUs.
- [ ] **10-bit HDR**: Wire end-to-end 10-bit/HDR transport path (capability detection now in place).

## Research Notes: macOS Audio Capture

### Strategy
Use `CMSampleBufferGetAudioBufferListWithRetainedBlockBuffer` to extract PCM data from `CMSampleBuffer`s received from `SCStream`.

### Dependencies
- **Crates**:
    - `objc2-core-media`: For `CMSampleBuffer`, `CMBlockBuffer`.
    - `coreaudio-sys` (or manual binding): For `AudioBufferList` layout.
- **Frameworks**: `CoreMedia`, `CoreAudio`, `AudioToolbox`.

### Implementation Details
1. **Link Frameworks**: Ensure `CoreMedia` is linked in `build.rs` or known to `objc2`.
2. **Define FFI**: `CMSampleBufferGetAudioBufferListWithRetainedBlockBuffer` might need manual `extern "C"` definition if not in `objc2-core-media`.
    ```rust
    extern "C" {
        pub fn CMSampleBufferGetAudioBufferListWithRetainedBlockBuffer(
            sbuf: &CMSampleBuffer,
            buffer_list_size_needed_out: *mut usize,
            buffer_list_out: *mut AudioBufferList,
            buffer_list_size: usize,
            block_buffer_allocator: *const c_void,
            block_buffer_memory_allocator: *const c_void,
            flags: u32,
            block_buffer_out: *mut *mut CMBlockBuffer
        ) -> i32; // OSStatus
    }
    ```
3. **Data Extraction**:
    - Call with `buffer_list_out = null` to get size.
    - Allocate `Vec<u8>` for `AudioBufferList`.
    - Call again to populate.
    - retained `block_buffer_out` ensures memory safety (must be released).
4. **Format Conversion**: `SCStream` typically delivers **32-bit float, non-interleaved** or **16-bit PCM** depending on `SCStreamConfiguration`. we need to verify the format using `CMAudioFormatDescription` attached to the sample buffer.
