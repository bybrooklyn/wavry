# ALVR (vendored subset)

This directory vendors a **minimal subset** of ALVR source code strictly for VR runtime integration (OpenXR / SteamVR). It is **not** a fork and **does not** include ALVR networking or transport logic.

Pinned commit:
- `1a4194ff137937c0a4f416ad2d6d1acedb851e8a`

Rules:
- Do NOT track HEAD.
- Do NOT enable ALVR networking or sockets.
- Wavry/RIFT owns transport, pacing, NACK, jitter buffer, and session control.

See `SOURCE_MAP.md` for the extracted files and their purpose.
See `CHANGES.md` for the precise list of local modifications.
