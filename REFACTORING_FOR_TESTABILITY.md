# Refactoring Changes for Testable Architecture

**Goal:** Make the identified regression tests non-fragile by improving separation of concerns and testability.

**Current Date:** 2025-11-18

---

## Executive Summary

The current architecture has good **data flow** (config → runner → results) but poor **output testability**. The main issues:

1. **Direct stdout/println! usage** - Makes output testing require subprocess spawning
2. **Global mutable state** - `OnceLock`, `lazy_static!` make tests interfere with each other
3. **Mixed concerns** - Business logic and rendering intertwined
4. **Hard-coded I/O** - Files always written to same paths, can't test in parallel

**Solution:** Apply dependency injection pattern and separate pure functions from I/O.

---

## Problem Analysis

### Current Architecture (Simplified)

```
┌──────────┐     ┌──────────┐     ┌──────────┐
│   CLI    │────▶│  Config  │────▶│  Runner  │
└──────────┘     └──────────┘     └──────────┘
                                       │
                                       ▼
                              ┌─────────────────┐
                              │   TestResult    │
                              └─────────────────┘
                                       │
                                       ▼
                              ┌─────────────────┐
                              │  Bridge Layer   │
                              └─────────────────┘
                                       │
                                       ▼
                              ┌─────────────────┐
                              │   OfferedRow    │
                              └─────────────────┘
                                       │
                    ┌──────────────────┼──────────────────┐
                    ▼                  ▼                  ▼
            ┌──────────────┐   ┌──────────────┐   ┌──────────────┐
            │ console_     │   │   report.rs  │   │   main.rs    │
            │ format.rs    │   │              │   │              │
            └──────────────┘   └──────────────┘   └──────────────┘
                    │                  │                  │
                    └──────────────────┴──────────────────┘
                                       │
                                       ▼
                              println! / stdout
                                   (UNTESTABLE)
```

### Testability Issues

| Issue | Location | Impact | Fragility |
|-------|----------|--------|-----------|
| **Global state** | `console_format.rs:353-354` | Tests can't run in parallel | HIGH |
| **Direct stdout** | `main.rs:95-131`, `console_format.rs:564+` | Can't capture output without subprocess | HIGH |
| **Hard-coded paths** | `main.rs:212`, `main.rs:236` | Tests overwrite each other | MEDIUM |
| **File I/O mixed with logic** | `compile.rs:19-39` | Can't test error logging without filesystem | MEDIUM |
| **Callback mutates** | `main.rs:95-121` | Hard to verify streaming behavior | MEDIUM |

---

## Refactoring Strategy

### Phase 1: Separate Pure Functions from I/O (LOW RISK)

**Principle:** Pure data transformations should be separate from side effects.

#### 1.1 Extract Output Builders

**Current (main.rs:94-131):**
```rust
let _test_results = match runner::run_tests(matrix.clone(), |result| {
    let row = bridge::test_result_to_offered_row(result);
    // ... separator logic ...
    report::print_offered_row(&row, is_last, prev_error.as_deref(), args.error_lines);
    // ... tracking ...
    offered_rows.push(row);
}) { ... }
```

**Refactored:**
```rust
// NEW: Pure function that builds output structure
struct OutputLine {
    row: OfferedRow,
    needs_separator: bool,
    error_display: Option<String>,
}

fn build_output_lines(results: Vec<TestResult>) -> Vec<OutputLine> {
    let mut lines = Vec::new();
    let mut prev_dependent = None;
    let mut prev_error = None;

    for result in results {
        let row = bridge::test_result_to_offered_row(&result);
        let needs_separator = prev_dependent
            .map(|prev| prev != row.primary.dependent_name)
            .unwrap_or(false);

        let error_display = if prev_error == report::extract_error_text(&row) {
            Some("[SAME ERROR]".to_string())
        } else {
            report::extract_error_text(&row)
        };

        lines.push(OutputLine { row, needs_separator, error_display });

        prev_dependent = Some(row.primary.dependent_name.clone());
        prev_error = report::extract_error_text(&row);
    }

    lines
}

// TESTABLE: No I/O, pure transformation
#[test]
fn test_output_lines_adds_separators() {
    let results = vec![/* ... */];
    let lines = build_output_lines(results);
    assert_eq!(lines[0].needs_separator, false);
    assert_eq!(lines[1].needs_separator, true); // Different dependent
}
```

**Benefits:**
- ✅ Testable without subprocess
- ✅ Separator logic verifiable
- ✅ Error deduplication testable
- ✅ No global state

#### 1.2 Make Rendering Accept Writer

