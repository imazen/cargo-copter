# Testability Refactoring - Quick Start Guide

**TL;DR:** Your code has good architecture but poor testability. These 6 refactorings will make all 26 recurring bugs testable without subprocess spawning.

---

## The Core Problem

```rust
// ❌ UNTESTABLE (requires subprocess)
pub fn print_table_header(...) {
    println!("│ Offered │ Spec │ ...");
}

#[test]
fn test_header_once() {
    // Can't capture println! output without spawning subprocess
    // subprocess = slow, fragile, hard to debug
}
```

```rust
// ✅ TESTABLE (accepts any writer)
pub fn print_table_header_to<W: Write>(writer: &mut W, ...) {
    writeln!(writer, "│ Offered │ Spec │ ...")?;
}

#[test]
fn test_header_once() {
    let mut buf = Vec::new();
    print_table_header_to(&mut buf, ...).unwrap();
    let output = String::from_utf8(buf).unwrap();
    assert_eq!(output.matches("│ Offered │").count(), 1);  // Fast! < 1ms
}
```

---

## 6 Refactorings (Priority Order)

### 1. Extract Pure Functions ⚡ (EASIEST, HIGHEST IMPACT)

**Current:** Logic mixed with I/O in `main.rs:95-131`

**Fix:** Separate data transformation from display

```rust
// NEW: Pure function (no I/O)
struct OutputLine {
    row: OfferedRow,
    needs_separator: bool,
    error_display: Option<String>,
}

fn build_output_lines(results: Vec<TestResult>) -> Vec<OutputLine> {
    // ... separator logic, error dedup ...
}

// TESTABLE
#[test]
fn test_separator_logic() {
    let results = vec![...];
    let lines = build_output_lines(results);
    assert_eq!(lines[0].needs_separator, false);
    assert_eq!(lines[3].needs_separator, true);  // New dependent
}
```

**Enables Testing:**
- ✅ Separator placement
- ✅ Error deduplication ("[SAME ERROR]")
- ✅ Double printing detection

---

### 2. Inject Writer (Accept `impl Write`) ⚡ (EASY, HIGH IMPACT)

**Current:** Functions hardcoded to `stdout`

**Fix:** Accept generic writer

```rust
// Before
pub fn print_table_header(...) {
    println!("...");
}

// After
pub fn print_table_header_to<W: Write>(writer: &mut W, ...) -> io::Result<()> {
    writeln!(writer, "...")?;
}

// Wrapper for backward compatibility
pub fn print_table_header(...) {
    print_table_header_to(&mut io::stdout(), ...).unwrap()
}
```

**Enables Testing:**
- ✅ Header printed exactly once
- ✅ No "copter:" debug messages
- ✅ Box-drawing validation
- ✅ All output format tests

---

### 3. Add Cargo.toml RAII Guard 🔒 (CRITICAL)

**Current:** No restoration mechanism, tests contaminate each other

**Fix:** RAII guard auto-restores

```rust
struct CargoTomlGuard {
    original_path: PathBuf,
    backup_path: PathBuf,
}

impl Drop for CargoTomlGuard {
    fn drop(&mut self) {
        // Restore original even on panic
        let _ = fs::copy(&self.backup_path, &self.original_path);
    }
}

// Usage
fn run_single_test(...) {
    let _guard = CargoTomlGuard::new(&cargo_toml)?;
    apply_patch(...)?;  // Modify Cargo.toml
    run_tests(...)?;
    // Auto-restored when guard drops
}

// TESTABLE
#[test]
fn test_restored_on_panic() {
    let result = std::panic::catch_unwind(|| {
        let _guard = CargoTomlGuard::new(...);
        panic!("Simulated error");
    });
    assert!(result.is_err());
    // Verify Cargo.toml restored
}
```

**Enables Testing:**
- ✅ No contamination between tests
- ✅ Safe parallel execution
- ✅ Guaranteed cleanup

---

### 4. Replace Global State with Context 🌐 (IMPORTANT)

**Current:** `OnceLock` in `console_format.rs` prevents parallel tests

```rust
// ❌ Global state
static WIDTHS: OnceLock<TableWidths> = OnceLock::new();

pub fn init_table_widths(...) {
    WIDTHS.set(...).ok();
}
```

