# Wavry TODO (Outstanding Work Only)

Current target version: `v0.0.5-unstable`  
Last updated: 2026-02-13

## Priority 0: Release-Blocking

- [ ] Fix Linux Wayland protocol stability so desktop capture never crashes with `Gdk-Message ... Error 71 (Protocol Error)` under KDE Plasma.
- [ ] Add robust Wayland capture fallback flow: portal readiness checks, clearer user-facing error states, and safe retry behavior.
- [ ] Validate Linux streaming + capture + input on KDE Plasma, GNOME, and Sway/Hyprland with test evidence documented.
- [ ] Close remaining Windows build regressions (including thread-safety around Windows audio capture types) and keep Windows CI green.
- [x] Enforce prerelease policy in tooling and CI so only `-canary` is accepted for prerelease tags.
- [x] Keep `-unstable` for internal/development versioning only (not release tag channels).
- [x] Ensure `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`, and `cargo test --workspace` are required CI gates for merge.
- [x] Add a CI step that fails if release packaging emits unnamed or ambiguous artifacts.
- [x] Ensure release artifacts are deterministic, minimal, and consistently named across all platforms.
- [x] Generate and publish checksums (`SHA256SUMS`) for every release artifact.
- [x] Generate and publish a machine-readable release manifest (`release-manifest.json`) listing file name, platform, arch, and checksum.
- [x] Remove unnecessary release artifacts from CI upload/publish jobs.
- [x] Fix macOS desktop artifact workflow to handle missing DMG directories safely without failing unrelated jobs.
- [x] Confirm no macOS Tauri release artifacts are produced if Swift-only macOS policy is adopted.

## Priority 0: Control Plane (Master + Relay + Auth)

- [ ] Make relay and auth deployment Docker-first and production-supported only through containerized paths.
- [x] Harden relay session lifecycle: lease validation, expiration behavior, reconnect behavior, and cleanup under churn.
- [ ] Add relay overload protection: per-IP and per-identity rate limiting, bounded queues, and backpressure metrics.
- [x] Add stronger control-plane auth boundaries between gateway/master/relay (service identity validation and explicit trust model).
- [x] Add control-plane chaos/failure tests: relay restart, master restart, packet loss, and high latency.
- [x] Add load and soak tests for relay/master with success thresholds and regression baselines.
- [x] Document operational runbooks for relay/master incidents and recovery steps.

## Priority 0: Docker Build/CI Speed + Reliability

- [x] Improve Docker build times with BuildKit cache mounts and remote cache reuse across CI runs.
- [x] Split Dockerfiles into stable dependency layers and frequently changing app layers.
- [x] Pin base images by digest and enable periodic update workflow with review.
- [x] Parallelize control-plane image builds where safe and reduce redundant rebuilds.
- [x] Add CI telemetry for build durations and cache hit rates, then enforce performance budgets.
- [x] Add smoke tests for produced Docker images before publish.

## Priority 1: Linux-First Product Quality

- [ ] Expand Linux-specific diagnostics to include compositor, portal service status, PipeWire session state, and encoder availability.
- [ ] Implement Linux integration tests that exercise real capture paths (Wayland portal + PipeWire) in CI or nightly runners.
- [ ] Improve Linux input parity and edge-case handling (gamepad hotplug, key mapping edge cases, clipboard behavior).
- [ ] Improve Linux audio routing coverage with explicit tests for default/mic/app routes.
- [ ] Add Linux packaging QA for AppImage/tarball/deb/rpm targets with install/uninstall checks.
- [ ] Add Linux performance profiling for encode/capture/network hot paths and track regressions.

## Priority 1: Security Hardening

- [x] Update threat model docs for relay/master/auth attack surfaces and trust boundaries.
- [x] Add fuzzing targets for control-plane message parsing and relay session state transitions.
- [x] Add structured security logging and audit events for control-plane auth failures and policy denials.
- [x] Add dependency and supply-chain hardening checks in CI (pinned actions, audit gates, SBOM generation).
- [x] Add release signing/attestation plan and implementation for binaries and container images.
- [x] Document key rotation and secret management procedures for production deployments.

## Priority 1: Website + Documentation

- [x] Keep website docs-first, with no oversized marketing landing experience.
- [x] Ensure theme consistency across all pages, including pricing cards and docs components.
- [x] Remove any text highlight/hover behavior that turns content unreadable (for example black-on-dark issues).
- [x] Fix footer alignment and responsive spacing across breakpoints.
- [x] Fix dark mode toggle behavior so it does not break layout or theming.
- [x] Remove mobile sidebar trigger if it conflicts with required UX, and keep navigation usable on small screens.
- [x] Fix docs separators and content container alignment on all docs pages.
- [x] Expand docs for relay/master architecture, data flow, failure modes, and scaling guidance.
- [x] Expand Linux documentation with verified setup and troubleshooting for Wayland/PipeWire/portals.
- [x] Expand operations docs with deployment topology, backup/restore, and incident response.
- [x] Expand developer docs with codebase map, local env bootstrap, and contribution workflow.
- [x] Keep licensing docs clear: AGPL/RIFT details, CLA requirements, and commercial terms.
- [x] Keep pricing page clear and actionable with contact: `contact@wavry.dev`.
- [x] Add explicit SaaS/integration licensing requirement: users must contact for terms.

## Priority 1: Product/Platform Decisions

- [x] Finalize and enforce macOS client strategy: Swift-only or dual-client; remove conflicting CI/release paths.
- [x] Define supported platform matrix and support policy (stable/beta/experimental) in docs.
- [x] Define release channels and versioning policy (`stable`, `canary`, internal `unstable`) and enforce in scripts/workflows.
- [x] Create a strict release checklist and gate publishing on checklist completion.

## Priority 2: Performance + Reliability Roadmap

- [ ] Add adaptive resolution/bandwidth control roadmap with concrete milestones.
- [ ] Add end-to-end latency budget instrumentation and reporting.
- [ ] Add long-session memory stability tests and leak detection automation.
- [ ] Add reconnect/migration test cases for unstable network conditions.
- [ ] Add optional advanced transport experiments behind feature flags.

## Acceptance Criteria Before Next Public Release

- [ ] Linux Wayland capture/session flow is stable across target compositors with documented test evidence.
- [x] Control-plane soak tests pass with defined SLOs.
- [ ] Release artifacts are clean, labeled, checksummed, and minimal.
- [ ] CI is warning-free and green across required targets.
- [ ] Docs are complete enough for install, operate, troubleshoot, and scale without source spelunking.
