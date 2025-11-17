# Integration Test Fixtures

This directory contains test fixtures for offline integration testing of cargo-crusader.

## Structure

### Base Crate Versions

- **`base-crate-v1/`** - Version 0.1.0 (baseline)
  - Provides `stable_api()` and `old_api()`
  - Represents the published/baseline version

- **`base-crate-v2/`** - Version 0.2.0 (work-in-progress)
  - Provides `stable_api()` and `new_api()`
  - Removes `old_api()` - **breaking change**
  - Represents the local work-in-progress version

### Dependent Crates

Each dependent tests a specific result state:

#### 1. `dependent-passing/`
- **Uses:** Only `stable_api()` (exists in both versions)
- **Baseline (v1):** ✓ check passes, ✓ test passes
- **Override (v2):** ✓ check passes, ✓ test passes
- **Expected Result:** `PASSED`

#### 2. `dependent-regressed/`
- **Uses:** `old_api()` (removed in v2)
- **Baseline (v1):** ✓ check passes, ✓ test passes
- **Override (v2):** ✗ check fails (compilation error)
- **Expected Result:** `REGRESSED`

#### 3. `dependent-broken/`
- **Uses:** Has type error (returns `42` from function expecting `String`)
- **Baseline (v1):** ✗ check fails (type mismatch)
- **Override (v2):** (not tested - baseline already failed)
- **Expected Result:** `BROKEN`

#### 4. `dependent-test-passing/`
- **Uses:** Only `stable_api()`
- **Baseline (v1):** ✓ check passes, ✓ test passes
- **Override (v2):** ✓ check passes, ✓ test passes
- **Expected Result:** `PASSED`

#### 5. `dependent-test-failing/`
- **Library code:** Uses only `stable_api()` (compiles with both)
- **Test code:** Calls `old_api()` directly
- **Baseline (v1):** ✓ check passes, ✓ test passes
- **Override (v2):** ✓ check passes, ✗ test fails (old_api doesn't exist)
- **Expected Result:** `REGRESSED` (test-time regression)

## Usage

These fixtures are designed for offline testing without requiring crates.io access.

### Manual Testing

Test with baseline version (v1):
```bash
cargo check --manifest-path test-crates/integration-fixtures/dependent-passing/Cargo.toml
cargo test --manifest-path test-crates/integration-fixtures/dependent-passing/Cargo.toml
```

Test with override version (v2):
```bash
# Manually edit dependent's Cargo.toml to point to base-crate-v2
# Or use cargo's path override mechanism
```

### Automated Testing

The integration test suite in `tests/offline_integration.rs` uses these fixtures to verify all result states are correctly detected.

## Verification

Run these commands to verify the fixtures work correctly:

```bash
# Should pass
cargo check --manifest-path test-crates/integration-fixtures/dependent-passing/Cargo.toml

# Should pass with v1
cargo check --manifest-path test-crates/integration-fixtures/dependent-regressed/Cargo.toml

# Should fail (type error)
cargo check --manifest-path test-crates/integration-fixtures/dependent-broken/Cargo.toml

# Should pass check, pass tests with v1
cargo test --manifest-path test-crates/integration-fixtures/dependent-test-failing/Cargo.toml
```

## Adding New Fixtures

To add new test scenarios:

1. Create a new directory under `integration-fixtures/`
2. Add `Cargo.toml` with dependency on `base-crate-v1`
3. Create `src/lib.rs` with the test scenario
4. Document the expected result in this README
5. Add corresponding test case in `tests/offline_integration.rs`
