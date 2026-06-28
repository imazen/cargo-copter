<!-- GENERATED FROM README.md by zenutils gen-readme-crates.sh — DO NOT EDIT. -->

# cargo-copter

**Test the downstream impact of a Rust crate change before you publish it** — catch regressions and API breakages in the crates that depend on yours.

cargo-copter cross-tests **any versions of your crate (or your uncommitted local WIP)** against **any versions of any dependents** (default: the top 5 by download count). For each (version × dependent) pair it runs install → check → test and tells you whether *your* change broke them, or whether they were already broken before you touched anything.

- Simulate publishing to crates.io with natural version resolution: `--test-versions "0.8.50 0.8.51"`.
- Simulate a downstream upgrade (a hand-edit to their `Cargo.toml`) with `--force-versions "0.8.52 0.8.53"`.

> **⚠️ Security:** cargo-copter executes arbitrary code from crates.io (build scripts, tests). Run it in a sandbox. The `--docker` flag adds basic isolation on Linux/WSL via a small wrapper script — convenient, but not a hardened boundary, so YMMV.

Why "cargo-copter"? *To make it absolutely **impossible** to find via Google.*

## Quick start

```bash
cargo binstall cargo-copter   # prebuilt binary
# or
cargo install cargo-copter    # build from source
```

Run it from your crate's directory, invoking the binary as **`cargo-copter`** (with the hyphen):

```bash
cd my-crate
cargo-copter --top-dependents 2
```

> **Note:** invoke `cargo-copter` (hyphen), not `cargo copter` (space). The
> `cargo <subcommand>` dispatch form is not wired up in this release, so
> `cargo copter ...` fails with `unexpected argument 'copter' found`. Use
> `cargo-copter ...` directly.

## Test your local work-in-progress version

There is **no `--wip` flag**. Running `cargo-copter` from your crate's directory (or with
`--path <DIR>`) and **without** any of `--crate`, `--test-versions`, or `--force-versions`
is what offers your local WIP version. In that default mode each dependent is tested twice:

1. **baseline** — the latest version of your crate currently published on crates.io
2. **WIP** — your uncommitted local source (the version in your local `Cargo.toml`)

so you see exactly what your unpublished changes do to each dependent versus what they
already ship against.

```bash
cd my-crate
cargo-copter --top-dependents 2      # baseline (published) + your local WIP, vs top 2 dependents
```

### Precondition: default dependent discovery needs your crate published

When you don't pass any `--dependent*` flag, dependents are discovered via the **crates.io
reverse-dependencies API** for your crate. A brand-new crate that has never been published
(or one that nothing depends on yet) therefore finds **zero** dependents and exits with
`Configuration error: No dependents to test`.

For an **unpublished** crate, point cargo-copter at local dependents instead — these paths
are read straight from disk and never touch crates.io:

```bash
# Unpublished crate: name the local dependents explicitly
cargo-copter --path . --dependent-paths ~/work/dep-a ~/work/dep-b

# ...or auto-discover local dependents (only crates that actually depend on yours are kept)
cargo-copter --path . --dependent-dir ~/work/ ~/work/zen/
cargo-copter --path . --dependent-glob "~/work/*/Cargo.toml"
```

## Example output

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

Markdown report: copter-report/report.md
Detailed failure logs: copter-report/failures.log

💡 To analyze API changes that may have caused regressions:
   cargo install cargo-public-api
   cargo public-api diff rgb@0.8.50 rgb@0.8.91  # compare two crates.io versions
   cd .copter/staging/rgb-0.8.91 && cargo public-api diff 0.8.50  # compare local against crates.io
```

## Docker

```bash
# Run in Docker (for security isolation)
cargo-copter --docker --top-dependents 5

# Or directly with Docker
docker run --rm -v $(pwd):/workspace ghcr.io/imazen/cargo-copter:latest \
  --path /workspace --top-dependents 5
```

## Common usage

```bash
# Test top dependents
cargo-copter --top-dependents 10

# Test specific dependents
cargo-copter --dependents image serde tokio

# Test specific dependent versions
cargo-copter --dependents image:0.25.8 serde:1.0.200

# Test multiple versions of your crate (baseline is included automatically)
cargo-copter --test-versions "0.8.50 0.8.51"

