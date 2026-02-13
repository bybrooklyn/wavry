# Wavry Platform Support Policy

This matrix defines current support levels and release expectations.

## Support Tiers

- `Stable`: production-ready, default recommendation.
- `Beta`: broadly usable, but may have regressions under some environments.
- `Experimental`: active development; behavior and compatibility may change.

## Current Matrix

| Platform | Client Runtime | Support Tier | Notes |
|---|---|---|---|
| Linux (desktop) | Tauri (`wavry-desktop`) | Beta | Wayland stability hardening is still in progress. |
| Windows (desktop) | Tauri (`wavry-desktop`) | Beta | Ongoing CI/build hardening for audio/capture edge cases. |
| macOS (desktop) | Native Swift app | Beta | Primary macOS desktop client path. |
| macOS (desktop, Tauri) | Tauri (`wavry-desktop`) | Unsupported | Not produced in release artifacts. |
| Android mobile | Native Android app | Experimental | Release APKs published; UX/perf still evolving. |
| Android Quest | Native Android app | Experimental | VR-focused path, still under active iteration. |
| Web client | `wavry-web` / web-reference | Experimental | Reference implementation and transport experimentation. |

## macOS Client Strategy

- The enforced strategy is **Swift-native macOS desktop**.
- macOS Tauri artifacts are not part of release outputs.
- CI release validation rejects any `wavry-desktop-tauri-macos-*` artifacts.

## Policy Notes

- Promotion from Beta/Experimental to Stable requires:
  - consistent CI signal,
  - validated platform test evidence,
  - release checklist completion.
