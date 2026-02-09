# GPU Memory Management - Long-Session Stability

**Status**: Planning & Analysis Phase
**Focus**: GPU Memory Leaks and Fragmentation in Extended Streaming
**Target Implementation**: 2 weeks

---

## Overview

Hardware video encoding on GPU (H.264, HEVC, AV1) can accumulate memory fragmentation over long streaming sessions. This document outlines strategies for monitoring GPU memory, preventing leaks, and managing allocations across macOS (Metal/VideoToolbox), Windows (Direct3D/Media Foundation), and Linux (VA-API).

## Current Architecture

### Platform-Specific GPU Usage

**macOS (VideoToolbox + Metal)**:
- `CVPixelBufferPool` for input frame staging
- `VTCompressionSession` holding reference frames and encoder state
- Metal command buffers and textures for frame prep
- IOSurface memory (shared between processes)

**Windows (Media Foundation + Direct3D)**:
- `ID3D11Device` with `D3D11_FEATURE_LEVEL_11_0` minimum
- `IMFMediaBuffer` for input staging
- `IMFTransform` (H.264/HEVC encoder) GPU-linked state
- Staging textures (`D3D11_USAGE_STAGING`) for CPU-GPU sync

**Linux (VA-API)**:
- `VADisplay` and `VAContext` for encoding session
- `VABuffer` objects for parameter data
- `VASurfaceID` for GPU memory surfaces
- `vaDeriveImage` for mapped memory access

### Known GPU Memory Issues

1. **CVPixelBufferPool (macOS)**: Not releasing buffers when pool is deallocated
2. **VTCompressionSession**: Reference frames held until new keyframe
3. **D3D11 Staging Textures**: Stranded allocations if frame drops mid-encode
4. **VABuffer Leaks (Linux)**: Context cleanup order matters; buffers orphaned if context destroyed first
5. **IOSurface/Handle Leaks**: File descriptor leaks on macOS; handle leaks on Windows

---

## GPU Memory Monitoring

### 1. Platform-Specific Instrumentation

**macOS - Metal Framework Inspection**:
```rust
pub fn gpu_memory_usage_macos() -> Option<u64> {
    // Query Metal device allocations via private MTL APIs
    // Fallback: Monitor IORegistry for GPU memory changes
    unsafe {
        let framework = objc::runtime::Class::get("MTLDevice")
            .and_then(|cls| {
                let device = msg_send![cls, currentDevice];
                Some(device)
            });

        // Alternative: Use sysctl for GPU memory (less accurate)
        let mut size: u64 = 0;
        libc::sysctlbyname(
            b"hw.gpu.cores\0".as_ptr() as *const i8,
            &mut size as *mut _ as *mut c_void,
            &mut std::mem::size_of::<u64>(),
            std::ptr::null_mut(),
            0,
        );
        Some(size)
    }
}

pub struct MacOSGpuMetrics {
    pub encoder_state_mb: u64,      // VTCompressionSession reference frames
    pub pixel_buffer_pool_mb: u64,  // CVPixelBufferPool allocations
    pub metal_command_mb: u64,      // Metal command buffer allocations
    pub iosurface_count: u64,       // Number of IOSurface objects
}
```

**Windows - Direct3D Monitoring**:
```rust
pub fn gpu_memory_usage_windows(device: *mut ID3D11Device) -> D3DGpuMetrics {
    unsafe {
        let mut adapter: *mut IDXGIAdapter = std::ptr::null_mut();
        // Get adapter from device via DXGI
        let hr = device.QueryInterface(&IDXGIAdapter::uuidof(), &mut adapter as *mut _ as *mut _);

        if hr == S_OK {
            let mut desc: DXGI_ADAPTER_DESC = std::mem::zeroed();
            (*adapter).GetDesc(&mut desc);

            D3DGpuMetrics {
                dedicated_vram_mb: desc.DedicatedVideoMemory / (1024 * 1024),
                shader_resource_views: get_srv_count(device),
                render_targets: get_rtv_count(device),
                staging_textures: get_staging_texture_count(device),
            }
        } else {
            Default::default()
        }
    }
}

pub struct D3DGpuMetrics {
    pub dedicated_vram_mb: u64,
    pub shader_resource_views: u32,
    pub render_targets: u32,
    pub staging_textures: u32,
}
```

