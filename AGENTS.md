# Repository Guidelines

## Project Structure & Module Organization
Wavry is a multi-platform monorepo centered on a Rust workspace.
- `crates/`: Rust crates for protocol (`rift-core`, `rift-crypto`), infra (`wavry-gateway`, `wavry-relay`, `wavry-master`), and apps (`wavry-server`, `wavry-client`, `wavry-desktop/src-tauri`, `wavry-ffi`).
- `apps/`: platform apps (`apps/android`, `apps/macos`, `apps/web-reference`).
- `scripts/`: repeatable build/dev entrypoints (desktop, Android, cross-build, smoke tests).
- `docs/`: architecture, protocol, testing, and security references.
- `third_party/alvr/`: vendored dependency; treat edits as explicit vendor updates.

## Architecture Snapshot
- Protocol/security core is `rift-core` + `rift-crypto` (RIFT, DELTA, Noise_XX, ChaCha20-Poly1305).
- Session apps are `wavry-server` and `wavry-client`; network coordination is `wavry-gateway` + `wavry-relay`.

## Build, Test, and Development Commands
- `cargo build --workspace`: build all Rust crates.
- `cargo test --workspace`: run Rust test suite.
- `cargo test -p rift-crypto --test integration`: run crypto integration coverage.
- `cargo run --bin wavry-gateway` and `cargo run --bin wavry-relay -- --master-url http://localhost:8080`: local signaling/relay stack.
- `cd crates/wavry-desktop && bun install && bun tauri dev`: run desktop app.
- `cd crates/wavry-desktop && bun run check`: Svelte/TypeScript type checks.
- `./scripts/dev-android.sh` (or `./scripts/run-android.sh`): Android build/install flow.
- `./scripts/linux-display-smoke.sh`: Linux display capture validation.

Prereqs: Rust 1.75+, `protobuf-compiler`, `pkg-config`, plus platform toolchains (Xcode 15+, PipeWire/portal, Android SDK/NDK).

## Coding Style & Naming Conventions
- Rust: edition 2021, 4-space indentation, `snake_case` functions/modules, `PascalCase` types, `SCREAMING_SNAKE_CASE` constants.
- Run `cargo fmt --all` before opening a PR; keep `cargo clippy --workspace --all-targets` clean when practical.
- Svelte components use `PascalCase` in `crates/wavry-desktop/src/lib/components/`; route files follow SvelteKit conventions (for example `+page.svelte`).
- Keep modules focused; place platform-specific code in dedicated files (`linux.rs`, `windows.rs`, `mac_*`).

## Testing Guidelines
- Prefer unit tests near implementation (`mod tests`) and integration tests under `crates/<crate>/tests/`.
- Add tests for protocol, crypto, and session-state changes.
- For platform/UI changes, run relevant manual runbooks in `docs/WAVRY_TESTING.md` and include results in the PR.
- For protocol/security updates, verify related specs remain aligned (`docs/RIFT_SPEC_V1.md`, `docs/DELTA_CC_SPEC.md`, `docs/WAVRY_SECURITY.md`).

## Commit & Pull Request Guidelines
- History is mixed; prefer Conventional Commit style: `feat(scope): ...`, `fix: ...`, `docs: ...`, `refactor: ...`.
- Keep commits scoped to one logical change.
- PRs should include: what changed, why, validation commands run, and linked issue(s).
- Include screenshots/video for desktop or Android UI changes.
- If protocol/security behavior changed, update corresponding docs in `docs/`.
- Add the CLA attestation line in the PR description: `I have read and agree to CLA.md`.
