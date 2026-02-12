# AV1 Performance Hardening - Validation Report

**Status**: In Progress (macOS local validation complete, cross-platform matrix pending)
**Last Updated**: 2026-02-12
**Target Platforms**: Apple M3 (macOS), Intel ARC (Windows/Linux)

---

## Overview

This document tracks the validation of AV1 hardware acceleration across target platforms. AV1 is the next-generation video codec offering superior compression to HEVC, critical for bandwidth-constrained remote streaming scenarios.

## Latest Validation Run (Local Mac)

**Date**: 2026-02-12  
**Host**: Apple M4, macOS 26.3 (build 25D122)  
**Command**: `./scripts/av1-hardware-smoke.sh`
**Report**: `docs/HWTEST_RESULTS.md`

### Observed Results

- macOS probe tests passed:
  - `mac_probe_always_reports_h264`
  - `mac_probe_av1_visibility_matches_hardware_support`
  - `mac_probe_av1_is_hardware_accelerated_when_present`
- `wavry-server` startup capability sampling reported:
  - `Local encoder candidates: [Hevc, H264]`
- AV1 was **not** advertised in realtime encoder candidates on this host.
- Realtime fallback behavior is correct (HEVC/H.264 remain available).

### Operational Meaning

- AV1 should remain optional for realtime streaming.
- Host selection logic is correctly avoiding software-only AV1 paths for realtime sessions.
- No code changes are required for fallback behavior; ongoing validation focuses on platforms where hardware AV1 is available.

## Current Implementation Status

### macOS (Apple M3 + VideoToolbox)

**File**: `crates/wavry-media/src/mac_screen_encoder.rs`

**Status**: ✅ Implemented, ⚠️ Partial 10-bit Support

#### Details
- **Codec Type**: `K_CMVIDEO_CODEC_TYPE_AV1` (0x61763031 = 'av01')
- **Hardware Detection**: Via `VTIsHardwareEncodeSupported()` (line 370-384)
- **Supported since**: macOS 13+
- **M3 Availability**: Yes (Metal 3 with AV1 encode support)

#### Code Analysis (line 489-497)
```rust
// 10-bit and HDR support
if config.enable_10bit && config.codec == Codec::Hevc {
    VTSessionSetProperty(
        session,
        kVTCompressionPropertyKey_ProfileLevel,
        kVTProfileLevel_HEVC_Main10_AutoLevel,
    );
    // AV1 10-bit is often implicit in AutoLevel or requires specific profile keys
    // which vary by macOS version.
}
```

**Issue**: AV1 10-bit configuration not explicitly handled.
**Reason**: `kVTProfileLevel_AV1_Main10_AutoLevel` may not exist in all macOS versions.

#### Validation Checklist
- [x] Verify hardware encoding detection path executes on local Apple Silicon host
- [x] Verify realtime codec fallback excludes unavailable AV1
- [ ] Test 1080p60 at 5-10 Mbps with AV1
- [ ] Compare thermal profile vs HEVC (5-min sustained)
- [ ] Verify 10-bit handling (implicit AutoLevel)
- [ ] Test fallback to HEVC/H.264 on older Macs

---

### Linux (Intel ARC + VA-API + GStreamer)

**File**: `crates/wavry-media/src/linux.rs`

**Status**: ✅ Implemented, ⚠️ Limited Platform Specific Tuning

#### Encoding Chain
```
Hardware VA-API:
- vaapav1enc (Intel ARC recommended)
- vaapivav1enc (Intel Iris alternative)

Software Fallback:
- svtav1enc (SVT-AV1, fastest)
- av1enc (libav1)
- rav1enc (rav1e, slower)
```

#### Current Code (line ~240-290)
```rust
Codec::Av1 => "av1parse",
Codec::Av1 => "video/x-av1,stream-format=(string)obu-stream,alignment=(string)tu",
Codec::Av1 => &["vaapav1enc", "vaapivav1enc", "nvav1enc"],
```

**Issues**:
1. No platform-specific tuning for Intel ARC
2. No explicit check for VA-API driver availability
3. Profile level not explicitly set

#### Validation Checklist
- [ ] Verify `vaapav1enc` element available on ARC system
- [ ] Test encoding latency (< 50ms per frame at 1080p60)
- [ ] Validate bitrate control accuracy
- [ ] Test keyframe insertion under congestion
- [ ] Profile CPU vs GPU utilization
- [ ] Test fallback chain (hw → sw → H.264)

---

### Windows (Intel ARC + Media Foundation)

**File**: `crates/wavry-media/src/windows.rs`

**Status**: ✅ Basic Support, ⚠️ ARC Specific Tuning Needed

#### Codec
- **Format**: `MFVideoFormat_AV1`
- **Requires**: Windows 11 22H2+
- **ARC Support**: Yes (via Intel Media Driver)

#### Validation Checklist
- [ ] Verify encoder available on ARC system
- [ ] Test real-time encoding capability
- [ ] Compare performance vs Linux equivalent
- [ ] Test quality/bitrate trade-offs

