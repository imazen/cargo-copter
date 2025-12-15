// Allow dead code for methods that may be used in the future
#![allow(dead_code)]

//! Core data structures for test results
//!
//! This module defines the primary data structures used throughout cargo-copter
//! for representing test results, dependencies, and execution metadata.

/// A single row in the five-column console table output
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct OfferedRow {
    /// Baseline test result: None = this IS baseline, Some(bool) = baseline exists and passed/failed
    pub baseline_passed: Option<bool>,

    /// Primary dependency being tested (depth 0)
    pub primary: DependencyRef,

    /// Version offered for testing (None for baseline rows)
    pub offered: Option<OfferedVersion>,

    /// Test execution results for primary dependency
    pub test: TestExecution,

    /// Transitive dependencies using different versions (depth > 0)
    pub transitive: Vec<TransitiveTest>,
}

impl OfferedRow {
    /// Check if this is a regression (baseline passed but offered failed)
    pub fn is_regression(&self) -> bool {
        matches!(self.baseline_passed, Some(true)) && !self.test_passed()
    }

    /// Check if all test commands passed
    pub fn test_passed(&self) -> bool {
        self.test.commands.iter().all(|cmd| cmd.result.passed)
    }

    /// Check if this is a baseline row (no offered version)
    pub fn is_baseline(&self) -> bool {
        self.offered.is_none()
    }
}

/// Reference to a dependency (primary or transitive)
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct DependencyRef {
    pub dependent_name: String,         // "image"
    pub dependent_version: String,      // "0.25.8"
    pub spec: String,                   // "^0.8.52" (what they require)
    pub resolved_version: String,       // "0.8.91" (what cargo chose)
    pub resolved_source: VersionSource, // CratesIo | Local | Git
    pub used_offered_version: bool,     // true if resolved == offered
}

/// Version offered for testing
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct OfferedVersion {
    pub version: String, // "this(0.8.91)" or "0.8.51"
    pub forced: bool,    // true shows [≠→!] suffix
}

/// Test execution (Install/Check/Test)
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct TestExecution {
    pub commands: Vec<TestCommand>, // fetch, check, test
}

impl TestExecution {
    /// Create a new empty test execution
    pub fn new() -> Self {
        Self { commands: Vec::new() }
    }

    /// Add a test command result
    pub fn add_command(&mut self, command: TestCommand) {
        self.commands.push(command);
    }

    /// Check if all commands passed
    pub fn all_passed(&self) -> bool {
        self.commands.iter().all(|cmd| cmd.result.passed)
    }

    /// Get the first failed command, if any
    pub fn first_failure(&self) -> Option<&TestCommand> {
        self.commands.iter().find(|cmd| !cmd.result.passed)
    }
}

impl Default for TestExecution {
    fn default() -> Self {
        Self::new()
    }
}

/// A single test command (fetch, check, or test)
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct TestCommand {
    pub command: CommandType,
    pub features: Vec<String>,
    pub result: CommandResult,
}

/// Type of command executed
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum CommandType {
    Fetch,
    Check,
    Test,
}

impl CommandType {
    pub fn as_str(&self) -> &'static str {
        match self {
            CommandType::Fetch => "fetch",
            CommandType::Check => "check",
            CommandType::Test => "test",
        }
    }
}

/// Result of executing a command
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct CommandResult {
    pub passed: bool,
    pub duration: f64,
    pub failures: Vec<CrateFailure>, // Which crate(s) failed
}

/// A crate that failed during testing
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct CrateFailure {
    pub crate_name: String,
    pub error_message: String,
}

/// Transitive dependency test (depth > 0)
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct TransitiveTest {
    pub dependency: DependencyRef,
    pub depth: usize,
}

/// Source of a version (crates.io, local, or git)
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum VersionSource {
    CratesIo,
    Local,
    Git,
}

impl VersionSource {
    pub fn as_str(&self) -> &'static str {
        match self {
            VersionSource::CratesIo => "crates.io",
            VersionSource::Local => "local",
            VersionSource::Git => "git",
        }
    }
}

