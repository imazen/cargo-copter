# Cargo Crusader Architecture Overview

## High-Level Data Flow

```
┌─────────────────────────────────────────────────────────────────────┐
│                         CLI ENTRY (cli.rs)                          │
│  --path, --dependents, --test-versions, --jobs, --output, etc.     │
└─────────────────────────┬───────────────────────────────────────────┘
                          │
                          ▼
┌─────────────────────────────────────────────────────────────────────┐
│                    Configuration (main.rs:133)                      │
│  Config { crate_name, version, git_hash, staging_dir, ... }        │
└─────────────────────────┬───────────────────────────────────────────┘
                          │
                          ▼
┌─────────────────────────────────────────────────────────────────────┐
│                    run() - Main orchestration                        │
│                          (main.rs:79)                               │
│                                                                     │
│  1. Get list of reverse dependencies (api.rs)                       │
│  2. Create thread pool for parallel testing                         │
│  3. For each dependent:                                             │
│       └─► run_test() spawns in thread pool                         │
└─────────────────────────┬───────────────────────────────────────────┘
                          │
                          ▼
┌─────────────────────────────────────────────────────────────────────┐
│         run_test_local() - Per-dependent orchestration              │
│                      (main.rs:576)                                  │
│                                                                     │
│  1. Resolve dependent version (crates.io API)                       │
│  2. Extract resolved version (cargo metadata)                       │
│  3. Check version compatibility                                     │
│  4. FOR EACH TEST VERSION (Phase 5):                               │
│       │                                                             │
│       ├─► compile_with_custom_dep(baseline) ─────┐                 │
│       │   (main.rs:766 - LEGACY)              │   │                 │
│       │   OR                                  │   │                 │
│       │   compile_crate() ─────────────────────┤   │                 │
│       │   (compile.rs:63 - MODERN)           │   │                 │
│       │                                        │   │                 │
│       ├─► Determine status                    │   │                 │
│       │   (PASSED/REGRESSED/BROKEN/SKIPPED)   │   │                 │
│       │                                        │   │                 │
│       └─► Collect TestResult ◄───────────────┘   │                 │
│                                                   │                 │
└─────────────────────────────────┬─────────────────┘                 │
                                  │                                   │
                                  ▼                                   │
┌─────────────────────────────────────────────────────────────────────┐
│              report_results() - Generate reports                    │
│                      (main.rs:954)                                  │
│                                                                     │
│  Results: Vec<TestResult> ──┬─► print_console_table() (report.rs) │
│                             │                                       │
│                             ├─► export_html_report() (report.rs)   │
│                             │                                       │
│                             └─► export_markdown_report() (report)  │
│                                                                     │
│  Outputs:                                                           │
│    - Console summary with colors                                    │
│    - crusader-report.html (detailed results)                       │
│    - crusader-analysis.md (LLM-friendly format)                    │
└─────────────────────────────────────────────────────────────────────┘
```

## Compilation Flow - Current vs Phase 5

### Current (Two-Version Testing)

```
Per Dependent:

  compile_with_custom_dep(CrateOverride::Default)
         │
         ├─ Emit .cargo/config with paths = [published]
         ├─ Run cargo build
         └─ Return CompileResult {success, stdout, stderr, ...}
         
         If FAILED → Status = BROKEN, Stop
         
  compile_with_custom_dep(CrateOverride::Source(path))
         │
         ├─ Emit .cargo/config with paths = [/path/to/wip]
         ├─ Run cargo build
         └─ Return CompileResult
         
         If FAILED → Status = REGRESSED
         If PASSED → Status = PASSED
```

### Phase 5 (Multi-Version Testing)

```
Per Dependent, Per Version:

  Version v1 ─┐
  Version v2  ├─► compile_crate(v, step=Fetch)
  Version v3  │        │
  ...         │        ├─ Create .cargo/config.toml with [patch.crates-io]
              │        ├─ Run cargo fetch --message-format=json
              │        └─ Parse JSON diagnostics
              │           Return CompileResult {success, diagnostics, ...}
              │
              ├─► compile_crate(v, step=Check)
              │        ├─ Run cargo check --message-format=json
              │        └─ Return CompileResult
              │
              └─► compile_crate(v, step=Test)
                       ├─ Run cargo test --message-format=json
                       └─ Return CompileResult

All three steps wrapped in: run_four_step_test()
```

