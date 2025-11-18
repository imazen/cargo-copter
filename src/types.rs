/// Core data structures for test results
///
/// This module defines the primary data structures used throughout cargo-copter
/// for representing test results, dependencies, and execution metadata.

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

/// Convert CompileResult to TestCommand for OfferedRow construction
pub fn compile_result_to_command(
    compile_result: &crate::compile::CompileResult,
    command_type: CommandType,
    crate_name: &str,
    max_error_lines: usize,
) -> TestCommand {
    let failures = if !compile_result.success {
        let error_msg = extract_error_with_fallback(
            &compile_result.diagnostics,
            &compile_result.stderr,
            max_error_lines,
        );
        vec![CrateFailure {
            crate_name: crate_name.to_string(),
            error_message: error_msg,
        }]
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
