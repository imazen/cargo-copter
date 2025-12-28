//! Core types for the compile/test execution system.
//!
//! This module defines all the data structures used in the three-step
//! Install/Check/Test (ICT) execution pipeline.

use crate::error_extract::Diagnostic;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::time::Duration;

/// How deeply we patch the dependency tree to force a specific version.
///
/// # Patching Strategies
///
/// The patching depth determines how aggressively we override Cargo's
/// normal dependency resolution:
///
/// ```text
/// PatchDepth::None   → No override, use Cargo's natural resolution
///                      (baseline testing)
///
/// PatchDepth::Force  → Modify Cargo.toml directly: rgb = "0.8.91"
///                      Marker: `!`
///                      Works when dependent's requirement is compatible
///
/// PatchDepth::Patch  → Add [patch.crates-io] section to Cargo.toml
///                      Marker: `!!`
///                      Unifies ALL versions of the crate across the tree
///                      Used when Force fails with "multiple versions" error
///
/// PatchDepth::DeepPatch → Recursive patching of transitive dependencies
///                         Marker: `!!!`
///                         For complex multi-version conflicts
///                         Shows advice about blocking crates
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum PatchDepth {
    /// No patching - use Cargo's natural dependency resolution.
    /// This is the baseline: test what the dependent would get normally.
    #[default]
    None,

    /// Force override via Cargo.toml modification.
    /// Changes the dependency version directly: `rgb = "0.8.91"`
    /// Marker: `!`
    Force,

    /// Patch via `[patch.crates-io]` section.
    /// Unifies all versions of the crate across the entire dependency tree.
    /// Used when Force fails due to "multiple versions of crate" errors.
    /// Marker: `!!`
    Patch,

    /// Deep recursive patching for complex conflicts.
    /// Marker: `!!!`
    DeepPatch,
}

impl PatchDepth {
    /// Get the marker string for display (e.g., "!", "!!", "!!!")
    pub fn marker(&self) -> &'static str {
        match self {
            PatchDepth::None => "",
            PatchDepth::Force => "!",
            PatchDepth::Patch => "!!",
            PatchDepth::DeepPatch => "!!!",
        }
    }

    /// Check if this represents any form of patching
    pub fn is_patched(&self) -> bool {
        !matches!(self, PatchDepth::None)
    }
}

/// Source of a version being tested.
///
/// This enum tracks where a version comes from:
/// - Published on crates.io (with optional forced flag)
/// - Local filesystem path (for WIP testing)
#[derive(Debug, Clone)]
pub enum VersionSource {
    /// A published version from crates.io
    Published {
        /// The semver version string (e.g., "0.8.91")
        version: String,
        /// Whether this version should be force-installed
        forced: bool,
    },

    /// A local filesystem version (WIP)
    Local {
        /// Path to the local crate directory or Cargo.toml
        path: PathBuf,
        /// Whether this version should be force-installed
        forced: bool,
    },
}

impl VersionSource {
    /// Get the version string
    pub fn version_string(&self) -> String {
        match self {
            VersionSource::Published { version, .. } => version.clone(),
            VersionSource::Local { path, .. } => {
                // Extract version from path or return "local"
                format!("local:{}", path.display())
            }
        }
    }

    /// Check if this is a forced version
    pub fn is_forced(&self) -> bool {
        match self {
            VersionSource::Published { forced, .. } => *forced,
            VersionSource::Local { forced, .. } => *forced,
        }
    }
}

/// Steps in the three-step ICT (Install/Check/Test) pipeline.
///
/// Each step runs a different Cargo command:
/// - Fetch: `cargo fetch` - download dependencies
/// - Check: `cargo check` - type-check without generating code
/// - Test: `cargo test` - run the test suite
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CompileStep {
    Fetch,
    Check,
    Test,
}

impl CompileStep {
    /// Get the human-readable name for display
    pub fn as_str(&self) -> &'static str {
        match self {
            CompileStep::Fetch => "fetch",
            CompileStep::Check => "check",
            CompileStep::Test => "test",
        }
    }

    /// Get the Cargo subcommand name
    pub fn cargo_subcommand(&self) -> &'static str {
        match self {
            CompileStep::Fetch => "fetch",
            CompileStep::Check => "check",
            CompileStep::Test => "test",
        }
    }
}

/// Result of executing a single Cargo command (fetch, check, or test).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompileResult {
    /// Which step this result is for
    pub step: CompileStep,

    /// Whether the command succeeded (exit code 0)
    pub success: bool,

    /// How long the command took to execute
    #[serde(with = "duration_serde")]
    pub duration: Duration,

    /// Raw stdout output
    pub stdout: String,

    /// Raw stderr output
    pub stderr: String,

    /// Parsed diagnostic messages from JSON output
    pub diagnostics: Vec<Diagnostic>,
}

impl CompileResult {
    /// Create a successful result
    pub fn success(step: CompileStep, duration: Duration) -> Self {
        Self { step, success: true, duration, stdout: String::new(), stderr: String::new(), diagnostics: Vec::new() }
    }

    /// Create a failed result
    pub fn failure(step: CompileStep, duration: Duration, stderr: String, diagnostics: Vec<Diagnostic>) -> Self {
        Self { step, success: false, duration, stdout: String::new(), stderr, diagnostics }
    }