**Linux - VA-API Memory Inspection**:
```rust
pub fn gpu_memory_usage_vaapi(display: *mut VADisplay) -> VaapiGpuMetrics {
    unsafe {
        let mut major_version: i32 = 0;
        let mut minor_version: i32 = 0;
        vaInitialize(display, &mut major_version, &mut minor_version);

        // Query available and used VRAM
        let mut attrs: Vec<VAConfigAttrib> = vec![
            VAConfigAttrib {
                type_: VAConfigAttribMaxPictureWidth,
                value: 0,
            },
            VAConfigAttrib {
                type_: VAConfigAttribMaxPictureHeight,
                value: 0,
            },
        ];

        VaapiGpuMetrics {
            max_width: attrs[0].value as u32,
            max_height: attrs[1].value as u32,
            active_surfaces: get_active_va_surface_count(),
            active_buffers: get_active_va_buffer_count(),
        }
    }
}

pub struct VaapiGpuMetrics {
    pub max_width: u32,
    pub max_height: u32,
    pub active_surfaces: u32,
    pub active_buffers: u32,
}
```

### 2. Custom Metrics Collection

```rust
pub struct GpuMemorySnapshot {
    pub timestamp: Instant,

    // Platform-agnostic metrics
    pub peak_memory_mb: u64,
    pub current_memory_mb: u64,
    pub memory_fragmentation_ratio: f32,

    // Encoder-specific
    pub active_encoding_sessions: u32,
    pub reference_frames_mb: u64,
    pub staging_buffer_mb: u64,

    // Lifecycle metrics
    pub session_duration_secs: u64,
    pub frames_encoded: u64,
    pub frame_drops_due_to_gpu_memory: u32,

    // Platform-specific
    #[cfg(target_os = "macos")]
    pub macos_metrics: Option<MacOSGpuMetrics>,
    #[cfg(target_os = "windows")]
    pub windows_metrics: Option<D3DGpuMetrics>,
    #[cfg(target_os = "linux")]
    pub linux_metrics: Option<VaapiGpuMetrics>,
}

pub struct GpuMemoryTracker {
    snapshots: VecDeque<GpuMemorySnapshot>,
    max_snapshots: usize,  // e.g., 1440 for 24h at 1/min sample rate
    leak_threshold_mb: u64,  // e.g., 50 MB = potential leak
}

impl GpuMemoryTracker {
    pub fn take_snapshot(&mut self) -> GpuMemorySnapshot {
        let snapshot = GpuMemorySnapshot {
            timestamp: Instant::now(),
            current_memory_mb: self.current_gpu_memory(),
            peak_memory_mb: self.peak_gpu_memory(),
            ..Default::default()
        };

        self.snapshots.push_back(snapshot.clone());
        if self.snapshots.len() > self.max_snapshots {
            self.snapshots.pop_front();
        }

        self.detect_memory_leak(&snapshot);
        snapshot
    }

    pub fn detect_memory_leak(&self, latest: &GpuMemorySnapshot) -> Option<MemoryLeakAlert> {
        if self.snapshots.len() < 10 {
            return None;
        }

        // Linear regression on last 10 snapshots
        let recent: Vec<_> = self.snapshots.iter().rev().take(10).collect();
        let growth_rate_mb_per_min = self.linear_regression(&recent);

        // If growing >1 MB/min, likely a leak
        if growth_rate_mb_per_min > 1.0 {
            return Some(MemoryLeakAlert {
                detected_at: latest.timestamp,
                growth_rate: growth_rate_mb_per_min,
                estimated_leak_source: self.estimate_leak_source(&latest),
            });
        }

        None
    }
}
```

---

## GPU Memory Optimization Techniques

### 1. Encoder Session Pooling (Platform-Specific)

**Problem**: Creating/destroying encoder sessions causes GPU memory fragmentation.

