# Changelog

All notable changes to cargo-copter are documented here. This project adheres to
[Keep a Changelog](https://keepachangelog.com/) and [Semantic Versioning](https://semver.org/).

## [Unreleased]

### Fixed
- Skip inapplicable reverse-deps/versions instead of aborting the run: a reverse-dep with no resolvable published version (yanked, unpublished, or path-only), and a historical dependent version that predates the dependency on the base crate, are now logged and skipped (dc9b2b4).
- Unify transitive workspace-sibling path-deps when testing a local WIP (`--path`), avoiding "multiple versions of crate X" (E0308) when a dependent also pulls in those siblings — `--config patch.crates-io.<sibling>.path=` is applied at the build root for the base crate and every local sibling (ceaad2a).

### Changed
- README overhaul: trimmed badge row for a CLI (CI / crates.io / license), reconciled CLI options and report paths against source, documented the workspace-sibling unification and skip-inapplicable behavior, and split the crates.io README into a generated `README.crates.md` (`readme = "README.crates.md"`; no badges, absolute links).
- Documented the local-WIP recipe and the "default discovery needs your crate published" precondition; reconciled output-path/default-N contradictions (#14, a65a683).
- Trimmed non-user-facing files (dev docs, CI config) from the published package (01cd0a4).
- README badges switched to `flat-square` style; fixed a rustdoc footnote link (3c449a8, a00de30).
- Added this CHANGELOG.

### Dependencies
- Bump rand 0.9.2 → 0.9.4 (#11) and rustls-webpki 0.103.10 → 0.103.13 (#12).

## [0.3.0] - 2026-03-24

### Added
- `--top-versions <Q>`: breadth-first dependent selection plus popularity-ranked depth testing (24626ef).
- `--dependent-glob` and `--dependent-dir` for discovering local dependents (4b8c820).
- Failure-categorization engine that classifies baseline failures by root cause (yanked deps, system libs, build.rs, nightly, version conflicts, platform-specific) (0cb8985, 0d34ea5).
- Debug invariant assertions ported from the refactor branch (9fef5ce).

### Fixed
- Distinguish check failures from test failures in summary reporting (c71d44c).
- Avoid a double summary in simple mode; integrate categorization into output (262e5e5).

## [0.2.7] - 2026-02-02

## [0.2.4] - 2025-12-27

## [0.2.3] - 2025-12-15

## [0.2.2] - 2025-12-15

## [0.2.1] - 2025-12-15

## [0.2.0] - 2025-12-15

### Added
- Docker support (`--docker`) for security isolation when building/testing untrusted dependents.

[Unreleased]: https://github.com/imazen/cargo-copter/compare/v0.3.0...HEAD
[0.3.0]: https://github.com/imazen/cargo-copter/releases/tag/v0.3.0
[0.2.7]: https://github.com/imazen/cargo-copter/releases/tag/v0.2.7
[0.2.4]: https://github.com/imazen/cargo-copter/releases/tag/v0.2.4
[0.2.3]: https://github.com/imazen/cargo-copter/releases/tag/v0.2.3
[0.2.2]: https://github.com/imazen/cargo-copter/releases/tag/v0.2.2
[0.2.1]: https://github.com/imazen/cargo-copter/releases/tag/v0.2.1
[0.2.0]: https://github.com/imazen/cargo-copter/releases/tag/v0.2.0