**Current (console_format.rs:564):**
```rust
pub fn print_table_header(...) {
    let mut writer = TableWriter::new(io::stdout(), false);
    writer.write_table_header(...).unwrap();
}
```

**Refactored:**
```rust
pub fn print_table_header_to<W: Write>(
    writer: &mut W,
    crate_name: &str,
    ...
) -> io::Result<()> {
    let mut table_writer = TableWriter::new(writer, false);
    table_writer.write_table_header(...)
}

pub fn print_table_header(...) {
    print_table_header_to(&mut io::stdout(), ...)
}

// TESTABLE
#[test]
fn test_header_printed_once() {
    let mut buf = Vec::new();
    print_table_header_to(&mut buf, "rgb", "0.8.91", 5, None, None).unwrap();
    let output = String::from_utf8(buf).unwrap();
    assert_eq!(output.matches("│ Offered │").count(), 1);
}
```

**Benefits:**
- ✅ Output captured in memory
- ✅ Fast tests (no subprocess)
- ✅ Easy to assert on content

---

### Phase 2: Remove Global State (MEDIUM RISK)

**Problem:** `OnceLock` in `console_format.rs` makes tests interfere.

#### 2.1 Replace Global Widths with Context Object

**Current (console_format.rs:353-354):**
```rust
static WIDTHS: OnceLock<TableWidths> = OnceLock::new();
static OVERRIDE_WIDTH: OnceLock<usize> = OnceLock::new();

pub fn init_table_widths(...) {
    let widths = calculate_widths(...);
    WIDTHS.set(widths).ok();
}

pub fn get_widths() -> &'static TableWidths {
    WIDTHS.get().unwrap_or(&DEFAULT_WIDTHS)
}
```

**Refactored:**
```rust
// NEW: Context object passed through
pub struct TableConfig {
    pub widths: TableWidths,
    pub use_colors: bool,
}

impl TableConfig {
    pub fn new(versions: &[String], display_version: &str, force_versions: bool) -> Self {
        Self {
            widths: calculate_widths(versions, display_version, force_versions),
            use_colors: true,
        }
    }

    pub fn for_testing() -> Self {
        Self {
            widths: DEFAULT_WIDTHS,
            use_colors: false,
        }
    }
}

// Update signatures
pub fn print_table_header_to<W: Write>(
    writer: &mut W,
    config: &TableConfig,  // NEW parameter
    crate_name: &str,
    ...
) -> io::Result<()> {
    let mut table_writer = TableWriter::new(writer, config.use_colors);
    table_writer.write_table_header(...)
}
```

**Migration Path:**
1. Add `config` parameter to all functions (keep globals for now)
2. Update callsites to pass config
3. Remove global state usage
4. Delete `OnceLock` statics

**Benefits:**
- ✅ Tests don't interfere
- ✅ Parallel test execution
- ✅ Explicit dependencies
- ✅ Easier to reason about

---

### Phase 3: Inject File Paths (LOW RISK)

**Problem:** Hard-coded output paths prevent parallel tests.

#### 3.1 Make Output Paths Configurable

**Current (main.rs:212, 236):**
```rust
let markdown_path = PathBuf::from("copter-report.md");
let json_path = PathBuf::from("copter-report.json");
```

**Refactored:**
```rust
// In CliArgs
pub struct CliArgs {
    // ...existing fields...
    #[clap(long, default_value = "copter-report.md")]
    pub markdown_output: PathBuf,

    #[clap(long, default_value = "copter-report.json")]
    pub json_output: PathBuf,
}

// Use in main.rs
fn generate_non_console_reports(...) {
    match report::export_markdown_table_report(
        rows,
        &args.markdown_output,  // Use configurable path
        ...
    ) { ... }

    match report::export_json_report(
        rows,
        &args.json_output,  // Use configurable path
        ...
    ) { ... }
}

// TESTABLE
#[test]
fn test_markdown_export() {
    let temp_dir = tempfile::tempdir().unwrap();
    let output_path = temp_dir.path().join("test-report.md");

    report::export_markdown_table_report(..., &output_path, ...).unwrap();

    let content = fs::read_to_string(output_path).unwrap();
    assert!(content.contains("| Offered |"));
}
```

**Benefits:**
- ✅ Tests use temporary directories
- ✅ Parallel test execution
- ✅ User can customize output location

---

### Phase 4: Separate Error Logging from Business Logic (MEDIUM RISK)

**Problem:** `compile.rs` mixes error logging with test execution.

#### 4.1 Extract Error Logger Interface

