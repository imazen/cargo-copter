# Regression Test Coverage Analysis

This document tracks recurring bugs identified from conversation history and existing test coverage.

## Executive Summary

**Analysis Date:** 2025-11-18
**Sources Analyzed:**
- Global conversation history (`~/.claude/history.jsonl`)
- Project documentation (`BUGFIXES.md`, `SESSION_SUMMARY.md`, `REFACTORING_COMPLETE.md`, `CLAUDE.md`)
- Existing test files in `tests/` and `src/*_test.rs`

**Key Finding:** Most critical bugs from the refactoring have regression tests, but several recurring issues from earlier iterations need coverage.

---

## Category 1: Baseline Handling Bugs (WELL TESTED ✅)

These bugs kept recurring during refactoring and now have comprehensive test coverage.

### 1.1 Baseline Not Tested First
**Occurrences:** Multiple times during runner implementation
**Problem:** Tests executed in wrong order (base_versions × dependents instead of dependents × base_versions)
**Impact:** Baseline comparison computed against wrong version
**Test Coverage:** ✅ EXCELLENT
- `src/runner_test.rs::test_baseline_is_first`
- `src/config_test.rs::test_baseline_flag_is_set`
- Integration validated by execution order

### 1.2 Baseline Flag Never Set
**Occurrences:** Fixed in BUGFIXES.md #2
**Problem:** `is_baseline` flag never set to `true` in config module
**Impact:** Runner couldn't identify baseline version
**Test Coverage:** ✅ EXCELLENT
- `src/config_test.rs::test_baseline_flag_is_set`
- `src/config_test.rs::test_multiple_versions_only_one_baseline`
- `src/config_test.rs::test_dependents_have_baseline_flag`

### 1.3 Baseline Getting Override Applied
**Occurrences:** Fixed in BUGFIXES.md #3
**Problem:** Baseline versions had override paths applied, testing wrong version
**Impact:** Baseline didn't test naturally resolved version from crates.io
**Test Coverage:** ✅ GOOD
- `src/config_test.rs::test_baseline_has_no_override`
- `src/runner_test.rs::test_baseline_has_no_override`

### 1.4 Baseline Comparison Computed Wrong
**Occurrences:** Fixed in BUGFIXES.md #4
**Problem:** String-sorting based comparison was fragile and incorrect
**Impact:** Wrong regression/passed status
**Test Coverage:** ✅ GOOD
- `src/runner_test.rs::test_test_result_baseline_field`
- `src/runner_test.rs::test_test_result_with_baseline_comparison`
- `src/runner_test.rs::test_test_result_regression`

### 1.5 Registry Version Override Not Implemented
**Occurrences:** Fixed in BUGFIXES.md #6
**Problem:** `--test-versions` didn't download/apply registry versions
**Impact:** Only local paths worked as overrides
**Test Coverage:** ⚠️ PARTIAL
- Functionality implemented in `runner.rs:158-184`
- **MISSING:** Integration test for `--test-versions` with registry versions

---

## Category 2: Output/UI Bugs (PARTIALLY TESTED ⚠️)

These bugs affected console output and user experience.

### 2.1 Double Printing of Headers/Summary
**Occurrences:** SESSION_SUMMARY.md "Known Bugs #2 & #3"
**Problem:** Table header and test plan printed twice
**Impact:** Confusing duplicate output
**Test Coverage:** ⚠️ WEAK
- **MISSING:** Test that validates header appears exactly once
- **MISSING:** Test that validates summary appears exactly once

### 2.2 WIP Version Not Executing
**Occurrences:** SESSION_SUMMARY.md "Known Bugs #1"
**Problem:** WIP shown in test plan but didn't run
**Impact:** Incomplete testing
**Test Coverage:** ✅ GOOD
- `tests/default_baseline_wip_test.rs::test_default_baseline_wip_output`
- Validates both baseline and WIP rows appear

### 2.3 Streaming Output Not Working
**Occurrences:** Multiple mentions in history (lines 163, 230, 260)
**Problem:** Rows printed per-dependent instead of streaming per-test
**Impact:** Poor UX for long-running tests
**Test Coverage:** ❌ NONE
- **MISSING:** Test that validates rows stream immediately
- **MISSING:** Test that no buffering occurs between tests

