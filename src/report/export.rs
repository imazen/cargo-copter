//! Report export functions for JSON and Markdown formats.
//!
//! This module handles exporting test results to various file formats
//! for storage and analysis.

use super::stats::{generate_comparison_table, summarize_offered_rows};
use super::table::format_offered_row_string;
use super::types::format_offered_row;
use crate::compile::CompileResult;
use crate::console_format::{self, TableWriter};
use crate::types::{OfferedRow, TestResult};
use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};

/// Export test results as JSON.
///
/// Creates a comprehensive JSON report including summary statistics,
/// comparison stats, and all test results.
///
/// # Arguments
/// * `rows` - All test result rows
/// * `output_path` - Path to write the JSON file
/// * `crate_name` - Name of the base crate being tested
/// * `display_version` - Version string to display
/// * `total_deps` - Total number of dependents tested
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

/// Export test results as Markdown with console table in code block.
///
/// Creates a Markdown report that includes:
/// - Header with crate info
/// - Summary statistics
/// - Full console table in a code block
/// - Comparison table
///
/// # Arguments
/// * `rows` - All test result rows
/// * `output_path` - Path to write the Markdown file
/// * `crate_name` - Name of the base crate being tested
/// * `display_version` - Version string to display
/// * `total_deps` - Total number of dependents tested
/// * `test_plan` - Optional test plan description
/// * `this_path` - Optional path to local crate
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

    // Write table header
    write!(
        file,
        "{}",
        console_format::format_table_header(crate_name, display_version, total_deps, test_plan, this_path)
    )?;

    // Write all rows
    for row in rows.iter() {
        // For simplicity, assume each row is its own group (no separators in markdown)
        let is_last_in_group = true;
        write!(file, "{}", format_offered_row_string(row, is_last_in_group))?;
    }

    // Write table footer
    write!(file, "{}", console_format::format_table_footer())?;

    // Generate and write comparison table using TableWriter
    let comparison_stats = generate_comparison_table(rows);
    let mut table_writer = TableWriter::new(&mut file, false); // No colors for markdown
    table_writer.write_comparison_table(&comparison_stats)?;

    writeln!(file, "```\n")?;

    Ok(())
}

/// Write raw cargo output to a failure log file.
///
/// Creates a detailed failure log for a single test result,
/// including all error output from failed steps.
///
/// # Arguments
/// * `report_dir` - Directory to write the log file
/// * `staging_dir` - Staging directory containing the test source
/// * `result` - The test result to log
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
    fn write_step_output(content: &mut String, result: &CompileResult, step_name: &str) {
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

/// Write combined log file with all failures.
///
/// Creates a single log file containing all failures from a test run.
///
/// # Arguments
/// * `report_dir` - Directory to write the log file
/// * `rows` - All test result rows
/// * `base_crate` - Name of the base crate being tested
///
/// # Returns
/// Path to the combined log file.
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
