// Copyright 2015 The Rust Project Developers. See the COPYRIGHT
// file at the top-level directory of this distribution and at
// http://rust-lang.org/COPYRIGHT.
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

mod api;
mod bridge;
mod cli;
mod compile;
mod config;
mod console_format;
mod docker;
mod download;
mod error_extract;
mod git;
mod manifest;
mod metadata;
mod report;
mod runner;
mod types;
mod ui;
mod version;

use std::fs;
use std::path::PathBuf;
use types::*;

fn main() {
    env_logger::init();

    // Parse CLI arguments
    let args = cli::CliArgs::parse_args();

    // Validate arguments
    if let Err(e) = args.validate() {
        ui::print_error(&e);
        std::process::exit(1);
    }

    // Clean staging directory if requested
    if args.clean {
        let staging_dir = args.get_staging_dir();
        if staging_dir.exists() {
            match fs::remove_dir_all(&staging_dir) {
                Ok(_) => {
                    println!("Cleaned staging directory: {}", staging_dir.display());
                }
                Err(e) => {
                    eprintln!("Warning: Failed to clean staging directory: {}", e);
                }
            }
        }
    }

    // Set console width override if specified (for testing)
    if let Some(width) = args.console_width {
        console_format::set_console_width(width);
    }

    // Build test matrix
    let matrix = match config::build_test_matrix(&args) {
        Ok(m) => m,
        Err(e) => {
            ui::print_error(&format!("Configuration error: {}", e));
            std::process::exit(1);
        }
    };

    // Initialize table widths for console output
    let version_strs: Vec<String> =
        matrix.base_versions.iter().map(|v| v.crate_ref.version.display()).collect();
    let display_version = version_strs.first().map(|s| s.as_str()).unwrap_or("unknown");
    let force_versions = matrix.base_versions.iter().any(|v| v.override_mode == OverrideMode::Force);
    report::init_table_widths(&version_strs, display_version, force_versions);

    // Print test plan and table header before streaming results
    let test_plan = format_test_plan_string(&matrix);
    let this_path = matrix.base_versions.iter().find_map(|v| match &v.crate_ref.source {
        CrateSource::Local { path } => Some(path.display().to_string()),
        _ => None,
    });
    report::print_table_header(
        &matrix.base_crate,
        display_version,
        matrix.dependents.len(),
        Some(&test_plan),
        this_path.as_deref(),
    );

    // Run tests with streaming output
    let mut offered_rows = Vec::new();
    let mut prev_dependent: Option<String> = None;
    let mut prev_error: Option<String> = None;

    let _test_results = match runner::run_tests(matrix.clone(), |result| {
        // Convert to OfferedRow immediately
        let row = bridge::test_result_to_offered_row(result);

        // Print separator between different dependents
        if let Some(ref prev) = prev_dependent {
            if *prev != row.primary.dependent_name {
                report::print_separator_line();
            }
        }

        // Determine if this is the last row for this dependent
        // (We can't know this in streaming mode, so always pass false)
        let is_last = false;

        // Print the row immediately
        report::print_offered_row(&row, is_last, prev_error.as_deref(), args.error_lines);

        // Update tracking
        prev_error = report::extract_error_text(&row);
        prev_dependent = Some(row.primary.dependent_name.clone());

        // Save for later report generation
        offered_rows.push(row);
    }) {
        Ok(results) => results,
        Err(e) => {
            ui::print_error(&format!("Test execution failed: {}", e));
            std::process::exit(1);
        }
    };

    // Print table footer after all rows
    report::print_table_footer();

    // Generate non-console reports (markdown, JSON)
    generate_non_console_reports(&offered_rows, &args, &matrix);

    // If using top-dependents and there were failures, suggest a targeted re-test
    if args.dependents.is_empty() && args.dependent_paths.is_empty() {
        suggest_failed_retest(&offered_rows, &args, &matrix);
    }

    // Determine exit code
    let summary = report::summarize_offered_rows(&offered_rows);
    let exit_code = if summary.regressed > 0 { -2 } else { 0 };

    std::process::exit(exit_code);
}

/// Print test plan showing what will be tested
fn print_test_plan(matrix: &TestMatrix, args: &cli::CliArgs) {
    let deps_display: Vec<String> = matrix
        .dependents
        .iter()
        .take(5)
        .map(|d| {
            let version = d.crate_ref.version.display();
            if version == "latest" {
                d.crate_ref.name.clone()
            } else {
                format!("{}:{}", d.crate_ref.name, version)
            }
        })
        .collect();

    let more_deps = if matrix.dependents.len() > 5 {
        format!(" ... and {} more", matrix.dependents.len() - 5)
    } else {
        String::new()
    };

    let mut versions_display = vec!["baseline".to_string()];
    for version_spec in &matrix.base_versions {
        let version_str = version_spec.crate_ref.version.display();
        if version_spec.override_mode == OverrideMode::Force {
            versions_display.push(format!("{} [!]", version_str));
        } else {
            versions_display.push(version_str);
        }
    }

    let test_plan = format!(
        "  Dependents: {}{}\n  Versions:   {}",
        deps_display.join(", "),
        more_deps,
        versions_display.join(", ")
    );

    // Determine source path for display
    let this_path = matrix
        .base_versions
        .iter()
        .find_map(|v| match &v.crate_ref.source {
            CrateSource::Local { path } => Some(path.display().to_string()),
            _ => None,
        });

    // Just print the test plan summary (table header printed separately during streaming)
    println!("Testing {} reverse dependencies of {}", matrix.dependents.len(), matrix.base_crate);
    println!("{}", test_plan);
    if let Some(path) = this_path {
        println!("  this = {} (your work-in-progress version)", path);
    }
    println!();
}

