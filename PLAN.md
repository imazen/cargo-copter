# Cargo Crusader - Implementation Plan

This document tracks remaining work and next steps. For completed work, see commit history.

---

## Current Status

**Phase 1-4**: ✅ Complete
- Test fixtures and offline testing
- CLI infrastructure (--path, --dependents, --test-versions)
- 4-step compilation testing
- API integration with pagination
- Enhanced console table with version display
- Persistent caching (10x speedup)
- HTML and markdown reports
- Git version tracking

**Test Coverage**: 52 tests passing

---

## Phase 5: Multi-Version Testing (✅ Largely Complete)

**Goal**: Enable testing dependents against multiple versions of the base crate.

**Status**: Core functionality implemented. Console format evolved beyond original specification.

### Tasks

#### 5.1 ✅ Refactor to Use `--config` Instead of File-Based Override

**Status**: COMPLETE - Using cargo --config for patch overrides

**Files**: `src/compile.rs` (patch configuration logic)

#### 5.2 ✅ Implement 3-Step ICT Testing

**Status**: COMPLETE - Implemented in src/compile.rs

**Steps**:
- I (Install): `cargo fetch` - Download dependencies
- C (Check): `cargo check` - Fast compilation
- T (Test): `cargo test` - Run test suite

**Early stopping**: If I fails, skip C and T. If C fails, skip T.

**Files**: `src/compile.rs` (three_step_test function)

#### 5.3 ✅ Add Multi-Version Data Structures

**Status**: COMPLETE - Data structures defined in src/main.rs

Implemented structures:
- `OfferedRow` - Main row structure (lines 530-545)
- `DependencyRef` - Dependency metadata (lines 549-564)
- `OfferedVersion` - Version being tested (lines 567-571)
- `TestExecution` - Test results (lines 574-577)
- `ThreeStepResult` - ICT results in src/compile.rs
- `VersionSource` - Source tracking (CratesIo | Local | Git)

See [CONSOLE-FORMAT.md](CONSOLE-FORMAT.md) for complete specifications.

#### 5.4 ✅ Implement Multi-Version Testing Loop

**Status**: COMPLETE - Implemented in src/main.rs (run_test_multi_version function, line 216)

**Baseline Inference Logic** (implemented):
- If `--path` specified → look for Cargo.toml in that directory
- If no `--path` → look for ./Cargo.toml
- If Cargo.toml found → use as baseline ("this")
- CLI supports --test-versions for multiple version testing
- Supports --force-versions to bypass semver requirements

**Files**: `src/main.rs` (lines 84-220 multi-version flow)

#### 5.5 ✅ Update Console Table for Per-Version Rows

**Status**: COMPLETE - Format evolved beyond original spec

**Implemented Format** (5 columns, see CONSOLE-FORMAT.md):
- **Offered** | **Spec** | **Resolved** | **Dependent** | **Result**
- Icons: ✓ (pass), ✗ (fail), ⊘ (skip), - (baseline)
- Resolution markers: = (exact), ↑ (upgrade), ≠ (mismatch)
- ICT status embedded in Result column: ✓✓✓, ✓✗-, etc.
- Error details with dropped-panel borders (columns 2-5)
- Multi-version transitive dependency support with ├─ prefixes

**Note**: Final design differs from original 5-column plan. Current format provides better context with separate Spec/Resolved columns.

**Files**: `src/report.rs` (console table rendering)

#### 5.6 🔄 Update HTML and Markdown Reports

**Status**: PARTIAL - Basic reports exist, multi-version enhancements needed

**Completed**:
- ✅ HTML report generation (src/report.rs)
- ✅ Markdown report generation (src/report.rs)
- ✅ Basic version tracking in reports

**Remaining**:
- [ ] Enhanced version matrix view in HTML
- [ ] Improved markdown grouping by dependent

**Files**: `src/report.rs` (report generation functions)

#### 5.7 ⏸️ Add Live Integration Tests

**Status**: DEFERRED - Offline test fixtures provide good coverage

**Current Testing**:
- 52 tests passing with fixture-based approach
- Test fixtures in `test-crates/integration-fixtures/`

**Remaining**:
- [ ] Optional `#[ignore]` tests against live crates.io
- [ ] Would require network access in CI

---

## Phase 6: Polish and Production Readiness

### 6.1 Performance Optimizations

- [ ] Parallel version testing (within dependent)
- [ ] Smart dependency resolution (skip redundant fetches)
- [ ] Progress bar for long-running tests

