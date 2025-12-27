# Test Coverage Gaps - Quick Reference

**Generated:** 2025-11-18
**Full Analysis:** See `REGRESSION_TEST_COVERAGE.md`

## Quick Stats

- **Total Bugs Identified:** 26 recurring issues from history
- **Well Tested:** 5 bugs (19%)
- **Partially Tested:** 7 bugs (27%)
- **Not Tested:** 14 bugs (54%)
- **Overall Coverage:** 65%

## Critical Gaps (Priority 1)

These bugs can cause **wrong test results**:

1. ❌ **Cargo.toml Restoration** - No test that Cargo.toml is restored between tests
   - History: Line 137, 140, 267
   - Impact: Test contamination, false results
   - Need: `tests/cargo_toml_restoration_test.rs`

2. ❌ **Resolved Version Accuracy** - No test that displayed version matches actual
   - History: Line 140, 161, 259
   - Impact: Users see wrong version info
   - Need: Test in `tests/metadata_extraction_test.rs`

3. ⚠️ **Registry Version Override** - Implemented but not integration tested
   - Fixed: BUGFIXES.md #6
   - Impact: `--test-versions` might break
   - Need: Integration test with crates.io versions

## User-Facing Gaps (Priority 2)

These bugs affect **user experience**:

4. ❌ **Double Printing** - Headers/summary printed twice
   - History: SESSION_SUMMARY.md
   - Need: Test counting header occurrences

5. ❌ **Streaming Output** - Rows should appear immediately
   - History: Line 163, 230, 260
   - Need: Test that rows don't batch

6. ❌ **Error Deduplication** - "[SAME ERROR]" feature not tested
   - History: Line 248, 250, 251, 253, 254
   - Need: Test with hex code normalization

7. ❌ **--error-lines Parameter** - No test it works
   - History: Line 301
   - Need: Tests for N, 0, and multiple errors

## Output Quality Gaps (Priority 3)

These bugs affect **visual output**:

8. ⚠️ **Box Drawing Validation** - Test measures but doesn't fail on bad patterns
   - History: Line 188, 191, 221, 239
   - Bad patterns: `─└`, `─┐│`, `│ │`
   - Need: Add assertions to `tests/table_alignment_test.rs`

9. ⚠️ **Spec Extraction** - Test checks but doesn't fail on "?"
   - History: Line 69, 79, 87, 212
   - Need: Strict assertion in `tests/default_baseline_wip_test.rs`

10. ❌ **Multi-Version Display** - When cargo resolves multiple versions
    - History: Line 212, 232, 259
    - Need: Test for multi-version detection and display

## Safety Gaps (Priority 4)

These bugs allow **dangerous behavior**:

11. ⚠️ **Legacy Path False Positive** - Documented but not prevented
    - Test: `tests/default_baseline_wip_test.rs`
    - Status: Test allows false positive to pass
    - Need: Make test FAIL when false positive detected

12. ❌ **--clean Effectiveness** - Flag exists, not tested
    - Need: Test that staging is actually purged

## Quick Test File Additions Needed

Create these new files:

```
tests/
├── cargo_toml_restoration_test.rs  (NEW) - Test contamination prevention
├── output_format_test.rs           (NEW) - Test streaming, headers, no debug messages
├── error_handling_test.rs          (NEW) - Test deduplication, --error-lines
└── metadata_extraction_test.rs     (NEW) - Test spec, resolved, multi-version
```

Enhance these existing files:

```
tests/
├── default_baseline_wip_test.rs    - Add strict spec assertion, fail on false positive
└── table_alignment_test.rs         - Add invalid pattern detection
```

## One-Liner Test Additions

Add these assertions to catch recurring bugs:

```rust
// In tests/default_baseline_wip_test.rs
assert!(!stdout.contains("│ ?"), "Spec should never be '?'");
assert_eq!(stdout.matches("│ Offered │").count(), 1, "Header printed once");
assert!(!stdout.contains("copter:"), "No debug messages in table");

// In tests/table_alignment_test.rs
assert!(!output.contains("─└"), "Invalid corner");
assert!(!output.contains("─┐│"), "Invalid join");
assert!(!output.contains("│ │"), "Double vertical");
```

## Commands to Verify Coverage

```bash
# Run all tests
cargo test

# Run integration test (requires network)
cargo test --test default_baseline_wip_test -- --ignored --nocapture

# Check for contamination (manual)
./target/release/cargo-copter --path test-crates/fixtures/rust-rgb-breaking --dependents load_image:3.3.1
# Verify Cargo.toml unchanged after run

# Test streaming (manual)
./target/release/cargo-copter --path ~/rust-rgb --top-dependents 5
# Verify rows appear immediately, not batched
```

## By the Numbers

### Bugs by Category

| Category | Total | Tested | Coverage |
|----------|-------|--------|----------|
| Baseline Handling | 5 | 5 | 100% ✅ |
| Output/UI | 4 | 1 | 25% ❌ |
| Data Extraction | 3 | 0 | 0% ❌ |
| Contamination | 3 | 0 | 0% ❌ |
| Table Formatting | 3 | 1 | 33% ⚠️ |
| Error Handling | 3 | 0 | 0% ❌ |
| False Positives | 1 | 0 | 0% ⚠️ |

### Test Files Status

| Test File | Lines | Coverage | Missing |
|-----------|-------|----------|---------|
| src/config_test.rs | 182 | Excellent ✅ | Registry override integration |
| src/runner_test.rs | 198 | Excellent ✅ | Order validation |
| tests/default_baseline_wip_test.rs | 222 | Good ⚠️ | Strict spec assertion |
| tests/table_alignment_test.rs | 147 | Weak ⚠️ | Pattern validation |
| tests/cargo_toml_restoration_test.rs | 0 | None ❌ | Everything |
| tests/output_format_test.rs | 0 | None ❌ | Everything |
| tests/error_handling_test.rs | 0 | None ❌ | Everything |
| tests/metadata_extraction_test.rs | 0 | None ❌ | Everything |

## Historical Bug Frequency

Most recurring issues (from conversation history):

1. **Baseline handling** - 5 separate bugs, all fixed and tested ✅
2. **Table formatting** - 4+ mentions, partially tested ⚠️
3. **Cargo.toml contamination** - 3+ mentions, not tested ❌
4. **Spec/version extraction** - 6+ mentions, partially tested ⚠️
5. **Error deduplication** - 5+ mentions, implemented but not tested ⚠️

## Next Session Checklist

When working on tests, tackle in this order:

1. ✅ Read this document
2. ⬜ Create `tests/cargo_toml_restoration_test.rs`
3. ⬜ Add strict assertions to `tests/default_baseline_wip_test.rs`
4. ⬜ Create `tests/metadata_extraction_test.rs`
5. ⬜ Create `tests/output_format_test.rs`
6. ⬜ Create `tests/error_handling_test.rs`
7. ⬜ Add pattern validation to `tests/table_alignment_test.rs`
8. ⬜ Run all tests and verify they catch the documented bugs

## See Also

- `REGRESSION_TEST_COVERAGE.md` - Full analysis with code examples
- `BUGFIXES.md` - Bugs fixed during refactoring
- `CLAUDE.md` - Common mistakes section
- `SESSION_SUMMARY.md` - Known bugs list
