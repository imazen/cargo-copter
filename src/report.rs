/// Report generation module - Data transformations and business logic
///
/// This module handles:
/// - Converting OfferedRow to FormattedRow (business logic)
/// - Calculating statistics and summaries
/// - Generating comparison tables
/// - Error signature extraction for deduplication
///
/// Console rendering is handled by the console_format module.
use crate::console_format::{self, ComparisonStats};
use crate::types::{CommandType, OfferedRow, TestResult, VersionSource};
use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};
use term::color::Color;

//
// Rendering Model Types
//

/// Status icon for the Offered column
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatusIcon {
    Passed,  // ✓
    Failed,  // ✗
    Skipped, // ⊘ (not used - version didn't match what cargo resolved)
}

impl StatusIcon {
    pub fn as_str(&self) -> &'static str {
        match self {
            StatusIcon::Passed => "✓",
            StatusIcon::Failed => "✗",
            StatusIcon::Skipped => "⊘",
        }
    }
}

/// Resolution marker showing how cargo resolved the version
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Resolution {
    Exact,    // = (cargo resolved to exact offered version)
    Upgraded, // ↑ (cargo upgraded within semver range)
    Mismatch, // ≠ (forced or semver incompatible)
}

impl Resolution {
    pub fn as_str(&self) -> &'static str {
        match self {
            Resolution::Exact => "=",
            Resolution::Upgraded => "↑",
            Resolution::Mismatch => "≠",
        }
    }
}

/// Content of the "Offered" cell - type-safe rendering model
#[derive(Debug, Clone, PartialEq)]
pub enum OfferedCell {
    /// Baseline test: "- baseline"
    Baseline,

    /// Tested version with status
    Tested {
        icon: StatusIcon,
        resolution: Resolution,
        version: String,
        forced: bool, // adds →! suffix if true
    },
}

impl OfferedCell {
    /// Convert OfferedRow to OfferedCell (business logic → rendering model)
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

        OfferedCell::Tested { icon, resolution, version: offered.version.clone(), forced: offered.forced }
    }

    /// Format the cell content for display
    pub fn format(&self) -> String {
        match self {
            OfferedCell::Baseline => "- baseline".to_string(),
            OfferedCell::Tested { icon, resolution, version, forced } => {
                let mut result = format!("{} {}{}", icon.as_str(), resolution.as_str(), version);
                if *forced {
                    result.push_str("→!");
                }
                result
            }
        }
    }
}

//
// Public API: Delegate to console_format module
//

/// Initialize table widths based on versions being tested
pub fn init_table_widths(versions: &[String], display_version: &str, force_versions: bool) {
    console_format::init_table_widths(versions, display_version, force_versions);
}

/// Print table header
pub fn print_table_header(
    crate_name: &str,
    display_version: &str,
    total_deps: usize,
    test_plan: Option<&str>,
    this_path: Option<&str>,
) {
    console_format::print_table_header(crate_name, display_version, total_deps, test_plan, this_path);
}

/// Format table header as a string
pub fn format_table_header(
    crate_name: &str,
    display_version: &str,
    total_deps: usize,
    test_plan: Option<&str>,
    this_path: Option<&str>,
) -> String {
    console_format::format_table_header(crate_name, display_version, total_deps, test_plan, this_path)
}

/// Print separator line between dependents
pub fn print_separator_line() {
    console_format::print_separator_line();
}

/// Format table footer as a string
pub fn format_table_footer() -> String {
    console_format::format_table_footer()
}

/// Print table footer
pub fn print_table_footer() {
    console_format::print_table_footer();
}

/// Normalize file paths by removing hex suffixes (e.g., file-abc123 -> file)
/// Handles both Unix (/) and Windows (\) paths
fn normalize_path_hex_codes(text: &str) -> String {
    let mut result = String::with_capacity(text.len());
    let mut i = 0;
    let chars: Vec<char> = text.chars().collect();

    while i < chars.len() {
        result.push(chars[i]);

        // Check if we just pushed a path separator
        if chars[i] == '/' || chars[i] == '\\' {
            // Look ahead to find the next path component
            let mut j = i + 1;
            let mut component = String::new();

            // Collect characters until next separator, space, or end
            while j < chars.len() && chars[j] != '/' && chars[j] != '\\' && !chars[j].is_whitespace() {
                component.push(chars[j]);
                j += 1;
            }

            // Check if component ends with -[hex] pattern
            if let Some(dash_pos) = component.rfind('-') {
                let potential_hex = &component[dash_pos + 1..];
                // Check if it's all hex digits and at least 8 chars
                if potential_hex.len() >= 8 && potential_hex.chars().all(|c| c.is_ascii_hexdigit()) {
                    // Remove the -[hex] suffix
                    result.push_str(&component[..dash_pos]);
                    i = j;
                    continue;
                }
            }

            // No hex pattern, add component as-is
            result.push_str(&component);
            i = j;
            continue;
        }

        i += 1;
    }

    result
}

