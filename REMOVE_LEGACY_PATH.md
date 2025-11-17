# Proposal: Remove Legacy Test Path

## Current Situation

Two completely separate code paths exist:

```rust
// src/main.rs:216-222
let result = if let Some(ref versions) = test_versions {
    run_test_multi_version(...)  // Phase 5: Correct, reliable
} else {
    run_test(...)  // Phase 1-4: BROKEN, produces false positives
};
```

**Problem**: The legacy path is the DEFAULT when users don't specify `--test-versions`.

## Why Legacy Path Exists

Looking at the history:
- **Phase 1-4**: Original implementation (`run_test` / `run_test_local`)
- **Phase 5**: Multi-version testing added (`run_test_multi_version`)
- Legacy path kept for "backward compatibility"

This was a mistake because:
1. The legacy path is fundamentally broken (false positives)
2. It should never have been kept as the default
3. "Backward compatibility" with broken behavior is harmful

## Proposal: Eliminate Legacy Path

### Step 1: Make Multi-Version the Default

```rust
// src/main.rs - proposed change
let test_versions = test_versions.or_else(|| {
    // When --test-versions not specified, infer baseline
    Some(vec![/* inferred baseline version */])
});

// Always use multi-version path (only one path exists now)
let result = run_test_multi_version(pool, config.clone(), rev_dep, version, test_versions);
```

### Step 2: Baseline Inference Logic

When `--test-versions` is not specified, automatically infer the baseline:

```rust
fn infer_baseline_version(rev_dep: &RevDepName, config: &Config) -> Vec<compile::VersionSource> {
    // Option 1: Extract from dependent's Cargo.lock (if exists)
    if let Ok(baseline) = extract_resolved_version(rev_dep, &config.crate_name, &config.staging_dir) {
        return vec![compile::VersionSource::Published(baseline)];
    }

    // Option 2: Use the version from crates.io API (what dependent currently uses)
    if let Some(resolved) = &rev_dep.resolved_version {
        return vec![compile::VersionSource::Published(resolved.clone())];
    }

    // Fallback: Use dependent's declared requirement (less precise)
    vec![compile::VersionSource::Published("latest".to_string())]
}
```

### Step 3: Remove Legacy Code

Delete entirely:
- `fn run_test()` (src/main.rs:1006)
- `fn run_test_local()` (wherever it's defined)
- `fn compile_with_custom_dep()` (if only used by legacy path)
- Legacy TestResult constructors: `passed()`, `regressed()`, etc. (src/main.rs:830-950)

## Benefits

1. **Eliminates false positives** - Only one reliable code path
2. **Simplifies codebase** - Remove ~300+ lines of buggy code
3. **Better defaults** - Users get correct behavior automatically
4. **Clearer API** - One way to do things, not two
5. **Easier maintenance** - Single code path to test and debug

## Migration Path

### For Users

**Before:**
```bash
# Broken - produces false positives
cargo-crusader --path ~/my-crate

# Working - requires manual flag
cargo-crusader --path ~/my-crate --test-versions 1.0.0
```

**After:**
```bash
# Always works - baseline inferred automatically
cargo-crusader --path ~/my-crate

# Can still specify explicit versions
cargo-crusader --path ~/my-crate --test-versions 1.0.0 1.1.0
```

**Zero breaking changes** - Behavior improves, no flags required!

### Implementation Plan

1. **Phase 1: Add baseline inference**
   - Implement `infer_baseline_version()`
   - Test with fixtures

2. **Phase 2: Make multi-version default**
   - Update dispatch logic to always use multi-version
   - Add tests for automatic baseline inference

3. **Phase 3: Remove legacy code**
   - Delete `run_test()` and related functions
   - Update tests that relied on legacy behavior
   - Run full test suite

4. **Phase 4: Documentation**
   - Update README (remove --test-versions requirement)
   - Update CLAUDE.md
   - Note in CHANGELOG

## Testing Strategy

1. **Verify baseline inference works**
   ```bash
   cargo test test_baseline_inference
   ```

2. **Verify no regressions**
   ```bash
   cargo test  # All 56+ tests should pass
   ```

3. **Manual testing**
   ```bash
   # Should automatically detect baseline and test WIP
   cargo-crusader --path test-crates/fixtures/rust-rgb-breaking \
       --dependents load_image:3.3.1

   # Should show: baseline PASSED, WIP REGRESSED (same as with --test-versions)
   ```

## Risks & Mitigation

**Risk**: Baseline inference might fail for some edge cases
**Mitigation**:
- Fallback to current published version
- Allow explicit `--test-versions` override
- Add logging for inference decisions

**Risk**: Performance impact (multi-version always runs ICT)
**Mitigation**:
- This is actually CORRECT behavior
- Users who want speed over correctness shouldn't use crusader
- Can add `--quick` flag later if needed (with big warnings)

## Timeline

- **Week 1**: Implement baseline inference
- **Week 2**: Update dispatch logic, test thoroughly
- **Week 3**: Remove legacy code, update docs
- **Week 4**: Release as v0.2.0 (minor version bump)

## Conclusion

The legacy path serves no legitimate purpose:
- It's broken (false positives)
- It's confusing (two code paths)
- It's harmful (default behavior is wrong)

**Recommendation**: Remove it completely and make the correct behavior the default.

---

## Appendix: Code to Remove

Estimated ~300-400 lines of code can be deleted:

```rust
// src/main.rs
fn run_test() { ... }                    // ~10 lines
fn run_test_local() { ... }              // ~100+ lines
fn compile_with_custom_dep() { ... }     // ~100+ lines
impl TestResult {
    fn passed() { ... }                  // ~30 lines
    fn regressed() { ... }               // ~30 lines
    fn broken() { ... }                  // ~30 lines
    fn error() { ... }                   // ~10 lines
    fn skipped() { ... }                 // ~10 lines
}

// Tests
#[test]
fn test_legacy_path_xyz() { ... }        // Multiple tests
```

All replaced with single, reliable multi-version path.
