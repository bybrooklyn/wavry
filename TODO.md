# Wavry TODO (Full Execution Ledger)

Current target version: `v0.0.5-unstable`  
Last updated: 2026-02-13

## How To Use This File

- This document now tracks both completed work and remaining work.
- Completed items include commit references, scope, and validation evidence.
- Remaining items include explicit implementation steps, verification commands, and exit criteria.
- Any task is only considered done after code changes, tests, docs updates, and CI pass.

## Global CI/CD Baseline (Must Stay True)

- [x] Required local preflight before pushing:
  - `cargo fmt --all`
  - `cargo clippy --workspace --all-targets -- -D warnings`
  - `cargo test --workspace`
- [x] Required control-plane regression validation:
  - `./scripts/control-plane-resilience.sh`
- [x] Required desktop style regression validation:
  - `cd crates/wavry-desktop && bun run check:style`
- [x] CI workflows must remain deterministic and release-safe:
  - release artifact naming checks enabled
  - checksums/manifests generation enabled
  - prerelease channel policy enforced

---

## Completed Work Ledger (Done)

### [x] `559ab98` docs(todo): mark control-plane audit logging complete

- Scope:
  - Updated roadmap tracking so control-plane audit logging is explicitly captured as completed.
- Why this mattered:
  - Reduced ambiguity around whether auth failure telemetry and policy-denial audit trails were still outstanding.
- Validation:
  - Manual doc review for consistency with implemented security logging work.

### [x] `716d637` test(ci): add control-plane and relay fuzz smoke targets

- Scope:
  - Added CI-facing smoke coverage for fuzz targets related to control-plane and relay paths.
- Why this mattered:
  - Prevented silent regressions in parser/state-machine surfaces likely to fail under malformed inputs.
- Validation:
  - Targeted CI test execution for fuzz smoke jobs.

### [x] `a63c785` fix(ci): remove rg dependency from audit pin scripts

- Scope:
  - Removed hard dependency on `rg` from audit pin scripts used in CI.
- Why this mattered:
  - Eliminated environment-dependent failures where `rg` was unavailable in minimal CI runners.
- Validation:
  - Script execution in CI-compatible shell environment without `rg` dependency.

### [x] `c901a33` feat(control-plane): add chaos and soak resilience gates

- Scope:
  - Added resilience validation entrypoint and CI integration for control-plane reliability testing.
  - Added new scripts:
    - `scripts/control-plane-resilience.sh`
    - `scripts/lib/control-plane-load-driver.py`
    - `scripts/lib/tcp-chaos-proxy.py`
  - Added a dedicated resilience job in:
    - `.github/workflows/quality-gates.yml`
- Why this mattered:
  - Converted reliability expectations into enforceable gates instead of best-effort manual checks.
- Validation:
  - `./scripts/control-plane-resilience.sh`
  - CI run confirmation for quality gates job with resilience stage.

### [x] `7eb1168` feat(control-plane): add relay service identity token auth

- Scope:
  - Implemented optional bearer-token auth between relay and master.
  - Added env-based enforcement and propagation:
    - master validation: `WAVRY_MASTER_RELAY_AUTH_TOKEN`
    - relay outbound token: `WAVRY_RELAY_MASTER_TOKEN`
  - Files:
    - `crates/wavry-master/src/main.rs`
    - `crates/wavry-relay/src/main.rs`
- Why this mattered:
  - Established explicit control-plane trust boundaries instead of relying on network location assumptions.
- Validation:
  - Positive path with matching tokens.
  - Negative path with missing/mismatched token returns auth failure.

### [x] `b367aa5` feat(deploy): enforce docker-first public bind policy

- Scope:
  - Enforced container-first behavior for public-bind endpoints in gateway/master/relay.
  - Added host override env controls for explicit non-default usage.
  - Files:
    - `crates/wavry-gateway/src/main.rs`
    - `crates/wavry-gateway/src/relay.rs`
    - `crates/wavry-master/src/main.rs`
    - `crates/wavry-relay/src/main.rs`
- Why this mattered:
  - Reduced accidental unsafe public exposure in local/non-container deployments.
