# Cargo Behavior: fetch/check/test

This document explains how Cargo's commands work and why we use this specific three-step pipeline.

## The Three-Step Pipeline

cargo-copter runs three Cargo commands in sequence:

```
┌─────────────┐     ┌─────────────┐     ┌─────────────┐
│ cargo fetch │────►│ cargo check │────►│ cargo test  │
└─────────────┘     └─────────────┘     └─────────────┘
     │                    │                    │
     ▼                    ▼                    ▼
 Download &          Type-check &         Compile &
 resolve deps        build deps           run tests
```

### Why This Order?

Each step provides increasingly more information at increasingly more cost:

| Step | Time | What It Tells Us |
|------|------|------------------|
| fetch | ~2-10s | Dependency resolution works |
| check | ~10-60s | Code compiles (type-safe) |
| test | ~30-300s | Tests pass |

By running them in order and stopping on failure, we:
1. Get fast feedback when deps don't resolve
2. Don't waste time running tests when code doesn't compile
3. Identify exactly which step failed

## Step 1: `cargo fetch`

**Command**: `cargo fetch --locked` (or just `cargo fetch`)

**What it does**:
1. Reads `Cargo.toml` to understand dependencies
2. Resolves the dependency graph (finds compatible versions)
3. Downloads all `.crate` files from crates.io
4. Writes `Cargo.lock` with the resolved versions

**Success means**: All dependencies can be resolved and downloaded.

**Common failures**:
- **Version conflict**: "multiple versions of crate X in dependency graph"
- **Network error**: Can't reach crates.io
- **Invalid spec**: Version constraint is impossible to satisfy

**Why fetch first?**
- It's the fastest step (no compilation)
- If deps can't resolve, nothing else matters
- Version conflicts are detected here

**Example output on conflict**:
```
error: there are multiple different versions of crate `rgb` in the dependency graph
  --> /path/to/Cargo.toml:10:1
   |
10 | rgb = "0.8"
   | ^^^
   |
   = note: `rgb` is used by `image`, `gifski`, etc.
```

## Step 2: `cargo check`

**Command**: `cargo check --message-format=json`

**What it does**:
1. Compiles all dependencies (cached if unchanged)
2. Type-checks the crate being tested
3. Performs borrow checking, lifetime analysis
4. Does NOT generate final binaries

**Success means**: The code compiles correctly.

**Common failures**:
- **Type mismatch**: API changed incompatibly
- **Missing method**: Method removed or renamed
- **Trait bound not satisfied**: Trait implementation changed

**Why check instead of build?**
- `check` is 2-5x faster than `build`
- No need for final code generation for compatibility testing
- Catches all type errors that `build` would

**Example output on API break**:
```json
{
  "reason": "compiler-message",
  "message": {
    "code": "E0599",
    "message": "no method named `from_hex` found for struct `Rgb`"
  }
}
```

## Step 3: `cargo test`

**Command**: `cargo test --no-fail-fast --message-format=json`

**What it does**:
1. Compiles the test harness (if not already)
2. Runs all `#[test]` functions
3. Runs doc tests
4. Reports pass/fail for each test

**Success means**: All tests pass.

**Common failures**:
- **Test assertion**: `assert_eq!` failed
- **Panic**: Code panicked during test
- **Timeout**: Test took too long

**Why run tests?**
- Catches behavioral changes that compile but produce wrong results
- Validates that API changes are semantically correct
- Most thorough compatibility check

**Example output**:
```
running 42 tests
test rgb::from_hex ... FAILED
test rgb::to_hex ... ok
```

## Early Stopping

The pipeline stops at the first failure:

```rust
// From compile/mod.rs
if !fetch_result.success {
    return Ok(ThreeStepResult {
        fetch: fetch_result,
        check: None,  // Not run
        test: None,   // Not run
        ...
    });
}
```

**Rationale**:
- No point checking code that can't fetch dependencies
- No point testing code that doesn't compile
- Saves significant time on failures

