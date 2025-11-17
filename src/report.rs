/// Report generation module - Clean rewrite for OfferedRow streaming
///
/// Provides console table output, HTML, and markdown reports

use std::fs::File;
use std::io::Write;
use std::path::PathBuf;
use crate::{OfferedRow, DependencyRef, OfferedVersion, TestExecution, TestCommand, CommandType, CommandResult, CrateFailure, TransitiveTest, VersionSource};
use term::color::Color;
use unicode_width::{UnicodeWidthStr, UnicodeWidthChar};
use terminal_size::{Width, terminal_size};
use lazy_static::lazy_static;

//
// Rendering Model Types
//

/// Status icon for the Offered column
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatusIcon {
    Passed,     // ✓
    Failed,     // ✗
    Skipped,    // ⊘
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
    Exact,      // = (cargo resolved to exact offered version)
    Upgraded,   // ↑ (cargo upgraded within semver range)
    Mismatch,   // ≠ (forced or semver incompatible)
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
        forced: bool,  // adds [≠→!] suffix if true
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

        // Determine status icon
        let icon = match (row.baseline_passed, overall_passed) {
            (Some(true), true) => StatusIcon::Passed,   // PASSED
            (Some(true), false) => StatusIcon::Failed,  // REGRESSED
            (Some(false), _) => StatusIcon::Failed,     // BROKEN (baseline failed)
            (None, true) => StatusIcon::Passed,         // PASSED (no baseline)
            (None, false) => StatusIcon::Failed,        // FAILED (no baseline)
        };

        // Determine resolution marker
        let resolution = if offered.forced {
            Resolution::Mismatch  // Forced versions always show ≠
        } else if row.primary.used_offered_version {
            Resolution::Exact     // Cargo chose exactly what we offered
        } else {
            Resolution::Upgraded  // Cargo upgraded to something else
        };

        OfferedCell::Tested {
            icon,
            resolution,
            version: offered.version.clone(),
            forced: offered.forced,
        }
    }

    /// Format the cell content for display
    pub fn format(&self) -> String {
        match self {
            OfferedCell::Baseline => "- baseline".to_string(),
            OfferedCell::Tested { icon, resolution, version, forced } => {
                let mut result = format!(
                    "{} {}{}",
                    icon.as_str(),
                    resolution.as_str(),
                    version
                );
                if *forced {
                    result.push_str(" [≠→!]");
                }
                result
            }
        }
    }
}

//
// Console Table Rendering
//

// Column widths for the 5-column table
#[derive(Clone, Copy)]
struct TableWidths {
    offered: usize,
    spec: usize,
    resolved: usize,
    dependent: usize,
    result: usize,
    total: usize,  // Total table width including borders
}

impl TableWidths {
    fn new(terminal_width: usize) -> Self {
        // Borders: │ = 6 characters (1 before each column + 1 at end)
        let borders = 6;
        let available = terminal_width.saturating_sub(borders);

        // Use fixed widths for columns with known/predictable values
        // Offered: "✗ ≠0.8.91-preview [≠→!]" max ~28 chars
        let offered = 25;
        // Spec: "^0.8.52" or "→ =this" max ~12 chars
        let spec = 12;
        // Resolved: "0.8.91-preview 📦" max ~18 chars
        let resolved = 18;
        // Result: "REGRESSED ✓✗  1.3s" fixed ~20 chars
        let result = 25;

        // Dependent gets remaining space (for long crate names)
        let fixed_total = offered + spec + resolved + result;
        let dependent = if available > fixed_total {
            available - fixed_total
        } else {
            20  // Minimum fallback
        };

        TableWidths {
            offered,
            spec,
            resolved,
            dependent,
            result,
            total: terminal_width,
        }
    }
}

/// Get terminal width or default to 120
fn get_terminal_width() -> usize {
    if let Some((Width(w), _)) = terminal_size() {
        w as usize
    } else {
        120  // Default width
    }
}

