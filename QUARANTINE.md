# Wavry Test Quarantine

This file tracks tests that are known to be flaky or unstable.
Tests listed here are moved to a separate "quarantine" workflow to prevent blocking the main CI/CD pipeline.

## Flaky Tests

| Test Name | Crate/Path | Reason | Owner | Date Added |
|-----------|------------|--------|-------|------------|
| `control-plane-resilience` | `scripts/control-plane-resilience.sh` | Timing sensitive, depends on system load | @brooklyn | 2026-02-13 |
| `fuzz_decode` | `crates/rift-core/tests/fuzz_decode.rs` | Long running, sometimes times out | @brooklyn | 2026-02-13 |

## Quarantine Process

1. **Identify**: If a test fails in CI without a clear code regression, mark it as potentially flaky.
2. **Document**: Add the test to this file with a reason.
3. **Isolate**: Modify the main workflow to skip this test and add it to the `quarantine` job.
4. **Fix**: Assign an owner to investigate and fix the root cause.
5. **Restore**: Once the test is proven stable (e.g., 50+ passing runs), move it back to the main pipeline.

## Quarantined Workflow Job

The `quality-gates.yml` workflow should be updated to skip these tests in the main run.
A separate `quarantine.yml` workflow can run them on a schedule to track stability.