## Data Structure Hierarchy

### Current (Two-Version)

```
TestResult
├── rev_dep: RevDep {
│   ├── name: "image"
│   ├── vers: Version 0.25.8
│   └── resolved_version: Some("1.0.0")
│
└── data: TestResultData::Passed(FourStepResult) {
    ├── baseline_check: CompileResult {...}
    ├── baseline_test: Some(CompileResult {...})
    ├── override_check: Some(CompileResult {...})
    └── override_test: Some(CompileResult {...})
}
```

### Phase 5 (Multi-Version)

```
Option A: Extend existing
──────────────────────────
TestResult
├── rev_dep: RevDep {...}
├── version_label: "0.8.0" or "this"
│
└── data: TestResultData::Passed(FourStepResult) {
    ├── fetch: CompileResult {...}     # NEW
    ├── check: CompileResult {...}     # NEW
    └── test: CompileResult {...}      # NEW
}

Option B: New wrapper
─────────────────────
VersionedTestResult {
    rev_dep: RevDep {...},
    version_results: Vec<VersionResult> {
        ├── VersionResult {
        │   version_label: "0.8.0",
        │   result: TestResultData::Passed(...)
        │}
        ├── VersionResult {
        │   version_label: "0.8.48",
        │   result: TestResultData::Passed(...)
        │}
        └── VersionResult {
            version_label: "this",
            result: TestResultData::Regressed(...)
        }
    }
}
```

## Compilation Result Types

### CompileResult (Single Step)

```
CompileResult {
    step: CompileStep,           # Fetch, Check, or Test
    success: bool,               # true if exit code 0
    stdout: String,              # Full stdout
    stderr: String,              # Full stderr
    duration: Duration,          # Elapsed time
    diagnostics: Vec<Diagnostic> # Parsed errors/warnings
}

Diagnostic {
    level: DiagnosticLevel,           # Error, Warning, Help, Note
    code: Option<String>,             # e.g., "E0308"
    message: String,                  # Error message text
    rendered: String,                 # Full formatted output
    primary_span: Option<SpanInfo>    # Location info
}
```

### FourStepResult (Multi-Step Test)

```
FourStepResult {
    baseline_check: CompileResult,              # Always run
    baseline_test: Option<CompileResult>,       # Optional
    override_check: Option<CompileResult>,      # Optional
    override_test: Option<CompileResult>,       # Optional
}

Status Detection:
  is_broken()     → baseline_check fails → Stop testing
  is_regressed()  → baseline ok, override fails → Breaking change
  is_passed()     → all steps pass → OK
```

## Caching System

```
.crusader/
├── crate-cache/
│   ├── image/
│   │   ├── image-0.25.8.crate    ◄─ Downloaded from crates.io once
│   │   └── image-0.24.0.crate
│   └── serde/
│       ├── serde-1.0.0.crate
│       └── serde-1.0.1.crate
│
└── staging/
    ├── image-0.25.8/
    │   ├── src/
    │   ├── Cargo.toml
    │   ├── target/               ◄─ Build artifacts cached
    │   └── .cargo/config         ◄─ Override config
    │
    └── serde-1.0.0/
        ├── src/
        ├── ...
        └── target/

Performance: 10x speedup on reruns (cached artifacts)
```

## Report Generation Flow