### 6.2 User Experience Improvements

- [ ] Better error messages with suggestions
- [ ] Colorized diffs for regression details
- [ ] Summary at start showing test matrix
- [ ] Estimated time remaining

### 6.3 Output Formats

- [ ] JSON output format (`--json`)
- [ ] JUnit XML for CI integration
- [ ] TAP (Test Anything Protocol) format

### 6.4 Advanced Features

- [ ] Watch mode (`--watch`) for continuous testing
- [ ] Baseline from git tags/branches (fetch from repo)
- [ ] Test against multiple Rust toolchains
- [ ] Custom test commands (not just cargo test)

---

## Future Enhancements (Phase 7+)

### 7.1 Docker Integration

- [ ] Official Docker image
- [ ] Sandboxing by default
- [ ] Resource limits built-in
- [ ] Multi-architecture support (x86_64, arm64)

### 7.2 CI/CD Templates

- [ ] GitHub Actions workflow templates
- [ ] GitLab CI templates
- [ ] CircleCI orbs
- [ ] Pre-commit hook templates

### 7.3 Advanced Analysis

- [ ] API diff generation (show what changed)
- [ ] Semver compliance checking
- [ ] Deprecation warnings extraction
- [ ] Dependency graph visualization

### 7.4 Ecosystem Integration

- [ ] crates.io badge support
- [ ] docs.rs integration
- [ ] GitHub PR comments with results
- [ ] Slack/Discord notifications

---

## Non-Goals / Won't Implement

- ~~`--baseline` flag~~ (removed - baseline inferred from Cargo.toml presence)
- ~~`--baseline-path` flag~~ (removed - use --path instead)
- Testing upstream changes (out of scope)
- GUI interface (CLI-focused)
- Windows-specific features (WSL2 sufficient)

---

## Implementation Priority

**High Priority** (Phase 5):
1. Refactor to --config
2. 3-step ICT testing
3. Multi-version loop
4. Console table updates

**Medium Priority** (Phase 6):
- JSON output
- Better error messages
- Progress indicators

**Low Priority** (Phase 7+):
- Docker images
- CI templates
- Advanced analysis

---

## Testing Strategy

### For Phase 5:
1. **Unit tests**: Test individual functions (format_ict_marks, status classification)
2. **Offline integration**: Use test fixtures with multiple versions
3. **Live integration**: Test against real crates.io (with --ignore)
4. **Manual testing**: Test with real crates (rgb, serde, etc.)

### Test Checklist:
- [ ] All existing tests still pass
- [ ] New tests cover multi-version logic
- [ ] Console output renders correctly
- [ ] HTML report displays properly
- [ ] Markdown report is well-formatted
- [ ] No regression in performance
- [ ] Error messages are clear

---

## Documentation Updates Needed

After Phase 5 completion:

**README.md**:
- Move multi-version testing to "✅ Currently Available"
- Update output format examples with actual ICT implementation
- Update CLI reference with finalized --test-versions behavior

---

## Breaking Changes Log

None planned for Phase 5. All changes are additive or internal refactoring.

**Deprecated** (will be removed):
- `CRUSADER_MANIFEST` env var (still works, but `--path` preferred)
- `CRUSADER_LIMIT` env var (never documented, use `--top-dependents`)

---

## Performance Targets

**Current**:
- ~1.4s per dependent (with caching)
- ~14.3s per dependent (cold cache)

**Target (Phase 5)**:
- ~0.5s per version (with caching, using --config)
- No regression in cold cache performance

**Target (Phase 6)**:
- Parallel version testing: N versions in ~1.5s (same as 1 version)
- Progress bar with ETA

---

## Quick Reference for Contributors

### Build and Test
```bash
cargo build --release
cargo test
./target/release/cargo-crusader --path ~/rust-rgb --top-dependents 1
```

### Key Files for Phase 5
- `src/main.rs:766` - compile_with_custom_dep() (needs --config refactor)
- `src/main.rs:79` - run() function (needs version loop)
- `src/compile.rs` - Add run_three_step_test()
- `src/report.rs:157` - Console table printing
- `src/cli.rs` - CLI arguments (already has --test-versions)

### Code Style
- Follow existing patterns
- Add debug logging for new features
- Update tests for new functionality
- Run `cargo clippy` before committing

---

## Questions for Decisions

None currently. Baseline inference logic is now clear and simple.

---

## Links

- **Project Overview**: See [README.md](README.md)
- **AI Guidance**: See [CLAUDE.md](CLAUDE.md)