// Calculate table widths once at startup
lazy_static! {
    static ref WIDTHS: TableWidths = TableWidths::new(get_terminal_width());
}

/// Print table header
/// Format table header as a string
pub fn format_table_header(crate_name: &str, display_version: &str, total_deps: usize) -> String {
    let term_width = get_terminal_width();
    let w = &*WIDTHS;

    let mut output = String::new();
    output.push_str(&format!("\n{}\n", "=".repeat(term_width)));
    output.push_str(&format!("Testing {} reverse dependencies of {}\n", total_deps, crate_name));
    output.push_str(&format!("  this = {} (your work-in-progress version)\n", display_version));
    output.push_str(&format!("{}\n", "=".repeat(term_width)));
    output.push('\n');

    output.push_str(&format!("┌{:─<width1$}┬{:─<width2$}┬{:─<width3$}┬{:─<width4$}┬{:─<width5$}┐\n",
             "", "", "", "", "",
             width1 = w.offered, width2 = w.spec, width3 = w.resolved,
             width4 = w.dependent, width5 = w.result));
    output.push_str(&format!("│{:^width1$}│{:^width2$}│{:^width3$}│{:^width4$}│{:^width5$}│\n",
             "Offered", "Spec", "Resolved", "Dependent", "Result         Time",
             width1 = w.offered, width2 = w.spec, width3 = w.resolved,
             width4 = w.dependent, width5 = w.result));
    output.push_str(&format!("├{:─<width1$}┼{:─<width2$}┼{:─<width3$}┼{:─<width4$}┼{:─<width5$}┤\n",
             "", "", "", "", "",
             width1 = w.offered, width2 = w.spec, width3 = w.resolved,
             width4 = w.dependent, width5 = w.result));

    output
}

pub fn print_table_header(crate_name: &str, display_version: &str, total_deps: usize) {
    print!("{}", format_table_header(crate_name, display_version, total_deps));
}

/// Print separator line between dependents
pub fn print_separator_line() {
    let w = &*WIDTHS;
    println!("├{:─<width1$}┼{:─<width2$}┼{:─<width3$}┼{:─<width4$}┼{:─<width5$}┤",
             "", "", "", "", "",
             width1 = w.offered, width2 = w.spec, width3 = w.resolved,
             width4 = w.dependent, width5 = w.result);
}

/// Format table footer as a string
pub fn format_table_footer() -> String {
    let w = &*WIDTHS;
    format!("└{:─<width1$}┴{:─<width2$}┴{:─<width3$}┴{:─<width4$}┴{:─<width5$}┘\n",
             "", "", "", "", "",
             width1 = w.offered, width2 = w.spec, width3 = w.resolved,
             width4 = w.dependent, width5 = w.result)
}

/// Print table footer
pub fn print_table_footer() {
    print!("{}", format_table_footer());
}