**Current (compile.rs:19-39):**
```rust
lazy_static! {
    static ref FAILURE_LOG: Mutex<Option<PathBuf>> = Mutex::new(None);
    // ...
}

pub fn init_failure_log(log_path: PathBuf) { ... }
pub fn log_failure(...) { ... }
```

**Refactored:**
```rust
// NEW: Trait for error logging
pub trait ErrorLogger {
    fn log_failure(
        &mut self,
        dependent: &str,
        dependent_version: &str,
        base_crate: &str,
        test_label: &str,
        command: &str,
        exit_code: Option<i32>,
        stdout: &str,
        stderr: &str,
        diagnostics: &[Diagnostic],
    );
}

// Production implementation
pub struct FileErrorLogger {
    log_path: PathBuf,
    build_log_path: PathBuf,
    last_error_signature: Option<String>,
}

impl ErrorLogger for FileErrorLogger {
    fn log_failure(&mut self, ...) {
        // Existing implementation
    }
}

// Test implementation
pub struct MemoryErrorLogger {
    pub entries: Vec<ErrorLogEntry>,
}

impl ErrorLogger for MemoryErrorLogger {
    fn log_failure(&mut self, ...) {
        self.entries.push(ErrorLogEntry { ... });
    }
}

// Update compile functions to accept logger
pub fn run_three_step_ict(
    config: &TestConfig,
    logger: &mut dyn ErrorLogger,  // NEW parameter
) -> ThreeStepResult {
    // ... on error ...
    logger.log_failure(...);
}

// TESTABLE
#[test]
fn test_error_logging_deduplication() {
    let mut logger = MemoryErrorLogger::default();

    // Simulate two identical errors
    logger.log_failure(..., "error X");
    logger.log_failure(..., "error X");

    assert_eq!(logger.entries.len(), 2);
    assert_eq!(logger.entries[1].is_duplicate, true);
}
```

**Benefits:**
- ✅ Test error logging without filesystem
- ✅ Verify deduplication logic
- ✅ Dependency injection principle

---

### Phase 5: Make Streaming Verifiable (MEDIUM RISK)

**Problem:** Can't verify rows stream without subprocess.

#### 5.1 Separate Streaming Collection from Display

**Current (main.rs:98-122):**
```rust
let _test_results = match runner::run_tests(matrix.clone(), |result| {
    let row = bridge::test_result_to_offered_row(result);
    // ... print immediately ...
    report::print_offered_row(&row, ...);
    offered_rows.push(row);
}) { ... }
```

**Refactored:**
```rust
// NEW: Capture timing information
struct StreamEvent {
    row: OfferedRow,
    timestamp: Instant,
}

// NEW: Collector that tracks timing
struct StreamCollector {
    events: Vec<StreamEvent>,
    start: Instant,
}

impl StreamCollector {
    fn collect(&mut self, result: &TestResult) {
        let row = bridge::test_result_to_offered_row(result);
        self.events.push(StreamEvent {
            row,
            timestamp: Instant::now(),
        });
    }
}

// Run tests and collect events
let mut collector = StreamCollector::new();
let _results = runner::run_tests(matrix.clone(), |result| {
    collector.collect(result);
})?;

// THEN render (separates collection from display)
for event in &collector.events {
    report::print_offered_row(&event.row, ...);
}

// TESTABLE
#[test]
fn test_streaming_immediate() {
    let mut collector = StreamCollector::new();

    // Simulate tests with delays
    runner::run_tests(matrix, |result| {
        collector.collect(result);
    }).unwrap();

    // Verify events appeared as tests completed (not batched)
    for i in 1..collector.events.len() {
        let delta = collector.events[i].timestamp - collector.events[i-1].timestamp;
        assert!(delta > Duration::from_millis(10), "Events should not batch");
    }
}
```

**Benefits:**
- ✅ Verify streaming behavior
- ✅ Detect batching bugs
- ✅ Test ordering

---

## Phase 6: Cargo.toml Restoration (HIGH PRIORITY)

**Problem:** No mechanism to ensure Cargo.toml restored between tests.

#### 6.1 RAII Guard for Cargo.toml