**Fix:** Pass context object

```rust
// ✅ Explicit dependency
pub struct TableConfig {
    pub widths: TableWidths,
    pub use_colors: bool,
}

pub fn print_table_header_to<W: Write>(
    writer: &mut W,
    config: &TableConfig,  // NEW
    ...
) -> io::Result<()>
```

**Enables Testing:**
- ✅ Tests run in parallel
- ✅ No interference between tests
- ✅ Explicit dependencies

---

### 5. Inject File Paths 📁 (NICE TO HAVE)

**Current:** Hard-coded `copter-report.md` and `copter-report.json`

**Fix:** Accept paths as parameters

```rust
// In CLI args
#[clap(long, default_value = "copter-report.md")]
pub markdown_output: PathBuf,

#[clap(long, default_value = "copter-report.json")]
pub json_output: PathBuf,

// TESTABLE
#[test]
fn test_markdown_export() {
    let temp_dir = tempfile::tempdir().unwrap();
    let output = temp_dir.path().join("test.md");
    export_markdown_report(&output, ...).unwrap();
    assert!(output.exists());
}
```

**Enables Testing:**
- ✅ Parallel test execution
- ✅ No file conflicts
- ✅ Clean temporary directories

---

### 6. Error Logger Trait (OPTIONAL)

**Current:** `lazy_static!` log file in `compile.rs`

**Fix:** Trait for dependency injection

```rust
pub trait ErrorLogger {
    fn log_failure(&mut self, ...);
}

pub struct FileErrorLogger { ... }
pub struct MemoryErrorLogger { pub entries: Vec<ErrorLogEntry> }

// TESTABLE
#[test]
fn test_error_deduplication() {
    let mut logger = MemoryErrorLogger::default();
    logger.log_failure("error A");
    logger.log_failure("error A");  // Same
    assert_eq!(logger.entries[1].is_duplicate, true);
}
```

**Enables Testing:**
- ✅ Error logging without filesystem
- ✅ Verify deduplication logic

---

## Migration Path (No Breaking Changes!)

### Week 1: Add New Code
```bash
# Add *_to() functions alongside old ones
# Add TableConfig alongside global state
# Add CargoTomlGuard

# All existing code still works!
cargo test  # All pass
```

### Week 2: Switch Callsites
```bash
# Update main.rs to use new functions
# Pass TableConfig instead of using globals

cargo test  # All still pass
./target/release/cargo-copter ...  # Verify output identical
```

### Week 3: Remove Old Code
```bash
# Delete OnceLock statics
# Delete lazy_static logs
# Delete old function variants

cargo test --all  # Verify nothing uses old APIs
```

---

## Test Coverage Before vs After

### Before Refactoring
```
✅ Unit tests: 12 (config, runner, types)
❌ Output tests: 0 (would need subprocess)
❌ Contamination tests: 0 (no mechanism)
❌ Formatting tests: 0 (hardcoded stdout)

Total: 12 tests, 65% bug coverage, subprocess needed
```

### After Refactoring
```
✅ Unit tests: 12 (existing)
✅ Pure function tests: 8 (separator, error dedup, formatting)
✅ Output tests: 10 (header, messages, box-drawing)
✅ Contamination tests: 3 (Cargo.toml, file isolation)
✅ Integration tests: 5 (full pipeline with tempdir)

Total: 38 tests, 100% bug coverage, all fast (<100ms)
```

---

## Quick Win: Start with #1 and #2

These two refactorings are:
- ✅ **Low risk** (additive, no breaking changes)
- ✅ **High impact** (enable 14 of 26 bug tests)
- ✅ **Small effort** (< 1 day combined)

### Step-by-Step for #1 (Pure Functions)

1. Create `src/report.rs::build_output_lines()`
2. Move logic from `main.rs:95-131` callback
3. Add unit tests in `src/report_test.rs`
4. Update `main.rs` to call new function

**Estimated time:** 2-3 hours

### Step-by-Step for #2 (Writer Injection)

1. Add `*_to<W: Write>` variants to `console_format.rs`
2. Keep old functions as wrappers
3. Add tests in `src/console_format_test.rs`
4. Update `main.rs` to use new variants

**Estimated time:** 2-3 hours

---

## Example: Complete Refactoring of One Function