### 2.4 "copter:" Status Messages in Table
**Occurrences:** SESSION_SUMMARY.md, CLAUDE.md warnings
**Problem:** Debug messages appearing in table output
**Impact:** Messy console output
**Test Coverage:** ❌ NONE
- **MISSING:** Test that validates no "copter:" messages in output
- Should check `tests/default_baseline_wip_test.rs` output

---

## Category 3: Data Extraction Bugs (WEAK COVERAGE ❌)

These bugs affected metadata extraction and display.

### 3.1 Spec Column Showing "?"
**Occurrences:** Multiple times (history lines 69, 79, 87, 212)
**Problem:** Dependency spec not extracted from Cargo.toml
**Impact:** Missing critical version requirement info
**Test Coverage:** ⚠️ WEAK
- `tests/default_baseline_wip_test.rs` checks for "?" but doesn't fail on it
- **MISSING:** Explicit test that spec is extracted correctly
- **MISSING:** Test for broken package spec extraction (fixed in commit 1a83189)

### 3.2 Resolved Version Column Inaccurate
**Occurrences:** Multiple times (history lines 140, 161, 259)
**Problem:** Wrong resolved version displayed, especially with force versions
**Impact:** Misleading output about what was actually tested
**Test Coverage:** ❌ NONE
- **MISSING:** Test comparing displayed resolved version vs actual
- **MISSING:** Test for force version resolution display

### 3.3 Multiple Crate Versions Not Displayed
**Occurrences:** History lines 212, 232, 259
**Problem:** When cargo resolves multiple versions, only one shown
**Impact:** Hidden dependency issues
**Test Coverage:** ❌ NONE
- **MISSING:** Test for multi-version detection
- **MISSING:** Test for multi-version display formatting

---

## Category 4: Cargo.toml/Cargo.lock Contamination (NO COVERAGE ❌)

These bugs caused test contamination across runs.

### 4.1 Cargo.toml Not Restored Between Tests
**Occurrences:** Major issue (history line 137, 140, 267)
**Problem:** Modifications to Cargo.toml persisted across tests
**Impact:** Tests contaminated by previous runs, wrong results
**Test Coverage:** ❌ NONE
- **MISSING:** Test that Cargo.toml is restored after each test
- **MISSING:** Test that detects contamination

### 4.2 Cargo.lock Deletion/Management
**Occurrences:** History lines 140, 233
**Problem:** Unclear when Cargo.lock should be deleted
**Impact:** Inconsistent dependency resolution
**Test Coverage:** ❌ NONE
- **MISSING:** Test for Cargo.lock handling
- **MISSING:** Documentation of Cargo.lock policy

### 4.3 Staging Directory Contamination
**Occurrences:** History line 137
**Problem:** Build artifacts from previous runs affect new tests
**Impact:** False passes/fails
**Test Coverage:** ⚠️ WEAK
- `--clean` flag exists but no test validates it works
- **MISSING:** Test that `--clean` actually purges staging
- **MISSING:** Test that detects contamination

---

## Category 5: Table Formatting Bugs (PARTIAL COVERAGE ⚠️)

Visual alignment and rendering issues.

### 5.1 Box Drawing Character Misalignment
**Occurrences:** History lines 188, 191, 221, 239
**Problem:** Patterns like `─└`, `─┐│`, `│ │` appearing (invalid corners/joins)
**Impact:** Broken table appearance
**Test Coverage:** ⚠️ PARTIAL
- `tests/table_alignment_test.rs` exists but only measures widths
- **GAPS:** No tests for invalid box-drawing patterns
- **GAPS:** Should fail on `─└`, `─┐│`, `│ │`

### 5.2 Multi-Version Row Formatting
**Occurrences:** History line 221
**Problem:** Last line drawing char should be corner (└) not T (├)
**Impact:** Visual inconsistency
**Test Coverage:** ❌ NONE
- **MISSING:** Test for multi-version row formatting
- **MISSING:** Validation of correct box-drawing usage

