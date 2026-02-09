# Code Coverage Strategy & Implementation

**Status**: Planning Phase
**Target**: >70% coverage for core crates
**Tool**: `cargo-tarpaulin`
**Last Updated**: 2026-02-09

---

## Overview

Code coverage metrics help ensure quality and identify untested paths. This document outlines the strategy for achieving >70% coverage on Wavry's critical crates.

## Target Crates (Priority Order)

### Tier 1: Protocol Layer (Critical)
- **rift-core** - Packet framing, DELTA congestion control
- **rift-crypto** - Noise handshake, AEAD encryption
- **Target Coverage**: >80%
- **Why**: Core streaming logic, security-critical

### Tier 2: Session Management
- **wavry-client** - Client-side session & signaling
- **wavry-server** - Host-side encoding & streaming
- **Target Coverage**: >75%
- **Why**: Main application logic

### Tier 3: Platform Integration
- **wavry-media** - Hardware encoding/decoding
- **wavry-platform** - Input injection, system APIs
- **Target Coverage**: >65% (hardware dependencies make 100% difficult)
- **Why**: Platform-specific, integration points

### Tier 4: Infrastructure
- **wavry-gateway** - Signaling & relay coordination
- **wavry-relay** - Packet forwarding
- **Target Coverage**: >70%
- **Why**: Network services

### Tier 5: Utilities
- **wavry-common** - Types, helpers, utilities
- **Target Coverage**: >80%
- **Why**: Foundation for other crates

---

## Setup & Tools

### Install cargo-tarpaulin
```bash
cargo install cargo-tarpaulin
```

### Run Coverage
```bash
# Single crate
cargo tarpaulin -p rift-core --out Html --output-dir coverage

# Multiple crates
cargo tarpaulin \
  -p rift-core \
  -p rift-crypto \
  -p wavry-common \
  --out Html --output-dir coverage

# Workspace (all crates)
cargo tarpaulin --workspace --out Html --output-dir coverage
```

### Generate Reports
```bash
# HTML report (browser-friendly)
cargo tarpaulin -p rift-core --out Html --output-dir coverage

# LCOV format (CI/CD integration)
cargo tarpaulin -p rift-core --out Lcov --output-dir coverage

# Plain text
cargo tarpaulin -p rift-core --out Stdout
```

---

## Current Baseline

**Measurement Date**: 2026-02-09

### Estimated Coverage by Crate (Before Tests)
```
rift-core:        ~35% (basic types, no integration tests)
rift-crypto:      ~40% (Noise, ChaCha20 - some paths untested)
wavry-common:     ~20% (types, basic helpers only)
wavry-client:     ~10% (session management missing tests)
wavry-server:     ~15% (encoding loop not tested)
wavry-media:      ~5%  (hardware dependencies)
wavry-platform:   ~10% (system API mocks needed)
wavry-gateway:    ~25% (admin panel tested, API mostly untested)
wavry-relay:      ~20% (forwarding logic untested)
```

**Overall**: ~20-25%
**Gap to Target**: +50 percentage points needed

---

## Testing Strategy

### Phase 1: Core Protocol (Week 1)
**Crates**: rift-core, rift-crypto
**Effort**: 40 hours
**Target**: 80% coverage

#### rift-core Tests
- [x] Packet parsing (OBU, headers)
- [x] Sequence number tracking
- [x] FEC parity generation
- [x] DELTA state machine
  - [ ] STABLE → RISING transition
  - [ ] RISING → CONGESTED transition
  - [ ] Recovery logic
  - [ ] Edge cases (RTT jitter, packet loss)
- [ ] Timestamp handling
- [ ] Bitrate calculations

#### rift-crypto Tests
- [x] Noise handshake
  - [ ] Happy path (full 3-round handshake)
  - [ ] Error cases (invalid keys, replayed messages)
  - [ ] Edge cases (zero-length payloads)
- [x] ChaCha20-Poly1305
  - [ ] Encryption/decryption roundtrip
  - [ ] AEAD tag verification
  - [ ] Different message lengths
  - [ ] Invalid ciphertexts
- [ ] Ed25519 signature verification
- [ ] Key derivation

### Phase 2: Session & Streaming (Week 2)
**Crates**: wavry-client, wavry-server, wavry-common
**Effort**: 35 hours
**Target**: 75% coverage

#### wavry-common Tests
- [ ] Type serialization/deserialization
- [ ] Codec enum variants
- [ ] Resolution validation
- [ ] Helper function edge cases

#### wavry-client Tests
- [ ] Session creation & teardown
- [ ] Signaling message handling
- [ ] RTT tracking
- [ ] Monitor discovery
- [ ] Error recovery

#### wavry-server Tests
- [ ] Encoding pipeline
- [ ] Keyframe injection
- [ ] Bitrate adaptation
- [ ] Packet transmission
- [ ] Cleanup on disconnect

### Phase 3: Platform & Hardware (Week 3)
**Crates**: wavry-media, wavry-platform
**Effort**: 30 hours
**Target**: 65% coverage (hardware limitations)

#### wavry-media Tests
- [ ] Codec capability probing
- [ ] Hardware encoder detection
- [ ] Dummy encoder (mock)
- [ ] Frame format conversion
- [ ] Resolution clamping

#### wavry-platform Tests
- [ ] Input injector initialization
- [ ] Event serialization
- [ ] Error handling
- [ ] Platform-specific mocks

### Phase 4: Infrastructure (Week 4)
**Crates**: wavry-gateway, wavry-relay
**Effort**: 25 hours
**Target**: 70% coverage

