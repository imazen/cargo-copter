//! Report type definitions for the rendering model.
//!
//! This module defines the type-safe rendering model used to convert
//! test results into displayable formats.

use crate::compile::PatchDepth;
use crate::types::{CommandType, OfferedRow, VersionSource};
use term::color::Color;

/// Status icon for the Offered column.
///
/// Shows whether the test passed, failed, or was skipped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatusIcon {
    /// Test passed: ✓
    Passed,
    /// Test failed: ✗
    Failed,
    /// Version wasn't used (cargo chose different version): ⊘
    Skipped,
}

impl StatusIcon {
    /// Get the unicode symbol for this status.
    pub fn as_str(&self) -> &'static str {
        match self {
            StatusIcon::Passed => "✓",
            StatusIcon::Failed => "✗",
            StatusIcon::Skipped => "⊘",
        }
    }
}

/// Resolution marker showing how cargo resolved the version.
///
/// This indicates whether the offered version was used exactly,
/// upgraded, or forced.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Resolution {
    /// Exact match: = (cargo resolved to exact offered version)
    Exact,
    /// Upgraded: ↑ (cargo upgraded within semver range)
    Upgraded,
    /// Mismatch: ≠ (forced or semver incompatible)
    Mismatch,
}

impl Resolution {
    /// Get the unicode symbol for this resolution.
    pub fn as_str(&self) -> &'static str {
        match self {
            Resolution::Exact => "=",
            Resolution::Upgraded => "↑",
            Resolution::Mismatch => "≠",
        }
    }
}

/// Content of the "Offered" cell - type-safe rendering model.
///
/// This enum represents what to display in the Offered column:
/// - Baseline test (no version override)
/// - Tested version with status, resolution, and markers
#[derive(Debug, Clone, PartialEq)]
pub enum OfferedCell {
    /// Baseline test: "- baseline"
    Baseline,

    /// Tested version with status
    Tested {
        /// Pass/fail/skip icon
        icon: StatusIcon,
        /// How cargo resolved the version
        resolution: Resolution,
        /// Version string (e.g., "0.8.91")
        version: String,
        /// Whether the version was forced
        forced: bool,
        /// Patching depth marker (!, !!, !!!)
        patch_depth: PatchDepth,
    },
}

impl OfferedCell {
    /// Convert OfferedRow to OfferedCell (business logic → rendering model).
    pub fn from_offered_row(row: &OfferedRow) -> Self {
        if row.offered.is_none() {
            return OfferedCell::Baseline;
        }

        let offered = row.offered.as_ref().unwrap();
        let overall_passed = row.test.commands.iter().all(|cmd| cmd.result.passed);

        // Check if non-forced version wasn't actually used (cargo resolved to something else)
        let not_used = !offered.forced && !row.primary.used_offered_version;

        // Determine status icon
        let icon = if not_used {
            StatusIcon::Skipped // Version wasn't used (cargo chose different version)
        } else {
            match (row.baseline_passed, overall_passed) {
                (Some(true), true) => StatusIcon::Passed,  // PASSED
                (Some(true), false) => StatusIcon::Failed, // REGRESSED
                (Some(false), _) => StatusIcon::Failed,    // BROKEN (baseline failed)
                (None, true) => StatusIcon::Passed,        // PASSED (no baseline)
                (None, false) => StatusIcon::Failed,       // FAILED (no baseline)
            }
        };

        // Determine resolution marker
        let resolution = if offered.forced {
            Resolution::Mismatch // Forced versions always show ≠
        } else if row.primary.used_offered_version {
            Resolution::Exact // Cargo chose exactly what we offered
        } else {
            Resolution::Upgraded // Cargo upgraded to something else
        };

        OfferedCell::Tested {
            icon,
            resolution,
            version: offered.version.clone(),
            forced: offered.forced,
            patch_depth: offered.patch_depth,
        }
    }

    /// Format the cell content for display.
    pub fn format(&self) -> String {
        match self {
            OfferedCell::Baseline => "- baseline".to_string(),
            OfferedCell::Tested { icon, resolution, version, patch_depth, .. } => {
                // Use the patch_depth marker instead of simple "→!" suffix
                // Markers: ! (force), !! (patch), !!! (deep patch)
                let marker = patch_depth.marker();
                let mut result = format!("{} {}{}", icon.as_str(), resolution.as_str(), version);
                if !marker.is_empty() {
                    result.push('→');
                    result.push_str(marker);
                }
                result
            }
        }
    }
}