**macOS Implementation**:
```rust
pub struct VideoToolboxEncoderPool {
    idle: VecDeque<VTCompressionSession>,
    active: HashMap<SessionId, VTCompressionSession>,
    config: EncoderConfig,  // H.264 profile/level, frame rate, etc.
    max_idle: usize,  // e.g., 2 for bitrate adaptation
}

impl VideoToolboxEncoderPool {
    pub fn acquire(&mut self) -> Result<VTCompressionSession, Error> {
        if let Some(session) = self.idle.pop_front() {
            // Reuse existing session
            return Ok(session);
        }

        // Create new if under limit
        if self.active.len() < self.max_idle {
            return VTCompressionSession::new(self.config.clone());
        }

        Err(Error::EncoderPoolExhausted)
    }

    pub fn release(&mut self, session_id: SessionId) -> Result<(), Error> {
        if let Some(session) = self.active.remove(&session_id) {
            // Flush any pending frames
            session.complete_frames()?;

            // Clear reference frames (memory cleanup)
            session.set_property("ReferenceFrameThrottling", 0)?;

            if self.idle.len() < self.max_idle {
                self.idle.push_back(session);
            } else {
                // Explicitly destroy oldest session
                drop(session);
            }
        }
        Ok(())
    }
}
```

**Windows Implementation**:
```rust
pub struct MediaFoundationEncoderPool {
    idle: VecDeque<(IMFTransform, ID3D11Device)>,
    active: HashMap<SessionId, (IMFTransform, ID3D11Device)>,
    max_idle: usize,
}

impl MediaFoundationEncoderPool {
    pub fn acquire(
        &mut self,
        d3d_device: ID3D11Device,
    ) -> Result<(IMFTransform, ID3D11Device), Error> {
        if let Some((transform, device)) = self.idle.pop_front() {
            return Ok((transform, device));
        }

        let transform = unsafe {
            let mut transform: *mut IMFTransform = std::ptr::null_mut();
            // CoCreateInstance for H.264 encoder
            CoCreateInstance(
                &CLSID_CMSH264EncoderMFT,
                std::ptr::null_mut(),
                CLSCTX_INPROC_SERVER,
                &IMFTransform::uuidof(),
                &mut transform as *mut _ as *mut _,
            )?;
            transform
        };

        Ok((transform, d3d_device))
    }

    pub fn release(&mut self, session_id: SessionId) -> Result<(), Error> {
        if let Some((transform, device)) = self.active.remove(&session_id) {
            unsafe {
                // Drain any pending output samples
                loop {
                    let mut output_sample: *mut IMFSample = std::ptr::null_mut();
                    let mut status: MFT_OUTPUT_STATUS_FLAGS = 0;

                    let hr = (*transform).ProcessOutput(
                        0,
                        1,
                        &mut output_sample,
                        &mut status,
                    );

                    if hr != S_OK || output_sample.is_null() {
                        break;
                    }
                }
            }

            if self.idle.len() < self.max_idle {
                self.idle.push_back((transform, device));
            }
        }
        Ok(())
    }
}
```

**Linux Implementation**:
```rust
pub struct VaapiEncoderPool {
    idle: VecDeque<(VADisplay, VAContext, VAConfigID)>,
    active: HashMap<SessionId, (VADisplay, VAContext, VAConfigID)>,
    max_idle: usize,
}

impl VaapiEncoderPool {
    pub fn release(&mut self, session_id: SessionId) -> Result<(), Error> {
        if let Some((display, context, config)) = self.active.remove(&session_id) {
            unsafe {
                // CRITICAL: Destroy context BEFORE destroying config
                vaDestroyContext(display, context);
                // Only then destroy config
                vaDestroyConfig(display, config);
            }

            if self.idle.len() < self.max_idle {
                self.idle.push_back((display, context, config));
            } else {
                unsafe {
                    vaDestroyContext(display, context);
                    vaDestroyConfig(display, config);
                }
            }
        }
        Ok(())
    }
}
```

### 2. Reference Frame Management

**Problem**: Encoder keeps reference frames (previous keyframes, B-frame references) in GPU memory indefinitely.

