# Wavry + ALVR Adapter (Hybrid Architecture)

**Status:** Integration scaffolded, transport remains Wavry/RIFT.
**Scope:** ALVR is a VR adapter only. No ALVR networking.

---

## Goals

- ALVR provides VR runtime integration (OpenXR/SteamVR), pose pipeline, and frame submission hooks.
- Wavry owns transport, timing, pacing, congestion control, NACK/retransmission, jitter buffer, and session control.
- Deterministic builds: ALVR pinned to a known commit and vendored locally.

---

## Repo Layout

- `third_party/alvr/`
  - Vendored ALVR subset (pinned commit).
  - `COMMIT`, `LICENSE`, `SOURCE_MAP.md`.
- `crates/wavry-vr/`
  - Transport‑agnostic VR adapter traits and shared types.
- `crates/wavry-vr-alvr/`
  - ALVR adapter implementation (feature‑gated; no transport ownership).

Runtime enablement:
- Client (Linux/Windows): `wavry-client --vr`
- Build: `wavry-vr-alvr` compiled with feature `alvr` (enabled by default in client).

---

## ALVR Modules Extracted

Pinned to commit `1a4194ff137937c0a4f416ad2d6d1acedb851e8a`.

Extracted subset (see `third_party/alvr/SOURCE_MAP.md`):
- `alvr/common/`
- `alvr/graphics/`
- `alvr/client_openxr/`
- `alvr/server_openvr/`
- `alvr/vrcompositor_wrapper/`
- `openvr/`
- `alvr/system_info/`
- `alvr/session/` (optional encoder/timing patterns)

**Excluded (network/transport):**
- `alvr/sockets/`
- `alvr/packets/`
- `alvr/client_core/`
- `alvr/server_core/`
- `alvr/server_io/`

---

## Adapter Boundary

`crates/wavry-vr/src/adapter.rs`

### ALVR → Wavry
- `on_video_frame(frame, timestamp, frame_id)`
- `on_pose_update(pose, timestamp)`
- `on_vr_timing(hz, vsync_offset)`

### Wavry → ALVR
- `on_network_stats(rtt, jitter, loss)`
- `on_encoder_control(skip_frames)`
- `configure_stream(codec, width, height)`

ALVR never touches sockets. Wavry never touches OpenXR/SteamVR.

---

## RIFT Additions

Control messages added for VR integration:
- `PoseUpdate` (timestamp + position + orientation)
- `VrTiming` (refresh rate + vsync offset)

These are used for pose delivery and runtime timing hints. Pose packets are prioritized and bypass jitter buffering.

---

## VR Packet Prioritization

- **Pose:** ultra‑high priority, no jitter buffer
- **Input:** immediate, no jitter buffer
- **Media (video):** adaptive paced + jitter buffer

**DSCP:**
- Pose + Input: `EF (0x2E)`
- Media: `EF (0x2E)` or `CS6 (0x30)`

---

## First Working VR Pipeline

1. Host (PC / server)
   - Encoded frames produced by Wavry encoder
   - RIFT transport with adaptive pacing, NACK, retransmit

2. Client (Linux/Windows PCVR)
   - OpenXR integration via ALVR adapter
     Linux: OpenGL on X11, Vulkan on Wayland
     Windows: D3D11
   - RIFT decode + jitter buffer
   - Frame submission to OpenXR swapchain (mono frame to both eyes in initial milestone)
   - Pose updates sent over RIFT (highest priority)

Target milestone:
- 720p @ 72/90Hz
- Pose streaming functional
- Video visible in headset
- Pacing + NACK + jitter buffer active

---

## License Attribution

- Root `THIRD_PARTY.md`
- `third_party/alvr/LICENSE`
- Each vendored ALVR source file is marked with:
  - `// Derived from ALVR (MIT)`
  - `// Original copyright preserved`

---

## Update Procedure

Use the vendor script (pinned commit by default):

```
./scripts/vendor_alvr.sh
```

To change commit (explicitly):

```
./scripts/vendor_alvr.sh <commit>
```

Any commit changes must be reflected in:
- `third_party/alvr/COMMIT`
- `THIRD_PARTY.md`
