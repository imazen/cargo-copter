# CLAUDE.md

AI assistant guidance for working with this codebase.

## Project

Cargo Copter tests downstream impact of Rust crate changes by building reverse dependencies against both published and work-in-progress versions.

**⚠️ SECURITY**: Executes arbitrary code from crates.io. Use sandboxed environments.

## ⚠️ Common Mistakes to Avoid

**Before ANY refactoring, ALWAYS:**
1. Read ALL relevant source files completely
2. Check integration tests in `tests/` directory
3. Run `cargo test --test default_baseline_wip_test -- --ignored --nocapture` to verify output
4. Verify streaming output still works (rows appear as tests complete)

**Critical Bugs to Watch:**
- **Double printing**: Check for duplicate headers/summaries when adding output
- **Lost output**: Ensure streaming callback actually prints (not just stores data)
- **WIP not tested**: Verify WIP version actually executes, not just appears in plan
- **Baseline ordering**: First test per dependent MUST be baseline with `override_mode=None`

**Integration Test Requirements:**
- Table rows must stream (print immediately after each test completes)
- No "copter:" status messages in table output
- Table header printed exactly once
- WIP version must actually execute (not just be queued)

## Quick Commands

```bash
cargo build --release
cargo test
./target/release/cargo-copter --path ~/rust-rgb --top-dependents 1
./target/release/cargo-copter --crate rgb --test-versions "0.8.50 0.8.51"
```

## Architecture Overview

The codebase uses a **unified multi-version architecture** where all test scenarios (baseline, WIP, explicit versions) are represented uniformly:

```
CLI Args → Config Resolution → Test Matrix → Runner → Results → Reports
   ↓            ↓                   ↓           ↓         ↓         ↓
Parse      Validate/Resolve     Immutable   Execute   Uniform   Display
```

## Key Modules

### Core Architecture (New)

- **`src/config.rs`** - Configuration resolution and validation
  - `build_test_matrix()` - Converts CLI args to immutable TestMatrix
  - Resolves version keywords ("this", "latest")
  - Validates paths and determines baselines

- **`src/runner.rs`** - Test execution engine
  - `run_tests()` - Executes the 2D test matrix
  - Resolves `Version::Latest` at runtime
  - Computes baseline comparisons post-execution

- **`src/types.rs`** - Core type system
  - `VersionedCrate` - Universal crate representation (base or dependent)
  - `VersionSpec` - Version with override mode and baseline flag
  - `TestMatrix` - Complete test specification
  - `TestResult` - Uniform result type
  - `TestStatus` - Type-safe status enum

- **`src/bridge.rs`** - Compatibility layer
  - Converts new `TestResult` to legacy `OfferedRow` for reports
  - Temporary during migration period

### Supporting Modules

- **`src/main.rs`** - Clean orchestration (286 lines, down from 1546!)
  - Parses CLI, builds matrix, runs tests, generates reports
  - No business logic - pure flow control

- **`src/cli.rs`** - Argument parsing (clap), supports space-delimited values

- **`src/api.rs`** - crates.io API client (paginated, 100/page)

- **`src/compile.rs`** - Three-step ICT (Install/Check/Test) execution
  - `run_three_step_ict()` - Runs fetch/check/test with early stopping
  - `TestConfig` - Builder pattern for test configuration

- **`src/report.rs`** - Report generation and formatting
  - Five-column console table
  - Error deduplication with signatures
  - Comparison statistics
  - Markdown and JSON export

- **`src/console_format.rs`** - Pure rendering (no business logic)
  - Table layout and borders
  - Color terminal output
  - Text truncation

- **`src/error_extract.rs`** - JSON diagnostic parsing

- **`src/download.rs`** - Crate downloading and caching

- **`src/manifest.rs`**, **`src/metadata.rs`**, **`src/version.rs`** - Cargo integration

- **`src/git.rs`**, **`src/ui.rs`**, **`src/toml_helpers.rs`** - Utilities

## Core Data Flow

### 1. Configuration Phase (`config.rs`)