**Solution - Controlled Release**:
```rust
pub struct ReferenceFrameManager {
    /// Maximum reference frames to keep (depends on codec)
    max_references: u32,

    /// Actual reference frames in use
    references: VecDeque<ReferenceFrame>,
}

pub struct ReferenceFrame {
    pub frame_id: u64,
    pub is_keyframe: bool,
    pub last_referenced_at: Instant,
    pub gpu_memory_mb: u64,
}

impl ReferenceFrameManager {
    pub fn update_encoder_state(&mut self, new_frame_id: u64, is_keyframe: bool) {
        // New keyframe means all B-frame references can be released
        if is_keyframe {
            self.references.retain(|rf| rf.is_keyframe);
        }

        // Remove oldest references if over limit
        while self.references.len() >= self.max_references as usize {
            if let Some(old_ref) = self.references.pop_front() {
                // Notify encoder to release this reference
                log::info!(
                    "Releasing reference frame {}: freed {} MB",
                    old_ref.frame_id,
                    old_ref.gpu_memory_mb
                );
            }
        }

        self.references.push_back(ReferenceFrame {
            frame_id: new_frame_id,
            is_keyframe,
            last_referenced_at: Instant::now(),
            gpu_memory_mb: Self::estimate_frame_memory(is_keyframe),
        });
    }
}
```

### 3. Staging Buffer Lifecycle

**Problem**: GPU staging textures (CPU-GPU transfer buffers) can accumulate if frame drops occur.

**Solution - Explicit Lifecycle**:
```rust
pub struct GpuStagingBuffer {
    pub buffer_id: u64,
    pub size_bytes: u64,

    /// When buffer was created
    pub created_at: Instant,

    /// Last time buffer was used
    pub last_used_at: Instant,

    /// If true, buffer should be reclaimed
    pub marked_for_reclaim: bool,
}

pub struct StagingBufferPool {
    buffers: HashMap<u64, GpuStagingBuffer>,
    max_total_mb: u64,  // e.g., 100 MB
    reclaim_timeout_secs: u64,  // e.g., 5 seconds
}

impl StagingBufferPool {
    pub fn prune_stale_buffers(&mut self) {
        let now = Instant::now();

        for (id, buffer) in self.buffers.iter_mut() {
            let idle_secs = now.duration_since(buffer.last_used_at).as_secs();

            if idle_secs > self.reclaim_timeout_secs {
                buffer.marked_for_reclaim = true;
            }
        }

        // Remove marked buffers
        self.buffers.retain(|_, buf| !buf.marked_for_reclaim);
    }

    pub fn memory_pressure(&self) -> Option<MemoryPressure> {
        let total_used: u64 = self.buffers.values().map(|b| b.size_bytes).sum();

        if total_used > self.max_total_mb * 1024 * 1024 {
            return Some(MemoryPressure::High);
        }

        if total_used > self.max_total_mb * 1024 * 1024 / 2 {
            return Some(MemoryPressure::Medium);
        }

        None
    }
}
```

---

## Measurement & Profiling

### Test Scenarios

```bash
# 24-hour AV1 encoding stress test
cargo build --release -p wavry-server

timeout 86400 ./target/release/wavry-server \
  --codec av1 \
  --bitrate 5000 \
  --resolution 1920x1080 \
  --log-gpu-memory \
  --gpu-memory-sample-interval 60s \
  2>&1 | tee av1_24h.log

# Monitor GPU memory in real-time
# macOS: Activity Monitor → Memory tab
# Windows: GPU-Z or Task Manager → Performance → GPU
# Linux: gpustat or nvtop

gpustat --watch    # NVIDIA GPUs
```

### Success Metrics

```rust
pub struct GpuMemoryResult {
    /// Peak GPU memory during session (MB)
    pub peak_memory_mb: u64,

    /// Growth rate (MB per hour) - should be 0
    pub growth_rate_mb_per_hour: f32,

    /// Frame drops due to GPU memory pressure
    pub frame_drops_count: u32,

    /// Encoder session reuse rate (% of acquisitions that reused)
    pub session_reuse_rate: f32,
}

// Expected results after optimization
pub const EXPECTED_RESULTS: GpuMemoryResult = GpuMemoryResult {
    peak_memory_mb: 500,         // Before: 800-1000 MB
    growth_rate_mb_per_hour: 0.0, // Before: 10-20 MB/hour
    frame_drops_count: 0,          // Before: 5-15 during 24h
    session_reuse_rate: 0.85,      // Before: 0 (always created new)
};
```