/// Generate non-console reports (markdown, JSON) and comparison table
fn generate_non_console_reports(rows: &[OfferedRow], args: &cli::CliArgs, matrix: &TestMatrix) {
    // Print comparison table
    let comparison_stats = report::generate_comparison_table(rows);
    report::print_comparison_table(&comparison_stats);

    // Export markdown report
    let markdown_path = PathBuf::from("copter-report.md");
    let test_plan = format_test_plan_string(matrix);
    let this_path = matrix
        .base_versions
        .iter()
        .find_map(|v| match &v.crate_ref.source {
            CrateSource::Local { path } => Some(path.display().to_string()),
            _ => None,
        });

    match report::export_markdown_table_report(
        rows,
        &markdown_path,
        &matrix.base_crate,
        &matrix.base_versions.first().map(|v| v.crate_ref.version.display()).unwrap_or_else(|| "unknown".to_string()),
        matrix.dependents.len(),
        Some(&test_plan),
        this_path.as_deref(),
    ) {
        Ok(_) => println!("\nMarkdown report saved to: {}", markdown_path.display()),
        Err(e) => eprintln!("Warning: Failed to save markdown report: {}", e),
    }

    // Export JSON report
    let json_path = PathBuf::from("copter-report.json");
    match report::export_json_report(
        rows,
        &json_path,
        &matrix.base_crate,
        &matrix.base_versions.first().map(|v| v.crate_ref.version.display()).unwrap_or_else(|| "unknown".to_string()),
        matrix.dependents.len(),
    ) {
        Ok(_) => println!("JSON report saved to: {}", json_path.display()),
        Err(e) => eprintln!("Warning: Failed to save JSON report: {}", e),
    }

    // Print summary
    let summary = report::summarize_offered_rows(rows);
    println!("\n=== Summary ===");
    println!("✓ Passed:    {}", summary.passed);
    println!("✗ Regressed: {}", summary.regressed);
    println!("⚠ Broken:    {}", summary.broken);
    println!("Total:       {}", summary.total);
}

/// Format test plan as a string
fn format_test_plan_string(matrix: &TestMatrix) -> String {
    let deps_display: Vec<String> = matrix
        .dependents
        .iter()
        .take(5)
        .map(|d| {
            let version = d.crate_ref.version.display();
            if version == "latest" {
                d.crate_ref.name.clone()
            } else {
                format!("{}:{}", d.crate_ref.name, version)
            }
        })
        .collect();

    let more_deps = if matrix.dependents.len() > 5 {
        format!(" ... and {} more", matrix.dependents.len() - 5)
    } else {
        String::new()
    };

    let mut versions_display = vec!["baseline".to_string()];
    for version_spec in &matrix.base_versions {
        let version_str = version_spec.crate_ref.version.display();
        if version_spec.override_mode == OverrideMode::Force {
            versions_display.push(format!("{} [!]", version_str));
        } else {
            versions_display.push(version_str);
        }
    }

    format!(
        "  Dependents: {}{}\n  Versions:   {}",
        deps_display.join(", "),
        more_deps,
        versions_display.join(", ")
    )
}

/// Suggest a command to re-test only the failed dependents
fn suggest_failed_retest(rows: &[OfferedRow], args: &cli::CliArgs, matrix: &TestMatrix) {
    // Collect dependents that had any failures
    let mut failed_dependents: std::collections::HashSet<String> = std::collections::HashSet::new();

    for row in rows {
        // Check if this dependent had any failures (regression or baseline failed)
        let failed = match row.baseline_passed {
            Some(true) => !row.test.all_passed(), // Regression: baseline passed, offered failed
            Some(false) => true,                   // Baseline already broken
            None => !row.test.all_passed(),       // This IS the baseline and it failed
        };

        if failed {
            failed_dependents.insert(row.primary.dependent_name.clone());
        }
    }

    // If there are failures and some passed, suggest a focused re-test
    if !failed_dependents.is_empty() && failed_dependents.len() < matrix.dependents.len() {
        println!("\n━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
        println!("💡 To re-test only the {} failed dependent(s):", failed_dependents.len());
        println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━\n");

        // Build the command
        let mut cmd = String::from("cargo-copter");

        // Add path or crate argument
        if let Some(ref path) = args.path {
            cmd.push_str(&format!(" --path {}", path.display()));
        } else if let Some(ref crate_name) = args.crate_name {
            cmd.push_str(&format!(" --crate {}", crate_name));
        }

        // Add test-versions if specified
        if !args.test_versions.is_empty() {
            cmd.push_str(" --test-versions");
            for v in &args.test_versions {
                cmd.push_str(&format!(" {}", v));
            }
        }

        // Add force-versions if specified
        if !args.force_versions.is_empty() {
            cmd.push_str(" --force-versions");
            for v in &args.force_versions {
                cmd.push_str(&format!(" {}", v));
            }
        }

        // Add the failed dependents
        cmd.push_str(" --dependents");
        let mut sorted_failed: Vec<_> = failed_dependents.iter().collect();
        sorted_failed.sort();
        for dep in sorted_failed {
            cmd.push_str(&format!(" {}", dep));
        }

        // Add other relevant flags
        if args.skip_normal_testing {
            cmd.push_str(" --skip-normal-testing");
        }
        if args.error_lines != 10 {
            cmd.push_str(&format!(" --error-lines {}", args.error_lines));
        }

        println!("  {}\n", cmd);
    }
}