/// Print an OfferedRow using the standard table format
pub fn print_offered_row(row: &OfferedRow, is_last_in_group: bool) {
    // Convert OfferedRow to column strings
    let (offered_str, spec_str, resolved_str, dependent_str, result_str, time_str, color, error_details, multi_version_rows) = format_offered_row(row);

    // Use dynamic widths
    let w = &*WIDTHS;

    // Print main row
    let offered_display = truncate_with_padding(&offered_str, w.offered - 2);
    let spec_display = truncate_with_padding(&spec_str, w.spec - 2);
    let resolved_display = truncate_with_padding(&resolved_str, w.resolved - 2);
    let dependent_display = truncate_with_padding(&dependent_str, w.dependent - 2);
    let result_display = format!("{:>12} {:>5}", result_str, time_str);
    let result_display = truncate_with_padding(&result_display, w.result - 2);

    // Print main row with color
    if let Some(ref mut t) = term::stdout() {
        let _ = t.fg(color);
        let _ = write!(t, "│ {} │", offered_display);
        let _ = write!(t, " {} │", spec_display);
        let _ = write!(t, " {} │", resolved_display);
        let _ = write!(t, " {} │", dependent_display);
        let _ = write!(t, " {} │", result_display);
        let _ = t.reset();
        println!();
    } else {
        println!("│ {} │ {} │ {} │ {} │ {} │",
                 offered_display, spec_display, resolved_display, dependent_display, result_display);
    }

    // Print error details with dropped-panel border (if any)
    if !error_details.is_empty() {
        let corner1_width = w.spec;
        let corner2_width = w.dependent;
        let padding_width = w.spec + w.resolved + w.dependent  - corner1_width - corner2_width;

        let shortened_offered = 4;
        let corner0_width = if shortened_offered != w.offered {
            w.offered - shortened_offered -1
        } else { 0};
        let error_text_width = w.total - 1 - shortened_offered - 1 - 1 - 1 - 1;

        if corner0_width > 0 {
            println!("│{:shortened_offered$}┌{:─<corner0$}┴{:─<corner1$}┘{:padding$}└{:─<corner2$}┘{:w_result$}│",
                     "", "", "", "", "",
                     shortened_offered = shortened_offered, corner0 = corner0_width, corner1 = corner1_width,
                     padding = padding_width, corner2 = corner2_width, w_result = w.result);
        } else {
            println!("│{:w_offered$}├{:─<corner1$}┘{:padding$}└{:─<corner2$}┘{:w_result$}│",
                    "", "", "", "", "",
                    w_offered = w.offered, corner1 = corner1_width,
                    padding = padding_width, corner2 = corner2_width, w_result = w.result);
        }
        for error_line in &error_details {
            let truncated = truncate_with_padding(error_line, error_text_width);
            println!("│{:shortened_offered$}│ {} │",
                     "", truncated,
                     shortened_offered = shortened_offered);
        }

        if !is_last_in_group {
            if corner0_width > 0 {
                println!("│{:shortened_offered$}└{:─<corner0$}┬{:─<corner1$}┬{:─<corner2$}┬{:─<corner3$}┬{:─<corner4$}┤",
                         "", "", "", "", "",
                         shortened_offered = shortened_offered, corner0 = corner0_width, corner1 = w.spec, corner2 = w.resolved,
                         corner3 = w.dependent, corner4 = w.result);
            } else {
                println!("│{:w_offered$}├{:─<w_spec$}┬{:─<w_resolved$}┬{:─<w_dependent$}┬{:─<w_result$}┤",
                        "", "", "", "", "",
                        w_offered = w.offered, w_spec = w.spec, w_resolved = w.resolved,
                        w_dependent = w.dependent, w_result = w.result);
            }
        }
    }

    // Print multi-version rows with ├─ prefixes (if any)
    if !multi_version_rows.is_empty() {
        for (_i, (spec, resolved, dependent)) in multi_version_rows.iter().enumerate() {
            let spec_display = format!("├─ {}", spec);
            let spec_display = truncate_with_padding(&spec_display, w.spec - 2);
            let resolved_display = format!("├─ {}", resolved);
            let resolved_display = truncate_with_padding(&resolved_display, w.resolved - 2);
            let dependent_display = format!("├─ {}", dependent);
            let dependent_display = truncate_with_padding(&dependent_display, w.dependent - 2);

            println!("│{:width$}│ {} │ {} │ {} │{:w_result$}│",
                     "", spec_display, resolved_display, dependent_display, "",
                     width = w.offered, w_result = w.result);
        }
    }
}

//
// OfferedRow to renderable format conversion
//

