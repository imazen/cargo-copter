# Cargo Copter

**Test the downstream impact of Rust crate changes before publishing. Locate regressions and API breakages.**

Test `any versions of your crate and/or a local WIP version` X `any versions of any specified dependents` (defaults to top 10 most popular dependents).

Let natural version resolution take place `--test-versions "0.8.50 0.8.51"` (to simulate publishing to crates.io)<br/>
OR use `--force-versions "0.8.52 0.8.53"` to simulate them upgrading to a new version of your crate with an edit of their cargo.toml.

**⚠️ Security**: Executes arbitrary code from crates.io. Always run in sandboxed environments. Try our --docker flag on WSL/linux for basic sandboxing, but YMMV since it's a bit hacky with a shell script.

Why did you name it cargo-copter? *To make it absolutely **impossible** to find via google.*

## Install

```bash
cargo binstall cargo-copter

# Or install from source
cargo install cargo-copter
```

## Test your local crate version against top dependents
```bash
cd my-crate
cargo copter --top-dependents 2
```

## Example Output

```
Testing 2 reverse dependencies of rgb
  Dependents: ansi_colours, resvg
  rgb versions: baseline, 0.8.91-alpha.3 [!], 0.8.52
  2 × 3 = 6 tests
  this = 0.8.52 bd35c97* (your work-in-progress version)

┌─────────────────────────────┬────────────┬──────────────────┬──────────────────────────────┬─────────────────────────┐
│           Offered           │    Spec    │     Resolved     │          Dependent           │   Result         Time   │
├─────────────────────────────┼────────────┼──────────────────┼──────────────────────────────┼─────────────────────────┤
│ - baseline                  │ 0.8        │ 0.8.52 📦        │ ansi_colours 1.2.3           │   passed ✓✓✓  8.3s      │
│ ✗ ≠0.8.91-alpha.3 [≠→!]     │ → =0.8.... │ 0.8.91-alpha.... │ ansi_colours 1.2.3           │ test failed ✓✓✗  1.7s   │
│    ┌────────────────────────┴────────────┘                  └──────────────────────────────┘                         │
│    │ cargo test failed on ansi_colours                                                                               │
│    │   error[E0277]: the trait bound `Gray<u8>: ToLab` is not satisfied                                              │
│    │     --> src/test.rs:45:47                                                                                       │
│    └────────────────────────┬────────────┬──────────────────┬──────────────────────────────┬─────────────────────────┤
│ ✓ =0.8.52                   │ 0.8        │ 0.8.52 📦        │ ansi_colours 1.2.3           │   passed ✓✓✓  3.1s      │
├─────────────────────────────┼────────────┼──────────────────┼──────────────────────────────┼─────────────────────────┤
│ - baseline                  │ 0.8        │ 0.8.52 📦        │ resvg 0.45.1                 │   passed ✓✓✓  8.4s      │
│ ✓ ≠0.8.91-alpha.3 [≠→!]     │ → =0.8.... │ 0.8.91-alpha.... │ resvg 0.45.1                 │   passed ✓✓✓  4.9s      │
│ ✓ =0.8.52                   │ 0.8        │ 0.8.52 📦        │ resvg 0.45.1                 │   passed ✓✓✓  2.1s      │
└─────────────────────────────┴────────────┴──────────────────┴──────────────────────────────┴─────────────────────────┘

Version Comparison:
                                   Default          0.8.52  0.8.91-alpha.3
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
Total tested                             2               2               2
Already broken                           0               -               -
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
Passed fetch                             2               2               2
Passed check                             2               2               2
Passed test                              2               2          -1 → 1
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
Fully passing                            2               2          -1 → 1

Markdown report: copter-report.md
Detailed failure logs: copter-failures.log

💡 To analyze API changes that may have caused regressions:
   cargo install cargo-public-api
   cargo public-api diff rgb@0.8.50 rgb@0.8.91  # compare two crates.io versions
   cd .copter/staging/rgb-0.8.91 && cargo public-api diff 0.8.50  # compare local against crates.io
```

## Installation from source

```bash
# From source
git clone https://github.com/imazen/cargo-copter
cd cargo-copter && cargo install
```

## Docker

```bash
# Run in Docker (for security isolation)
cargo-copter --docker --top-dependents 5

# Or directly with Docker
docker run --rm -v $(pwd):/workspace ghcr.io/imazen/cargo-copter:latest \
  --path /workspace --top-dependents 5
```

## Common Usage

```bash
# Test top dependents
cargo-copter --top-dependents 10

# Test specific dependents
cargo-copter --dependents image serde tokio

# Test specific dependent versions
cargo-copter --dependents image:0.25.8 serde:1.0.200

# Test multiple versions (includes baseline automatically)
cargo-copter --test-versions "0.8.50 0.8.51"

# Force incompatible versions (bypasses semver)
cargo-copter --force-versions "0.9.0 1.0.0-rc.1"

# Test published crate without local source
cargo-copter --crate rgb --force-versions "0.8.51"

# Clean cache and retest
cargo-copter --clean --top-dependents 5

# Test local unpublished dependents (works without crates.io)
cargo-copter --path . --dependent-paths ~/work/my-dep1 ~/work/my-dep2

# Auto-discover local dependents in directories
cargo-copter --path . --dependent-dir ~/work/ ~/work/zen/

# Auto-discover via glob (filters to crates that depend on yours)
cargo-copter --path . --dependent-glob "~/work/*/Cargo.toml"

# Breadth + depth: test top dependents, then add popular older versions
cargo-copter --top-dependents 5 --top-versions 50
```

## CLI Options

