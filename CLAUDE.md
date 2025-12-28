# CLAUDE.md

AI assistant guidance for cargo-copter.

## What This Tool Does

Tests downstream impact of Rust crate changes by running `cargo fetch/check/test` on reverse dependencies (dependents).

```
base_crate → [dependents] → test each with baseline vs offered versions
```

**Security**: Executes arbitrary code from crates.io. Use sandboxed environments.

## Critical Rules

1. **Read before refactoring**: Always read ALL relevant files before making changes
2. **Run tests after changes**: `cargo test` must pass
3. **Streaming output**: Table rows must print as tests complete (not batched)
4. **Baseline first**: First test per dependent MUST be baseline (no override)

## Quick Commands

```bash
cargo build --release && cargo test
cargo clippy && cargo fmt

# Test local WIP
./target/release/cargo-copter --path ~/my-crate --top-dependents 5

# Test specific versions
./target/release/cargo-copter --crate rgb --test-versions "0.8.50 0.8.91"
```

## Module Structure

```
src/
├── main.rs           # Orchestration only (~280 lines)
├── config.rs         # CLI → TestMatrix
├── runner.rs         # Executes test matrix
├── types.rs          # Core types (VersionedCrate, TestResult, etc.)
├── bridge.rs         # TestResult → OfferedRow (for reports)
├── compile/          # Three-step ICT pipeline
│   ├── mod.rs        # run_three_step_ict()
│   ├── types.rs      # PatchDepth, CompileResult, ThreeStepResult
│   ├── config.rs     # TestConfig builder
│   ├── executor.rs   # Runs cargo commands
│   ├── patching.rs   # Cargo.toml manipulation
│   ├── retry.rs      # Multi-version conflict handling
│   └── logging.rs    # Failure logs
├── report/           # Report generation
│   ├── mod.rs        # Re-exports
│   ├── types.rs      # FormattedRow, StatusIcon, etc.
│   ├── stats.rs      # Summary statistics
│   ├── table.rs      # Console table output
│   ├── simple.rs     # AI-friendly output format
│   └── export.rs     # JSON/Markdown export
└── console_format.rs # Pure rendering
```

## Critical Concepts

### The Three-Step Pipeline

```
cargo fetch → cargo check → cargo test
     │              │             │
     └── Stops on failure ────────┘
```

See `docs/CARGO_BEHAVIOR.md` for details.

### Patching Strategies

When forcing a specific version, the tool escalates:

| Depth | Marker | Method |
|-------|--------|--------|
| None | - | Use Cargo's natural resolution (baseline) |
| Force | `!` | Modify Cargo.toml dependency directly |
| Patch | `!!` | Add `[patch.crates-io]` section |
| DeepPatch | `!!!` | Patch failed, show blocking deps advice |

See `docs/PATCHING_STATE_MACHINE.md` for details.

### Test Classification

| Status | Meaning |
|--------|---------|
| Passed | Baseline passed, offered passed |
| Regressed | Baseline passed, offered failed |
| StillBroken | Baseline failed, offered failed |
| Fixed | Baseline failed, offered passed (rare) |

## Key Types

```rust
// What to test
TestMatrix { base_versions, dependents, staging_dir, ... }

// Result of one test
TestResult { base_version, dependent, execution: ThreeStepResult }

// ICT pipeline result
ThreeStepResult { fetch, check, test, patch_depth, ... }

// For report output
OfferedRow { primary, offered, baseline_passed, test, ... }
```

## Common Pitfalls

| Problem | Cause | Fix |
|---------|-------|-----|
| Double headers | Multiple print calls | Check all output paths |
| Lost output | Callback doesn't print | Verify streaming callback |
| WIP not tested | Added to plan but not executed | Trace execution path |
| Wrong baseline | Position-based detection | Use explicit `is_baseline` flag |

## Integration Tests

```bash
# Run all tests
cargo test

# Run specific integration test with output
cargo test --test default_baseline_wip_test -- --ignored --nocapture
```

Key test files:
- `tests/offline_integration.rs` - Fixture-based tests
- `tests/default_baseline_wip_test.rs` - WIP flow test

## Adding Features

1. **New version source**: Update `types.rs` → `runner.rs` → `config.rs` → `bridge.rs`
2. **New output format**: Add to `report/` module
3. **New CLI flag**: Update `cli.rs` → `config.rs` → relevant module

## Debugging

```bash
RUST_LOG=debug cargo run -- --path ./ --top-dependents 1
```

## Documentation

- `docs/PATCHING_STATE_MACHINE.md` - How version overrides work
- `docs/CARGO_BEHAVIOR.md` - The fetch/check/test pipeline