    /// Check if this step failed
    pub fn failed(&self) -> bool {
        !self.success
    }
}

/// Result of running the complete three-step ICT pipeline.
///
/// This captures the results of all three steps (fetch, check, test),
/// along with metadata about the test execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThreeStepResult {
    /// Result of `cargo fetch`
    pub fetch: CompileResult,

    /// Result of `cargo check` (None if skipped or fetch failed)
    pub check: Option<CompileResult>,

    /// Result of `cargo test` (None if skipped or earlier step failed)
    pub test: Option<CompileResult>,

    /// Whether the version was force-installed (bypassing semver)
    pub forced_version: bool,

    /// How deeply we patched the dependency tree
    pub patch_depth: PatchDepth,

    /// The version that was actually resolved (from Cargo.lock)
    pub actual_version: Option<String>,

    /// The expected/offered version being tested (e.g., "0.8.91")
    pub expected_version: Option<String>,

    /// The original version requirement from the dependent's Cargo.toml
    pub original_requirement: Option<String>,

    /// All versions of the base crate found in the dependency tree.
    /// Format: (spec, resolved_version, dependent_name)
    /// Used to detect multi-version conflicts.
    pub all_crate_versions: Vec<(String, String, String)>,

    /// Crates that are blocking a clean resolution.
    /// These are transitive dependencies that pull in incompatible versions.
    pub blocking_crates: Vec<String>,
}

impl ThreeStepResult {
    /// Check if all executed steps succeeded
    pub fn is_success(&self) -> bool {
        self.fetch.success
            && self.check.as_ref().is_none_or(|c| c.success)
            && self.test.as_ref().is_none_or(|t| t.success)
    }

    /// Get the first step that failed, if any
    pub fn first_failure(&self) -> Option<CompileStep> {
        if !self.fetch.success {
            return Some(CompileStep::Fetch);
        }
        if let Some(ref check) = self.check
            && !check.success
        {
            return Some(CompileStep::Check);
        }
        if let Some(ref test) = self.test
            && !test.success
        {
            return Some(CompileStep::Test);
        }
        None
    }

    /// Get the total duration of all executed steps
    pub fn total_duration(&self) -> Duration {
        let mut total = self.fetch.duration;
        if let Some(ref check) = self.check {
            total += check.duration;
        }
        if let Some(ref test) = self.test {
            total += test.duration;
        }
        total
    }
}

/// Serde support for Duration (serializes as seconds)
mod duration_serde {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};
    use std::time::Duration;

    pub fn serialize<S>(duration: &Duration, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        duration.as_secs_f64().serialize(serializer)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Duration, D::Error>
    where
        D: Deserializer<'de>,
    {
        let secs = f64::deserialize(deserializer)?;
        Ok(Duration::from_secs_f64(secs))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_patch_depth_markers() {
        assert_eq!(PatchDepth::None.marker(), "");
        assert_eq!(PatchDepth::Force.marker(), "!");
        assert_eq!(PatchDepth::Patch.marker(), "!!");
        assert_eq!(PatchDepth::DeepPatch.marker(), "!!!");
    }

    #[test]
    fn test_patch_depth_is_patched() {
        assert!(!PatchDepth::None.is_patched());
        assert!(PatchDepth::Force.is_patched());
        assert!(PatchDepth::Patch.is_patched());
        assert!(PatchDepth::DeepPatch.is_patched());
    }

    #[test]
    fn test_compile_step_as_str() {
        assert_eq!(CompileStep::Fetch.as_str(), "fetch");
        assert_eq!(CompileStep::Check.as_str(), "check");
        assert_eq!(CompileStep::Test.as_str(), "test");
    }

    #[test]
    fn test_compile_step_cargo_subcommand() {
        assert_eq!(CompileStep::Fetch.cargo_subcommand(), "fetch");
        assert_eq!(CompileStep::Check.cargo_subcommand(), "check");
        assert_eq!(CompileStep::Test.cargo_subcommand(), "test");
    }

    #[test]
    fn test_three_step_result_success() {
        let result = ThreeStepResult {
            fetch: CompileResult::success(CompileStep::Fetch, Duration::from_secs(1)),
            check: Some(CompileResult::success(CompileStep::Check, Duration::from_secs(2))),
            test: Some(CompileResult::success(CompileStep::Test, Duration::from_secs(3))),
            forced_version: false,
            patch_depth: PatchDepth::None,
            actual_version: Some("0.8.91".to_string()),
            expected_version: Some("0.8.91".to_string()),
            original_requirement: Some("^0.8".to_string()),
            all_crate_versions: vec![],
            blocking_crates: vec![],
        };
        assert!(result.is_success());
        assert_eq!(result.first_failure(), None);
        assert_eq!(result.total_duration(), Duration::from_secs(6));
    }

    #[test]
    fn test_three_step_result_fetch_failure() {
        let result = ThreeStepResult {
            fetch: CompileResult::failure(CompileStep::Fetch, Duration::from_secs(1), "error".to_string(), vec![]),
            check: None,
            test: None,
            forced_version: false,
            patch_depth: PatchDepth::None,
            actual_version: None,
            expected_version: None,
            original_requirement: None,
            all_crate_versions: vec![],
            blocking_crates: vec![],
        };
        assert!(!result.is_success());
        assert_eq!(result.first_failure(), Some(CompileStep::Fetch));
    }
}