## Skip Flags

Users can skip later steps:

| Flag | Effect |
|------|--------|
| `--only-fetch` | Run fetch only, skip check and test |
| `--only-check` | Run fetch and check, skip test |

**Use cases**:
- `--only-fetch`: Quick version compatibility check
- `--only-check`: Find compile errors without waiting for tests

## Message Format: JSON

We use `--message-format=json` for structured error parsing:

```bash
cargo check --message-format=json
```

This outputs:
```json
{"reason":"compiler-message","message":{"code":"E0599",...}}
{"reason":"build-finished","success":false}
```

**Why JSON?**
- Structured parsing of error codes
- Can extract specific error messages
- Distinguish between "compile error" and "other failure"

## Caching

Cargo caches compiled artifacts in `target/`:

```
staging/
└── image-0.24.0/
    ├── Cargo.toml
    ├── src/
    └── target/          ← Cached build artifacts
        ├── debug/
        └── .cargo-lock
```

**Benefits**:
- Subsequent runs are 2-10x faster
- Only changed dependencies recompile
- Incremental compilation works

**Our caching strategy**:
- Keep staging directories between runs (default)
- `--clean` flag to purge and start fresh

## Environment Variables

We pass specific env vars to Cargo:

| Variable | Value | Purpose |
|----------|-------|---------|
| `CARGO_TERM_COLOR` | `always` | Colored output for logs |
| `RUSTFLAGS` | (user-configurable) | Custom compiler flags |

## Features

The `--features` flag is passed to all Cargo commands:

```bash
cargo fetch --features "serde,std"
cargo check --features "serde,std"
cargo test --features "serde,std"
```

This ensures consistent feature resolution across all steps.

## Timeout Handling

Each step has a timeout (not currently configurable):

| Step | Default Timeout |
|------|-----------------|
| fetch | 5 minutes |
| check | 10 minutes |
| test | 30 minutes |

If a step times out, it's treated as a failure.

## Exit Codes

Cargo exit codes we care about:

| Code | Meaning |
|------|---------|
| 0 | Success |
| 1 | Compile error or test failure |
| 101 | Cargo itself failed (e.g., couldn't parse Cargo.toml) |

We treat non-zero as failure.

## Practical Examples

### Example 1: Clean Path (All Pass)

```
$ cargo-copter --crate rgb --top-dependents 1

fetch: ✓ 2.3s
check: ✓ 15.2s
test:  ✓ 42.1s

Total: 59.6s
Result: PASSED
```

### Example 2: Fetch Fails (Version Conflict)

```
$ cargo-copter --crate rgb --force-versions 0.7.0 --top-dependents 1

fetch: ✗ 1.2s (multiple versions of crate `rgb`)

  Retrying with [patch.crates-io]...

fetch: ✓ 2.1s (with patch)
check: ✓ 18.4s
test:  ✓ 45.2s

Result: PASSED with !! marker
```

### Example 3: Check Fails (API Break)

```
$ cargo-copter --crate rgb --force-versions 0.7.0 --top-dependents 1

fetch: ✓ 2.3s
check: ✗ 12.1s

  error[E0599]: no method named `from_hex` found for struct `Rgb`

test: (skipped - check failed)

Result: REGRESSION
```

### Example 4: Test Fails (Behavioral Change)

```
$ cargo-copter --crate rgb --test-versions 0.8.91 --top-dependents 1

fetch: ✓ 2.1s
check: ✓ 14.8s
test:  ✗ 38.2s

  test rgb::conversion::test_from_hex ... FAILED
  assertion failed: expected 255, got 256

Result: REGRESSION
```

## Files Involved

- `src/compile/executor.rs`: Runs Cargo commands
- `src/compile/types.rs`: `CompileStep` enum and result types
- `src/compile/mod.rs`: Three-step orchestration
- `src/error_extract.rs`: JSON message parsing