/// Formatted row data ready for display.
///
/// This struct contains all the pre-formatted strings needed to
/// render a single row in the table.
pub struct FormattedRow {
    /// Offered column content
    pub offered: String,
    /// Spec column content (version requirement)
    pub spec: String,
    /// Resolved column content (actual version used)
    pub resolved: String,
    /// Dependent column content (crate name and version)
    pub dependent: String,
    /// Result column content (pass/fail status)
    pub result: String,
    /// Time column content
    pub time: String,
    /// Color for the row
    pub color: Color,
    /// Error details (if test failed)
    pub error_details: Vec<String>,
    /// Multi-version rows (for transitive deps)
    pub multi_version_rows: Vec<(String, String, String)>,
}

/// Test summary statistics.
///
/// Aggregated counts of test results across all tested versions.
pub struct TestSummary {
    /// Number of tests that passed
    pub passed: usize,
    /// Number of tests that regressed (baseline passed, offered failed)
    pub regressed: usize,
    /// Number of tests that were broken (baseline failed)
    pub broken: usize,
    /// Total number of tests (passed + regressed + broken)
    pub total: usize,
}

/// Buffered results for one dependent (all versions tested).
///
/// Used in simple output mode to collect all results for a dependent
/// before printing them.
#[derive(Default)]
pub struct DependentResults {
    /// Name of the dependent crate
    pub dependent_name: String,
    /// Version of the dependent crate
    pub dependent_version: String,
    /// Baseline test result (if any)
    pub baseline: Option<OfferedRow>,
    /// All offered version test results
    pub offered_versions: Vec<OfferedRow>,
}

