//! Auto-retry logic for multi-version conflict resolution.
//!
//! This module handles the detection and resolution of "multiple versions of crate X"
//! errors that occur when Cargo cannot unify dependencies.
//!
//! # How Auto-Retry Works
//!
//! When `cargo fetch` fails with a "multiple versions" error:
//!
//! 1. **Detect**: Parse error output for "multiple different versions of crate"
//! 2. **Retry**: Apply `[patch.crates-io]` to unify all versions
//! 3. **Report**: If still failing, identify which transitive deps are blocking
//!
//! This allows forced version testing to work even when the dependency graph
//! would normally conflict.

use crate::error_extract;
use log::debug;

/// Result of analyzing a multi-version conflict.
#[derive(Debug, Clone)]
pub struct ConflictAnalysis {
    /// Whether a multi-version conflict was detected
    pub has_conflict: bool,

    /// The crate(s) with conflicting versions
    pub conflicting_crates: Vec<String>,

    /// Crates that are blocking resolution (transitive deps)
    pub blocking_crates: Vec<String>,
}

impl ConflictAnalysis {
    /// Create an analysis indicating no conflict.
    pub fn no_conflict() -> Self {
        Self { has_conflict: false, conflicting_crates: vec![], blocking_crates: vec![] }
    }

    /// Create an analysis for a detected conflict.
    pub fn with_conflict(crate_name: &str, blocking: Vec<String>) -> Self {
        Self { has_conflict: true, conflicting_crates: vec![crate_name.to_string()], blocking_crates: blocking }
    }
}

/// Check if the output indicates a "multiple versions of crate" error.
///
/// This function examines both stdout and stderr for the error message.
pub fn detect_multi_version_conflict(stdout: &str, stderr: &str) -> bool {
    error_extract::has_multiple_version_conflict(stdout) || error_extract::has_multiple_version_conflict(stderr)
}

/// Analyze a multi-version conflict to identify blocking crates.
///
/// Returns a `ConflictAnalysis` with details about the conflict.
pub fn analyze_conflict(stdout: &str, stderr: &str, base_crate: &str) -> ConflictAnalysis {
    // Combine outputs for analysis
    let combined = format!("{}\n{}", stdout, stderr);

    if !error_extract::has_multiple_version_conflict(&combined) {
        return ConflictAnalysis::no_conflict();
    }

    // Extract blocking crates from the error
    let blocking = error_extract::extract_crates_needing_patch(&combined, base_crate);

    debug!("Multi-version conflict detected for {} - blocking crates: {:?}", base_crate, blocking);

    ConflictAnalysis::with_conflict(base_crate, blocking)
}

/// Determine if we should retry with `[patch.crates-io]`.
///
/// Returns true if:
/// 1. We detected a multi-version conflict
/// 2. We haven't already applied patch
pub fn should_retry_with_patch(analysis: &ConflictAnalysis, already_patched: bool) -> bool {
    analysis.has_conflict && !already_patched
}

/// Format advice message about blocking crates for deep patch cases.
///
/// This is shown when `[patch.crates-io]` alone isn't enough.
pub fn format_blocking_crates_advice(blocking_crates: &[String], base_crate: &str) -> String {
    if blocking_crates.is_empty() {
        return String::new();
    }

    let mut advice = format!("\n💡 These crates are pulling in different versions of {}:\n", base_crate);

    for crate_name in blocking_crates {
        advice.push_str(&format!("   - {}\n", crate_name));
    }

    advice.push_str(&format!(
        "\nTo test despite this, these crates may need to be patched\n\
         to use a compatible version of {}.\n",
        base_crate
    ));

    advice
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_no_conflict() {
        assert!(!detect_multi_version_conflict("success", ""));
    }

    #[test]
    fn test_detect_conflict_in_stderr() {
        let stderr = "error: there are multiple different versions of crate `rgb` in the dependency graph";
        assert!(detect_multi_version_conflict("", stderr));
    }

    #[test]
    fn test_detect_two_versions_variant() {
        let stderr = "error: there are two different versions of crate `rgb` in the dependency graph";
        assert!(detect_multi_version_conflict("", stderr));
    }

    #[test]
    fn test_analyze_no_conflict() {
        let analysis = analyze_conflict("ok", "", "rgb");
        assert!(!analysis.has_conflict);
    }

    #[test]
    fn test_analyze_conflict() {
        let stderr = "error: there are multiple different versions of crate `rgb` in the dependency graph";
        let analysis = analyze_conflict("", stderr, "rgb");
        assert!(analysis.has_conflict);
        assert!(analysis.conflicting_crates.contains(&"rgb".to_string()));
    }

    #[test]
    fn test_should_retry() {
        let analysis = ConflictAnalysis::with_conflict("rgb", vec![]);
        assert!(should_retry_with_patch(&analysis, false));
        assert!(!should_retry_with_patch(&analysis, true));
    }

    #[test]
    fn test_format_advice_empty() {
        assert!(format_blocking_crates_advice(&[], "rgb").is_empty());
    }

    #[test]
    fn test_format_advice_with_crates() {
        let advice = format_blocking_crates_advice(&["ravif".to_string()], "rgb");
        assert!(advice.contains("ravif"));
        assert!(advice.contains("rgb"));
    }
}
