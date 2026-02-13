---
title: Developer Workflows
description: Practical development loops for building, testing, debugging, and extending Wavry.
---

This page is the contributor operating guide for day-to-day engineering work.

## Workspace Commands

From repository root:

```bash
cargo build --workspace
cargo check --workspace
cargo test --workspace
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
```

## Focused Crate Loops

```bash
cargo test -p rift-core
cargo test -p rift-crypto
cargo test -p wavry-client
cargo test -p wavry-server
cargo test -p wavry-gateway
cargo test -p wavry-relay
```

Run a single test:

```bash
cargo test --workspace <TEST_NAME>
cargo test -p <crate> <TEST_NAME>
```

## Desktop App Loop (Tauri)

```bash
cd crates/wavry-desktop
bun install
bun run check
bun run tauri dev
```

If Tauri build fails because `frontendDist` is missing, ensure web build output exists at `crates/wavry-desktop/build` or regenerate frontend assets before running native packaging.

## Control Plane Local Loop

Run each service in a separate terminal:

```bash
cargo run --bin wavry-gateway
cargo run --bin wavry-master -- --listen 127.0.0.1:8080
cargo run --bin wavry-relay -- --master-url http://127.0.0.1:8080
```

Then start host/client:

```bash
cargo run --bin wavry-server -- --gateway-url ws://127.0.0.1:3000/ws
cargo run --bin wavry-client -- --connect 127.0.0.1:0
```

## Linux and Wayland Validation Loop

```bash
./scripts/linux-display-smoke.sh
```

For desktop runtime verification, use Tauri commands exposed in `crates/wavry-desktop/src-tauri/src/commands.rs`:

- `linux_runtime_health`
- `linux_host_preflight`

## Android Loop

```bash
./scripts/dev-android.sh
./scripts/run-android.sh
```

## CI-Oriented Validation Before Release

1. `cargo fmt --all`
2. `cargo clippy --workspace --all-targets -- -D warnings`
3. `cargo test --workspace`
4. website checks (`apps/website` build and style checks)
5. verify release artifact names against [Release Artifacts](/release-artifacts)

## Contribution Workflow and CLA

Before opening a pull request:

1. Branch from `main` and keep the change focused to one logical scope.
2. Run required local quality gates (`fmt`, `clippy`, `test`, and website checks when docs/UI are touched).
3. Use Conventional Commit style for commit subjects (`feat:`, `fix:`, `docs:`, etc.).
4. Include the CLA attestation in commit or PR context:
   `I have read and agree to CLA.md`.
5. Ensure all required CI workflows are green before merge.

References:

- [CLA.md](https://github.com/bybrooklyn/wavry/blob/main/CLA.md)
- [CONTRIBUTING.md](https://github.com/bybrooklyn/wavry/blob/main/CONTRIBUTING.md)

## Where to Start for Major Changes

| Change Type | First Files |
|---|---|
| Protocol evolution | `crates/rift-core/src/lib.rs`, `crates/rift-core/src/cc.rs`, `crates/rift-crypto/src/noise.rs` |
| Relay behavior | `crates/wavry-relay/src/main.rs`, `crates/wavry-master/src/selection.rs` |
| Gateway auth/security | `crates/wavry-gateway/src/auth.rs`, `crates/wavry-gateway/src/security.rs` |
| Linux media path | `crates/wavry-media/src/linux.rs`, `crates/wavry-platform/src/linux/mod.rs` |
| Desktop UX + commands | `crates/wavry-desktop/src/routes/+page.svelte`, `crates/wavry-desktop/src-tauri/src/commands.rs` |

## Related Docs

- [Codebase Reference](/codebase-reference)
- [Runtime and Service Reference](/runtime-and-service-reference)
- [Runbooks and Checklists](/runbooks-and-checklists)
- [Internal Design Docs](/internal-design-docs)