# Force incompatible versions (bypasses semver)
cargo-copter --force-versions "0.9.0 1.0.0-rc.1"

# Test a published crate without local source
cargo-copter --crate rgb --force-versions "0.8.51"

# Clean the cache and retest
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

## CLI options

```
-p, --path <PATH>              Path to the crate under test (directory or Cargo.toml)
-c, --crate <NAME>             Test a published crate by name (no local source needed)
    --top-dependents <N>       Test the top N dependents by download count [default: 5]
    --top-versions <Q>         Budget of extra dependent-version slots, ranked by downloads
    --dependents <CRATE[:VER]> Test specific crates from crates.io (space-separated)
    --dependent-paths <PATH>   Test local crate paths (works with unpublished crates)
    --dependent-glob <GLOB>    Discover local dependents via glob patterns
    --dependent-dir <DIR>      Discover local dependents in directories (one level deep)
    --test-versions <VER>...   Test multiple versions in semver-respecting patch mode
    --force-versions <VER>...  Force versions, bypassing semver
    --skip-normal-testing      Skip the auto-added patch-mode test for forced versions
    --only-fetch               Only fetch dependencies (skip check and test)
    --only-check               Only fetch and check (skip tests)
    --clean                    Clean the staging cache before running
    --staging-dir <PATH>       Staging/cache directory [default: ~/.cache/cargo-copter/staging]
    --error-lines <N>          Number of error lines to show per failure [default: 10]
    --output-html <PATH>       HTML report output path [default: copter-report.html]
    --json                     Emit results as JSON
    --simple                   Verbal output format (good for AI parsing / large dep counts)
    --docker                   Run inside a Docker container for isolation (Linux/WSL)
    --console-width <COLS>     Override the detected console width
```

## How it works

1. **Baseline test**: tests each dependent against the currently published version of your crate.
2. **Offered-version tests**: tests against the versions you specify (`--test-versions`, `--force-versions`, or your local WIP).
3. **Three-step ICT**: Install (fetch) → Check → Test, stopping early on the first failure.
4. **Classification**:
   - ✓ **passed**: baseline and offered both passed
   - ✗ **regressed**: baseline passed, offered failed — *your* change is implicated
   - ✗ **broken**: baseline check/fetch already failed (not your problem)
   - ⊘ **skipped**: a version was offered but cargo didn't actually resolve to it
5. **Robust to inapplicable cells**: a reverse-dep with no resolvable published version (yanked, unpublished, or path-only) and a historical dependent version that predates the dependency on your crate are logged and **skipped** — they no longer abort the whole run.
6. **End-of-run report** separates "your fault" from "not your problem", categorizing baseline failures by root cause (yanked deps, system libs, build.rs, nightly, version conflicts, platform-specific).

## Version testing modes

### Patch mode (default with `--test-versions`)
- Uses `[patch.crates-io]` in `Cargo.toml`
- Respects semver requirements
- Cargo can ignore an offered version if it doesn't satisfy the dependent's spec

### Force mode (`--force-versions`)
- Directly rewrites the dependency in the dependent's `Cargo.toml`
- Bypasses semver requirements
- Always tests the exact version specified
- Auto-adds a normal patch-mode test too, unless `--skip-normal-testing`

### Transitive unification (automatic)
When a forced version produces a "multiple versions of crate X" error — because a dependent
pulls in your crate both directly and transitively (e.g. testing `rgb` against `image`, which
depends on `ravif`, which also uses `rgb`) — cargo-copter **automatically retries** with
`[patch.crates-io]` applied to unify all copies of your crate across the dependency tree. No
flag is required. Retried rows are tagged in the output:

- `[!!]` = auto-patched (needed `[patch.crates-io]` to unify transitive versions)
- `[!!!]` = deep conflict (still failed even after `[patch.crates-io]`; see the blocking deps)

Unification also covers **workspace siblings of a local WIP**. When you test a `--path` crate
that is a workspace member (e.g. `magetypes` path-depending on `archmage`) and a dependent
also pulls in those siblings, cargo-copter emits `--config patch.crates-io.<sibling>.path=`
at the build root for the base crate *and* every local path-dep sibling — unifying transitive
copies that a member-level `[patch]` can't reach (cargo only honors `[patch]` in the workspace
root). So a WIP member no longer collides with the crates.io copy a dependent resolves.