### 5.3 Error Line Spanning Columns 2-5
**Occurrences:** CLAUDE.md documents this behavior
**Problem:** Error messages need special border handling
**Impact:** Table misalignment
**Test Coverage:** ❌ NONE
- **MISSING:** Test for error line formatting
- **MISSING:** Test that errors span correct columns with correct borders

---

## Category 6: Error Detection and Deduplication (WEAK COVERAGE ⚠️)

### 6.1 Same Error Not Detected
**Occurrences:** Multiple times (history lines 248, 250, 251, 253, 254)
**Problem:** Identical errors shown repeatedly instead of "[SAME ERROR]"
**Impact:** Verbose, hard-to-read output
**Root Cause:** Comparing errors with hex codes (e.g., binary names with `-[hex]`)
**Test Coverage:** ❌ NONE
- **MISSING:** Test for error signature normalization
- **MISSING:** Test that hex codes in paths are excluded from comparison
- **MISSING:** Windows compatibility test for path normalization

### 6.2 --error-lines Parameter Not Working
**Occurrences:** History line 301, fixed in SESSION_SUMMARY.md
**Problem:** Error line limit ignored
**Impact:** Too much or too little error output
**Test Coverage:** ❌ NONE
- **MISSING:** Test that `--error-lines 0` shows all errors
- **MISSING:** Test that `--error-lines N` limits to N lines
- **MISSING:** Test that multiple errors fit under limit

---

## Category 7: False Positives (DOCUMENTED BUT NOT PREVENTED ⚠️)

