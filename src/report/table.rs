//! Table output formatting for test results.
//!
//! This module handles printing test results in a tabular format,
//! including error boxes and multi-version rows.

use super::types::{FormattedRow, format_offered_row};
use crate::console_format::{self, ComparisonStats};
use crate::types::OfferedRow;
use std::collections::BTreeSet;

//
// Public API: Delegate to console_format module
//

/// Initialize table widths based on versions being tested.
pub fn init_table_widths(versions: &[String], display_version: &str, force_versions: bool) {
    console_format::init_table_widths(versions, display_version, force_versions);
}

/// Print table header.
pub fn print_table_header(
    crate_name: &str,
    display_version: &str,
    total_deps: usize,
    test_plan: Option<&str>,
    this_path: Option<&str>,
) {
    console_format::print_table_header(crate_name, display_version, total_deps, test_plan, this_path);
}

/// Format table header as a string.
pub fn format_table_header(
    crate_name: &str,
    display_version: &str,
    total_deps: usize,
    test_plan: Option<&str>,
    this_path: Option<&str>,
) -> String {
    console_format::format_table_header(crate_name, display_version, total_deps, test_plan, this_path)
}

/// Print separator line between dependents.
pub fn print_separator_line() {
    console_format::print_separator_line();
}

/// Format table footer as a string.
pub fn format_table_footer() -> String {
    console_format::format_table_footer()
}

/// Print table footer.
pub fn print_table_footer() {
    console_format::print_table_footer();
}

/// Print comparison table.
pub fn print_comparison_table(stats_list: &[ComparisonStats]) {
    console_format::print_comparison_table(stats_list);
}

//
// Error signature extraction for deduplication
//

/// Extract error signature for comparison - normalizes line numbers and sorts errors.
///
/// This creates a canonical representation of errors for comparison,
/// so we can detect when two test runs have the same errors.
pub fn error_signature(text: &str) -> String {
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

/// Extract error text from an OfferedRow for deduplication.
pub fn extract_error_text(row: &OfferedRow) -> Option<String> {
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

/// Print an OfferedRow using the standard table format.
///
/// # Arguments
/// * `row` - The row to print
/// * `is_last_in_group` - Whether this is the last row in a dependent group
/// * `prev_error` - Previous error signature for deduplication
/// * `max_error_lines` - Maximum error lines to show (0 = unlimited)
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
        let full_formatted = format_offered_row(row, 0);
        let current_error = full_formatted.error_details.join("\n");
        // Use error signature for robust comparison
        let current_signature = error_signature(&current_error);
        if current_signature == prev {
            // Clear error details and update result to show "same failure"
            formatted.error_details.clear();
            // Replace the failure type with "same failure", keeping ICT marks
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

/// Format an OfferedRow as a string (similar to print_offered_row but returns String).
///
/// Used for markdown export where we need the string output.
pub fn format_offered_row_string(row: &OfferedRow, is_last_in_group: bool) -> String {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_signature_empty() {
        assert_eq!(error_signature(""), "");
    }

    #[test]
    fn test_error_signature_with_errors() {
        let text = "error[E0432]: unresolved import `foo`\nerror[E0433]: failed to resolve";
        let sig = error_signature(text);
        // Should extract both error codes
        assert!(sig.contains("error[E0432]:"));
        assert!(sig.contains("error[E0433]:"));
    }

    #[test]
    fn test_error_signature_deduplicates() {
        let text = "error[E0432]: unresolved import\nerror[E0432]: unresolved import";
        let sig = error_signature(text);
        // Should only appear once
        assert_eq!(sig.matches("E0432").count(), 1);
    }
}
