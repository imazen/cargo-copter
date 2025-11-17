# 🐛 FALSE POSITIVE BUG: Legacy Path Reports Success on Breaking Changes

## Summary

**CRITICAL BUG**: When `--test-versions` is NOT specified, cargo-crusader uses a legacy code path that produces **false positives** - reporting PASSED when the WIP version actually breaks dependents.

## Reproduction

```bash
# FALSE POSITIVE (legacy path)
./target/release/cargo-crusader \
    --path test-crates/fixtures/rust-rgb-breaking \
    --dependents load_image:3.3.1

# Output:
# - baseline: PASSED ✓✓✓
# - WIP:      PASSED ✓✓✓  🐛 FALSE POSITIVE!

# CORRECT DETECTION (multi-version path)
./target/release/cargo-crusader \
    --path test-crates/fixtures/rust-rgb-breaking \
    --dependents load_image:3.3.1 \
    --test-versions 0.8.52

# Output:
# - baseline (0.8.52): PASSED ✓✓✓
# - WIP (0.8.91):      REGRESSED ✓✗  ✓ CORRECT!
#   cargo check failed with 22 errors
```

## Root Cause

**File**: `src/main.rs:216-222`

```rust
let result = if let Some(ref versions) = test_versions {
    // Phase 5: Multi-version testing (CORRECT)
    run_test_multi_version(pool, config.clone(), rev_dep, version, versions.clone())
} else {
    // Legacy: Single baseline vs override test (BUGGY)
    run_test(pool, config.clone(), rev_dep, version)
};
```

### Legacy Path Issues

1. **Uses `cargo build` instead of `cargo check`**
   - `cargo build` can succeed even when `cargo check` fails
   - Misses type errors and many compilation issues
   - In this case: build succeeds, but check fails with 22 errors

2. **Doesn't populate OfferedRow fields correctly**
   - `spec`: shows "?" instead of actual requirement ("^0.8.52")
   - `resolved_version`: shows "?" for WIP instead of "0.8.91"
   - `original_requirement`: set to `None` in legacy constructors

3. **No three-step ICT testing**
   - Multi-version: Fetch → Check → Test (with early stopping)
   - Legacy: Just build (incomplete)

## Impact

**CRITICAL**: Users relying on the default behavior (no `--test-versions`) will:
- ✗ Receive false confidence that their changes are safe
- ✗ Miss breaking changes before publishing
- ✗ Ship breaking versions to crates.io

## Test Coverage

**Integration test**: `tests/default_baseline_wip_test.rs`

```bash
# Run the test to see the false positive
cargo test --test default_baseline_wip_test \
    test_default_baseline_wip_output -- --ignored --nocapture
```

The test documents this bug and validates detection of the false positive.

## Recommendations

### Immediate (User-facing)

1. **Document the requirement to use `--test-versions`**
   - Update README with prominent warning
   - Make `--test-versions` required in examples
   - Consider deprecation warning when not specified

2. **Add CLI warning**
   ```
   ⚠️  WARNING: Running without --test-versions may produce false positives!
   ⚠️  For accurate regression detection, use: --test-versions <VERSION>
   ```

### Long-term (Code fixes)

**Option A: Make multi-version the default**
```rust
// Infer baseline version when --test-versions not specified
let test_versions = test_versions.or_else(|| {
    // Auto-add baseline version
    Some(vec![inferred_baseline_version])
});
// Always use multi-version path
run_test_multi_version(...)
```

**Option B: Fix legacy path to use ICT**
- Update `run_test()` to use three-step ICT
- Extract and populate `original_requirement`
- Use same compilation strategy as multi-version

**Option C: Remove legacy path entirely**
- Deprecate and remove `run_test()`
- Always require `--test-versions` (or infer baseline)
- Single code path = fewer bugs

## Related Files

- `src/main.rs:216-222` - Path dispatch logic
- `src/main.rs:run_test()` - Legacy path (buggy)
- `src/main.rs:run_test_multi_version()` - Multi-version path (correct)
- `src/main.rs:830-862` - Legacy TestResult constructors (set `original_requirement: None`)
- `tests/default_baseline_wip_test.rs` - Test documenting the bug

## Comparison: Legacy vs Multi-Version

| Aspect | Legacy Path | Multi-Version Path |
|--------|-------------|-------------------|
| **Trigger** | Default (no --test-versions) | `--test-versions VERSION` |
| **Compilation** | `cargo build` | ICT: fetch → check → test |
| **Spec field** | "?" | "^0.8.52" ✓ |
| **Resolved field** | "?" for WIP | "0.8.91" ✓ |
| **Detects breaking changes** | ✗ FALSE POSITIVES | ✓ Accurate |
| **Performance** | Faster (incomplete) | Thorough (slower) |
| **Production ready** | ✗ NO | ✓ YES |

## Recommended Fix Priority

**P0 - Critical**: This bug causes false confidence in broken code.

Suggested approach:
1. Add prominent CLI warning (quick fix)
2. Update docs to require `--test-versions`
3. Implement Option A (make multi-version default with baseline inference)
4. Remove legacy path in next major version