#### wavry-gateway Tests
- [ ] Admin API endpoints
- [ ] User CRUD operations
- [ ] Session management
- [ ] Database transactions
- [ ] Relay coordination

#### wavry-relay Tests
- [ ] Packet forwarding
- [ ] Token validation
- [ ] Rate limiting
- [ ] IP binding
- [ ] Cleanup logic

---

## Testing Best Practices

### Test Structure
```rust
#[cfg(test)]
mod tests {
    use super::*;

    // Happy path
    #[test]
    fn test_happy_path() {
        // Setup
        // Exercise
        // Verify
    }

    // Error cases
    #[test]
    fn test_error_invalid_input() {
        // Verify proper error handling
    }

    // Edge cases
    #[test]
    fn test_edge_case_boundary() {
        // Verify boundary behavior
    }

    // Integration
    #[test]
    fn test_integration_component_a_with_b() {
        // Test component interaction
    }
}
```

### Mock & Fixture Guidelines
- Use `mockito` or `mock!` macros for external deps
- Create fixture builders for complex structures
- Keep mocks simple and realistic

### Coverage Exclusions
Mark code that shouldn't affect coverage:
```rust
#[cfg(not(test))]
fn expensive_runtime_check() { ... }

// Platform-specific
#[cfg(target_os = "macos")]
unsafe fn platform_specific_ffi() { ... }

// Fallback paths (theoretical)
#[cfg(all(test, false))]  // Never true in practice
fn unreachable_error_path() { ... }
```

---

## Continuous Integration

### GitHub Actions Workflow
```yaml
name: Code Coverage

on: [push, pull_request]

jobs:
  coverage:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v3

      - uses: dtolnay/rust-toolchain@stable

      - name: Install tarpaulin
        run: cargo install cargo-tarpaulin

      - name: Run coverage
        run: |
          cargo tarpaulin \
            -p rift-core \
            -p rift-crypto \
            -p wavry-common \
            --out Lcov --output-dir coverage

      - name: Upload coverage
        uses: codecov/codecov-action@v3
        with:
          files: ./coverage/lcov.info
          flags: unittests
          fail_ci_if_error: true
          # Fail if coverage drops below 70%
          threshold: 70
```

### Local Pre-commit Hook
```bash
#!/bin/bash
# .git/hooks/pre-commit

cargo tarpaulin -p rift-core --out Stdout || {
    echo "❌ Coverage check failed"
    exit 1
}
```

---

## Monitoring & Dashboards

### Coverage Goals by Crate
| Crate | Current | Target | Gap |
|-------|---------|--------|-----|
| rift-core | 35% | 80% | +45% |
| rift-crypto | 40% | 80% | +40% |
| wavry-common | 20% | 80% | +60% |
| wavry-client | 10% | 75% | +65% |
| wavry-server | 15% | 75% | +60% |
| wavry-media | 5% | 65% | +60% |
| wavry-platform | 10% | 70% | +60% |
| wavry-gateway | 25% | 70% | +45% |
| wavry-relay | 20% | 70% | +50% |
| **Overall** | **~22%** | **>70%** | **+48%** |

### Metrics to Track
- **Line Coverage**: % of executable lines covered
- **Branch Coverage**: % of conditional branches tested
- **Function Coverage**: % of functions with at least one test
- **Trend**: Week-over-week improvement

---

## Common Pitfalls & Solutions

### Problem: Tests Don't Compile
**Solution**: Use feature flags to enable test-only code
```rust
#[cfg(test)]
mod mocks {
    // Mock implementations only in tests
}
```

### Problem: Hardware Dependencies Prevent Testing
**Solution**: Create abstractions with test implementations
```rust
pub trait Encoder: Send {
    fn encode(&mut self, frame: RawFrame) -> Result<EncodedFrame>;
}

pub struct DummyEncoder;  // For testing

impl Encoder for DummyEncoder {
    fn encode(&mut self, _frame: RawFrame) -> Result<EncodedFrame> {
        // Dummy implementation
    }
}
```

### Problem: Flaky Tests (Timing Issues)
**Solution**: Use deterministic mocks instead of real timing
```rust
#[test]
fn test_with_mock_time() {
    let mut mock_clock = MockClock::new();
    // Control time explicitly
}
```

### Problem: Circular Dependencies
**Solution**: Use trait objects or feature-based separation
```rust
#[cfg(test)]
use mock_db::MockDatabase as Database;
#[cfg(not(test))]
use actual_db::ActualDatabase as Database;
```

---

## Success Criteria

- [ ] All Tier 1 crates (rift-core, rift-crypto) >80% coverage
- [ ] All Tier 2 crates (wavry-client, wavry-server) >75% coverage
- [ ] All Tier 3+ crates >65% coverage
- [ ] Overall workspace >70% coverage
- [ ] No new code without tests (enforced in PR review)
- [ ] CI/CD pipeline validates coverage before merge

---

## Timeline

| Week | Crates | Target Coverage | Status |
|------|--------|-----------------|--------|
| Week 1 | rift-core, rift-crypto | 80% | Pending |
| Week 2 | wavry-client, wavry-server, wavry-common | 75% | Pending |
| Week 3 | wavry-media, wavry-platform | 65% | Pending |
| Week 4 | wavry-gateway, wavry-relay | 70% | Pending |
| **Overall** | **All crates** | **>70%** | Pending |

---

## Resources

- [Tarpaulin Documentation](https://github.com/xd009642/tarpaulin)
- [Rust Testing Guide](https://doc.rust-lang.org/book/ch11-00-testing.html)
- [Coverage Best Practices](https://www.atlassian.com/continuous-delivery/software-testing/code-coverage)

