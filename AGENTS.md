# Agent Guidelines for Wavry

Guidelines for AI agents working in the Wavry codebase.

## Quick Commands

### Build & Test
```bash
# Build everything
cargo build --workspace

# Run all tests
cargo test --workspace

# Run a single test
cargo test --workspace <TEST_NAME>
cargo test --workspace -p <crate> <TEST_NAME>

# Run tests for a specific crate
cargo test -p rift-crypto
cargo test -p rift-crypto --test integration

# Check (fast compile check)
cargo check --workspace

# Format code (MUST run before committing)
cargo fmt --all

# Lint (keep clean)
cargo clippy --workspace --all-targets -- -D warnings
```

### Desktop App (Tauri)
```bash
cd crates/wavry-desktop
bun install
bun run tauri dev          # Run dev mode
bun run check          # TypeScript type check
```

### Android
```bash
./scripts/dev-android.sh          # Build mobile
./scripts/run-android.sh          # Build + install + launch
```

### Local Dev Stack
```bash
# Terminal 1: Gateway
cargo run --bin wavry-gateway

# Terminal 2: Relay
cargo run --bin wavry-relay -- --master-url http://localhost:8080

# Linux display test
./scripts/linux-display-smoke.sh
```

## Code Style

### Rust
- Edition 2021, 4-space indentation
- `snake_case`: functions, modules, variables
- `PascalCase`: types, traits, enums, structs
- `SCREAMING_SNAKE_CASE`: constants, statics
- Run `cargo fmt --all` before committing
- Keep `cargo clippy --workspace --all-targets` clean

### Imports Order
1. `std` library imports
2. External crate imports (alphabetical)
3. Internal crate imports (`crate::`)
4. `super::` imports if needed
5. Conditional imports (`#[cfg(...)]`) last

```rust
use std::net::SocketAddr;
use std::sync::Arc;

use anyhow::{anyhow, Result};
use tokio::net::UdpSocket;
use tracing::{debug, info};

use crate::helpers::env_bool;
use crate::types::ClientConfig;

#[cfg(target_os = "linux")]
use wavry_media::GstVideoRenderer;
```

### Error Handling
- Use `anyhow::{anyhow, Result}` for application errors
- Use `thiserror::Error` for custom error types
- Propagate errors with `?` operator
- Add context with `.context()` when helpful
- Log errors with `tracing::error!` before returning

```rust
use anyhow::{anyhow, Context, Result};

fn do_something() -> Result<()> {
    let data = read_file().context("failed to read config")?;
    process(&data)?;
    Ok(())
}
```

### Naming Conventions
- Be descriptive: `session_manager` not `sm`
- Boolean predicates: `is_connected`, `has_capability`
- Async functions: `async fn connect()` (no special prefix)
- Test functions: `fn test_<description>()`
- Constants at module level, not inside functions

### Types & Safety
- Prefer `u64` for sizes/counts unless specific range needed
- Use `NonZeroU*` types when zero is invalid
- Use `Option<T>` over sentinel values
- Prefer `&str` over `&String`, `&[T]` over `&Vec<T>`
- Use `Arc<Mutex<T>>` or `tokio::sync::Mutex` for shared state

## Testing

### Test Organization
- Unit tests: `mod tests { ... }` in source files
- Integration tests: `crates/<crate>/tests/*.rs`
- Test naming: descriptive, e.g., `fn test_connection_timeout()`

### Writing Tests
```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encrypt_decrypt() {
        let key = [0u8; 32];
        let plaintext = b"hello world";
        let ciphertext = encrypt(&key, plaintext).unwrap();
        let decrypted = decrypt(&key, &ciphertext).unwrap();
        assert_eq!(decrypted, plaintext);
    }

    #[tokio::test]
    async fn test_async_operation() {
        let result = async_operation().await;
        assert!(result.is_ok());
    }
}
```

### Platform-Specific Code
Place in dedicated files:
- `linux.rs`, `windows.rs`, `mac_*.rs`
- Use `#[cfg(target_os = "linux")]` for small differences
- Keep platform code in `wavry-media` crate

## Project Structure

- `crates/`: Rust workspace crates
  - `rift-core/`: Protocol (RIFT, DELTA CC)
  - `rift-crypto/`: Encryption (Noise XX, ChaCha20-Poly1305)
  - `wavry-client/`, `wavry-server/`: Session apps
  - `wavry-gateway/`, `wavry-relay/`: Network coordination
  - `wavry-desktop/`: Tauri UI app
- `apps/`: Platform apps (Android, macOS)
- `docs/`: Architecture & protocol specs
- `scripts/`: Build & dev helpers
- `third_party/alvr/`: Vendored VR runtime (don't modify)

## Commit Guidelines

- Use Conventional Commits: `feat:`, `fix:`, `docs:`, `refactor:`
- Keep commits focused on single logical changes
- Update docs in `docs/` if protocol/security behavior changes
- Include CLA attestation: "I have read and agree to CLA.md"
- Run tests and linting before committing

## Logging & Tracing

- Use `tracing` crate: `info!`, `debug!`, `warn!`, `error!`
- Include context: `info!(session_id, "connection established")`
- Use `#[instrument]` for function-level tracing
- Avoid `println!` in production code

## Security Notes

- Never log secrets, tokens, or private keys
- Validate all network inputs before processing
- Use `Noise_XX_25519_ChaChaPoly_BLAKE2s` for crypto
- Check `docs/WAVRY_SECURITY.md` for threat model

## Prerequisites

- Rust 1.75+
- protobuf-compiler
- pkg-config
- Platform toolchains: Xcode 15+ (macOS), PipeWire (Linux), Android SDK/NDK
