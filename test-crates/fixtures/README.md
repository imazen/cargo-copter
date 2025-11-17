# Test Fixtures

This directory contains fixtures for integration testing cargo-crusader.

## rust-rgb-breaking/

WIP version of the `rgb` crate (v0.8.91) with breaking changes that cause downstream compilation failures.

**Usage:**
```bash
# Test with full multi-version flow (recommended)
./target/release/cargo-crusader --path test-crates/fixtures/rust-rgb-breaking \
    --dependents load_image:3.3.1 --test-versions 0.8.52

# Test with legacy flow (simpler, may miss regressions)
./target/release/cargo-crusader --path test-crates/fixtures/rust-rgb-breaking \
    --dependents load_image:3.3.1
```

**Expected Results:**
- **Baseline (0.8.52)**: PASSED ✓✓✓
- **WIP (0.8.91)**: REGRESSED ✓✗ (cargo check fails with 22 errors)

**Related Tests:**
- `tests/wip_breaking_change_test.rs` - Validates regression detection with --test-versions
- `tests/default_baseline_wip_test.rs` - Documents legacy path behavior without --test-versions

## ansi_colours/

Downloaded version of `ansi_colours-1.2.3` for testing.

**Dependency on rgb:** `rgb = "0.8"` (optional feature)