> The old `--patch-transitive` flag is **deprecated and hidden** — it is now effectively a
> no-op, since auto-retry handles transitive unification on its own. It is kept only for
> backwards compatibility and prints a deprecation notice if you pass it.

## Caching

Cache location (platform-specific):
- Linux: `~/.cache/cargo-copter/staging/{crate}-{version}/`
- macOS: `~/Library/Caches/cargo-copter/staging/{crate}-{version}/`
- Windows: `%LOCALAPPDATA%/cargo-copter/staging/{crate}-{version}/`

Contains:
- Unpacked sources
- Build artifacts (`target/`)
- ~10x speedup on subsequent runs

Downloaded `.crate` files live in `~/.cache/cargo-copter/crate-cache/` (or the platform equivalent).

## Reports

All reports are written to `./copter-report/`:

- **Markdown**: `report.md` — optimized for LLM analysis
- **JSON**: `report.json` — structured data for CI/automation
- **Consolidated failures**: `failures.log`
- **Per-failure logs**: `{dependent}-{version}_{base-version}.txt` — full compiler output for each failure

An **HTML report** is also written to the `--output-html` path (default `copter-report.html`).

Per-failure logs include the full path to the staged source code for easy navigation:

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

The `copter-report/` directory is automatically appended to `.gitignore` if one exists.

## Table symbols

**Offered column**:
- `-` = baseline row
- `✓` = test passed
- `✗` = test failed
- `⊘` = version skipped
- `=` = exact version match
- `↑` = upgraded to a newer version
- `≠` = version mismatch
- `[≠→!]` = forced version

**Resolved column**:
- `📦` = published from crates.io
- `📁` = local path

**Result column**:
- `✓✓✓` = Install + Check + Test passed
- `✓✓✗` = Install + Check passed, Test failed
- `✓✗-` = Install passed, Check failed, Test skipped

## Build from source

```bash
git clone https://github.com/imazen/cargo-copter
cd cargo-copter
cargo install --path .

# Or build and test in place
cargo build --release
cargo test

# Run with debug logging
RUST_LOG=debug ./target/release/cargo-copter --top-dependents 1
```

## Links