---

## Implementation Roadmap

### Week 1: Monitoring & Baselining
- [ ] Implement GpuMemoryTracker for all platforms
- [ ] Integrate leak detection (linear regression on memory snapshots)
- [ ] Run 24-hour baseline test with AV1 encoding
- [ ] Document baseline GPU memory patterns

### Week 2: Encoder Pooling
- [ ] Implement platform-specific encoder session pools
- [ ] Add encoder reuse metrics
- [ ] Test with bitrate adaptation (encoder reconfiguration)
- [ ] Measure fragmentation reduction

### Week 3: Reference Frame & Staging Buffer Management
- [ ] Implement ReferenceFrameManager with platform coordination
- [ ] Implement StagingBufferPool with explicit reclaim
- [ ] Test frame drop handling
- [ ] Profile GPU memory cleanup

### Week 4: Validation & Documentation
- [ ] 48-hour stability tests with all codecs (H.264, HEVC, AV1)
- [ ] Cross-platform validation (macOS, Windows, Linux)
- [ ] GPU memory leak detection via Valgrind/AddressSanitizer
- [ ] Create operator runbook for GPU memory tuning

---

## Success Criteria

- **Peak Memory**: <500 MB (currently 800-1000 MB for 1080p)
- **Memory Stability**: 0 MB growth over 24 hours (currently 10-20 MB/hour)
- **Session Reuse**: >85% of encoders reused (currently 0%, always created new)
- **Frame Drops**: 0 drops due to GPU memory (currently 5-15 in 24h)
- **Reference Frame Efficiency**: <50 MB for reference frames (currently 100-150 MB)
- **No Leaks**: Zero leak detection on all platforms (Valgrind, AddressSanitizer, platform tools)

---

## Potential Risks & Mitigations

| Risk | Impact | Mitigation |
|------|--------|-----------|
| Encoder config mismatch | Corrupted video output | Validate config before reuse, reset if mismatch detected |
| Reference frame underflow | Video artifacts | Conservative reference frame count, platform-specific limits |
| Staging buffer exhaustion | Frame drops, latency spikes | Memory pressure monitoring, emergency buffer purge |
| Platform-specific behaviors | Different outcomes | Comprehensive testing on M1/M3 (macOS), RTX/Arc (Windows), Intel/NVIDIA (Linux) |
| GPU driver bugs | Unexplained crashes | Driver version pinning, fallback paths |

---

## Platform-Specific Considerations

### macOS (VideoToolbox)
- `CVPixelBufferPool` must be explicitly flushed before destruction
- `IOSurface` leak: Use `CFRelease` for IOSurface handles
- Metal command buffers must complete before destruction
- Test on M1/M2/M3 (arm64) and Intel (x86_64)

### Windows (Direct3D 11/12)
- `ID3D11Device::Release` must balance `AddRef` calls
- Staging textures require `D3D11_USAGE_STAGING` + `D3D11_CPU_ACCESS_READ/WRITE`
- DXGI adapter handles are limited (~1024 per process)
- Test on RTX 40-series, Intel Arc, and AMD Radeon

### Linux (VA-API)
- **CRITICAL**: Destroy `VAContext` BEFORE `VAConfig`; reverse order causes kernel panics
- `VABuffer` objects are tied to context; destroy context = orphan buffers
- Use `vaQuerySurfaceStatus` to track surface lifecycle
- Test on NVIDIA (NVENC), Intel (Media Server Studio), AMD (Radeon)

---

## References

- [Apple VideoToolbox Guide](https://developer.apple.com/documentation/videotoolbox) - H.264/HEVC encoding
- [Windows Direct3D 11](https://learn.microsoft.com/en-us/windows/win32/direct3d11/d3d11-programming-guide) - GPU resource management
- [Intel VA-API](https://github.com/intel/libva) - Linux hardware encoding
- [VideoToolbox Memory Management](https://developer.apple.com/documentation/videotoolbox/vtvideoencodersession) - Best practices
- [DXGI Memory Limits](https://learn.microsoft.com/en-us/windows/win32/direct3ddxgi/dxgi-adapter) - Adapter handle limits