/// Format a row from OfferedRow to FormattedRow.
///
/// # Arguments
/// * `row` - The OfferedRow to format
/// * `max_error_lines` - Maximum error lines to include (0 = unlimited)
pub fn format_offered_row(row: &OfferedRow, max_error_lines: usize) -> FormattedRow {
    // Format Offered column using type-safe OfferedCell
    let offered_cell = OfferedCell::from_offered_row(row);
    let offered_str = offered_cell.format();

    // Format Spec column
    let spec_str = if let Some(ref offered) = row.offered {
        if offered.forced { format!("→ ={}", offered.version) } else { row.primary.spec.clone() }
    } else {
        row.primary.spec.clone()
    };

    // Format Resolved column
    let source_icon = match row.primary.resolved_source {
        VersionSource::CratesIo => "📦",
        VersionSource::Local => "📁",
        VersionSource::Git => "🔀",
    };
    let resolved_str = format!("{} {}", row.primary.resolved_version, source_icon);

    // Format Dependent column
    let dependent_str = format!("{} {}", row.primary.dependent_name, row.primary.dependent_version);

    // Determine which step failed (if any)
    let overall_passed = row.test.commands.iter().all(|cmd| cmd.result.passed);
    let failed_step = row.test.commands.iter().find(|cmd| !cmd.result.passed).map(|cmd| match cmd.command {
        CommandType::Fetch => "fetch failed",
        CommandType::Check => "build failed",
        CommandType::Test => "test failed",
    });

    // Check if this version wasn't actually used (non-forced and cargo chose different version)
    let not_used =
        if let Some(ref offered) = row.offered { !offered.forced && !row.primary.used_offered_version } else { false };

    // Detect if this is a baseline row (no offered version)
    let is_baseline = row.offered.is_none();

    let result_status = if not_used {
        "not used".to_string()
    } else if is_baseline {
        // Baseline row: if it failed, show "build broken" or "test broken"
        if overall_passed {
            "passed".to_string()
        } else if let Some(step) = failed_step {
            // Replace "failed" with "broken" for baseline failures
            step.replace("failed", "broken")
        } else {
            "broken".to_string()
        }
    } else {
        // Offered row: compare against baseline
        match (row.baseline_passed, overall_passed, failed_step) {
            (Some(true), true, _) => "passed".to_string(),
            (Some(true), false, Some(step)) => step.to_string(),
            (Some(true), false, None) => "regressed".to_string(),
            // For offered rows when baseline was broken
            (Some(false), _, Some(step)) => step.replace("failed", "broken"),
            (Some(false), _, None) => "broken".to_string(),
            (None, true, _) => "passed".to_string(),
            (None, false, Some(step)) => step.to_string(),
            (None, false, None) => "failed".to_string(),
        }
    };

    // Format ICT marks
    let mut ict_marks = String::new();
    for cmd in &row.test.commands {
        match cmd.command {
            CommandType::Fetch => ict_marks.push(if cmd.result.passed { '✓' } else { '✗' }),
            CommandType::Check => ict_marks.push(if cmd.result.passed { '✓' } else { '✗' }),
            CommandType::Test => ict_marks.push(if cmd.result.passed { '✓' } else { '✗' }),
        }
    }
    // Pad to 3 chars with '-' for skipped steps
    while ict_marks.len() < 3 {
        ict_marks.push('-');
    }

    let result_str = format!("{} {}", result_status, ict_marks);

    // Calculate total time
    let total_time: f64 = row.test.commands.iter().map(|cmd| cmd.result.duration).sum();
    let time_str = format!("{:.1}s", total_time);

    // Determine color
    let color = if not_used {
        term::color::YELLOW // Brown (YELLOW/33) for skipped (not used) versions
    } else if is_baseline && !overall_passed {
        term::color::BRIGHT_YELLOW // Bright yellow (93) for failed baseline rows
    } else {
        match (row.baseline_passed, overall_passed) {
            (Some(true), true) => term::color::BRIGHT_GREEN,
            (Some(true), false) => term::color::BRIGHT_RED,
            (Some(false), _) => term::color::BRIGHT_YELLOW, // Bright yellow for broken
            (None, true) => term::color::BRIGHT_GREEN,
            (None, false) => term::color::BRIGHT_RED,
        }
    };

    // Extract error details
    let mut error_details = Vec::new();
    for cmd in &row.test.commands {
        if !cmd.result.passed {
            let cmd_name = match cmd.command {
                CommandType::Fetch => "fetch",
                CommandType::Check => "check",
                CommandType::Test => "test",
            };
            for failure in &cmd.result.failures {
                error_details.push(format!("cargo {} failed on {}", cmd_name, failure.crate_name));
                // Add error message if not empty (full error - truncate based on max_error_lines)
                if !failure.error_message.is_empty() {
                    // Split into lines and display each with bullet
                    let lines: Vec<&str> = failure.error_message.lines().collect();
                    let display_lines = if max_error_lines == 0 {
                        &lines[..] // Show all lines
                    } else {
                        &lines[..lines.len().min(max_error_lines)]
                    };

                    for line in display_lines {
                        if !line.trim().is_empty() {
                            error_details.push(format!("  {}", line));
                        }
                    }

                    // Add truncation indicator if we cut lines
                    if max_error_lines > 0 && lines.len() > max_error_lines {
                        error_details.push(format!("  ... ({} more lines)", lines.len() - max_error_lines));
                    }
                }
            }
        }
    }

    // Format transitive dependency rows (multi-version rows)
    let mut multi_version_rows = Vec::new();
    for transitive in &row.transitive {
        let source_icon = match transitive.dependency.resolved_source {
            VersionSource::CratesIo => "📦",
            VersionSource::Local => "📁",
            VersionSource::Git => "🔀",
        };
        multi_version_rows.push((
            transitive.dependency.spec.clone(),
            format!("{} {}", transitive.dependency.resolved_version, source_icon),
            format!("{} {}", transitive.dependency.dependent_name, transitive.dependency.dependent_version),
        ));
    }

    FormattedRow {
        offered: offered_str,
        spec: spec_str,
        resolved: resolved_str,
        dependent: dependent_str,
        result: result_str,
        time: time_str,
        color,
        error_details,
        multi_version_rows,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_status_icon_as_str() {
        assert_eq!(StatusIcon::Passed.as_str(), "✓");
        assert_eq!(StatusIcon::Failed.as_str(), "✗");
        assert_eq!(StatusIcon::Skipped.as_str(), "⊘");
    }

    #[test]
    fn test_resolution_as_str() {
        assert_eq!(Resolution::Exact.as_str(), "=");
        assert_eq!(Resolution::Upgraded.as_str(), "↑");
        assert_eq!(Resolution::Mismatch.as_str(), "≠");
    }

    #[test]
    fn test_offered_cell_baseline() {
        let cell = OfferedCell::Baseline;
        assert_eq!(cell.format(), "- baseline");
    }

    #[test]
    fn test_offered_cell_tested() {
        let cell = OfferedCell::Tested {
            icon: StatusIcon::Passed,
            resolution: Resolution::Exact,
            version: "0.8.91".to_string(),
            forced: false,
            patch_depth: PatchDepth::None,
        };
        assert_eq!(cell.format(), "✓ =0.8.91");
    }

    #[test]
    fn test_offered_cell_forced() {
        let cell = OfferedCell::Tested {
            icon: StatusIcon::Failed,
            resolution: Resolution::Mismatch,
            version: "0.8.91".to_string(),
            forced: true,
            patch_depth: PatchDepth::Force,
        };
        assert_eq!(cell.format(), "✗ ≠0.8.91→!");
    }

    #[test]
    fn test_offered_cell_patched() {
        let cell = OfferedCell::Tested {
            icon: StatusIcon::Passed,
            resolution: Resolution::Mismatch,
            version: "0.8.91".to_string(),
            forced: true,
            patch_depth: PatchDepth::Patch,
        };
        assert_eq!(cell.format(), "✓ ≠0.8.91→!!");
    }
}