- Validation:
  - Runtime startup checks for bind mode with/without overrides.

### [x] `4f5f00c` fix(ci): avoid sybil-prefix collisions in soak endpoints

- Scope:
  - Fixed synthetic identity/address generation in soak tests to avoid master sybil-prefix false positives.
- Why this mattered:
  - Removed flaky failures in resilience CI runs caused by test data generation rather than real defects.
- Validation:
  - Re-ran control-plane resilience and verified stable registration/lease behavior.

### [x] `f5ecf15` feat(relay): add per-identity lease rate limiting

- Scope:
  - Added per-identity rate limiting guardrail with env configuration.
  - New knob:
    - `WAVRY_RELAY_IDENTITY_RATE_LIMIT_PPS`
  - File:
    - `crates/wavry-relay/src/main.rs`
- Why this mattered:
  - Prevented a single identity from monopolizing lease processing and degrading multi-tenant stability.
- Validation:
  - Unit/runtime checks for throttling behavior under burst load.

### [x] `4462c39` feat(relay): add bounded queue backpressure controls

- Scope:
  - Added bounded inbound packet queue capacity and explicit backpressure behavior.
  - New knob:
    - `WAVRY_RELAY_PACKET_QUEUE_CAPACITY`
  - Added metrics/logging for queue pressure and drops.
  - File:
    - `crates/wavry-relay/src/main.rs`
- Why this mattered:
  - Prevented unbounded memory growth and made overload behavior predictable/observable.
- Validation:
  - Stress conditions with queue saturation and expected throttling/drop telemetry.

### [x] `c280b34` chore(linux): expand runtime diagnostics smoke checks

- Scope:
  - Extended Linux smoke script coverage:
    - compositor detection
    - PipeWire service/session checks
    - portal checks
  - File:
    - `scripts/linux-display-smoke.sh`
- Why this mattered:
  - Improved first-response debugging for Linux capture failures before deep protocol investigation.
- Validation:
  - Script run on Linux runtime images with expected pass/fail diagnostics.

### [x] `2822843` docs(roadmap): define adaptive streaming milestones

- Scope:
  - Added milestone-based adaptive streaming roadmap doc:
    - `docs/ADAPTIVE_STREAMING_ROADMAP.md`
- Why this mattered:
  - Converted high-level performance ideas into phased, implementable milestones.
- Validation:
  - Doc cross-check with existing backlog and acceptance criteria.

### [x] `8558441` test(control-plane): add reconnect and failover migration checks

- Scope:
  - Added tests for reconnect and migration under unstable network/failover conditions.
  - Updated relay re-registration behavior after heartbeat failures/master restart.
  - File:
    - `crates/wavry-relay/src/main.rs`
- Why this mattered:
  - Raised confidence that sessions recover correctly during real-world control-plane disruptions.
- Validation:
  - Targeted tests and resilience script scenarios covering reconnect/migration paths.

### [x] Additional docs/runbook updates completed

- Updated:
  - `docs/CONTROL_PLANE_RUNBOOKS.md`
  - `docs/RELAY_OPERATIONS.md`
  - `docs/GATEWAY_OPERATIONS.md`
  - `docs/WAVRY_TESTING.md`
- Why this mattered:
  - Reduced operational guesswork during incidents and made recovery procedures explicit.

### [x] Repeated verification loop completed during this work stream

- Commands run repeatedly while implementing/fixing:
  - `cargo fmt --all`
  - `cargo clippy -p wavry-gateway -p wavry-master -p wavry-relay --all-targets -- -D warnings`
  - `cargo test -p wavry-gateway -p wavry-master -p wavry-relay -- --nocapture`
  - `./scripts/control-plane-resilience.sh`
- Outcome:
  - Addressed identified CI instability in resilience tests and re-established passing runs.

---

## Remaining Work (Not Done Yet)

## Priority 0: Release-Blocking

### [ ] Linux Wayland protocol stability (KDE/GNOME/Sway/Hyprland)

- Objective:
  - Eliminate protocol crashes such as `Gdk-Message ... Error 71 (Protocol Error)` in capture/session flows.