```rust
CliArgs → build_test_matrix() → TestMatrix {
    base_crate: String,
    base_versions: Vec<VersionSpec>,     // All versions to test
    dependents: Vec<VersionSpec>,        // All dependents to test
    staging_dir, skip_check, skip_test, error_lines
}
```

**Key invariants:**
- All paths validated/resolved
- Version keywords parsed (Latest variants remain for runtime resolution)
- Baseline flags set correctly
- Override modes determined

### 2. Execution Phase (`runner.rs`)

```rust
TestMatrix → run_tests() → Vec<TestResult> {
    base_version: VersionedCrate,        // Which base version
    dependent: VersionedCrate,           // Which dependent
    execution: ThreeStepResult,          // ICT results
    baseline: Option<BaselineComparison> // Comparison data
}
```

**Process:**
1. Resolve `Version::Latest` to concrete versions
2. Iterate 2D matrix: for each dependent, test each base_version
3. Run three-step ICT for each pair
4. Attach baseline comparisons post-execution

### 3. Reporting Phase (`report.rs` + `bridge.rs`)

```rust
Vec<TestResult> → bridge → Vec<OfferedRow> → reports
```

The bridge layer converts the new unified types to legacy `OfferedRow` format for existing report generation.

## Core Type System

### Version Representation

```rust
// Universal crate representation
pub struct VersionedCrate {
    pub name: String,
    pub version: Version,  // Semver | Git | Latest
    pub source: CrateSource,  // Registry | Local | Git
}

// Version identifier
pub enum Version {
    Semver(String),        // "0.8.52"
    Git { rev: String },   // "abc123f"
    Latest,                // Resolved at runtime
}

// Source location
pub enum CrateSource {
    Registry,              // crates.io
    Local { path: PathBuf },  // Filesystem
    Git { url, rev },      // Git repo
}
```

### Test Specification

```rust
// A version to test (base or dependent)
pub struct VersionSpec {
    pub crate_ref: VersionedCrate,
    pub override_mode: OverrideMode,  // None | Patch | Force
    pub is_baseline: bool,  // ✓ Explicit flag, not position-based!
}

// Test matrix - 2D: base_versions × dependents
pub struct TestMatrix {
    pub base_versions: Vec<VersionSpec>,
    pub dependents: Vec<VersionSpec>,
    // ... config
}
```

**Key design decision:** Baseline is **metadata** (`is_baseline: bool`), not position or Option-based!

### Test Results

```rust
pub struct TestResult {
    pub base_version: VersionedCrate,
    pub dependent: VersionedCrate,
    pub execution: ThreeStepResult,
    pub baseline: Option<BaselineComparison>,
}

pub enum TestStatus {
    Baseline { passed: bool },  // This IS the baseline
    Passed,                     // Baseline passed, this passed
    Regressed,                  // Baseline passed, this failed
    Fixed,                      // Baseline failed, this passed
    StillBroken,                // Baseline failed, this failed
}
```

**Benefits:**
- ✅ Type-safe status (no Option unwrapping!)
- ✅ Exhaustive matching enforced by compiler
- ✅ Clear semantics

## Test Classification

Test results are classified based on baseline comparison:

- **Baseline** - First version tested (reference point)
- **Passed** - Baseline passed, offered passed → ✓
- **Regressed** - Baseline passed, offered failed → ✗
- **Fixed** - Baseline failed, offered passed → ✓ (rare)
- **StillBroken** - Baseline failed, offered failed → ⚠

## Console Table Format

Five columns: **Offered | Spec | Resolved | Dependent | Result**

**Key behaviors:**
- Baseline row: `- baseline`
- Offered row: `{icon} {resolution}{version} [{forced}]`
- Icons: ✓ (tested pass), ✗ (tested fail), ⊘ (skipped), - (baseline)
- Resolution: = (exact), ↑ (upgraded), ≠ (mismatch/forced)
- **Error lines**: Span columns 2-5, borders on 2 & 4 only
- **Error deduplication**: Repeated errors show `[SAME ERROR]` instead of full text
- **Multi-version rows**: Use `├─` prefixes in columns 2-4
- **Separators**: Full horizontal line between different dependents

