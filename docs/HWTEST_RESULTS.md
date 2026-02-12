# Hardware Test Results

## 2026-02-12 - Apple M4 (macOS 26.3)

### Environment

- Host: Apple M4
- OS: macOS 26.3 (25D122)
- Wavry: v0.0.4-unstable

### Commands

```bash
./scripts/av1-hardware-smoke.sh
```

### Results

- macOS codec probe tests:
  - `mac_probe_always_reports_h264`: pass
  - `mac_probe_av1_visibility_matches_hardware_support`: pass
  - `mac_probe_av1_is_hardware_accelerated_when_present`: pass
- Realtime host capability sample:
  - `Local encoder candidates: [Hevc, H264]`
- AV1 availability:
  - AV1 is not currently exposed as a realtime encoder candidate on this host.
- Fallback behavior:
  - Realtime codec fallback to HEVC/H.264 is functioning as designed.

### Conclusion

- AV1 realtime path is validated as **unavailable** on this hardware/software combination.
- The host behaves safely by excluding AV1 and continuing with supported codecs.