- Implementation steps:
  - Reproduce with deterministic matrix runs:
    - KDE Plasma (Wayland)
    - GNOME (Wayland)
    - Sway or Hyprland
  - Add structured error taxonomy in capture path:
    - protocol violation
    - portal unavailable
    - PipeWire node loss
    - compositor disconnect
  - Add guarded retry policy:
    - bounded retry attempts
    - cooldown/backoff between retries
    - hard stop with actionable user message
  - Add portal preflight and timeout surface in UI and logs.
  - Add regression tests for reconnect/capture restart after compositor and portal interruptions.
- Required code areas:
  - `crates/wavry-desktop/*`
  - `crates/wavry-media/*`
  - Linux integration scripts under `scripts/`
- Verification commands:
  - `cargo check --workspace`
  - `cargo test --workspace`
  - `./scripts/linux-display-smoke.sh`
  - platform-runner matrix jobs for compositor permutations
- Exit criteria:
  - No reproducible protocol crash across target compositors in 30+ repeated runs each.
  - Failure modes are recoverable or clearly surfaced with next-action guidance.

### [ ] Robust Wayland capture fallback UX and behavior

- Objective:
  - Ensure users always get clear remediation when capture cannot start.
- Implementation steps:
  - Add explicit fallback decision tree in desktop app:
    - portal missing
    - permission denied
    - PipeWire unavailable
    - unsupported compositor feature
  - Add user-facing copy for each failure mode with concrete resolution steps.
  - Add retry button/state reset behavior that does not require full app restart.
- Verification commands:
  - `cd crates/wavry-desktop && bun run check`
  - `cd crates/wavry-desktop && bun run check:style`
- Exit criteria:
  - Every capture failure path maps to deterministic UI state + actionable guidance.

### [ ] Windows build regressions + CI stability

- Objective:
  - Keep Windows build/test green, including thread-safety around audio capture types.
- Implementation steps:
  - Reproduce current Windows CI failures locally or in GH Windows runners.
  - Isolate send/sync violations and unsafe cross-thread captures.
  - Add tests for audio capture threading lifecycle (init/start/stop/drop).
  - Add targeted CI job that validates Windows audio capture crate boundaries.
- Verification commands:
  - `cargo check --workspace`
  - `cargo clippy --workspace --all-targets -- -D warnings`
  - Windows CI workflow run in GitHub Actions
- Exit criteria:
  - Windows required checks pass consistently for 10 consecutive runs.

### [ ] Final release artifact quality gate confirmation

- Objective:
  - Close remaining acceptance gap proving artifacts are minimal, labeled, and reproducible.
- Implementation steps:
  - Execute full release dry-run.
  - Confirm every artifact has:
    - deterministic filename
    - platform/arch label
    - checksum in `SHA256SUMS`
    - entry in `release-manifest.json`
  - Confirm no extra files are uploaded by release workflows.
- Verification commands:
  - release workflow dry-run job
  - artifact inspection script in CI
- Exit criteria:
  - Release dry-run passes without manual corrections.

## Priority 0: CI Reliability and Release Cadence (Critical for “don’t fail” objective)

### [ ] Add CI flake triage and quarantine process

- Objective:
  - Prevent intermittent failures from blocking safe releases.
- Implementation steps:
  - Tag known flaky tests and move to quarantine workflow.
  - Keep merge-gating jobs deterministic and stable.
  - Add weekly flake burn-down issue with owners.
- Verification:
  - CI failure rate trend monitored over 2+ weeks.
- Exit criteria:
  - Non-code-related CI failures stay below agreed threshold.

### [ ] Add release train automation with explicit go/no-go gates

- Objective:
  - Ensure releases are produced on schedule with auditable decision points.
- Implementation steps:
  - Define release cadence (weekly/biweekly).
  - Add release-train workflow requiring:
    - green required checks
    - changelog present
    - manifest/checksum/signature pass
  - Auto-create release PR with checklist.
- Verification:
  - One full dry-run release train and one real release.
- Exit criteria:
  - Release can be cut from automation with zero manual patching.

### [ ] Add rollback verification for every release

- Objective:
  - Ensure failed release deployment can be reverted quickly and safely.