## Override Mechanisms

**Patch mode** (default): `[patch.crates-io]` respects semver

```rust
VersionSpec {
    override_mode: OverrideMode::Patch,
    // ...
}
```

**Force mode** (`--force-versions`): Direct dependency replacement, bypasses semver

```rust
VersionSpec {
    override_mode: OverrideMode::Force,
    // ...
}
```

## Caching

Default cache location (platform-specific user cache directory):
- Linux: `~/.cache/cargo-copter/`
- macOS: `~/Library/Caches/cargo-copter/`
- Windows: `%LOCALAPPDATA%/cargo-copter/`

Contents:
- `staging/{crate}-{version}/` - Unpacked sources + build artifacts
- `crate-cache/` - Downloaded .crate files
- Provides **10x speedup** on reruns

## CLI Flags

```bash
--test-versions <VER>...     # Multiple versions, space-delimited supported
--force-versions <VER>...    # Bypass semver requirements
--features <FEATURES>...     # Passed to cargo fetch/check/test
--crate <NAME>               # Test published crate without local source
--only-fetch                 # Only fetch dependencies (skip check and test)
--only-check                 # Only fetch and check (skip tests)
--clean                      # Purge staging directory before running tests
--error-lines <N>            # Max lines to show per error (default: 10, 0=unlimited)
--top-dependents <N>         # Test top N dependents by downloads
--dependents <CRATE>...      # Specific dependents to test (name or name:version)
```

**Examples:**
```bash
# Test local WIP against top dependents
cargo-copter --path ./ --top-dependents 10

# Test multiple published versions
cargo-copter --crate rgb --test-versions "0.8.48 0.8.50 0.8.51"

# Force-test incompatible version
cargo-copter --test-versions 0.7.0 --force-versions 0.7.0

# Clean cache and retest
cargo-copter --clean --top-dependents 5

# Show more error details
cargo-copter --error-lines 50 --top-dependents 5
```

## Common Workflows

### Test local WIP against top dependents
```bash
cd ~/my-crate
cargo-copter --top-dependents 10
```

### Test multiple versions of published crate
```bash
cargo-copter --crate rgb --test-versions "0.8.48 0.8.50 0.8.51"
```

### Force test incompatible version
```bash
cargo-copter --test-versions 0.7.0 --force-versions 0.7.0
```

### Clean cache and retest
```bash
cargo-copter --clean --top-dependents 5
```

### Show more error details
```bash
cargo-copter --error-lines 50 --top-dependents 5  # 50 lines per error
cargo-copter --error-lines 0 --top-dependents 5   # Unlimited
```

## Development Notes

### Adding a New Version Source

1. Add variant to `CrateSource` enum in `types.rs`
2. Update `runner.rs` to handle resolution
3. Update `config.rs` to parse from CLI
4. Update `bridge.rs` for compatibility

### Debugging

Enable debug logging:
```bash
RUST_LOG=debug cargo run -- --path ./ --top-dependents 1
```

### Testing

```bash
cargo test
cargo build --release
./target/release/cargo-copter --help
```

## Architecture Principles

1. **No special cases** - Baseline is just a flag, not a position
2. **Immutable config** - TestMatrix is validated upfront, never modified
3. **Late resolution** - `Version::Latest` resolved at runtime for freshness
4. **Type safety** - Impossible states prevented by types
5. **Single responsibility** - Each module has one job
6. **Explicit over implicit** - Baseline flags, override modes are explicit

## Migration Notes

The codebase recently underwent a major refactoring from 1546 lines in main.rs to ~300 lines across well-organized modules:

- **Before**: Position-based baseline detection, Option-based dispatch, special cases everywhere
- **After**: Explicit metadata, enum-based dispatch, uniform execution paths

The `bridge.rs` module provides compatibility with legacy `OfferedRow` types during the transition period.
- memorize always read in all rust files before refactoring anything