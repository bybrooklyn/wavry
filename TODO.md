# Wavry Project TODOs

## Immediate Next Steps (Phase 6: UI & macOS Parity)

### PCVR (Linux/Windows)
- [x] **OpenXR PCVR adapter**: Linux Wayland (Vulkan) + X11 (OpenGL), Windows D3D11.
- [ ] **Stereo submission**: per-eye swapchain layout and view-specific transforms.
- [ ] **Controller/hand input**: OpenXR action bindings for tracked devices.
- [ ] **Vulkan upload optimization**: staging reuse + async submit (avoid queue-wait per frame).
- [ ] **Frame format + color space**: confirm swapchain format selection and gamma handling.

### macOS Audio
- [x] **Implement `MacAudioCapturer` Logic**:
    - [x] Extract PCM data from `CMSampleBuffer` using `CMSampleBufferGetAudioBufferListWithRetainedBlockBuffer`.
    - [x] Encode to Opus (48kHz stereo, 5ms frames) for low-latency transport.
- [x] **Implement `MacAudioRenderer`**:
    - [x] Decode Opus packets and play via `cpal` with a short buffer.

### Gamepad Support
- [ ] **Connect Gamepad UI to Backend**:
    - [ ] Update `start_session` command to accept `gamepad_enabled` and `deadzone` from `appState`.
    - [ ] Pass these settings to `ClientConfig` and `InputInjector`.
    - [ ] Implement deadzone filtering in `MacInputInjector` and `WindowsInputInjector`.

### Host Integration
- [ ] **Monitor Selection**:
    - [ ] Implement `enumerate_displays` in `MacProbe` (currently returns empty).
    - [ ] expose display list to `wavry-desktop` UI.
    - [ ] Allow user to select which display to capture in `start_host`.

## Future Phases

### Mobile & Native Clients
- [ ] Android Core (NDK) implementation.
- [ ] Android UI (Jetpack Compose).
- [ ] Native macOS Client (SwiftUI) for better performance than Tauri.

### Infrastructure & Optimization
- [ ] **AV1 Support**: Add AV1 hardware encoding support for M3/M4 Macs and supported GPUs.
- [ ] **10-bit HDR**: Investigate pipeline support for 10-bit color.

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