/// Extract error signature for comparison - normalizes line numbers and sorts errors
pub fn error_signature(text: &str) -> String {
    use std::collections::BTreeSet;

    // Extract error codes with their messages
    let mut errors = BTreeSet::new();

    for line in text.lines() {
        // Match error lines like "error[E0432]: ..." and normalize
        if let Some(start) = line.find("error[")
            && let Some(end) = line[start..].find("]:")
        {
            let code = &line[start..start + end + 2];
            let message = line[start + end + 2..].trim();
            // Remove specific line references to focus on error type
            let normalized = message.split("-->").next().unwrap_or(message).trim();
            errors.insert(format!("{} {}", code, normalized));
        }
    }

    // Sort and join all errors
    errors.into_iter().collect::<Vec<_>>().join("\n")
}

/// Extract error text from an OfferedRow for deduplication
pub fn extract_error_text(row: &OfferedRow) -> Option<String> {
    // Extract errors from ALL rows (including baseline) for comparison
    // Skipped rows should match baseline since they use the same version

    // Extract FULL error (0 = unlimited) for comparison purposes
    let formatted = format_offered_row(row, 0);
    if formatted.error_details.is_empty() {
        None
    } else {
        // Use error signature for robust comparison across non-deterministic error ordering
        let error_text = formatted.error_details.join("\n");
        Some(error_signature(&error_text))
    }
}

/// Print an OfferedRow using the standard table format
pub fn print_offered_row(row: &OfferedRow, is_last_in_group: bool, prev_error: Option<&str>, max_error_lines: usize) {
    // Convert OfferedRow to formatted data
    let mut formatted = format_offered_row(row, max_error_lines);

    // Don't show "same failure" on baseline rows (they're the reference point)
    let is_baseline = row.offered.is_none();

    // When errors match exactly, show "same failure" instead of repeating the error
    // Applies to both regression and broken scenarios, but NOT for baseline rows
    if !is_baseline
        && let Some(prev) = prev_error
        && !formatted.error_details.is_empty()
    {
        // Extract FULL error for comparison (not truncated)
        // prev_error is full, so we need full current error too
        let full_formatted = format_offered_row(row, 0);
        let current_error = full_formatted.error_details.join("\n");
        // Use error signature for robust comparison
        let current_signature = error_signature(&current_error);
        if current_signature == prev {
            // Clear error details and update result to show "same failure"
            // Keep ICT marks and time
            formatted.error_details.clear();
            // Replace the failure type with "same failure", keeping ICT marks
            // For broken scenarios, also replace "test broken" -> "same failure"
            formatted.result = formatted
                .result
                .replace("test failed", "same failure")
                .replace("build failed", "same failure")
                .replace("fetch failed", "same failure")
                .replace("test broken", "same failure")
                .replace("build broken", "same failure");
        }
    }

    // Format result column
    let result_display = if formatted.time.is_empty() {
        // "still failing" case - no ICT marks or time
        format!("{:>18}", formatted.result)
    } else {
        format!("{:>12} {:>5}", formatted.result, formatted.time)
    };

    // Print main row with color (delegate to console_format)
    console_format::print_main_row(
        [&formatted.offered, &formatted.spec, &formatted.resolved, &formatted.dependent, &result_display],
        formatted.color,
    );

    // Print error box if present (delegate to console_format)
    if !formatted.error_details.is_empty() {
        console_format::print_error_box_top();

        for error_line in &formatted.error_details {
            console_format::print_error_box_line(error_line);
        }

        if !is_last_in_group {
            console_format::print_error_box_bottom();
        }
    }

    // Print multi-version dependency rows (delegate to console_format)
    console_format::print_multi_version_rows(&formatted.multi_version_rows);
}

//
// OfferedRow to renderable format conversion
//

/// Formatted row data ready for display
pub struct FormattedRow {
    pub offered: String,
    pub spec: String,
    pub resolved: String,
    pub dependent: String,
    pub result: String,
    pub time: String,
    pub color: Color,
    pub error_details: Vec<String>,
    pub multi_version_rows: Vec<(String, String, String)>,
}