- Implementation steps:
  - Add rollback script/playbook per target platform.
  - Test rollback in staging from latest release candidate.
- Exit criteria:
  - Documented and tested rollback under 15 minutes.

## Priority 1: Linux-First Product Quality

### [ ] Linux real-capture integration tests in CI/nightly

- Implementation steps:
  - Add nightly runner with Wayland + PipeWire + portal stack.
  - Automate session creation, capture start, short stream, teardown.
  - Store logs/artifacts for failure forensics.
- Verification:
  - nightly workflow success trend and reproducible logs.

### [ ] Linux input parity and edge-case handling

- Implementation steps:
  - Cover gamepad hotplug, keyboard layout mapping, clipboard transitions.
  - Add integration tests and device-matrix notes.
- Verification:
  - device scenario matrix completed with pass evidence.

### [ ] Linux audio routing coverage

- Implementation steps:
  - Add tests for default sink/source and app-specific routing selection.
  - Validate route changes during active session.
- Verification:
  - route-switch tests pass without session restart.

### [ ] Linux packaging QA

- Implementation steps:
  - Add install/uninstall smoke tests for AppImage/tarball/deb/rpm.
  - Verify desktop integration and dependency checks.
- Verification:
  - packaging matrix job with pass artifacts.

### [ ] Linux performance regression tracking

- Implementation steps:
  - Add benchmark scenarios for capture/encode/network loop.
  - Track baseline and fail on major regression threshold.
- Verification:
  - performance dashboard or stored benchmark history.

## Priority 1: Documentation Completion

### [ ] Final docs completeness pass for release readiness

- Objective:
  - Ensure install, operate, troubleshoot, and scaling workflows are complete without source spelunking.
- Implementation steps:
  - Audit docs against runbook checklist:
    - install paths
    - configuration examples
    - incident response
    - scaling and failure modes
  - Add missing command snippets and expected outputs.
  - Add cross-links between architecture and operations docs.
- Verification:
  - Fresh-reader review from teammate with no prior context.
- Exit criteria:
  - No unresolved doc gaps on release checklist.

## Priority 2: Performance + Reliability Roadmap Execution

### [ ] End-to-end latency budget instrumentation

- Implementation steps:
  - Define stage-level latency budgets (capture, encode, network, decode, render).
  - Emit structured metrics per stage and aggregate session percentile stats.
- Verification:
  - baseline report generated from representative sessions.

### [ ] Long-session memory stability + leak automation

- Implementation steps:
  - Add 2h/8h soak profiles with memory sampling.
  - Add threshold-based failure gates for leak slope.
- Verification:
  - soak jobs publish memory trend artifacts and pass thresholds.

### [ ] Advanced transport experiments behind feature flags

- Implementation steps:
  - Gate transport variants with explicit runtime flags.
  - Add A/B benchmark harness and fallback safety checks.
- Verification:
  - experiments do not affect stable path unless flag enabled.

---

## Acceptance Criteria Before Next Public Release

- [ ] Linux Wayland capture/session flow is stable across target compositors with documented evidence.
- [x] Control-plane soak tests pass with defined SLOs.
- [ ] Release artifacts are clean, labeled, checksummed, and minimal.
- [ ] CI is warning-free and green across required targets for sustained runs.
- [ ] Docs are complete enough for install, operation, troubleshooting, and scaling.

## Execution Order (Strict)

1. Complete Linux Wayland protocol stability and fallback UX tasks.
2. Close Windows CI regressions and re-establish sustained green required checks.
3. Run full release dry-run and close artifact quality gaps.
4. Complete docs completeness pass with runbook validation.
5. Execute release train workflow and publish when all gates are green.

## Next Immediate Actions (First 72 Hours)

1. Run compositor-matrix crash reproduction and capture logs for KDE/GNOME/Sway-Hyprland.
2. Implement capture fallback error taxonomy and user remediation states in desktop app.
3. Add nightly Linux real-capture integration job with artifact retention.
4. Start Windows audio thread-safety triage with dedicated CI diagnostics output.
5. Execute release dry-run and verify `SHA256SUMS` + `release-manifest.json` contents.
