# ALVR Source Map (Vendored Subset)

Pinned commit: `1a4194ff137937c0a4f416ad2d6d1acedb851e8a`

## Extracted modules

- `alvr/common/`
  - Purpose: common math / pose / input primitives (Pose, Fov, controller inputs)
  - Needed by: OpenXR/SteamVR adapter

- `alvr/graphics/`
  - Purpose: frame submission helpers and GPU staging utilities
  - Needed by: OpenXR/SteamVR adapter (frame presentation glue)

- `alvr/client_openxr/`
  - Purpose: OpenXR runtime integration (headset pose + frame submission)
  - NOTE: Networking portions must be removed/disabled; Wavry owns transport

- `alvr/server_openvr/`
  - Purpose: SteamVR/OpenVR driver integration (pose + compositor bridge)
  - NOTE: Networking portions must be removed/disabled; Wavry owns transport

- `alvr/vrcompositor_wrapper/`
  - Purpose: compositor glue required by SteamVR on Linux

- `openvr/`
  - Purpose: OpenVR headers / SDK glue

- `alvr/system_info/`
  - Purpose: platform detection helpers used by OpenXR code

- `alvr/session/`
  - Purpose: encoder/timing configuration patterns (optional use)

## Non‑extracted (explicitly excluded)

- `alvr/sockets/`
- `alvr/packets/`
- `alvr/client_core/`
- `alvr/server_core/`
- `alvr/server_io/`

These are network/transport related and forbidden for this integration.
