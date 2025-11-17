/// Report generation module - Clean rewrite for OfferedRow streaming
///
/// Provides console table output, HTML, and markdown reports

use std::fs::File;
use std::io::Write;
use std::path::PathBuf;
use std::sync::OnceLock;
use crate::{OfferedRow, CommandType, VersionSource};
use term::color::Color;
use unicode_width::{UnicodeWidthStr, UnicodeWidthChar};
use terminal_size::{Width, terminal_size};
use crate::console_tables::{ColSize, TableFormatter};

//
// Rendering Model Types
//

/// Status icon for the Offered column
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatusIcon {
    Passed,     // ✓
    Failed,     // ✗
}

impl StatusIcon {
    pub fn as_str(&self) -> &'static str {
        match self {
            StatusIcon::Passed => "✓",
            StatusIcon::Failed => "✗",
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
        Self::new_with_offered(terminal_width, None)
    }

    fn new_with_offered(terminal_width: usize, offered_width: Option<usize>) -> Self {
        // Borders: │ = 6 characters (1 before each column + 1 at end)
        let borders = 6;
        let available = terminal_width.saturating_sub(borders);

        // Use fixed widths for columns with known/predictable values
        // Offered: use provided width or default to 25
        let offered = offered_width.unwrap_or(25);
        // Spec: "^0.8.52" or "→ =this" max ~12 chars
        let spec = 12;
        // Resolved: "0.8.91-preview 📦" max ~18 chars
        let resolved = 18;
        // Result: "build failed ✓✗-  1.3s" fixed ~25 chars
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

    /// Calculate minimum offered column width for given versions
    pub fn calculate_offered_width(versions: &[String], _display_version: &str, force_versions: bool) -> usize {
        let mut max_width = "- baseline".len(); // 10 chars

        // Forced marker is 6 chars: " [≠→!]"
        let forced_width = if force_versions { 6 } else { 0 };

        // Check all test versions
        for version in versions {
            // Format: "{icon} {resolution}{version}[ [≠→!]]"
            // Icon (1) + space (1) + resolution (1) + version + optional forced marker
            let width = 1 + 1 + 1 + version.len() + forced_width;
            max_width = max_width.max(width);
        }

        // Add generous padding for comfortable spacing (6 chars breathing room)
        max_width + 6
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

// Table widths - initialized once with actual version data
static WIDTHS: OnceLock<TableWidths> = OnceLock::new();

/// Initialize table widths based on versions being tested
pub fn init_table_widths(versions: &[String], display_version: &str, force_versions: bool) {
    let offered_width = TableWidths::calculate_offered_width(versions, display_version, force_versions);
    let widths = TableWidths::new_with_offered(get_terminal_width(), Some(offered_width));
    let _ = WIDTHS.set(widths); // Ignore error if already initialized
}

/// Get table widths (with fallback to defaults if not initialized)
fn get_widths() -> &'static TableWidths {
    WIDTHS.get_or_init(|| TableWidths::new(get_terminal_width()))
}

//
// Column layout helpers for TableFormatter
//

/// Create standard 5-column layout (all columns want separators)
fn standard_5col_layout() -> Vec<ColSize> {
    let w = get_widths();
    vec![
        ColSize::new(w.offered, true),
        ColSize::new(w.spec, true),
        ColSize::new(w.resolved, true),
        ColSize::new(w.dependent, true),
        ColSize::new(w.result, true),
    ]
}

/// Create error row layout (offered column + merged columns 2-5)
fn error_row_layout() -> Vec<ColSize> {
    let w = get_widths();
    vec![
        ColSize::new(w.offered, false),  // No separator for offered column
        // Merged columns 2-5: spec + resolved + dependent + result + 3 internal borders
        ColSize::new(w.spec + w.resolved + w.dependent + w.result + 3, true),
    ]
}

/// Create header row layout (same as standard but might have different separator rules)
fn header_row_layout() -> Vec<ColSize> {
    standard_5col_layout()
}

/// Format a standard 5-column row as a string (for TableFormatter)
fn format_5col_row_string(
    offered: &str,
    spec: &str,
    resolved: &str,
    dependent: &str,
    result: &str,
) -> String {
    let w = get_widths();
    let offered_display = truncate_with_padding(offered, w.offered - 2);
    let spec_display = truncate_with_padding(spec, w.spec - 2);
    let resolved_display = truncate_with_padding(resolved, w.resolved - 2);
    let dependent_display = truncate_with_padding(dependent, w.dependent - 2);
    let result_display = truncate_with_padding(result, w.result - 2);

    format!("│ {} │ {} │ {} │ {} │ {} │",
            offered_display, spec_display, resolved_display,
            dependent_display, result_display)
}

/// Print table header
/// Format table header as a string
pub fn format_table_header(crate_name: &str, display_version: &str, total_deps: usize) -> String {
    let term_width = get_terminal_width();
    let w = get_widths();

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
    let w = get_widths();
    println!("├{:─<width1$}┼{:─<width2$}┼{:─<width3$}┼{:─<width4$}┼{:─<width5$}┤",
             "", "", "", "", "",
             width1 = w.offered, width2 = w.spec, width3 = w.resolved,
             width4 = w.dependent, width5 = w.result);
}

/// Format table footer as a string
pub fn format_table_footer() -> String {
    let w = get_widths();
    format!("└{:─<width1$}┴{:─<width2$}┴{:─<width3$}┴{:─<width4$}┴{:─<width5$}┘\n",
             "", "", "", "", "",
             width1 = w.offered, width2 = w.spec, width3 = w.resolved,
             width4 = w.dependent, width5 = w.result)
}

/// Print table footer
pub fn print_table_footer() {
    print!("{}", format_table_footer());
}

/// Extract error text from an OfferedRow for deduplication
pub fn extract_error_text(row: &OfferedRow) -> Option<String> {
    let formatted = format_offered_row(row);
    if formatted.error_details.is_empty() {
        None
    } else {
        Some(formatted.error_details.join("\n"))
    }
}

/// Helper to print error box top border
fn print_error_box_top(w: &TableWidths) {
    let shortened_offered = 4;
    let corner0_width = if shortened_offered != w.offered {
        w.offered - shortened_offered - 1
    } else { 0 };

    if corner0_width > 0 {
        println!("│{:shortened$}┌{:─<c0$}┴{:─<c1$}┘{:padding$}└{:─<c2$}┘{:result$}│",
                 "", "", "", "", "", "",
                 shortened = shortened_offered,
                 c0 = corner0_width,
                 c1 = w.spec,
                 padding = w.resolved,
                 c2 = w.dependent,
                 result = w.result);
    } else {
        println!("│{:offered$}├{:─<spec$}┘{:padding$}└{:─<dep$}┘{:result$}│",
                "", "", "", "", "",
                offered = w.offered,
                spec = w.spec,
                padding = w.resolved,
                dep = w.dependent,
                result = w.result);
    }
}

/// Helper to print error box content line
fn print_error_box_line(line: &str, w: &TableWidths) {
    let shortened_offered = 4;
    let error_text_width = w.total - 1 - shortened_offered - 1 - 1 - 1 - 1;
    let truncated = truncate_with_padding(line, error_text_width);
    println!("│{:shortened$}│ {} │",
             "", truncated,
             shortened = shortened_offered);
}

/// Helper to print error box bottom border (transitioning back to main table)
fn print_error_box_bottom(w: &TableWidths) {
    let shortened_offered = 4;
    let corner0_width = if shortened_offered != w.offered {
        w.offered - shortened_offered - 1
    } else { 0 };

    if corner0_width > 0 {
        println!("│{:shortened$}└{:─<c0$}┬{:─<c1$}┬{:─<c2$}┬{:─<c3$}┬{:─<c4$}┤",
                 "", "", "", "", "", "",
                 shortened = shortened_offered,
                 c0 = corner0_width,
                 c1 = w.spec,
                 c2 = w.resolved,
                 c3 = w.dependent,
                 c4 = w.result);
    } else {
        println!("│{:offered$}├{:─<spec$}┬{:─<resolved$}┬{:─<dep$}┬{:─<result$}┤",
                "", "", "", "", "",
                offered = w.offered,
                spec = w.spec,
                resolved = w.resolved,
                dep = w.dependent,
                result = w.result);
    }
}

/// Print a main 5-column row with proper formatting and color
fn print_main_row(cells: [&str; 5], color: Color) {
    let w = get_widths();
    let displays: Vec<String> = cells.iter().zip([w.offered, w.spec, w.resolved, w.dependent, w.result].iter())
        .map(|(cell, width)| truncate_with_padding(cell, width - 2))
        .collect();

    if let Some(ref mut t) = term::stdout() {
        let _ = t.fg(color);
        let _ = write!(t, "│ {} │ {} │ {} │ {} │ {} │",
                      displays[0], displays[1], displays[2], displays[3], displays[4]);
        let _ = t.reset();
        println!();
    } else {
        println!("│ {} │ {} │ {} │ {} │ {} │",
                 displays[0], displays[1], displays[2], displays[3], displays[4]);
    }
}

/// Print multi-version dependency rows
fn print_multi_version_rows(rows: &[(String, String, String)]) {
    if rows.is_empty() { return; }

    let w = get_widths();
    let last_idx = rows.len() - 1;

    for (i, (spec, resolved, dependent)) in rows.iter().enumerate() {
        let prefix = if i == last_idx { "└─" } else { "├─" };
        let spec_display = format!("{} {}", prefix, spec);
        let spec_display = truncate_with_padding(&spec_display, w.spec - 2);
        let resolved_display = format!("{} {}", prefix, resolved);
        let resolved_display = truncate_with_padding(&resolved_display, w.resolved - 2);
        let dependent_display = format!("{} {}", prefix, dependent);
        let dependent_display = truncate_with_padding(&dependent_display, w.dependent - 2);

        println!("│{:width$}│ {} │ {} │ {} │{:w_result$}│",
                 "", spec_display, resolved_display, dependent_display, "",
                 width = w.offered, w_result = w.result);
    }
}

/// Print an OfferedRow using the standard table format
pub fn print_offered_row(row: &OfferedRow, is_last_in_group: bool, prev_error: Option<&str>) {
    // Convert OfferedRow to formatted data
    let mut formatted = format_offered_row(row);

    // Check if this error is the same as the previous one
    if let Some(prev) = prev_error {
        if !formatted.error_details.is_empty() {
            let current_error = formatted.error_details.join("\n");
            if current_error == prev {
                // Clear error details and show "still failing" in result column
                formatted.error_details.clear();
                formatted.result = "still failing".to_string();
                formatted.time = String::new();
            }
        }
    }

    // Format result column
    let result_display = if formatted.time.is_empty() {
        // "still failing" case - no ICT marks or time
        format!("{:>18}", formatted.result)
    } else {
        format!("{:>12} {:>5}", formatted.result, formatted.time)
    };

    // Print main row with color
    print_main_row(
        [&formatted.offered, &formatted.spec, &formatted.resolved,
         &formatted.dependent, &result_display],
        formatted.color
    );

    // Print error box if present
    if !formatted.error_details.is_empty() {
        let w = get_widths();
        print_error_box_top(&w);

        for error_line in &formatted.error_details {
            print_error_box_line(error_line, &w);
        }

        if !is_last_in_group {
            print_error_box_bottom(&w);
        }
    }

    // Print multi-version dependency rows
    print_multi_version_rows(&formatted.multi_version_rows);
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
fn format_offered_row(row: &OfferedRow) -> FormattedRow {
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

    // Determine which step failed (if any)
    let overall_passed = row.test.commands.iter().all(|cmd| cmd.result.passed);
    let failed_step = row.test.commands.iter()
        .find(|cmd| !cmd.result.passed)
        .map(|cmd| match cmd.command {
            CommandType::Fetch => "fetch failed",
            CommandType::Check => "build failed",
            CommandType::Test => "test failed",
        });

    let result_status = match (row.baseline_passed, overall_passed, failed_step) {
        (Some(true), true, _) => "passed".to_string(),
        (Some(true), false, Some(step)) => step.to_string(),
        (Some(true), false, None) => "regressed".to_string(),
        (Some(false), _, _) => "broken".to_string(),
        (None, true, _) => "passed".to_string(),
        (None, false, Some(step)) => step.to_string(),
        (None, false, None) => "failed".to_string(),
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
// Text formatting utilities
//

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
        let chars: Vec<char> = s.chars().collect();

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

/// Statistics for comparison table
#[derive(Debug, Clone)]
pub struct ComparisonStats {
    pub version_label: String,  // "Default" or version number
    pub total_tested: usize,
    pub already_broken: Option<usize>,  // Only for baseline
    pub passed_fetch: usize,
    pub passed_check: usize,
    pub passed_test: usize,
    pub fully_passing: usize,
    pub regressions: Vec<String>,  // List of "dependent:version" that regressed
}

/// Generate comparison table statistics
pub fn generate_comparison_table(rows: &[OfferedRow]) -> Vec<ComparisonStats> {
    use std::collections::{HashMap, HashSet};

    // First, collect baseline stats
    let baseline_rows: Vec<&OfferedRow> = rows.iter()
        .filter(|r| r.offered.is_none())
        .collect();

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

        let passed_fetch = row.test.commands.iter()
            .filter(|cmd| cmd.command == CommandType::Fetch)
            .all(|cmd| cmd.result.passed);

        let passed_check = row.test.commands.iter()
            .filter(|cmd| cmd.command == CommandType::Check || cmd.command == CommandType::Fetch)
            .all(|cmd| cmd.result.passed);

        let passed_test = row.test.commands.iter()
            .all(|cmd| cmd.result.passed);

        if !passed_check {
            baseline_stats.already_broken = Some(baseline_stats.already_broken.unwrap() + 1);
        } else {
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
    }

    let mut all_stats = vec![baseline_stats];

    // Group offered rows by version
    let mut by_version: HashMap<String, Vec<&OfferedRow>> = HashMap::new();
    for row in rows {
        if let Some(ref offered) = row.offered {
            by_version.entry(offered.version.clone())
                .or_insert_with(Vec::new)
                .push(row);
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
            already_broken: None,  // Don't show for offered versions
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

            let passed_fetch = row.test.commands.iter()
                .filter(|cmd| cmd.command == CommandType::Fetch)
                .all(|cmd| cmd.result.passed);

            let passed_check = row.test.commands.iter()
                .filter(|cmd| cmd.command == CommandType::Check || cmd.command == CommandType::Fetch)
                .all(|cmd| cmd.result.passed);

            let passed_test = row.test.commands.iter()
                .all(|cmd| cmd.result.passed);

            // Only count if not already broken in baseline
            let baseline_row = baseline_rows.iter()
                .find(|br| br.primary.dependent_name == *dep_name);

            let baseline_passed_check = baseline_row.map(|br| {
                br.test.commands.iter()
                    .filter(|cmd| cmd.command == CommandType::Check || cmd.command == CommandType::Fetch)
                    .all(|cmd| cmd.result.passed)
            }).unwrap_or(false);

            let baseline_passed_test = baseline_row.map(|br| {
                br.test.commands.iter().all(|cmd| cmd.result.passed)
            }).unwrap_or(false);

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
                    let baseline_version = baseline_row
                        .map(|br| br.primary.resolved_version.as_str())
                        .unwrap_or("?");
                    stats.regressions.push(format!("{} ({})", dep_name, baseline_version));
                }
            }
        }

        all_stats.push(stats);
    }

    all_stats
}

/// Print comparison table
pub fn print_comparison_table(stats_list: &[ComparisonStats]) {
    if stats_list.is_empty() {
        return;
    }

    println!("\nVersion Comparison:");

    // Print header
    print!("{:<26}", "");
    for stats in stats_list {
        print!("{:>16}", stats.version_label);
    }
    println!();
    println!("{}", "━".repeat(26 + stats_list.len() * 16));

    // Helper to print a row with baseline value only
    let print_simple = |label: &str, get_val: fn(&ComparisonStats) -> usize| {
        print!("{:<26}", label);
        for stats in stats_list {
            print!("{:>16}", get_val(stats));
        }
        println!();
    };

    // Helper to print a row with deltas
    let print_delta = |label: &str, get_val: fn(&ComparisonStats) -> usize| {
        print!("{:<26}", label);
        for (i, stats) in stats_list.iter().enumerate() {
            let val = get_val(stats);
            if i == 0 {
                print!("{:>16}", val);
            } else {
                let prev = get_val(&stats_list[i - 1]);
                let fixed = if val > prev { val - prev } else { 0 };
                let regressed = if val < prev { prev - val } else { 0 };
                let delta_str = match (fixed, regressed) {
                    (0, 0) => format!("{}", val),
                    (f, 0) => format!("+{} → {}", f, val),
                    (0, r) => format!("-{} → {}", r, val),
                    (f, r) => format!("+{} -{} → {}", f, r, val),
                };
                print!("{:>16}", delta_str);
            }
        }
        println!();
    };

    print_simple("Total tested", |s| s.total_tested);

    // Already broken (special case - shows "-" for non-baseline)
    print!("{:<26}", "Already broken");
    for stats in stats_list {
        print!("{:>16}", stats.already_broken.map_or("-".to_string(), |c| c.to_string()));
    }
    println!();

    println!("{}", "━".repeat(26 + stats_list.len() * 16));

    print_delta("Passed fetch", |s| s.passed_fetch);
    print_delta("Passed check", |s| s.passed_check);
    print_delta("Passed test", |s| s.passed_test);

    println!("{}", "━".repeat(26 + stats_list.len() * 16));

    print_delta("Fully passing", |s| s.fully_passing);
    println!();
}

//
// Temporary compatibility stubs for old API (TO BE REMOVED)
//

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
    for row in rows.iter() {
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
    let formatted = format_offered_row(row);
    let w = get_widths();

    let mut output = String::new();

    // Main row
    let offered_display = truncate_with_padding(&formatted.offered, w.offered - 2);
    let spec_display = truncate_with_padding(&formatted.spec, w.spec - 2);
    let resolved_display = truncate_with_padding(&formatted.resolved, w.resolved - 2);
    let dependent_display = truncate_with_padding(&formatted.dependent, w.dependent - 2);
    let result_display = format!("{:>12} {:>5}", formatted.result, formatted.time);
    let result_display = truncate_with_padding(&result_display, w.result - 2);

    output.push_str(&format!("│ {} │ {} │ {} │ {} │ {} │\n",
        offered_display, spec_display, resolved_display, dependent_display, result_display));

    // Error details (if any)
    if !formatted.error_details.is_empty() {
        let error_text_width = w.total - 1 - w.offered - 1 - 1 - 1 - 1;
        let corner1_width = w.spec;
        let corner2_width = w.dependent;
        let padding_width = w.spec + w.resolved + w.dependent - corner1_width - corner2_width;

        output.push_str(&format!("│{:w_offered$}├{:─<corner1$}┘{:padding$}└{:─<corner2$}┘{:w_result$}│\n",
            "", "", "", "", "",
            w_offered = w.offered, corner1 = corner1_width,
            padding = padding_width, corner2 = corner2_width, w_result = w.result));

        for error_line in &formatted.error_details {
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

    // Multi-version rows (└─ for last row)
    if !formatted.multi_version_rows.is_empty() {
        let last_idx = formatted.multi_version_rows.len() - 1;
        for (i, (spec, resolved, dependent)) in formatted.multi_version_rows.iter().enumerate() {
            let prefix = if i == last_idx { "└─" } else { "├─" };
            let spec_display = format!("{} {}", prefix, spec);
            let spec_display = truncate_with_padding(&spec_display, w.spec - 2);
            let resolved_display = format!("{} {}", prefix, resolved);
            let resolved_display = truncate_with_padding(&resolved_display, w.resolved - 2);
            let dependent_display = format!("{} {}", prefix, dependent);
            let dependent_display = truncate_with_padding(&dependent_display, w.dependent - 2);

            output.push_str(&format!("│{:width$}│ {} │ {} │ {} │{:w_result$}│\n",
                "", spec_display, resolved_display, dependent_display, "",
                width = w.offered, w_result = w.result));
        }
    }

    output
}