```
Vec<TestResult>
    │
    ├─► print_console_table()
    │   ├─ Format status (PASSED/REGRESSED/BROKEN/SKIPPED/ERROR)
    │   ├─ Format dependent (name + version)
    │   ├─ Format dependencies (baseline version + checkmarks)
    │   ├─ Format testing (WIP version + checkmarks)
    │   └─ Format duration (total time)
    │   Output: Terminal table with colors
    │
    ├─► export_html_report()
    │   ├─ Summary statistics (boxes with counts)
    │   ├─ Results table (sortable)
    │   ├─ Detailed sections per crate
    │   │  ├─ Baseline check (stdout/stderr)
    │   │  ├─ Baseline test (stdout/stderr)
    │   │  ├─ Override check (stdout/stderr)
    │   │  └─ Override test (stdout/stderr)
    │   └─ Output: HTML file
    │
    └─► export_markdown_report()
        ├─ Summary table (pipe-delimited)
        ├─ Regressions section (with details)
        ├─ Broken crates section
        ├─ Skipped crates section
        └─ Output: Markdown file

Console Table (Five-Column Format):
┌────────────────────┬──────────┬─────────────────┬─────────────────────┬─────────────────────┐
│ Offered            │ Spec     │ Resolved        │ Dependent           │ Result         Time │
├────────────────────┼──────────┼─────────────────┼─────────────────────┼─────────────────────┤
│ - baseline         │ ^0.8.52  │ 0.8.51 📦       │ image 0.25.8        │ PASSED ✓✓✓     2.1s │
│ ✓ =this(0.8.91)    │ ^0.8.52  │ 0.8.91 📁       │ image 0.25.8        │ PASSED ✓✓✓     1.9s │
├────────────────────┼──────────┼─────────────────┼─────────────────────┼─────────────────────┤
│ - baseline         │ ^0.8     │ 0.8.51 📦       │ pixels 0.14         │ PASSED ✓✓✓     1.5s │
│ ✓ =this(0.8.91)    │ ^0.8     │ 0.8.91 📁       │ pixels 0.14         │ PASSED ✓✓✓     1.4s │
└────────────────────┴──────────┴─────────────────┴─────────────────────┴─────────────────────┘

Legend: I=Install (cargo fetch), C=Check (cargo check), T=Test (cargo test)
Icons: ✓=passed ✗=failed ⊘=skipped -=baseline  📦=crates.io 📁=local
```

## Console Output Format

The console table uses a **five-column format** that displays both baseline and offered versions:

**Columns**: Offered | Spec | Resolved | Dependent | Result

**Key Features**:
- Baseline rows (published version) vs. offered rows (WIP/test version)
- Status icons: ✓ (passed), ✗ (failed/regressed), ⊘ (skipped), - (baseline)
- Resolution markers: = (exact), ↑ (upgrade), ≠ (mismatch/forced)
- ICT status (Install/Check/Test): ✓✓✓, ✓✗-, etc.
- Source indicators: 📦 (crates.io), 📁 (local), 🔀 (git)
- Error details with dropped-panel borders spanning columns 2-5
- Multi-version transitive dependency trees with ├─ prefixes

**See [CONSOLE-FORMAT.md](CONSOLE-FORMAT.md) for complete format specification, data structures, rendering logic, and 9 demo scenarios.**

## Module Dependencies

```
main.rs (Orchestration)
    │
    ├─► cli.rs (Argument parsing)
    │
    ├─► api.rs (crates.io API)
    │   └─► Fetches reverse dependencies
    │
    ├─► compile.rs (Compilation logic)
    │   ├─► Runs cargo commands
    │   └─► Manages .cargo overrides
    │       └─► error_extract.rs (JSON parsing)
    │
    ├─► error_extract.rs (Diagnostic parsing)
    │   └─► Parses cargo JSON output
    │
    └─► report.rs (Report generation)
        ├─► Console table formatting
        ├─► HTML generation
        └─► Markdown generation

External Dependencies:
    - clap (CLI argument parsing)
    - semver (Version handling)
    - serde_json (JSON parsing)
    - crates_io_api (crates.io HTTP API)
    - threadpool (Parallel testing)
    - term (Colored terminal output)
    - toml (Cargo.toml parsing)
```

## Status Classification Logic