### Before
```rust
// console_format.rs
pub fn print_table_header(crate_name: &str, ...) {
    let widths = get_widths();  // Global state
    println!("┌{:─<w1$}┬...", "", w1 = widths.offered);  // Hardcoded stdout
    // ... more println! ...
}

// UNTESTABLE: Needs subprocess, uses global state
```

### After
```rust
// console_format.rs
pub fn print_table_header_to<W: Write>(
    writer: &mut W,        // Injected writer
    config: &TableConfig,  // Explicit config
    crate_name: &str,
    ...
) -> io::Result<()> {
    writeln!(writer, "┌{:─<w1$}┬...", "", w1 = config.widths.offered)?;
    // ... more writeln! ...
    Ok(())
}

// Backward compatibility wrapper
pub fn print_table_header(...) {
    let config = get_global_config();  // Still works during migration
    print_table_header_to(&mut io::stdout(), config, ...).unwrap()
}

// TESTABLE
#[test]
fn test_header_format() {
    let mut buf = Vec::new();
    let config = TableConfig::for_testing();
    print_table_header_to(&mut buf, &config, "rgb", "0.8.91", 5, None, None).unwrap();

    let output = String::from_utf8(buf).unwrap();
    assert_eq!(output.matches("│ Offered │").count(), 1);
    assert!(output.contains("┌───"));
    assert!(!output.contains("copter:"));
}
```

---

## Common Pitfalls to Avoid

### ❌ Don't: Use temp files in tests
```rust
#[test]
fn test_output() {
    let output = Command::new("cargo-copter")
        .args(&["--path", "..."])
        .output().unwrap();
    // Slow, fragile, hard to debug
}
```

### ✅ Do: Use in-memory buffers
```rust
#[test]
fn test_output() {
    let mut buf = Vec::new();
    print_table_header_to(&mut buf, ...).unwrap();
    let output = String::from_utf8(buf).unwrap();
    // Fast, reliable, easy to debug
}
```

### ❌ Don't: Modify real files in tests
```rust
#[test]
fn test_export() {
    export_markdown("copter-report.md", ...).unwrap();
    // Overwrites real file, tests interfere
}
```

### ✅ Do: Use tempdir
```rust
#[test]
fn test_export() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("test.md");
    export_markdown(&path, ...).unwrap();
    // Isolated, parallel-safe
}
```

### ❌ Don't: Test exact output strings
```rust
#[test]
fn test_table() {
    assert_eq!(output, "┌───┬───┬...");  // Fragile!
}
```

### ✅ Do: Test structural properties
```rust
#[test]
fn test_table() {
    assert_eq!(output.matches("│ Offered │").count(), 1);  // Header once
    assert!(!output.contains("copter:"));  // No debug messages
    assert!(!output.contains("─└"));  // No invalid box chars
}
```

---

## Success Metrics

After refactoring, you should have:

- ✅ **38+ tests** covering all 26 recurring bugs
- ✅ **< 100ms** total test time (vs 10+ seconds with subprocess)
- ✅ **100% reliable** (no flaky tests)
- ✅ **Parallel execution** (no global state conflicts)
- ✅ **Easy debugging** (in-memory buffers, not temp files)

---

## Questions?

**Q: Will this break existing functionality?**
A: No! All refactorings are additive. Old code keeps working.

**Q: How long will this take?**
A: 2-3 days for all 6 refactorings. Can start with #1 and #2 in < 1 day.

**Q: What if I only do refactorings #1 and #2?**
A: You'll still enable 14 of 26 bug tests (54% → 87% coverage). Great ROI!

**Q: Do I need to change the CLI interface?**
A: No! All changes are internal. Users see no difference.

**Q: Can I do this incrementally?**
A: Yes! Add new code alongside old, migrate callsites, then delete old code.

---

## Next Steps

1. **Read:** `REFACTORING_FOR_TESTABILITY.md` (full details)
2. **Start:** Refactoring #1 (pure functions) - lowest risk, highest impact
3. **Test:** Add tests as you refactor (TDD approach)
4. **Verify:** Compare output before/after each step
5. **Iterate:** One refactoring at a time

**Estimated total effort:** 2-3 focused days
**Estimated benefit:** 100% test coverage on all recurring bugs, 100x faster tests

Good luck! 🚀