**New pattern (compile.rs):**
```rust
/// RAII guard that backs up and restores Cargo.toml
pub struct CargoTomlGuard {
    original_path: PathBuf,
    backup_path: PathBuf,
    restored: bool,
}

impl CargoTomlGuard {
    pub fn new(cargo_toml_path: &Path) -> io::Result<Self> {
        let backup_path = cargo_toml_path.with_extension("toml.copter-backup");

        // Back up original
        fs::copy(cargo_toml_path, &backup_path)?;

        Ok(Self {
            original_path: cargo_toml_path.to_path_buf(),
            backup_path,
            restored: false,
        })
    }

    pub fn restore(&mut self) -> io::Result<()> {
        if !self.restored {
            fs::copy(&self.backup_path, &self.original_path)?;
            fs::remove_file(&self.backup_path)?;
            self.restored = true;
        }
        Ok(())
    }
}

impl Drop for CargoTomlGuard {
    fn drop(&mut self) {
        if !self.restored {
            let _ = self.restore();
        }
    }
}

// Usage in runner
fn run_single_test(...) -> ThreeStepResult {
    let cargo_toml = dependent_dir.join("Cargo.toml");
    let _guard = CargoTomlGuard::new(&cargo_toml)?;

    // Modify Cargo.toml...
    apply_patch(&cargo_toml, ...)?;

    // Run test...
    let result = compile::run_three_step_ict(...)?;

    // Cargo.toml restored automatically when _guard drops
    Ok(result)
}

// TESTABLE
#[test]
fn test_cargo_toml_restored_on_panic() {
    let temp_dir = tempfile::tempdir().unwrap();
    let cargo_toml = temp_dir.path().join("Cargo.toml");
    fs::write(&cargo_toml, "original").unwrap();

    let result = std::panic::catch_unwind(|| {
        let _guard = CargoTomlGuard::new(&cargo_toml).unwrap();
        fs::write(&cargo_toml, "modified").unwrap();
        panic!("Simulated error");
    });

    assert!(result.is_err());
    assert_eq!(fs::read_to_string(cargo_toml).unwrap(), "original");
}
```

**Benefits:**
- ✅ Guaranteed restoration even on panic
- ✅ No contamination between tests
- ✅ Explicit lifecycle management

---

## Implementation Plan

### Priority Order

| Phase | Risk | Effort | Impact | Order |
|-------|------|--------|--------|-------|
| 1. Pure functions | LOW | Small | High | **1st** |
| 3. File paths | LOW | Small | Medium | **2nd** |
| 6. Cargo.toml guard | MEDIUM | Small | High | **3rd** |
| 2. Remove global state | MEDIUM | Medium | High | **4th** |
| 4. Error logger trait | MEDIUM | Medium | Medium | **5th** |
| 5. Streaming verification | MEDIUM | Large | Low | **6th** |

### Incremental Migration Steps

#### Step 1: Add Pure Functions (No Breaking Changes)
```bash
# Add new functions alongside old ones
git checkout -b refactor/pure-functions

# Files to modify:
# - src/report.rs: Add build_output_lines()
# - src/console_format.rs: Add *_to() variants that accept Write

# Tests to add:
# - src/report_test.rs: Test output line construction
# - src/console_format_test.rs: Test rendering to buffer

cargo test  # Should pass with new functions
```

#### Step 2: Extract TableConfig (Parallel with old code)
```bash
git checkout -b refactor/table-config

# Files to modify:
# - src/console_format.rs: Add TableConfig struct
# - src/console_format.rs: Add config parameter (keep globals for fallback)

# Tests to add:
# - src/console_format_test.rs: Test with explicit config

cargo test  # Should pass with both code paths
```

#### Step 3: Add Cargo.toml Guard
```bash
git checkout -b refactor/cargo-toml-guard

# Files to create:
# - src/cargo_guard.rs: CargoTomlGuard implementation

# Files to modify:
# - src/runner.rs: Use guard in run_single_test

# Tests to add:
# - tests/cargo_toml_restoration_test.rs

cargo test  # New tests should catch contamination
```

#### Step 4: Switch Callsites
```bash
git checkout -b refactor/use-new-apis

# Files to modify:
# - src/main.rs: Use new *_to() functions with stdout
# - src/main.rs: Create and pass TableConfig

cargo test
cargo build --release
./target/release/cargo-copter --path test-crates/fixtures/rust-rgb-breaking --dependents load_image:3.3.1

# Verify output identical to before
```

#### Step 5: Remove Old Code
```bash
git checkout -b refactor/remove-globals

# Files to modify:
# - src/console_format.rs: Remove OnceLock statics
# - src/compile.rs: Remove lazy_static logs

# Verify nothing uses old APIs
cargo test --all
```

---

## Testing Strategy After Refactoring

### Unit Tests (Fast, No I/O)