```
-p, --path <PATH>              Path to crate (directory or Cargo.toml)
-c, --crate <NAME>             Test published crate by name
    --top-dependents <N>       Test top N by downloads [default: 5]
    --top-versions <Q>         Budget for extra version slots across dependents
    --dependents <CRATE[:VER]> Test specific crates (space-separated)
    --dependent-paths <PATH>   Test local crate paths (works with unpublished crates)
    --dependent-glob <GLOB>    Discover local dependents via glob patterns
    --dependent-dir <DIR>      Discover local dependents in directories (1 level deep)
    --test-versions <VER>...   Test multiple versions
    --force-versions <VER>...  Force versions (bypass semver)
    --staging-dir <PATH>       Cache directory [default: ~/.cache/cargo-copter/staging]
    --only-fetch               Only fetch dependencies (skip check and test)
    --only-check               Only fetch and check (skip tests)
    --clean                    Clean cache before testing
    --error-lines <N>          Error lines to show [default: 10]
    --simple                   Verbal output format (good for AI parsing)
    --skip-normal-testing      Skip auto-patch mode for forced versions
    --json                     JSON output
```

## How It Works

1. **Baseline test**: Tests each dependent with currently published version
2. **Offered version tests**: Tests with specified versions (--test-versions, --force-versions, or local WIP)
3. **Three-step ICT**: Install (fetch) → Check → Test (stops early on failure)
4. **Classification**:
   - ✓ **passed**: Baseline and offered both passed
   - ✗ **regressed**: Baseline passed, offered failed
   - ✗ **broken**: Baseline check/fetch failed (not your problem)
   - ⊘ **skipped**: Version offered but not used by cargo
5. **End-of-run report** separates "your fault" from "not your problem", with baseline failures categorized by root cause (yanked deps, system libs, build.rs, nightly, version conflicts, platform-specific)

## Version Testing Modes

### Patch Mode (default with --test-versions)
- Uses `[patch.crates-io]` in Cargo.toml
- Respects semver requirements
- Cargo can ignore if version doesn't satisfy spec

### Force Mode (--force-versions)
- Directly modifies dependency in Cargo.toml
- Bypasses semver requirements
- Always tests the exact version specified
- Auto-adds patch mode test unless --skip-normal-testing

### Transitive Patch Mode (--patch-transitive)
- Adds `[patch.crates-io]` to dependent's Cargo.toml
- Unifies ALL versions of your crate across the entire dependency tree
- Resolves "multiple versions of crate X" errors
- Use when dependents have transitive deps that also use your crate

Example: Testing `rgb` against `image` which depends on `ravif` which also uses `rgb`:
```bash
cargo-copter --dependents image --patch-transitive
```
Without `--patch-transitive`, `image` might fail with "the trait `AsPixels` is not implemented"
because `image` uses one version of `rgb` while `ravif` uses another.

## Caching

Cache location (platform-specific):
- Linux: `~/.cache/cargo-copter/staging/{crate}-{version}/`
- macOS: `~/Library/Caches/cargo-copter/staging/{crate}-{version}/`
- Windows: `%LOCALAPPDATA%/cargo-copter/staging/{crate}-{version}/`

Contains:
- Unpacked sources
- Build artifacts (target/)
- 10x speedup on subsequent runs

Downloaded .crate files: `~/.cache/cargo-copter/crate-cache/` (or platform equivalent)

## Reports

All reports are saved to `./copter-report/`:

- **Markdown**: `report.md` - optimized for LLM analysis
- **JSON**: `report.json` - structured data for CI/automation
- **Failure logs**: `{dependent}-{version}_{base-version}.txt` - full compiler output for each failure

Failure logs include the full path to the staged source code for easy navigation:
```
# Failure Log: image 0.25.9 with base crate version 0.8.52
# Generated: 2025-12-15 10:30:00
# Source: /home/user/.cache/cargo-copter/staging/image-0.25.9

=== CHECK (cargo check) ===
Status: FAILED (1.7s)

error[E0277]: the trait bound `Rgb<u8>: From<...>` is not satisfied
  --> src/lib.rs:42:15
   ...
```

The `copter-report/` directory is automatically added to `.gitignore` if one exists.

## Table Symbols

**Offered column**:
- `-` = Baseline row
- `✓` = Test passed
- `✗` = Test failed
- `⊘` = Version skipped
- `=` = Exact version match
- `↑` = Upgraded to newer version
- `≠` = Version mismatch
- `[≠→!]` = Forced version

**Resolved column**:
- `📦` = Published from crates.io
- `📁` = Local path

**Result column**:
- `✓✓✓` = Install + Check + Test passed
- `✓✓✗` = Install + Check passed, Test failed
- `✓✗-` = Install passed, Check failed, Test skipped

## Development

```bash
# Build and test
cargo build --release
cargo test

# Run with debug logging
RUST_LOG=debug ./target/release/cargo-copter --top-dependents 1
```

## License

MIT/Apache-2.0 (standard Rust dual-license)

## Links

- GitHub: https://github.com/imazen/cargo-copter
- Rust API Evolution RFC: https://github.com/rust-lang/rfcs/blob/master/text/1105-api-evolution.md
- Inspired by: [cargo-crusader](https://github.com/rust-lang/cargo-crusader)


# AI

This was made with Claude Code and around 300 prompts to keep it on track. It wasn't a net savings in time vs. writing it myself, but at least I could do it from my phone. 
You can probably tell that it needs a lot of refactoring and improved test coverage. However, this is the kind of tool that doesn't need to be perfect, it just needs to be good enough to be useful. It's not a library.