---

## Hardware Detection Strategy

### Detection Order
1. **Probe Phase**: Call `supported_encoders()` on platform probe
   - macOS: `VTIsHardwareEncodeSupported(K_CMVIDEO_CODEC_TYPE_AV1)`
   - Linux: Try GStreamer element creation with timeout
   - Windows: Query Media Foundation factory

2. **Fallback Chain**:
   ```
   Preferred (AV1) → Fallback 1 (HEVC) → Fallback 2 (H.264)
   ```

3. **Capability Query**:
   ```rust
   pub fn encoder_capabilities(&self) -> Result<Vec<VideoCodecCapability>>
   ```
   Returns: codec, hardware_accelerated flag, 10-bit/HDR support

---

## Performance Benchmarking

### Metrics to Track

#### 1. Encoding Latency
- **Definition**: Time from raw frame input to complete encoded packet
- **Target**: < 50ms (1080p60)
- **Method**: Timestamp difference in encoder

#### 2. Bitrate Efficiency
- **Target SSIM**: > 0.95 vs HEVC at equivalent bitrate
- **Bitrate**: 5-10 Mbps for 1080p60 with AV1
- **Method**: Stream actual content, measure packet times

#### 3. CPU/GPU Utilization
- **Target**: > 80% GPU utilization on hardware encoders
- **macOS**: Use `Activity Monitor` or `Xcode Instruments`
- **Linux**: Use `nvidia-smi`, `intel-gpu-top`, or `radeontop`
- **Windows**: Task Manager GPU metrics

#### 4. Thermal/Power
- **macOS**: Sustained < 65°C on M3
- **Duration**: 5-minute continuous stream
- **Method**: `powermetrics` on macOS

---

## Test Harness

### Unit Tests
```rust
#[cfg(test)]
mod tests {
    #[test]
    fn test_av1_hardware_detection_m3() {
        // macOS only
        #[cfg(target_os = "macos")]
        {
            let probe = MacProbe;
            let encoders = probe.supported_encoders().unwrap();
            assert!(encoders.contains(&Codec::Av1), "AV1 should be supported on M3");
        }
    }

    #[test]
    fn test_av1_encoding_latency() {
        // Test that encoding completes in < 50ms
    }

    #[test]
    fn test_fallback_chain() {
        // Ensure fallback to HEVC/H.264 works
    }
}
```

### Integration Tests
- Create 10-second test stream at 1080p60
- Measure codec selection and performance
- Verify no resource leaks

---

## Known Limitations

### macOS
- AV1 10-bit profile key varies by macOS version (13.x, 14.x, 15.x)
- No explicit profile constant in current bindings
- AutoLevel may not explicitly enable 10-bit

### Linux
- Intel Arc driver maturity varies by distro
- VA-API requires proper driver installation
- GStreamer plugin availability not guaranteed

### Windows
- Requires Windows 11 22H2+
- Intel Media Driver must be up-to-date
- Media Foundation AV1 is relatively new

---

## Validation Checklist

### Phase 1: Basic Detection (Week 1)
- [x] Local Apple Silicon host: Confirm hardware detection + fallback chain visibility
- [ ] Intel ARC Linux: Verify encoder element available
- [ ] Intel ARC Windows: Confirm Media Foundation support
- [ ] Fallback: Test graceful degradation on unsupported systems

### Phase 2: Performance (Week 2)
- [ ] M3: Measure latency vs HEVC
- [ ] M3: Sustained thermal test (5 min)
- [ ] ARC: Bitrate efficiency test
- [ ] ARC: Latency under congestion

### Phase 3: Quality (Week 3)
- [ ] SSIM measurements vs HEVC
- [ ] Real-world streaming test (1 hour)
- [ ] Monitor for crashes/leaks

### Phase 4: Documentation (Week 4)
- [ ] Update codec selection logic
- [ ] Document hardware requirements
- [ ] Create user-facing settings

---

## Success Criteria

1. **Hardware Detection**
   - All target platforms correctly identify AV1 capability
   - Unsupported platforms gracefully fall back

2. **Performance**
   - Encoding latency < 50ms (1080p60)
   - No significant CPU/memory regression vs HEVC

3. **Reliability**
   - 30-minute sustained stream without crashes
   - Proper cleanup on abnormal termination

4. **Quality**
   - Bitrate efficiency ≥ HEVC (or documented tradeoff)
   - Keyframe handling under network congestion

---

## Related Code

- **Codec Probe Trait**: `crates/wavry-media/src/lib.rs:113-134`
- **EncodeConfig**: `crates/wavry-media/src/lib.rs:59-69`
- **VideoCodecCapability**: `crates/wavry-media/src/lib.rs:94-111`

---

## References

- [AV1 Codec Specification](https://aomediacodec.org/av1-specification/)
- [Apple VideoToolbox AV1 Support](https://developer.apple.com/documentation/videotoolbox)
- [Intel Media Driver (iHD)](https://github.com/intel/media-driver)
- [libva Documentation](https://github.com/intel/libva)