```rust
// tests/output_formatting_test.rs
#[test]
fn test_separator_between_dependents() {
    let results = create_test_results();  // Pure data
    let lines = build_output_lines(results);

    assert_eq!(lines[0].needs_separator, false);
    assert_eq!(lines[3].needs_separator, true);  // New dependent
}

#[test]
fn test_error_deduplication() {
    let results = vec![
        create_result_with_error("error A"),
        create_result_with_error("error A"),  // Same
        create_result_with_error("error B"),  // Different
    ];

    let lines = build_output_lines(results);
    assert_eq!(lines[0].error_display, Some("error A".to_string()));
    assert_eq!(lines[1].error_display, Some("[SAME ERROR]".to_string()));
    assert_eq!(lines[2].error_display, Some("error B".to_string()));
}

#[test]
fn test_header_printed_once() {
    let mut buf = Vec::new();
    let config = TableConfig::for_testing();

    print_table_header_to(&mut buf, &config, "rgb", "0.8.91", 5, None, None).unwrap();

    let output = String::from_utf8(buf).unwrap();
    assert_eq!(output.matches("│ Offered │").count(), 1);
}
```

### Integration Tests (With tempdir)

```rust
// tests/full_run_test.rs
#[test]
fn test_full_run_output_format() {
    let temp_dir = tempfile::tempdir().unwrap();
    let args = CliArgs {
        markdown_output: temp_dir.path().join("report.md"),
        json_output: temp_dir.path().join("report.json"),
        // ... other args ...
    };

    // Run full pipeline
    let matrix = config::build_test_matrix(&args).unwrap();
    let results = runner::run_tests(matrix, |_| {}).unwrap();

    // Verify output files
    let md = fs::read_to_string(args.markdown_output).unwrap();
    assert!(md.contains("| Offered |"));
    assert!(!md.contains("copter:"));  // No debug messages
}
```

### Subprocess Tests (Slow, for E2E)

```rust
// tests/e2e_test.rs
#[test]
#[ignore]  // Slow, run with --ignored
fn test_cli_output_format() {
    let output = Command::new("cargo-copter")
        .args(&["--path", "test-crates/fixtures/rust-rgb-breaking"])
        .args(&["--dependents", "load_image:3.3.1"])
        .output()
        .unwrap();

    let stdout = String::from_utf8(output.stdout).unwrap();

    // High-level checks only
    assert!(stdout.contains("│ Offered │"));
    assert_eq!(stdout.matches("│ Offered │").count(), 1);  // Header once
    assert!(!stdout.contains("copter:"));  // No debug messages
}
```

---

## API Stability Considerations

### Public API (Unchanged)
- `cargo-copter` CLI interface (no changes)
- JSON report format (no changes)
- Markdown report format (no changes)

### Internal API (Changed)
- `console_format::*` functions gain `config` parameter
- `compile::*` functions gain `logger` parameter
- New `TableConfig` type exposed
- New `ErrorLogger` trait exposed

### Migration Guide for Users

**No user changes needed!** All refactoring is internal.

---

## Success Criteria

After refactoring, these tests should be **fast and reliable**:

- ✅ `test_header_appears_once()` - No subprocess, < 1ms
- ✅ `test_no_copter_status_messages()` - No subprocess, < 1ms
- ✅ `test_separator_between_dependents()` - Pure function, < 1ms
- ✅ `test_error_deduplication()` - Pure function, < 1ms
- ✅ `test_cargo_toml_restored()` - With tempdir, < 10ms
- ✅ `test_streaming_immediate()` - With timing, < 100ms
- ✅ `test_parallel_test_execution()` - Multiple instances, < 1s

**Before:** 1 integration test (subprocess, 10+ seconds, fragile)
**After:** 20+ unit tests (in-memory, < 100ms total, reliable)

---

## Risks and Mitigation

### Risk 1: Breaking Existing Behavior
**Mitigation:**
- Add new code alongside old
- Run both paths in parallel during migration
- Extensive manual testing before removing old code

### Risk 2: Test Fragility from Timing
**Mitigation:**
- Use `Duration` comparisons with tolerance
- Mock time sources in tests
- Focus on ordering, not exact durations

### Risk 3: Performance Regression
**Mitigation:**
- Benchmark before/after
- Avoid allocations in hot paths
- Profile with `cargo flamegraph`

---

## Conclusion

These refactorings will enable **reliable, fast regression tests** by:

1. **Separating pure logic from I/O** - Testable without subprocess
2. **Removing global state** - Tests can run in parallel
3. **Dependency injection** - Easy to mock/substitute
4. **RAII patterns** - Guaranteed cleanup

**Estimated Effort:** 2-3 days of focused work
**Risk Level:** Low-Medium (incremental changes)
**Benefit:** 10x faster tests, 100% coverage on critical paths

All 26 recurring bugs can be covered with fast, non-fragile tests after these changes.