/// Extract error summary with fallback to stderr
///
/// Attempts to extract a clean error summary from diagnostics, falling back
/// to full stderr if extraction fails.
pub fn extract_error_with_fallback(
    diagnostics: &[crate::error_extract::Diagnostic],
    stderr: &str,
    _max_lines: usize,
) -> String {
    // Always extract FULL error for storage - truncation happens at display time
    let msg = crate::error_extract::extract_error_summary(diagnostics, 0); // 0 = unlimited
    if !msg.is_empty() {
        msg
    } else {
        // Return full stderr
        stderr.to_string()
    }
}

// ============================================================================
// NEW: Unified Multi-Version Architecture
// ============================================================================

/// A concrete version identifier
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum Version {
    /// Semantic version (e.g., "0.8.52", "1.0.0-alpha.1")
    Semver(String),
    /// Git revision (e.g., "abc123f", "main", "v1.0.0")
    Git { rev: String },
    /// Unresolved - will be determined at test time
    Latest,
}

impl Version {
    /// Get display string for this version
    pub fn display(&self) -> String {
        match self {
            Version::Semver(v) => v.clone(),
            Version::Git { rev } => format!("git:{}", rev),
            Version::Latest => "latest".to_string(),
        }
    }

    /// Check if this version is resolved (not Latest)
    pub fn is_resolved(&self) -> bool {
        !matches!(self, Version::Latest)
    }
}

/// Where a crate version comes from
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum CrateSource {
    /// Published on crates.io
    Registry,
    /// Local filesystem path
    Local { path: std::path::PathBuf },
    /// Git repository
    Git { url: String, rev: Option<String> },
}

impl CrateSource {
    pub fn as_str(&self) -> &'static str {
        match self {
            CrateSource::Registry => "registry",
            CrateSource::Local { .. } => "local",
            CrateSource::Git { .. } => "git",
        }
    }
}

/// A versioned crate from any source
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct VersionedCrate {
    /// Crate name
    pub name: String,
    /// Version identifier
    pub version: Version,
    /// Where this crate comes from
    pub source: CrateSource,
}

impl VersionedCrate {
    /// Create a new versioned crate from crates.io with semver
    pub fn from_registry(name: impl Into<String>, version: impl Into<String>) -> Self {
        Self { name: name.into(), version: Version::Semver(version.into()), source: CrateSource::Registry }
    }

    /// Create a new versioned crate from local path
    pub fn from_local(name: impl Into<String>, version: impl Into<String>, path: std::path::PathBuf) -> Self {
        Self { name: name.into(), version: Version::Semver(version.into()), source: CrateSource::Local { path } }
    }

    /// Create a new versioned crate with latest version from registry
    pub fn latest_from_registry(name: impl Into<String>) -> Self {
        Self { name: name.into(), version: Version::Latest, source: CrateSource::Registry }
    }

    /// Get display string for this crate
    pub fn display(&self) -> String {
        format!("{} {}", self.name, self.version.display())
    }
}

/// Override mechanism for testing
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum OverrideMode {
    /// No override - use naturally resolved version
    None,
    /// Patch via [patch.crates-io] (respects semver)
    Patch,
    /// Force via direct Cargo.toml replacement (bypasses semver)
    Force,
}

/// A version specification for testing
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct VersionSpec {
    /// The versioned crate
    pub crate_ref: VersionedCrate,
    /// Override mechanism (for base crate versions)
    pub override_mode: OverrideMode,
    /// Whether this is the baseline reference
    pub is_baseline: bool,
}

impl VersionSpec {
    /// Create a new baseline version spec
    pub fn baseline(crate_ref: VersionedCrate) -> Self {
        Self { crate_ref, override_mode: OverrideMode::None, is_baseline: true }
    }

    /// Create a new version spec with patch mode
    pub fn with_patch(crate_ref: VersionedCrate) -> Self {
        Self { crate_ref, override_mode: OverrideMode::Patch, is_baseline: false }
    }