- GitHub: <https://github.com/imazen/cargo-copter>
- Rust API Evolution RFC: <https://github.com/rust-lang/rfcs/blob/master/text/1105-api-evolution.md>
- Inspired by [cargo-crusader](https://github.com/rust-lang/cargo-crusader)

## AI

This was made with Claude Code and around 300 prompts to keep it on track. It wasn't a net
savings in time vs. writing it myself, but at least I could do it from my phone. You can
probably tell that it needs a lot of refactoring and improved test coverage. However, this is
the kind of tool that doesn't need to be perfect — it just needs to be good enough to be
useful. It's not a library.

## License

cargo-copter is dual-licensed under either the [MIT license](https://opensource.org/licenses/MIT) or the [Apache License, Version 2.0](https://www.apache.org/licenses/LICENSE-2.0), at your option.

## Image tech I maintain

| | |
|:--|:--|
| **Codecs** ¹ | [zenjpeg] · [zenpng] · [zenwebp] · [zengif] · [zenavif] · [zenjxl] · [zenbitmaps] · [heic] · [zentiff] · [zenpdf] · [zensvg] · [zenjp2] · [zenraw] · [ultrahdr] |
| Codec internals | [zenjxl-decoder] · [jxl-encoder] · [zenrav1e] · [rav1d-safe] · [zenavif-parse] · [zenavif-serialize] |
| Compression | [zenflate] · [zenzop] · [zenzstd] |
| Processing | [zenresize] · [zenquant] · [zenblend] · [zenfilters] · [zensally] · [zentone] |
| Pixels & color | [zenpixels] · [zenpixels-convert] · [linear-srgb] · [garb] |
| Pipeline & framework | [zenpipe] · [zencodec] · [zencodecs] · [zenlayout] · [zennode] · [zenwasm] · [zentract] |
| Metrics | [zensim] · [fast-ssim2] · [butteraugli] · [zenmetrics] · [resamplescope-rs] |
| Pickers & ML | [zenanalyze] · [zenpredict] · [zenpicker] |
| Products | [Imageflow] image engine ([.NET][imageflow-dotnet] · [Node][imageflow-node] · [Go][imageflow-go]) · [Imageflow Server] · [ImageResizer] (C#) |

<sub>¹ pure-Rust, `#![forbid(unsafe_code)]` codecs, as of 2026</sub>

### General Rust awesomeness

[zenbench] · [archmage] · [magetypes] · [enough] · [whereat] · **cargo-copter**

[Open source](https://www.imazen.io/open-source) · [@imazen](https://github.com/imazen) · [@lilith](https://github.com/lilith) · [lib.rs/~lilith](https://lib.rs/~lilith)

[zenjpeg]: https://github.com/imazen/zenjpeg
[zenpng]: https://github.com/imazen/zenpng
[zenwebp]: https://github.com/imazen/zenwebp
[zengif]: https://github.com/imazen/zengif
[zenavif]: https://github.com/imazen/zenavif
[zenjxl]: https://github.com/imazen/zenjxl
[zenbitmaps]: https://github.com/imazen/zenbitmaps
[heic]: https://github.com/imazen/heic
[zentiff]: https://github.com/imazen/zentiff
[zenpdf]: https://github.com/imazen/zenpdf
[zensvg]: https://github.com/imazen/zenextras
[zenjp2]: https://github.com/imazen/zenextras
[zenraw]: https://github.com/imazen/zenraw
[ultrahdr]: https://github.com/imazen/ultrahdr
[zenjxl-decoder]: https://github.com/imazen/zenjxl-decoder
[jxl-encoder]: https://github.com/imazen/jxl-encoder
[zenrav1e]: https://github.com/imazen/zenrav1e
[rav1d-safe]: https://github.com/imazen/rav1d-safe
[zenavif-parse]: https://github.com/imazen/zenavif-parse
[zenavif-serialize]: https://github.com/imazen/zenavif-serialize
[zenflate]: https://github.com/imazen/zenflate
[zenzop]: https://github.com/imazen/zenzop
[zenzstd]: https://github.com/imazen/zenzstd
[zenresize]: https://github.com/imazen/zenresize
[zenquant]: https://github.com/imazen/zenquant
[zenblend]: https://github.com/imazen/zenblend
[zenfilters]: https://github.com/imazen/zenfilters
[zensally]: https://github.com/imazen/zensally
[zentone]: https://github.com/imazen/zentone
[zenpixels]: https://github.com/imazen/zenpixels
[zenpixels-convert]: https://github.com/imazen/zenpixels
[linear-srgb]: https://github.com/imazen/linear-srgb
[garb]: https://github.com/imazen/garb
[zenpipe]: https://github.com/imazen/zenpipe
[zencodec]: https://github.com/imazen/zencodec
[zencodecs]: https://github.com/imazen/zencodecs
[zenlayout]: https://github.com/imazen/zenlayout
[zennode]: https://github.com/imazen/zennode
[zenwasm]: https://github.com/imazen/zenwasm
[zentract]: https://github.com/imazen/zentract
[zensim]: https://github.com/imazen/zensim
[fast-ssim2]: https://github.com/imazen/fast-ssim2
[butteraugli]: https://github.com/imazen/butteraugli
[zenmetrics]: https://github.com/imazen/zenmetrics
[resamplescope-rs]: https://github.com/imazen/resamplescope-rs
[zenanalyze]: https://github.com/imazen/zenanalyze
[zenpredict]: https://github.com/imazen/zenanalyze
[zenpicker]: https://github.com/imazen/zenanalyze
[zenbench]: https://github.com/imazen/zenbench
[archmage]: https://github.com/imazen/archmage
[magetypes]: https://github.com/imazen/archmage
[enough]: https://github.com/imazen/enough
[whereat]: https://github.com/lilith/whereat
[Imageflow]: https://github.com/imazen/imageflow
[Imageflow Server]: https://github.com/imazen/imageflow-dotnet-server
[ImageResizer]: https://github.com/imazen/resizer
[imageflow-dotnet]: https://github.com/imazen/imageflow-dotnet
[imageflow-node]: https://github.com/imazen/imageflow-node
[imageflow-go]: https://github.com/imazen/imageflow-go