```
Input: FourStepResult {baseline_check, baseline_test, override_check, override_test}

┌─────────────────────────────────────────────────────────────┐
│ Is baseline_check successful?                              │
└──┬────────────────────────────────────────┬────────────────┘
   │ NO                                     │ YES
   │                                         │
   ▼                                         ▼
┌────────────────────┐         ┌──────────────────────────────┐
│ Status = BROKEN    │         │ Is baseline_test successful? │
│                    │         │ (if it ran)                  │
│ (Pre-existing      │         └──┬──────────────────────┬────┘
│  issue in          │            │ NO                  │ YES
│  published         │            │                      │
│  version)          │            ▼                      ▼
└────────────────────┘    ┌─────────────────┐    ┌───────────────────────────┐
                          │ Status = BROKEN │    │ Is override_check passed? │
                          └─────────────────┘    │ (if it ran)               │
                                                 └──┬─────────────┬──────────┘
                                                    │ NO          │ YES
                                                    │             │
                                                    ▼             ▼
                                              ┌──────────┐  ┌──────────────────┐
                                              │REGRESSED │  │Is override_test? │
                                              │(breaking │  │   (if ran)       │
                                              │ change)  │  └──┬─────────┬─────┘
                                              └──────────┘     │ NO      │ YES
                                                               │         │
                                                               ▼         ▼
                                                          ┌──────────┐  ┌────────┐
                                                          │REGRESSED│  │PASSED  │
                                                          └──────────┘  └────────┘
```

## CLI Arguments Processing

```
CliArgs::parse_args()
    │
    ├─ Validate arguments
    │  └─ Can't skip both check and test
    │  └─ Need at least one dependent source
    │  └─ Jobs must be >= 1
    │
    └─ Create Config {
        crate_name,         ◄─ From Cargo.toml
        version,            ◄─ From Cargo.toml
        git_hash,           ◄─ From `git rev-parse --short HEAD`
        is_dirty,           ◄─ From `git status --porcelain`
        staging_dir,        ◄─ --staging-dir (default: .crusader/staging)
        base_override,      ◄─ CrateOverride::Default (published)
        next_override,      ◄─ CrateOverride::Source(manifest)
        limit,              ◄─ From CRUSADER_LIMIT env var
    }

Phase 5 Addition:
    └─ test_versions: Vec<String>  ◄─ From --test-versions (NOW USED)
```

## Error Handling & Diagnostics

```
cargo --message-format=json
    │
    └─► stdout (JSON lines)
         │
         └─► parse_cargo_json()
              │
              ├─ Filter by reason="compiler-message"
              ├─ Extract code (e.g., "E0308")
              ├─ Extract message (e.g., "mismatched types")
              ├─ Extract spans (file, line, column)
              ├─ Extract rendered (full formatted error)
              │
              └─► Vec<Diagnostic>
                   │
                   └─► Included in CompileResult.diagnostics
                       │
                       └─► Used in reports
                           ├─ Console: Error codes highlighted
                           ├─ HTML: Full diagnostic output
                           └─ Markdown: Structured error details
```

## Phase 5 Data Flow (New)

```
--test-versions [0.8.0, 0.8.48, this]
    │
    ▼
run() - Determine test_versions from CLI
    │
    ├─ If --test-versions provided → use it
    ├─ If --path provided → infer "this" from Cargo.toml
    └─ Combine into final versions list
    
    │
    ▼
For each dependent:
    │
    └─► run_test_local()
        │
        ├─ For version="0.8.0":
        │  └─ compile_crate(0.8.0) → [Fetch, Check, Test]
        │     └─ Collect results → VersionResult
        │
        ├─ For version="0.8.48":
        │  └─ compile_crate(0.8.48) → [Fetch, Check, Test]
        │     └─ Collect results → VersionResult
        │
        └─ For version="this":
           └─ compile_crate("this") → [Fetch, Check, Test]
              └─ Collect results → VersionResult
              
              │
              ▼
         Determine status per version:
         ├─ "0.8.0"  → REGRESSED (check failed)
         ├─ "0.8.48" → PASSED
         └─ "this"   → PASSED
         
         │
         └─► Generate reports with version matrix
             ├─ Console: 3 rows (one per version)
             ├─ HTML: Version column + expanded details
             └─ Markdown: Version matrix table
```