    /// Create a new version spec with force mode
    pub fn with_force(crate_ref: VersionedCrate) -> Self {
        Self { crate_ref, override_mode: OverrideMode::Force, is_baseline: false }
    }
}

/// Complete test specification - a matrix of versions × dependents
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TestMatrix {
    /// The base crate being tested (what you're publishing)
    pub base_crate: String,

    /// Versions of the BASE CRATE to test
    /// Example: ["0.8.50" (baseline), "0.8.52", "0.9.0-local"]
    pub base_versions: Vec<VersionSpec>,

    /// Dependents to test against, each with their version
    /// Example: [("image", "0.25.8"), ("imageproc", "latest")]
    pub dependents: Vec<VersionSpec>,

    /// Staging directory for builds
    pub staging_dir: std::path::PathBuf,

    /// Test execution flags
    pub skip_check: bool,
    pub skip_test: bool,
    pub error_lines: usize,
}

impl TestMatrix {
    /// Total number of tests that will run
    pub fn test_count(&self) -> usize {
        self.base_versions.len() * self.dependents.len()
    }

    /// Iterator over all (base_version, dependent) pairs to test
    pub fn test_pairs(&self) -> impl Iterator<Item = (&VersionSpec, &VersionSpec)> {
        self.base_versions.iter().flat_map(move |v| self.dependents.iter().map(move |d| (v, d)))
    }
}

/// Comparison with baseline version
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct BaselineComparison {
    /// Did baseline pass all steps?
    pub baseline_passed: bool,
    /// The baseline version that was compared against
    pub baseline_version: String,
}

/// Result of testing one (version, dependent) pair
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TestResult {
    /// Which version of the base crate was tested
    pub base_version: VersionedCrate,
    /// Which dependent was tested
    pub dependent: VersionedCrate,
    /// The three-step ICT result
    pub execution: crate::compile::ThreeStepResult,
    /// Baseline comparison (if this is not the first/baseline version)
    pub baseline: Option<BaselineComparison>,
}

/// Test status for reporting
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum TestStatus {
    /// This is the baseline test
    Baseline { passed: bool },
    /// Baseline passed, this version passed
    Passed,
    /// Baseline passed, this version failed (regression)
    Regressed,
    /// Baseline failed, this version passed (fix)
    Fixed,
    /// Baseline failed, this version failed (still broken)
    StillBroken,
}

impl TestResult {
    /// Determine the status for reporting
    pub fn status(&self) -> TestStatus {
        let current_passed = self.execution.is_success();

        match &self.baseline {
            None => {
                // This IS the baseline - no comparison
                TestStatus::Baseline { passed: current_passed }
            }
            Some(cmp) => match (cmp.baseline_passed, current_passed) {
                (true, true) => TestStatus::Passed,
                (true, false) => TestStatus::Regressed,
                (false, true) => TestStatus::Fixed,
                (false, false) => TestStatus::StillBroken,
            },
        }
    }

    /// Check if this is a baseline test
    pub fn is_baseline(&self) -> bool {
        self.baseline.is_none()
    }

    /// Check if this test passed
    pub fn passed(&self) -> bool {
        self.execution.is_success()
    }
}

/// Convert CompileResult to TestCommand for OfferedRow construction
pub fn compile_result_to_command(
    compile_result: &crate::compile::CompileResult,
    command_type: CommandType,
    crate_name: &str,
    max_error_lines: usize,
) -> TestCommand {
    let failures = if !compile_result.success {
        let error_msg =
            extract_error_with_fallback(&compile_result.diagnostics, &compile_result.stderr, max_error_lines);
        vec![CrateFailure { crate_name: crate_name.to_string(), error_message: error_msg }]
    } else {
        vec![]
    };

    TestCommand {
        command: command_type,
        features: vec![],
        result: CommandResult {
            passed: compile_result.success,
            duration: compile_result.duration.as_secs_f64(),
            failures,
        },
    }
}
