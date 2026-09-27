# Downstream audit fixes — 2026-09-27

The audit of archmage/magetypes exposed three cargo-copter bugs at `2d50bf89`.
The fixes are on main; no crate release was made for this work.

| Bug | Fix | Regression coverage |
|---|---|---|
| `latest` selected yanked releases | `d6b1566`: filter yanked entries before semver comparison, including preview selection | Unsorted versions, yanked stable and preview releases, preview-only and empty results |
| Forced workspace dependencies lost inherited features | `36eda128`: combine workspace and member settings before replacing the source | Real compilation requiring both feature sets and disabled defaults; renamed, table and target-specific entries |
| Failed runs left rewritten manifests; stale backups overwrote current edits | `36eda128`: snapshot current manifest and workspace lockfile, restore on every returned result/error | Fetch, check and test failures, execution errors, present/absent lockfiles, stale backups and a subsequent sibling baseline |

The workspace regression also exposed incorrect version verification: scanning
all members could report a sibling's version. Verification now follows the
tested member's dependency edges and uses the same patch configuration as the
build. Metadata is collected before restoration.

Restoration also runs during Rust unwinding. Abrupt process termination does
not execute the guard. Local edits made concurrently during an audit are not
isolated; use a disposable copy for those workflows. Legacy registry-cache
backups remain supported, but are never used to restore local checkouts.

## Verified real consumers

Tested on Linux x86_64 with Rust 1.98.1. The tested archmage checkout was
`aac61604` (magetypes 0.9.29). These were fetch/check audits, not runtime or
cross-platform consumer tests.

| Consumer selected automatically | Baseline | Local WIP |
|---|---|---|
| linear-srgb 0.6.12 | Pass | Pass |
| zenavif 0.1.6 | Pass | Pass |
| zenfilters 0.1.0 | Pass | Pass |
| zenpixels-convert 0.2.16 | Pass | Pass |
| zenwebp 0.4.4 | Fetch failure | Same fetch failure |

All five automatic selections avoided the previously selected yanked releases.
zenwebp's own `webpx = "^0.1.4"` dependency is yanked, so it is correctly
classified as already broken, with zero regressions in this batch.

The local `zensim-train-core` snapshot passes both baseline and WIP checks with
its workspace-inherited `magetypes/avx512` feature. SHA-256 comparisons before
and after show all 14 snapshot manifests and lockfiles unchanged.

Full local evidence is in
`/home/lilith/data/cargo-copter/audit-2026-09-27/`: `local.log`,
`local-report/report.json`, `before.json`, `after.json`, and
`published/{run.log,copter-report/report.json}`. These are source snapshots and
build reports, not files to copy over active consumer checkouts.

Commands (invoke through the host's `run-heavy` wrapper):

```sh
cargo-copter --path /path/to/archmage/magetypes \
  --dependent-paths /path/to/snapshot/zensim/zensim-train-core \
  --only-check --simple --staging-dir /path/to/local-staging
cargo-copter --path /path/to/archmage/magetypes \
  --dependents linear-srgb zenavif zenfilters zenpixels-convert zenwebp \
  --only-check --simple --staging-dir /path/to/published-staging
```

## Tool validation

- `cargo test --all-targets -- --test-threads=1`: 103 passed, 5 existing ignored.
- `cargo clippy --all-targets -- -D warnings`: passed.
- The explicitly enabled `default_baseline_wip_test` passes after `27203a5`.
  It uses the current binary and isolated output directories, requires baseline
  fetch/check/test success, then WIP fetch success and check failure. The
  actual failure is the breaking rgb fixture's missing `Gray::value` API.
- CLI output contains one table header and one summary, with baseline before
  WIP. The five-consumer rerun emitted completed rows while later crates were
  still running.

`just check`, `just regressions`, and `just baseline-wip` record the repeatable
validation commands. The last explicitly enables the network integration test.