/// Convert OfferedRow to renderable row data
/// Returns: (offered_str, spec_str, resolved_str, dependent_str, result_str, time_str, color, error_details, multi_version_rows)
fn format_offered_row(row: &OfferedRow) -> (String, String, String, String, String, String, Color, Vec<String>, Vec<(String, String, String)>) {
    // Format Offered column using type-safe OfferedCell
    let offered_cell = OfferedCell::from_offered_row(row);
    let offered_str = offered_cell.format();

    // Format Spec column
    let spec_str = if let Some(ref offered) = row.offered {
        if offered.forced {
            format!("→ ={}", offered.version)
        } else {
            row.primary.spec.clone()
        }
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

    // Format Result column
    let overall_passed = row.test.commands.iter().all(|cmd| cmd.result.passed);
    let result_status = match (row.baseline_passed, overall_passed) {
        (Some(true), true) => "PASSED",
        (Some(true), false) => "REGRESSED",
        (Some(false), _) => "BROKEN",
        (None, true) => "PASSED",
        (None, false) => "FAILED",
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
    let total_time: f64 = row.test.commands.iter()
        .map(|cmd| cmd.result.duration)
        .sum();
    let time_str = format!("{:.1}s", total_time);

    // Determine color
    let color = match (row.baseline_passed, overall_passed) {
        (Some(true), true) => term::color::BRIGHT_GREEN,
        (Some(true), false) => term::color::BRIGHT_RED,
        (Some(false), _) => term::color::BRIGHT_YELLOW,
        (None, true) => term::color::BRIGHT_GREEN,
        (None, false) => term::color::BRIGHT_RED,
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
                // Add error message if not empty (already formatted by extract_error_summary)
                if !failure.error_message.is_empty() {
                    // Split into lines and display each with bullet
                    for line in failure.error_message.lines().take(10) {
                        if !line.trim().is_empty() {
                            error_details.push(format!("  {}", line));
                        }
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

    (offered_str, spec_str, resolved_str, dependent_str, result_str, time_str, color, error_details, multi_version_rows)
}

//
// Text formatting utilities
//

/// Truncate string to fit width, adding "..." if truncated
fn truncate_str(s: &str, max_width: usize) -> String {
    let char_count = s.chars().count();

    if char_count <= max_width {
        s.to_string()
    } else if max_width >= 3 {
        let truncate_at = max_width - 3;
        let truncated: String = s.chars().take(truncate_at).collect();
        format!("{}...", truncated)
    } else {
        let truncated: String = s.chars().take(max_width).collect();
        truncated
    }
}

/// Count the display width of a string, accounting for wide Unicode characters
fn display_width(s: &str) -> usize {
    // Use unicode-width crate for accurate width calculation
    UnicodeWidthStr::width(s)
}

/// Truncate and pad string to exact width
fn truncate_with_padding(s: &str, width: usize) -> String {
    let display_w = display_width(s);

    if display_w > width {
        // Truncate
        let mut result = String::new();
        let mut current_width = 0;
        let mut chars: Vec<char> = s.chars().collect();

        // Reserve space for "..."
        let target_width = if width >= 3 { width - 3 } else { width };

        for c in chars.iter() {
            let c_width = UnicodeWidthChar::width(*c).unwrap_or(1);

            if current_width + c_width > target_width {
                break;
            }

            result.push(*c);
            current_width += c_width;
        }

        if width >= 3 {
            result.push_str("...");
            current_width += 3;
        }

        // Pad if needed
        if current_width < width {
            result.push_str(&" ".repeat(width - current_width));
        }

        result
    } else {
        // Pad with spaces to reach the width
        let padding = width - display_w;
        format!("{}{}", s, " ".repeat(padding))
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
                (Some(true), true) => passed += 1,      // PASSED
                (Some(true), false) => regressed += 1,  // REGRESSED
                (Some(false), _) => broken += 1,        // BROKEN
                (None, true) => passed += 1,            // PASSED (no baseline)
                (None, false) => broken += 1,           // FAILED (no baseline)
            }
        }
    }

    TestSummary {
        passed,
        regressed,
        broken,
        total: passed + regressed + broken,
    }
}

/// Summarize TestResults into counts
pub fn summarize_results(results: &[crate::TestResult]) -> TestSummary {
    let mut passed = 0;
    let mut regressed = 0;
    let mut broken = 0;

    for result in results {
        match &result.data {
            crate::TestResultData::Skipped(_) => {
                // Skip counting skipped tests
            }
            crate::TestResultData::Error(_) => {
                broken += 1;
            }
            crate::TestResultData::MultiVersion(ref outcomes) => {
                // Baseline is the first outcome
                let baseline = outcomes.first();

                let mut has_regressed = false;
                let mut has_broken = false;

                for (idx, outcome) in outcomes.iter().enumerate() {
                    if idx == 0 {
                        // Baseline - if it fails, mark as broken
                        if !outcome.result.is_success() {
                            has_broken = true;
                        }
                    } else {
                        // Offered version - classify based on baseline
                        if !outcome.result.is_success() {
                            if let Some(base) = baseline {
                                if base.result.is_success() {
                                    has_regressed = true;
                                } else {
                                    has_broken = true;
                                }
                            } else {
                                has_broken = true;
                            }
                        } else {
                            // Offered version passed
                            if let Some(base) = baseline {
                                if base.result.is_success() {
                                    // Don't count yet - only count if ALL offered versions pass
                                }
                            }
                        }
                    }
                }

                // Count based on worst outcome
                if has_regressed {
                    regressed += 1;
                } else if has_broken {
                    broken += 1;
                } else {
                    passed += 1;
                }
            }
        }
    }

    TestSummary {
        passed,
        regressed,
        broken,
        total: passed + regressed + broken,
    }
}

/// Format summary statistics as a string
pub fn format_summary(summary: &TestSummary) -> String {
    let mut output = String::new();
    output.push_str("\nSummary:\n");
    output.push_str(&format!("  ✓ Passed:    {}\n", summary.passed));
    output.push_str(&format!("  ✗ Regressed: {}\n", summary.regressed));
    output.push_str(&format!("  ⚠ Broken:    {}\n", summary.broken));
    output.push_str("  ━━━━━━━━━━━━━\n");
    output.push_str(&format!("  Total:       {}\n", summary.total));
    output.push('\n');
    output
}

/// Print summary statistics
pub fn print_summary(summary: &TestSummary) {
    print!("{}", format_summary(summary));
}

//
// HTML and Markdown report generation (simplified)
//

/// Generate HTML report from OfferedRows
pub fn generate_html_report(rows: &[OfferedRow], crate_name: &str, display_version: &str, output_path: &PathBuf) -> std::io::Result<()> {
    let mut file = File::create(output_path)?;

    writeln!(file, "<!DOCTYPE html>")?;
    writeln!(file, "<html><head><meta charset='UTF-8'>")?;
    writeln!(file, "<title>Cargo Copter Report - {}</title>", crate_name)?;
    writeln!(file, "<style>")?;
    writeln!(file, "body {{ font-family: monospace; margin: 20px; }}")?;
    writeln!(file, "table {{ border-collapse: collapse; width: 100%; }}")?;
    writeln!(file, "th, td {{ border: 1px solid #ccc; padding: 8px; text-align: left; }}")?;
    writeln!(file, ".passed {{ color: green; }}")?;
    writeln!(file, ".regressed {{ color: red; }}")?;
    writeln!(file, ".broken {{ color: orange; }}")?;
    writeln!(file, "</style></head><body>")?;
    writeln!(file, "<h1>Cargo Copter Report</h1>")?;
    writeln!(file, "<p>Crate: <strong>{}</strong> ({})</p>", crate_name, display_version)?;
    writeln!(file, "<table><thead><tr>")?;
    writeln!(file, "<th>Offered</th><th>Spec</th><th>Resolved</th><th>Dependent</th><th>Result</th>")?;
    writeln!(file, "</tr></thead><tbody>")?;

    for row in rows {
        let (offered, spec, resolved, dependent, result, time, _, _, _) = format_offered_row(row);
        let class = if row.offered.is_some() {
            let overall_passed = row.test.commands.iter().all(|cmd| cmd.result.passed);
            match (row.baseline_passed, overall_passed) {
                (Some(true), true) => "passed",
                (Some(true), false) => "regressed",
                _ => "broken",
            }
        } else {
            ""
        };

        writeln!(file, "<tr class='{}'><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td>{} {}</td></tr>",
                 class, sanitize(&offered), sanitize(&spec), sanitize(&resolved),
                 sanitize(&dependent), sanitize(&result), sanitize(&time))?;
    }

    writeln!(file, "</tbody></table>")?;

    let summary = summarize_offered_rows(rows);
    writeln!(file, "<h2>Summary</h2>")?;
    writeln!(file, "<p>Passed: {}, Regressed: {}, Broken: {}</p>",
             summary.passed, summary.regressed, summary.broken)?;

    writeln!(file, "</body></html>")?;
    Ok(())
}

/// Generate Markdown report from OfferedRows
pub fn generate_markdown_report(rows: &[OfferedRow], crate_name: &str, display_version: &str, output_path: &PathBuf) -> std::io::Result<()> {
    let mut file = File::create(output_path)?;

    writeln!(file, "# Cargo Copter Report\n")?;
    writeln!(file, "**Crate**: {} ({})\n", crate_name, display_version)?;
    writeln!(file, "## Test Results\n")?;
    writeln!(file, "| Offered | Spec | Resolved | Dependent | Result |")?;
    writeln!(file, "|---------|------|----------|-----------|--------|")?;

    for row in rows {
        let (offered, spec, resolved, dependent, result, time, _, _, _) = format_offered_row(row);
        writeln!(file, "| {} | {} | {} | {} | {} {} |",
                 offered, spec, resolved, dependent, result, time)?;
    }

    let summary = summarize_offered_rows(rows);
    writeln!(file, "\n## Summary\n")?;
    writeln!(file, "- ✓ Passed: {}", summary.passed)?;
    writeln!(file, "- ✗ Regressed: {}", summary.regressed)?;
    writeln!(file, "- ⚠ Broken: {}", summary.broken)?;
    writeln!(file, "- **Total**: {}", summary.total)?;

    Ok(())
}

/// Sanitize HTML special characters
fn sanitize(s: &str) -> String {
    s.chars()
        .flat_map(|c| match c {
            '<' => "&lt;".chars().collect(),
            '>' => "&gt;".chars().collect(),
            '&' => "&amp;".chars().collect(),
            _ => vec![c],
        })
        .collect()
}

//
// Temporary compatibility stubs for old API (TO BE REMOVED)
//

/// Stub for old API - needs migration to OfferedRow
pub fn print_immediate_failure(_result: &crate::TestResult) {
    // TODO: Migrate to OfferedRow-based error printing
    eprintln!("Warning: print_immediate_failure not yet migrated to OfferedRow");
}

/// Stub for old API - needs migration to OfferedRow
pub fn print_console_table_v2(_results: &[crate::TestResult], _crate_name: &str, _display_version: &str) {
    // TODO: Migrate to OfferedRow streaming
    println!("Warning: print_console_table_v2 not yet migrated to OfferedRow");
    println!("Use: print_table_header(), print_offered_row(), print_table_footer()");
}

/// Generate markdown report with console table in code block
pub fn export_markdown_table_report(rows: &[OfferedRow], output_path: &PathBuf, crate_name: &str, display_version: &str, total_deps: usize) -> std::io::Result<()> {
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

    // Write table header
    write!(file, "{}", format_table_header(crate_name, display_version, total_deps))?;

    // Write all rows
    for (i, row) in rows.iter().enumerate() {
        // Determine if this is the last row in its group
        // For simplicity, assume each row is its own group (no separators in markdown)
        let is_last_in_group = true;

        // Format the row (we need a string-returning version of print_offered_row)
        write!(file, "{}", format_offered_row_string(row, is_last_in_group))?;
    }

    // Write table footer
    write!(file, "{}", format_table_footer())?;

    writeln!(file, "```\n")?;

    Ok(())
}

/// Format an OfferedRow as a string (similar to print_offered_row but returns String)
fn format_offered_row_string(row: &OfferedRow, is_last_in_group: bool) -> String {
    let (offered_str, spec_str, resolved_str, dependent_str, result_str, time_str, _color, error_details, multi_version_rows) = format_offered_row(row);
    let w = &*WIDTHS;

    let mut output = String::new();

    // Main row
    let offered_display = truncate_with_padding(&offered_str, w.offered - 2);
    let spec_display = truncate_with_padding(&spec_str, w.spec - 2);
    let resolved_display = truncate_with_padding(&resolved_str, w.resolved - 2);
    let dependent_display = truncate_with_padding(&dependent_str, w.dependent - 2);
    let result_display = format!("{:>12} {:>5}", result_str, time_str);
    let result_display = truncate_with_padding(&result_display, w.result - 2);

    output.push_str(&format!("│ {} │ {} │ {} │ {} │ {} │\n",
        offered_display, spec_display, resolved_display, dependent_display, result_display));

    // Error details (if any)
    if !error_details.is_empty() {
        let error_text_width = w.total - 1 - w.offered - 1 - 1 - 1 - 1;
        let corner1_width = w.spec;
        let corner2_width = w.dependent;
        let padding_width = w.spec + w.resolved + w.dependent - corner1_width - corner2_width;

        output.push_str(&format!("│{:w_offered$}├{:─<corner1$}┘{:padding$}└{:─<corner2$}┘{:w_result$}│\n",
            "", "", "", "", "",
            w_offered = w.offered, corner1 = corner1_width,
            padding = padding_width, corner2 = corner2_width, w_result = w.result));

        for error_line in &error_details {
            let truncated = truncate_with_padding(error_line, error_text_width);
            output.push_str(&format!("│{:w_offered$}│ {} │\n", "", truncated, w_offered = w.offered));
        }

        if !is_last_in_group {
            output.push_str(&format!("│{:w_offered$}├{:─<w_spec$}┬{:─<w_resolved$}┬{:─<w_dependent$}┬{:─<w_result$}┤\n",
                "", "", "", "", "",
                w_offered = w.offered, w_spec = w.spec, w_resolved = w.resolved,
                w_dependent = w.dependent, w_result = w.result));
        }
    }

    // Multi-version rows (if any)
    if !multi_version_rows.is_empty() {
        for (_i, (spec, resolved, dependent)) in multi_version_rows.iter().enumerate() {
            let spec_display = format!("├─ {}", spec);
            let spec_display = truncate_with_padding(&spec_display, w.spec - 2);
            let resolved_display = format!("├─ {}", resolved);
            let resolved_display = truncate_with_padding(&resolved_display, w.resolved - 2);
            let dependent_display = format!("├─ {}", dependent);
            let dependent_display = truncate_with_padding(&dependent_display, w.dependent - 2);

            output.push_str(&format!("│{:width$}│ {} │ {} │ {} │{:w_result$}│\n",
                "", spec_display, resolved_display, dependent_display, "",
                width = w.offered, w_result = w.result));
        }
    }

    output
}

/// Compatibility wrapper for old API
pub fn export_markdown_report(_rows: &[crate::TestResult], _output_path: &PathBuf, _crate_name: &str, _display_version: &str) -> std::io::Result<()> {
    // Deprecated - use export_markdown_table_report with OfferedRows instead
    Ok(())
}

/// Compatibility wrapper for old API
pub fn export_html_report(rows: Vec<crate::TestResult>, output_path: &PathBuf, crate_name: &str, display_version: &str) -> std::io::Result<TestSummary> {
    // TODO: Convert TestResult to OfferedRow, then call generate_html_report
    eprintln!("Warning: export_html_report needs TestResult -> OfferedRow conversion");
    Ok(TestSummary { passed: 0, regressed: 0, broken: 0, total: 0 })
}
