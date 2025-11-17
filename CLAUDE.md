# CLAUDE.md

AI assistant guidance for working with this codebase.

## Project

Cargo Copter tests downstream impact of Rust crate changes by building reverse dependencies against both published and work-in-progress versions.

**⚠️ SECURITY**: Executes arbitrary code from crates.io. Use sandboxed environments.

## Quick Commands

```bash
cargo build --release
cargo test
./target/release/cargo-copter --path ~/rust-rgb --top-dependents 1
./target/release/cargo-copter --crate rgb --test-versions "0.8.50 0.8.51"
```

## Key Files

- `src/main.rs` - Orchestration, multi-version testing flow
- `src/cli.rs` - Argument parsing (clap), supports space-delimited values
- `src/api.rs` - crates.io API (paginated, 100/page)
- `src/compile.rs` - Three-step ICT (Install/Check/Test) logic
- `src/report.rs` - Five-column console table, HTML/markdown generation
- `src/error_extract.rs` - JSON diagnostic parsing

## Core Data Structures

See **[CONSOLE-FORMAT.md](CONSOLE-FORMAT.md)** for complete data structure definitions:
- `OfferedRow` - Top-level row structure with baseline tracking
- `DependencyRef` - Primary dependency metadata
- `OfferedVersion` - Version being tested
- `TestExecution` - Test command results (fetch/check/test)
- `TransitiveTest` - Multi-version transitive dependencies

Implementation: `src/main.rs` (lines 530-600)

## Architecture

1. **Parse CLI** → Validate args, extract versions (space-delimited supported)
2. **Read Cargo.toml** → Extract crate name/version, capture git state
3. **Query crates.io** → Fetch reverse dependencies (paginated)
4. **ThreadPool testing** → Each dependent tested in parallel
5. **For each dependent**:
   - Download/cache `.crate` file
   - **Baseline test** (published version)
   - **Offered version tests** (WIP or specified versions)
   - **Three-step ICT**: Install (fetch) → Check → Test (early stop on failure)
   - Extract diagnostics from `--message-format=json`
6. **Generate reports** → Console (live), HTML, Markdown (AI-optimized)

## Override Mechanism

**Patch mode** (default): `[patch.crates-io]` respects semver
**Force mode** (`--force-versions`): Direct dependency replacement, bypasses semver

## Test Classification

- **PASSED**: Baseline passed, offered passed → ✓
- **REGRESSED**: Baseline passed, offered failed → ✗
- **BROKEN**: Baseline failed → ✗ (both rows fail)
- **Skipped**: Offered but not tested (resolved elsewhere) → ⊘

## Console Table Format

Five columns: **Offered | Spec | Resolved | Dependent | Result**

**Key behaviors**:
- Baseline row: `- baseline`
- Offered row: `{icon} {resolution}{version} [{forced}]`
- Icons: ✓ (tested pass), ✗ (tested fail), ⊘ (skipped), - (baseline)
- Resolution: = (exact), ↑ (upgraded), ≠ (mismatch/forced)
- **Error lines**: Span columns 2-5, borders on 2 & 4 only
- **Multi-version rows**: Use `├─` prefixes in columns 2-4
- **Separators**: Full horizontal line between different dependents

See **[CONSOLE-FORMAT.md](CONSOLE-FORMAT.md)** for complete specification with 9 demo scenarios.

## Caching

- `.copter/staging/{crate}-{version}/` - Unpacked sources + build artifacts
- `.copter/crate-cache/` - Downloaded .crate files
- Provides **10x speedup** on reruns

## CLI Flags (Updated)

```bash
--test-versions <VER>...     # Multiple versions, space-delimited supported
--force-versions             # Bypass semver requirements
--features <FEATURES>...     # Passed to cargo fetch/check/test
-j, --jobs <N>               # Parallel testing
--crate <NAME>               # Test published crate without local source
```

**Examples**:
```bash
cargo-copter --test-versions "0.8.0 0.8.48" 0.8.91
cargo-copter --crate rgb --test-versions 0.8.50 --force-versions
cargo-copter --features "serde unstable" --jobs 4
```

## Common Workflows

### Test local WIP against top dependents
```bash
cd ~/my-crate
cargo-copter --top-dependents 10 --jobs 4
```

### Test multiple versions of published crate
```bash
cargo-copter --crate rgb --test-versions "0.8.48 0.8.50 0.8.51"
```

### Force test incompatible version
```bash
cargo-copter --test-versions 0.7.0 --force-versions
```

## Next Steps

See [PLAN.md](PLAN.md) for remaining Phase 5+ work:
- Extract original_requirement from dependent's Cargo.toml
- Detect multi-version cargo tree scenarios
- Live crates.io integration tests
- Real-time console printing per dependent