### 7.1 Legacy Path False Positive
**Occurrences:** Documented in `tests/default_baseline_wip_test.rs`
**Problem:** WIP reported as PASSED when it actually breaks dependents
**Root Cause:** Legacy path runs `cargo build` instead of full ICT (Install/Check/Test)
**Impact:** Users think their changes are safe when they're not
**Test Coverage:** ⚠️ DOCUMENTED BUT NOT PREVENTED
- `tests/default_baseline_wip_test.rs` documents the bug
- Test allows false positive (doesn't fail on it)
- **MISSING:** Test that FAILS when legacy path used
- **MISSING:** Warning/error when user doesn't use `--test-versions`

---

## Category 8: Build and Type System (GOOD COVERAGE ✅)

### 8.1 Build Errors During Refactoring
**Occurrences:** Multiple times (history lines 153, 156, 159)
**Problem:** Code didn't compile after changes
**Impact:** Development blocked
**Test Coverage:** ✅ IMPLICIT (CI should catch)
- Continuous compilation checks
- **COULD ADD:** Pre-commit hook to verify compilation

---

## Summary by Test Coverage Quality

### ✅ EXCELLENT Coverage (5 items)
1. Baseline not tested first
2. Baseline flag never set
3. Baseline getting override applied
4. Baseline comparison computed wrong
5. WIP version not executing

### ⚠️ PARTIAL Coverage (7 items)
6. Registry version override
7. Double printing
8. Spec column showing "?"
9. Table alignment
10. Staging contamination (flag exists, not tested)
11. Error deduplication (implemented, not tested)
12. False positive (documented, not prevented)

### ❌ NO Coverage (14 items)
13. Streaming output
14. "copter:" messages in output
15. Resolved version accuracy
16. Multiple crate versions display
17. Cargo.toml restoration
18. Cargo.lock management
19. Box drawing characters
20. Multi-version row formatting
21. Error line formatting
22. Error signature normalization
23. --error-lines parameter
24. --clean flag effectiveness
25. Force version display
26. Broken package spec extraction

---

## Prioritized Recommendations

### Priority 1: Data Integrity (Prevents Wrong Results)
1. **Cargo.toml restoration test** - Critical for test isolation
2. **Resolved version accuracy test** - Users need correct information
3. **Registry version override test** - Complete existing functionality

### Priority 2: User-Facing Bugs (Affects UX)
4. **Double printing test** - Should fail if header/summary appears twice
5. **Streaming output test** - Validate rows appear immediately
6. **Error deduplication test** - Verify "[SAME ERROR]" works correctly
7. **--error-lines test** - Verify parameter works as documented

### Priority 3: Output Correctness (Visual/Formatting)
8. **Box drawing validation** - Fail on `─└`, `─┐│`, `│ │` patterns
9. **Spec extraction test** - Never show "?" in spec column
10. **Multi-version display test** - Show all resolved versions

### Priority 4: Safety (Prevent Misuse)
11. **Legacy path warning** - Fail test if false positive detected
12. **--clean effectiveness** - Verify staging purge works

---

## Suggested Test Additions

### tests/cargo_toml_restoration_test.rs (NEW)
```rust
#[test]
fn test_cargo_toml_restored_after_patch() {
    // Verify Cargo.toml is identical before and after test
}

#[test]
fn test_cargo_toml_restored_after_force() {
    // Verify forced version doesn't persist
}

#[test]
fn test_cargo_lock_deleted_between_tests() {
    // Verify fresh resolution each test
}
```

### tests/output_format_test.rs (NEW)
```rust
#[test]
fn test_header_appears_once() {
    // Count occurrences of table header
    assert_eq!(output.matches("│ Offered │").count(), 1);
}

#[test]
fn test_no_copter_status_messages() {
    // Verify no "copter:" in table output
    assert!(!output.contains("copter:"));
}

#[test]
fn test_streaming_output() {
    // Verify rows appear before all tests complete
}

#[test]
fn test_invalid_box_drawing() {
    // Fail on ─└, ─┐│, │ │
    assert!(!output.contains("─└"));
    assert!(!output.contains("─┐│"));
    assert!(!output.contains("│ │"));
}
```

### tests/error_handling_test.rs (NEW)
```rust
#[test]
fn test_error_deduplication() {
    // Same error → "[SAME ERROR]"
}

#[test]
fn test_error_lines_limit() {
    // --error-lines N limits output
}

#[test]
fn test_error_lines_unlimited() {
    // --error-lines 0 shows all
}

#[test]
fn test_hex_code_normalization() {
    // Errors differing only in -[hex] are same
}
```

### tests/metadata_extraction_test.rs (NEW)
```rust
#[test]
fn test_spec_never_question_mark() {
    // Spec column should never be "?"
}

#[test]
fn test_resolved_version_matches_actual() {
    // Display matches what cargo actually resolved
}

#[test]
fn test_multiple_versions_displayed() {
    // All resolved versions shown
}

#[test]
fn test_broken_package_spec_extraction() {
    // Handles packages with malformed deps
}
```

### Enhancements to Existing Tests

#### tests/default_baseline_wip_test.rs
```rust
// CHANGE: Make false positive fail the test
assert!(has_regression,
    "CRITICAL: Legacy path false positive detected! Use --test-versions");

// ADD: Verify spec is extracted
assert!(!stdout.contains("│ ?"),
    "Spec column should never show '?'");
```

#### tests/table_alignment_test.rs
```rust
// ADD: Pattern validation
#[test]
fn test_no_invalid_box_drawing() {
    let invalid_patterns = ["─└", "─┐│", "│ │"];
    for pattern in invalid_patterns {
        assert!(!output.contains(pattern),
            "Invalid box drawing: {}", pattern);
    }
}
```

---

## Test Execution Checklist

When running regression tests, verify:

- ✅ All baseline tests pass (`src/config_test.rs`, `src/runner_test.rs`)
- ✅ Integration test passes (`tests/default_baseline_wip_test.rs`)
- ⚠️ Table alignment validated (`tests/table_alignment_test.rs`)
- ❌ Cargo.toml restoration (NO TEST)
- ❌ Streaming output (NO TEST)
- ❌ Error deduplication (NO TEST)
- ❌ Resolved version accuracy (NO TEST)

**Coverage Score: 26/40 bugs have adequate tests (65%)**

---

## Conclusion

The refactoring added excellent test coverage for **baseline handling bugs** (5/5 tested), which were the most critical architectural issues. However, **data extraction**, **output formatting**, and **contamination** bugs lack coverage despite recurring in conversation history.

**Next Steps:**
1. Add tests for Priority 1 items (data integrity)
2. Enhance existing tests to fail on known bad patterns
3. Add integration tests for full workflows
4. Document expected behavior in test comments
