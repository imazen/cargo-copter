# Cargo Copter

> Test the downstream impact of crate changes before publishing to crates.io

Test your crate changes with **Cargo Copter** and practice [responsible API evolution](https://github.com/rust-lang/rfcs/blob/master/text/1105-api-evolution.md).

## ⚠️ Security Warning

**CRITICAL**: This program executes arbitrary untrusted code from crates.io. **Always run in sandboxed environments** (Docker, VMs, containers).

---

## Quick Start

```bash
# Install
git clone https://github.com/imazen/cargo-copter
cd cargo-copter
cargo build --release
export PATH=$PATH:$(pwd)/target/release/

# Run
cd /path/to/your/crate
cargo-copter
```

**Output:**
```
Testing 2 reverse dependencies of rgb
  this = 0.8.91 4cc3e60* (your work-in-progress version)

Legend: I=Install (cargo fetch), C=Check (cargo check), T=Test (cargo test)

┌────────────────────┬──────────┬─────────────────┬─────────────────────┬─────────────────────┐
│ Offered            │ Spec     │ Resolved        │ Dependent           │ Result         Time │
├────────────────────┼──────────┼─────────────────┼─────────────────────┼─────────────────────┤
│ - baseline         │ ^0.8.48  │ 0.8.51 📦       │ image 0.25.8        │ PASSED ✓✓✓     2.1s │
│ ✓ =this(0.8.91)    │ ^0.8.48  │ 0.8.91 📁       │ image 0.25.8        │ PASSED ✓✓✓     1.9s │
├────────────────────┼──────────┼─────────────────┼─────────────────────┼─────────────────────┤
│ - baseline         │ ^0.8.0   │ 0.8.51 📦       │ lodepng 3.10.5      │ PASSED ✓✓✓     1.7s │
│ ✓ =this(0.8.91)    │ ^0.8.0   │ 0.8.91 📁       │ lodepng 3.10.5      │ PASSED ✓✓✓     1.5s │
└────────────────────┴──────────┴─────────────────┴─────────────────────┴─────────────────────┘

Summary:
  ✓ Passed:    2
  ✗ Regressed: 0
  ⊘ Skipped:   0

HTML report: copter-report.html
Markdown report: copter-report.md
```

---

## Common Commands

```bash
# Test top 10 dependents
cargo-copter --top-dependents 10

# Test specific crates (supports version pinning)
cargo-copter --dependents image:0.25.8 serde tokio

# Parallel testing with a custom caching dir (10x faster)
cargo-copter --jobs 4 --staging-dir .copter/staging

# Fast check-only (skip tests)
cargo-copter --no-test --jobs 8

# Test against multiple crate versions
cargo-copter --test-versions "0.8.0 0.8.48" 0.8.91

# Force version testing (bypass semver)
cargo-copter --test-versions 0.9.0--force-versions 0.7.1

# Test with specific features enabled
cargo-copter --features "serde unstable"

# Test different crate path
cargo-copter --path ~/my-crate

# Test published crate without local source
cargo-copter --crate rgb --test-versions 0.8.50 0.8.51
```

---

## CLI Reference

### Primary Options
```
-p, --path <PATH>               Path to crate (directory or Cargo.toml)
-c, --crate <NAME>              Crate name (for testing published crates)
--top-dependents <N>            Test top N by downloads [default: 5]
--dependents <CRATE[:VER]>...   Test specific crates (supports version pins)
--dependent-paths <PATH>...     Test local crates
-j, --jobs <N>                  Parallel jobs [default: 1]
--staging-dir <PATH>            Cache directory [default: .copter/staging]
--output <PATH>                 HTML output [default: copter-report.html]
--no-check                      Skip cargo check
--no-test                       Skip cargo test
--json                          JSON output
```

### Multi-Version Testing
```
--test-versions <VER>...        Test specific versions (space-delimited supported)
--force-versions <VER>...       Force testing specific versions (bypass semver requirements)
--features <FEATURES>...        Feature flags passed to cargo commands
```

### Version Syntax
```bash
# Pin specific versions
cargo-copter --dependents image:0.25.8 serde:1.0.0

# Test multiple versions (space-delimited within args or across args)
cargo-copter --test-versions "0.8.0 0.8.48" 0.8.91


# Pass feature flags to cargo
cargo-copter --features "default serde" --features rgb/unstable
```

---

## Result States

| Status | Icon | Description |
|--------|------|-------------|
| **PASSED** | ✓ | Compiled and tested successfully with offered version |
| **REGRESSED** | ✗ | Baseline passed but offered version failed |
| **BROKEN** | ✗ | Both baseline and offered version failed |
| **Skipped** | ⊘ | Version offered but not tested (resolved elsewhere) |

**Icon meanings in Offered column:**
- `✓` = Test ran with this version and passed
- `✗` = Test ran with this version and failed
- `⊘` = Version was offered but skipped (not used by cargo)
- `-` = Baseline test row
- `=` = Cargo resolved to exact offered version
- `↑` = Cargo upgraded to newer compatible version
- `≠` = Version mismatch or forced

---

## Output Formats

### Five-Column Console Table
```
Testing 2 reverse dependencies of rgb
  this = 0.8.91 a138e69* (your work-in-progress version)
  features: default

Legend: I=Install (cargo fetch), C=Check (cargo check), T=Test (cargo test)

┌────────────────────┬──────────┬─────────────────┬─────────────────────┬─────────────────────┐
│ Offered            │ Spec     │ Resolved        │ Dependent           │ Result         Time │
├────────────────────┼──────────┼─────────────────┼─────────────────────┼─────────────────────┤
│ - baseline         │ ^0.8.52  │ 0.8.51 📦       │ image 0.25.8        │ PASSED ✓✓✓     2.1s │
│ ✓ =this(0.8.91)    │ ^0.8.52  │ 0.8.91 📁       │ image 0.25.8        │ PASSED ✓✓✓     1.9s │
├────────────────────┼──────────┼─────────────────┼─────────────────────┼─────────────────────┤
│ - baseline         │ ^0.8     │ 0.8.51 📦       │ pixels 0.14         │ PASSED ✓✓✓     1.5s │
│ ✗ =this(0.8.91)    │ ^0.8     │ 0.8.91 📁       │ pixels 0.14         │ REGRESSED ✓✗-  1.4s │
│                    ├──────────┘                  └─────────────────────┘                    │
│                    │ cargo check failed on pixels:0.14                                      │
│                    │   • error[E0061]: function takes 2 arguments                           │
└────────────────────┴─────────────────────────────────────────────────────────────────────────┘

Summary:
  ✓ Passed:    1
  ✗ Regressed: 1
  ⊘ Skipped:   0
```

**Features:**
- Baseline + offered versions for each dependent
- Separator lines between different dependents
- Error details expand with L-shaped borders (columns 2-5)
- Multi-version tree display with `├─` prefixes
- Forced versions show `[≠→!]` suffix

See [CONSOLE-FORMAT.md](CONSOLE-FORMAT.md) for complete format specification and all demo scenarios.

### HTML Report
- Visual summary cards with statistics
- Detailed compilation logs for each dependent
- Expandable error sections
- Color-coded statuses

### Markdown Report (AI-Optimized)
- Regressions first (most actionable)
- Structured error details with JSON diagnostics
- Concise passing section
- Ready for LLM analysis

---

## Performance

### Without Caching
- ~14.3s per dependent (first run)
- Full compilation from scratch

### With Caching (`--staging-dir`)
- ~1.4s per dependent (subsequent runs)
- **10x faster** with persistent cache
- ~770MB disk usage for build artifacts

### Parallelization
- Use `--jobs N` (N = CPU cores)
- ~4x speedup on 4-core systems
- Parallelizes among dependents, not within

---

## Architecture

### Execution Flow
1. **Configuration** - Read Cargo.toml, extract name/version, capture git state
2. **Discovery** - Query crates.io API for reverse dependencies (paginated)
3. **Testing** - ThreadPool tests each dependent in parallel
4. **Classification** - Determine PASSED/REGRESSED/BROKEN/ERROR
5. **Reporting** - Generate console, HTML, and markdown reports

### Caching Strategy
- **Source cache**: `.copter/staging/{crate}-{version}/` (unpacked sources)
- **Build artifacts**: Same location, includes `target/` directory
- **Downloads**: `.copter/crate-cache/` (original .crate files)

### Override Mechanism
**Current**: Uses `.cargo/config` with `paths = [...]`
**Phase 5 Target**: Use `cargo --config 'patch.crates-io.{crate}.path="..."'` for cleaner multi-version testing

---

## Contributing

See [PLAN.md](PLAN.md) for Phase 5+ roadmap:
- Multi-version testing (`--test-versions`)
- 3-step ICT flow (Install/Check/Test with early stopping)
- Refactor to `--config` flag
- Live integration tests
- JSON output format

All contributions welcome! Priority areas:
1. Complete Phase 5 multi-version testing
2. Add live crates.io integration tests
3. Implement JSON output
4. Docker images for security

---

## Security Best Practices

1. ✅ **Run in Docker/VM/containers** (isolated execution)
2. ✅ **Network isolation** (limit egress to crates.io only)
3. ✅ **Resource limits** (CPU, memory, disk quotas)
4. ✅ **Non-root execution** (never run as root)
5. ✅ **Regular cache cleanup** (remove old artifacts)

**Docker Example**:
```bash
docker run --rm -v $(pwd):/work imazen/cargo-copter \
  --path /work --top-dependents 5
```

---

## Troubleshooting

**Error: "Cannot specify both --no-check and --no-test"**
→ Choose one or neither

**Error: "Must specify at least one dependent source"**
→ Use `--top-dependents N`, `--dependents`, or `--dependent-paths`

**Disk space exhausted**
→ Clear cache: `rm -rf .copter/`

**Compilation timeout**
→ Use `--no-test` for faster check-only runs

---

## Exit Codes

- `0` - Success, no regressions detected
- `-2` - Regressions detected (breaking changes found)
- Other - Internal error

---

## Development

```bash
# Build and test
cargo build --release
cargo test

# Test against real crate
RUST_LOG=debug ./target/release/cargo-copter --path ~/rust-rgb --top-dependents 1
```

**Project Structure**:
```
src/
├── main.rs           # Entry point, orchestration
├── cli.rs            # CLI parsing
├── api.rs            # crates.io API client
├── compile.rs        # Compilation logic
├── report.rs         # Report generation
└── error_extract.rs  # Error parsing

tests/
├── api_integration_test.rs
├── cli_parsing_test.rs
└── offline_integration.rs

test-crates/integration-fixtures/  # Test fixtures
```

**Test Fixtures** (in `test-crates/integration-fixtures/`):
- **base-crate-v1/v2**: Published baseline vs. WIP with breaking change
- **dependent-passing**: Uses only stable API → PASSED
- **dependent-regressed**: Uses removed API → REGRESSED
- **dependent-broken**: Has type error → BROKEN
- **dependent-test-failing**: Tests fail with new version

---

## CI/CD Integration

```yaml
# .github/workflows/copter.yml
name: Test Downstream Impact
on: [pull_request]

jobs:
  copter:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v3
      - uses: actions-rs/toolchain@v1
        with:
          toolchain: stable

      - name: Install cargo-copter
        run: |
          git clone https://github.com/imazen/cargo-copter
          cd cargo-copter
          cargo install --path .

      - name: Test top 10 dependents
        run: cargo-copter --top-dependents 10 --jobs 4

      - name: Upload report
        if: always()
        uses: actions/upload-artifact@v3
        with:
          name: copter-report
          path: copter-report.html
```

---

## Modernization (2025)

Recently updated from 7-year-old dependencies:
- Rust 2021 edition
- `ureq` 2.10 (replaced old `curl`)
- `serde` 1.0 (replaced deprecated `rustc-serialize`)
- `tempfile` 3.8 (replaced `tempdir` 0.3)
- `toml` 0.8 (updated from 0.1)
- `crates_io_api` 0.12 (new integration)
- `clap` 4.5 for robust CLI parsing

**Added**:
- Enhanced error diagnostics with JSON parsing
- Persistent caching infrastructure (10x speedup)
- AI-optimized markdown reports
- Git version tracking
- Parallel testing

---

## License

MIT/Apache-2.0

This is the standard license for Rust projects.

---

## Links

- **GitHub**: https://github.com/imazen/cargo-copter
- **crates.io**: https://crates.io
- **Rust API Evolution RFC**: https://github.com/rust-lang/rfcs/blob/master/text/1105-api-evolution.md
- **Development Roadmap**: [PLAN.md](PLAN.md)

---

**Ready to take flight?** Run `cargo-copter` in your crate and ensure your changes don't break the ecosystem! 🚁