/// Convert OfferedRow to renderable row data
fn format_offered_row(row: &OfferedRow, max_error_lines: usize) -> FormattedRow {
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
            (Some(false), _) => term::color::BRIGHT_YELLOW, // Bright yellow for broken (baseline was broken)
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

//
// Summary and statistics
//

pub struct TestSummary {
    pub passed: usize,
    pub regressed: usize,
    pub broken: usize,
    pub total: usize,
}

/// Calculate summary statistics from OfferedRows
pub fn summarize_offered_rows(rows: &[OfferedRow]) -> TestSummary {
    let mut passed = 0;
    let mut regressed = 0;
    let mut broken = 0;

    for row in rows {
        // Only count non-baseline rows
        if row.offered.is_some() {
            let overall_passed = row.test.commands.iter().all(|cmd| cmd.result.passed);

            match (row.baseline_passed, overall_passed) {
                (Some(true), true) => passed += 1,     // PASSED
                (Some(true), false) => regressed += 1, // REGRESSED
                (Some(false), _) => broken += 1,       // BROKEN
                (None, true) => passed += 1,           // PASSED (no baseline)
                (None, false) => broken += 1,          // FAILED (no baseline)
            }
        }
    }

    TestSummary { passed, regressed, broken, total: passed + regressed + broken }
}

/// Generate comparison table statistics
pub fn generate_comparison_table(rows: &[OfferedRow]) -> Vec<ComparisonStats> {
    use std::collections::{HashMap, HashSet};

    // First, collect baseline stats
    let baseline_rows: Vec<&OfferedRow> = rows.iter().filter(|r| r.offered.is_none()).collect();

    let mut baseline_stats = ComparisonStats {
        version_label: "Default".to_string(),
        total_tested: 0,
        already_broken: Some(0),
        passed_fetch: 0,
        passed_check: 0,
        passed_test: 0,
        fully_passing: 0,
        regressions: vec![],
    };

    let mut seen_baseline: HashSet<String> = HashSet::new();
    for row in &baseline_rows {
        let dep_name = &row.primary.dependent_name;
        if !seen_baseline.insert(dep_name.clone()) {
            continue;
        }

        baseline_stats.total_tested += 1;

        let passed_fetch =
            row.test.commands.iter().filter(|cmd| cmd.command == CommandType::Fetch).all(|cmd| cmd.result.passed);

        let passed_check = row
            .test
            .commands
            .iter()
            .filter(|cmd| cmd.command == CommandType::Check || cmd.command == CommandType::Fetch)
            .all(|cmd| cmd.result.passed);

        let passed_test = row.test.commands.iter().all(|cmd| cmd.result.passed);

        // Only count as "already broken" if TEST failed (not build/check failures)
        if passed_check && !passed_test {
            baseline_stats.already_broken = Some(baseline_stats.already_broken.unwrap() + 1);
        }

        if passed_fetch {
            baseline_stats.passed_fetch += 1;
        }
        if passed_check {
            baseline_stats.passed_check += 1;
        }
        if passed_test {
            baseline_stats.passed_test += 1;
            baseline_stats.fully_passing += 1;
        }
    }

    let mut all_stats = vec![baseline_stats];

    // Group offered rows by version
    let mut by_version: HashMap<String, Vec<&OfferedRow>> = HashMap::new();
    for row in rows {
        if let Some(ref offered) = row.offered {
            by_version.entry(offered.version.clone()).or_default().push(row);
        }
    }

    // Sort versions (simple string sort for now)
    let mut versions: Vec<String> = by_version.keys().cloned().collect();
    versions.sort();

    for version in versions {
        let version_rows = &by_version[&version];

        let mut stats = ComparisonStats {
            version_label: version.clone(),
            total_tested: 0,
            already_broken: None, // Don't show for offered versions
            passed_fetch: 0,
            passed_check: 0,
            passed_test: 0,
            fully_passing: 0,
            regressions: vec![],
        };

        let mut seen: HashSet<String> = HashSet::new();
        for row in version_rows {
            let dep_name = &row.primary.dependent_name;
            if !seen.insert(dep_name.clone()) {
                continue;
            }

            stats.total_tested += 1;

            let passed_fetch =
                row.test.commands.iter().filter(|cmd| cmd.command == CommandType::Fetch).all(|cmd| cmd.result.passed);

            let passed_check = row
                .test
                .commands
                .iter()
                .filter(|cmd| cmd.command == CommandType::Check || cmd.command == CommandType::Fetch)
                .all(|cmd| cmd.result.passed);

            let passed_test = row.test.commands.iter().all(|cmd| cmd.result.passed);

            // Only count if not already broken in baseline
            let baseline_row = baseline_rows.iter().find(|br| br.primary.dependent_name == *dep_name);

            let baseline_passed_check = baseline_row
                .map(|br| {
                    br.test
                        .commands
                        .iter()
                        .filter(|cmd| cmd.command == CommandType::Check || cmd.command == CommandType::Fetch)
                        .all(|cmd| cmd.result.passed)
                })
                .unwrap_or(false);

            let baseline_passed_test =
                baseline_row.map(|br| br.test.commands.iter().all(|cmd| cmd.result.passed)).unwrap_or(false);

            if baseline_passed_check {
                // Only count working dependents
                if passed_fetch {
                    stats.passed_fetch += 1;
                }
                if passed_check {
                    stats.passed_check += 1;
                }
                if passed_test {
                    stats.passed_test += 1;
                    stats.fully_passing += 1;
                }

                // Track regressions: baseline passed but offered failed
                if baseline_passed_test && !passed_test {
                    let baseline_version = baseline_row.map(|br| br.primary.resolved_version.as_str()).unwrap_or("?");
                    stats.regressions.push(format!("{} ({})", dep_name, baseline_version));
                }
            }
        }

        all_stats.push(stats);
    }

    all_stats
}

/// Print comparison table (delegate to console_format)
pub fn print_comparison_table(stats_list: &[ComparisonStats]) {
    console_format::print_comparison_table(stats_list);
}

//
// JSON Export
//

/// Export test results as JSON
pub fn export_json_report(
    rows: &[OfferedRow],
    output_path: &PathBuf,
    crate_name: &str,
    display_version: &str,
    total_deps: usize,
) -> std::io::Result<()> {
    use serde_json::json;

    let summary = summarize_offered_rows(rows);
    let comparison_stats = generate_comparison_table(rows);

    let report = json!({
        "crate_name": crate_name,
        "crate_version": display_version,
        "total_dependents": total_deps,
        "summary": {
            "passed": summary.passed,
            "regressed": summary.regressed,
            "broken": summary.broken,
            "total": summary.total,
        },
        "comparison_stats": comparison_stats,
        "test_results": rows,
    });

    let file = File::create(output_path)?;
    serde_json::to_writer_pretty(file, &report)?;

    Ok(())
}

//
// Temporary compatibility stubs for old API (TO BE REMOVED)
//

/// Generate markdown report with console table in code block
pub fn export_markdown_table_report(
    rows: &[OfferedRow],
    output_path: &PathBuf,
    crate_name: &str,
    display_version: &str,
    total_deps: usize,
    test_plan: Option<&str>,
    this_path: Option<&str>,
) -> std::io::Result<()> {
    let mut file = File::create(output_path)?;
    let summary = summarize_offered_rows(rows);

    // Write markdown header
    writeln!(file, "# Cargo Copter Test Report\n")?;
    writeln!(file, "**Crate**: {} ({})", crate_name, display_version)?;
    writeln!(file, "**Dependents Tested**: {}\n", total_deps)?;

    // Write summary
    writeln!(file, "## Summary\n")?;
    writeln!(file, "- ✓ Passed: {}", summary.passed)?;
    writeln!(file, "- ✗ Regressed: {}", summary.regressed)?;
    writeln!(file, "- ⚠ Broken: {}", summary.broken)?;
    writeln!(file, "- **Total**: {}\n", summary.total)?;

    // Write console table in code block
    writeln!(file, "## Test Results\n")?;
    writeln!(file, "```")?;

    // Write table header (with test plan to match console output exactly)
    write!(file, "{}", format_table_header(crate_name, display_version, total_deps, test_plan, this_path))?;

    // Write all rows
    for row in rows.iter() {
        // Determine if this is the last row in its group
        // For simplicity, assume each row is its own group (no separators in markdown)
        let is_last_in_group = true;

        // Format the row (we need a string-returning version of print_offered_row)
        write!(file, "{}", format_offered_row_string(row, is_last_in_group))?;
    }

    // Write table footer
    write!(file, "{}", format_table_footer())?;

    // Generate and write comparison table using TableWriter
    let comparison_stats = generate_comparison_table(rows);
    let mut table_writer = console_format::TableWriter::new(&mut file, false); // No colors for markdown
    table_writer.write_comparison_table(&comparison_stats)?;

    writeln!(file, "```\n")?;

    Ok(())
}

/// Format an OfferedRow as a string (similar to print_offered_row but returns String)
fn format_offered_row_string(row: &OfferedRow, is_last_in_group: bool) -> String {
    // Use unlimited error lines for markdown export
    let formatted = format_offered_row(row, 0);
    let w = console_format::get_widths();

    let mut output = String::new();

    // Main row
    let offered_display = console_format::truncate_with_padding(&formatted.offered, w.offered - 2);
    let spec_display = console_format::truncate_from_start_with_padding(&formatted.spec, w.spec - 2);
    let resolved_display = console_format::truncate_from_start_with_padding(&formatted.resolved, w.resolved - 2);
    let dependent_display = console_format::truncate_from_start_with_padding(&formatted.dependent, w.dependent - 2);
    let result_display = format!("{:>12} {:>5}", formatted.result, formatted.time);
    let result_display = console_format::truncate_with_padding(&result_display, w.result - 2);

    output.push_str(&format!(
        "│ {} │ {} │ {} │ {} │ {} │\n",
        offered_display, spec_display, resolved_display, dependent_display, result_display
    ));

    // Error details (if any)
    if !formatted.error_details.is_empty() {
        let error_text_width = w.total - 1 - w.offered - 1 - 1 - 1 - 1;
        let corner1_width = w.spec;
        let corner2_width = w.dependent;
        let padding_width = w.spec + w.resolved + w.dependent - corner1_width - corner2_width;

        output.push_str(&format!(
            "│{:w_offered$}├{:─<corner1$}┘{:padding$}└{:─<corner2$}┘{:w_result$}│\n",
            "",
            "",
            "",
            "",
            "",
            w_offered = w.offered,
            corner1 = corner1_width,
            padding = padding_width,
            corner2 = corner2_width,
            w_result = w.result
        ));

        for error_line in &formatted.error_details {
            let truncated = console_format::truncate_with_padding(error_line, error_text_width);
            output.push_str(&format!("│{:w_offered$}│ {} │\n", "", truncated, w_offered = w.offered));
        }

        if !is_last_in_group {
            output.push_str(&format!(
                "│{:w_offered$}├{:─<w_spec$}┬{:─<w_resolved$}┬{:─<w_dependent$}┬{:─<w_result$}┤\n",
                "",
                "",
                "",
                "",
                "",
                w_offered = w.offered,
                w_spec = w.spec,
                w_resolved = w.resolved,
                w_dependent = w.dependent,
                w_result = w.result
            ));
        }
    }

    // Multi-version rows (└─ for last row)
    if !formatted.multi_version_rows.is_empty() {
        let last_idx = formatted.multi_version_rows.len() - 1;
        for (i, (spec, resolved, dependent)) in formatted.multi_version_rows.iter().enumerate() {
            let prefix = if i == last_idx { "└─" } else { "├─" };
            let spec_display = format!("{} {}", prefix, spec);
            let spec_display = console_format::truncate_from_start_with_padding(&spec_display, w.spec - 2);
            let resolved_display = format!("{} {}", prefix, resolved);
            let resolved_display = console_format::truncate_from_start_with_padding(&resolved_display, w.resolved - 2);
            let dependent_display = format!("{} {}", prefix, dependent);
            let dependent_display =
                console_format::truncate_from_start_with_padding(&dependent_display, w.dependent - 2);

            output.push_str(&format!(
                "│{:width$}│ {} │ {} │ {} │{:w_result$}│\n",
                "",
                spec_display,
                resolved_display,
                dependent_display,
                "",
                width = w.offered,
                w_result = w.result
            ));
        }
    }

    output
}

/// Write raw cargo output to a failure log file
pub fn write_failure_log(report_dir: &Path, staging_dir: &Path, result: &TestResult) {
    let dependent_name = &result.dependent.name;
    let dependent_version = result.dependent.version.display();
    let base_version = result.base_version.version.display();

    // Create filename: dependent-version_base-version.txt
    let filename = format!("{}-{}_{}.txt", dependent_name, dependent_version, base_version);
    let log_path = report_dir.join(&filename);

    // Build the staging path for this dependent
    let dependent_staging_path = staging_dir.join(format!("{}-{}", dependent_name, dependent_version));

    let mut content = String::new();
    content.push_str(&format!(
        "# Failure Log: {} {} with base crate version {}\n",
        dependent_name, dependent_version, base_version
    ));
    content.push_str(&format!("# Generated: {}\n", chrono::Local::now().format("%Y-%m-%d %H:%M:%S")));
    content.push_str(&format!(
        "# Source: {}\n\n",
        dependent_staging_path.canonicalize().unwrap_or(dependent_staging_path).display()
    ));

    // Helper to write diagnostics or fall back to stderr
    fn write_step_output(content: &mut String, result: &crate::compile::CompileResult, step_name: &str) {
        content.push_str(&format!("=== {} ===\n", step_name));
        content.push_str(&format!("Status: FAILED ({:.1}s)\n\n", result.duration.as_secs_f64()));

        // Prefer parsed diagnostics (human-readable) over raw stderr
        if !result.diagnostics.is_empty() {
            for diag in &result.diagnostics {
                content.push_str(&diag.rendered);
                if !diag.rendered.ends_with('\n') {
                    content.push('\n');
                }
            }
        } else if !result.stderr.is_empty() {
            // Fall back to stderr if no diagnostics parsed
            content.push_str(&result.stderr);
            if !result.stderr.ends_with('\n') {
                content.push('\n');
            }
        }
        content.push('\n');
    }

    // Write fetch step output if it failed
    if !result.execution.fetch.success {
        write_step_output(&mut content, &result.execution.fetch, "FETCH (cargo fetch)");
    }

    // Write check step output if it failed
    if let Some(ref check) = result.execution.check
        && !check.success
    {
        write_step_output(&mut content, check, "CHECK (cargo check)");
    }

    // Write test step output if it failed
    if let Some(ref test) = result.execution.test
        && !test.success
    {
        write_step_output(&mut content, test, "TEST (cargo test)");
    }

    // Write to file
    match File::create(&log_path) {
        Ok(mut file) => {
            if let Err(e) = file.write_all(content.as_bytes()) {
                eprintln!("Warning: Failed to write failure log {}: {}", filename, e);
            }
        }
        Err(e) => {
            eprintln!("Warning: Failed to create failure log {}: {}", filename, e);
        }
    }
}

//
// Simple output format (AI-friendly, verbal)
//

/// Buffered results for one dependent (all versions tested)
#[derive(Default)]
pub struct DependentResults {
    pub dependent_name: String,
    pub dependent_version: String,
    pub baseline: Option<OfferedRow>,
    pub offered_versions: Vec<OfferedRow>,
}

/// Print simple header for test run with list of all dependents
pub fn print_simple_header(base_crate: &str, display_version: &str, dependents: &[String], base_versions: &[String]) {
    println!("Testing {}:{} against {} dependents", base_crate, display_version, dependents.len());
    println!();
    println!("Dependents: {}", dependents.join(", "));
    println!("Versions to test: {}", base_versions.join(", "));
    println!();
}

/// Collect results for a dependent and print when complete
pub fn print_simple_dependent_result(results: &DependentResults, base_crate: &str, report_dir: &Path) {
    let dep = format!("{}:{}", results.dependent_name, results.dependent_version);

    // Get baseline status
    let baseline_row = results.baseline.as_ref();
    let baseline_passed = baseline_row.map(|r| r.test_passed()).unwrap_or(false);
    let baseline_check_passed = baseline_row
        .map(|r| {
            r.test
                .commands
                .iter()
                .filter(|c| c.command == CommandType::Check || c.command == CommandType::Fetch)
                .all(|c| c.result.passed)
        })
        .unwrap_or(false);

    // Determine baseline test status (separate from build)
    let baseline_test_passed = baseline_row
        .map(|r| r.test.commands.iter().all(|c| c.result.passed))
        .unwrap_or(false);

    // Analyze all offered versions
    let mut build_regressions: Vec<(&OfferedRow, &'static str)> = Vec::new();
    let mut test_regressions: Vec<&OfferedRow> = Vec::new();
    let mut passed_versions: Vec<String> = Vec::new();
    let mut still_broken: Vec<String> = Vec::new();

    for row in &results.offered_versions {
        let version = row.offered.as_ref().map(|o| o.version.as_str()).unwrap_or("?");
        let forced = row.offered.as_ref().map(|o| o.forced).unwrap_or(false);
        let version_display = if forced {
            format!("{}:{} [forced]", base_crate, version)
        } else {
            format!("{}:{}", base_crate, version)
        };
        let this_passed = row.test_passed();

        if this_passed {
            passed_versions.push(version_display);
        } else {
            let failed_step = failed_step_name(row);

            if failed_step == "build" || failed_step == "fetch" {
                // BUILD/FETCH failure - this is a regression if baseline build passed
                if baseline_check_passed {
                    build_regressions.push((row, failed_step));
                } else {
                    still_broken.push(version_display);
                }
            } else {
                // TEST failure - only a regression if baseline tests passed
                if baseline_test_passed {
                    test_regressions.push(row);
                } else {
                    still_broken.push(version_display);
                }
            }
        }
    }

    // Output the results - prioritize build regressions
    if !build_regressions.is_empty() {
        for (row, step) in &build_regressions {
            let version = row.offered.as_ref().map(|o| o.version.as_str()).unwrap_or("?");
            let forced = row.offered.as_ref().map(|o| o.forced).unwrap_or(false);
            let forced_marker = if forced { " [forced]" } else { "" };

            let baseline_note = if baseline_test_passed {
                "baseline passed"
            } else {
                "baseline build passed, tests were already failing"
            };

            println!(
                "REGRESSION: {} with {}:{}{} - {} failed ({})",
                dep, base_crate, version, forced_marker, step, baseline_note
            );
            // Print first error line
            if let Some(error) = first_error_line(row) {
                println!("  {}", error);
            }
        }
    }

    // Test regressions (less critical than build regressions)
    if !test_regressions.is_empty() {
        for row in &test_regressions {
            let version = row.offered.as_ref().map(|o| o.version.as_str()).unwrap_or("?");
            let forced = row.offered.as_ref().map(|o| o.forced).unwrap_or(false);
            let forced_marker = if forced { " [forced]" } else { "" };

            println!(
                "REGRESSION: {} with {}:{}{} - tests failed (baseline tests passed)",
                dep, base_crate, version, forced_marker
            );
            // Print first error line
            if let Some(error) = first_error_line(row) {
                println!("  {}", error);
            }
        }
    }

    // Report passed versions
    if !passed_versions.is_empty() {
        println!("OK: {} - passed with {}", dep, passed_versions.join(", "));
    }

    // Report still broken (not regressions, baseline was already failing at same level)
    if !still_broken.is_empty() && build_regressions.is_empty() && test_regressions.is_empty() {
        // Only mention if no regressions to avoid noise
        println!("STILL BROKEN: {} with {} (same failure as baseline)", dep, still_broken.join(", "));
    }

    // If baseline failed and no offered versions
    if results.offered_versions.is_empty() && !baseline_passed {
        let step = baseline_row.map(failed_step_name).unwrap_or("unknown");
        println!("BASELINE FAILED: {} ({} failed)", dep, step);
    }
}

/// Get a human-readable description of the first failed step
fn failed_step_name(row: &OfferedRow) -> &'static str {
    for cmd in &row.test.commands {
        if !cmd.result.passed {
            return match cmd.command {
                CommandType::Fetch => "fetch",
                CommandType::Check => "build",
                CommandType::Test => "test suite",
            };
        }
    }
    "unknown"
}

/// Get the first error line from a failed row (for --simple output)
fn first_error_line(row: &OfferedRow) -> Option<String> {
    for cmd in &row.test.commands {
        if !cmd.result.passed {
            for failure in &cmd.result.failures {
                if !failure.error_message.is_empty() {
                    // Find the first line that starts with "error" (case-insensitive)
                    for line in failure.error_message.lines() {
                        let trimmed = line.trim();
                        if trimmed.starts_with("error") {
                            // Truncate long lines
                            let display = if trimmed.len() > 100 {
                                format!("{}...", &trimmed[..100])
                            } else {
                                trimmed.to_string()
                            };
                            return Some(display);
                        }
                    }
                    // Fallback: first non-empty line
                    if let Some(first) = failure.error_message.lines().find(|l| !l.trim().is_empty()) {
                        let trimmed = first.trim();
                        let display = if trimmed.len() > 100 {
                            format!("{}...", &trimmed[..100])
                        } else {
                            trimmed.to_string()
                        };
                        return Some(display);
                    }
                }
            }
        }
    }
    None
}

/// Write combined log file with all failures
pub fn write_combined_log(report_dir: &Path, rows: &[OfferedRow], base_crate: &str) -> PathBuf {
    let log_path = report_dir.join("failures.log");

    let mut content = String::new();
    content.push_str("# Cargo Copter - Combined Failure Log\n");
    content.push_str(&format!("# Generated: {}\n", chrono::Local::now().format("%Y-%m-%d %H:%M:%S")));
    content.push_str(&format!("# Base crate: {}\n\n", base_crate));

    let mut failure_count = 0;

    for row in rows {
        if row.test_passed() {
            continue;
        }

        failure_count += 1;
        let dep = format!("{}:{}", row.primary.dependent_name, row.primary.dependent_version);
        let version = row.offered.as_ref().map(|o| o.version.as_str()).unwrap_or("baseline");
        let version_display = format!("{}:{}", base_crate, version);

        content.push_str("========================================\n");
        content.push_str(&format!("FAILURE #{}: {} with {}\n", failure_count, dep, version_display));
        content.push_str("========================================\n\n");

        // Find the failed step and its error
        for cmd in &row.test.commands {
            if !cmd.result.passed {
                content.push_str(&format!("Failed at: {}\n\n", cmd.command.as_str()));
                for failure in &cmd.result.failures {
                    content.push_str(&failure.error_message);
                    if !failure.error_message.ends_with('\n') {
                        content.push('\n');
                    }
                }
                break;
            }
        }
        content.push_str("\n\n");
    }

    if failure_count == 0 {
        content.push_str("No failures recorded.\n");
    }

    if let Err(e) = std::fs::write(&log_path, &content) {
        eprintln!("Warning: Failed to write combined log: {}", e);
    }

    log_path
}

/// Print simple summary at end
pub fn print_simple_summary(rows: &[OfferedRow], report_dir: &Path, base_crate: &str, combined_log_path: &Path) {
    use std::collections::{HashMap, HashSet};

    // Group results by version
    // Key: (version_string, forced), Value: (regressed_deps, worked_deps)
    let mut by_version: HashMap<(String, bool), (Vec<String>, Vec<String>)> = HashMap::new();
    let mut broken_already: Vec<String> = Vec::new();

    // First pass: identify baseline failures (broken already)
    let mut baseline_failed_deps: HashSet<String> = HashSet::new();
    let mut baseline_check_passed_deps: HashSet<String> = HashSet::new();

    for row in rows {
        if row.offered.is_none() {
            let dep = format!("{}:{}", row.primary.dependent_name, row.primary.dependent_version);
            if !row.test_passed() {
                baseline_failed_deps.insert(row.primary.dependent_name.clone());
                // Check if check passed (for step-level regression detection)
                let check_passed = row
                    .test
                    .commands
                    .iter()
                    .filter(|c| c.command == CommandType::Check || c.command == CommandType::Fetch)
                    .all(|c| c.result.passed);
                if check_passed {
                    baseline_check_passed_deps.insert(row.primary.dependent_name.clone());
                }
                broken_already.push(dep);
            }
        }
    }

    // Second pass: categorize offered version results
    for row in rows {
        if row.offered.is_none() {
            continue; // Skip baseline rows
        }

        let offered = row.offered.as_ref().unwrap();
        let version = offered.version.clone();
        let forced = offered.forced;
        let dep = format!("{}:{}", row.primary.dependent_name, row.primary.dependent_version);

        let entry = by_version.entry((version, forced)).or_insert_with(|| (Vec::new(), Vec::new()));

        if row.test_passed() {
            entry.1.push(dep); // worked
        } else {
            // Check if this is a regression
            let is_regression = if matches!(row.baseline_passed, Some(true)) {
                true // Traditional regression: baseline fully passed
            } else if baseline_check_passed_deps.contains(&row.primary.dependent_name) {
                // Step-level regression: baseline check passed but offered check/fetch failed
                let failed_step = failed_step_name(row);
                failed_step == "fetch" || failed_step == "check"
            } else {
                false // Not a regression, baseline was already broken at this step
            };

            if is_regression {
                entry.0.push(dep); // regressed
            }
            // If not a regression, it's already counted in broken_already
        }
    }

    // Print summary
    println!();
    println!("========================================");
    println!("SUMMARY");
    println!("========================================");

    // Print regressions by version
    for ((version, forced), (regressed, _)) in &by_version {
        if !regressed.is_empty() {
            let forced_marker = if *forced { "[forced]" } else { "" };
            println!("REGRESSED with {}:{}{}: {}", base_crate, version, forced_marker, regressed.join(", "));
        }
    }

    // Print worked by version
    for ((version, forced), (_, worked)) in &by_version {
        if !worked.is_empty() {
            let forced_marker = if *forced { "[forced]" } else { "" };
            println!("WORKED with {}:{}{}: {}", base_crate, version, forced_marker, worked.join(", "));
        }
    }

    // Print broken already
    if !broken_already.is_empty() {
        println!("BROKEN ALREADY: {}", broken_already.join(", "));
    }

    // Count totals
    let total_regressed: usize = by_version.values().map(|(r, _)| r.len()).sum();
    let total_worked: usize = by_version.values().map(|(_, w)| w.len()).sum();

    println!();
    println!("Regressed: {}", total_regressed);
    println!("Worked:    {}", total_worked);
    println!("Broken:    {}", broken_already.len());

    // Always show report paths
    println!();
    println!("Reports:");
    println!("  Combined log: {}", combined_log_path.display());
    println!("  Markdown:     {}/report.md", report_dir.display());
    println!("  JSON:         {}/report.json", report_dir.display());
}
