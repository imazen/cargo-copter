//! Simple output format for AI-friendly, verbal test results.
//!
//! This module provides a simpler, more verbose output format that's
//! easier to read and parse programmatically. It's designed for use
//! with `--simple` flag.

use super::export::write_combined_log;
use super::types::DependentResults;
use crate::compile::PatchDepth;
use crate::types::{CommandType, OfferedRow};
use std::collections::{HashMap, HashSet};
use std::path::Path;

/// Print simple header for test run with list of all dependents.
pub fn print_simple_header(base_crate: &str, display_version: &str, dependents: &[String], base_versions: &[String]) {
    println!("Testing {}:{} against {} dependents", base_crate, display_version, dependents.len());
    println!();
    println!("Dependents: {}", dependents.join(", "));
    println!("Versions to test: {}", base_versions.join(", "));
    println!();
    println!("Markers: [!] forced  [!!] auto-patched  [!!!] deep conflict (see blocking deps)");
    println!();
}

/// Collect results for a dependent and print when complete.
pub fn print_simple_dependent_result(results: &DependentResults, base_crate: &str, _report_dir: &Path) {
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
    let baseline_test_passed = baseline_row.map(|r| r.test.commands.iter().all(|c| c.result.passed)).unwrap_or(false);

    // Get baseline version info for reporting
    let baseline_version = baseline_row.map(|r| r.primary.resolved_version.as_str()).unwrap_or("?");
    let baseline_spec = baseline_row.map(|r| r.primary.spec.as_str()).unwrap_or("?");

    // Analyze all offered versions
    let mut build_regressions: Vec<(&OfferedRow, &'static str)> = Vec::new();
    let mut test_regressions: Vec<&OfferedRow> = Vec::new();
    let mut passed_versions: Vec<String> = Vec::new();
    let mut still_broken: Vec<String> = Vec::new();

    // Track versions that needed special patching for explanations
    let mut patch_explanations: Vec<(String, PatchDepth)> = Vec::new();

    for row in &results.offered_versions {
        let version = row.offered.as_ref().map(|o| o.version.as_str()).unwrap_or("?");
        let patch_depth = row.offered.as_ref().map(|o| o.patch_depth).unwrap_or_default();
        let marker = patch_depth.marker();
        let version_display = if !marker.is_empty() {
            format!("{}:{} [{}]", base_crate, version, marker)
        } else {
            format!("{}:{}", base_crate, version)
        };
        let this_passed = row.test_passed();

        if this_passed {
            passed_versions.push(version_display.clone());
            // Track if patching was needed for explanation
            if patch_depth == PatchDepth::Patch || patch_depth == PatchDepth::DeepPatch {
                patch_explanations.push((version.to_string(), patch_depth));
            }
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
            let patch_depth = row.offered.as_ref().map(|o| o.patch_depth).unwrap_or_default();
            let marker = patch_depth.marker();
            let depth_marker = if !marker.is_empty() { format!(" [{}]", marker) } else { String::new() };

            let baseline_info = format!("{}:{} ({})", base_crate, baseline_version, baseline_spec);
            let baseline_note = if baseline_test_passed {
                format!("baseline {} passed", baseline_info)
            } else {
                format!("baseline {} built, tests failed", baseline_info)
            };

            println!(
                "REGRESSION: {} with {}:{}{} - {} failed ({})",
                dep, base_crate, version, depth_marker, step, baseline_note
            );
            // Print first error line
            if let Some(error) = first_error_line(row) {
                println!("  {}", error);
            }
            // Explain what patching was attempted for [!!] cases
            if patch_depth == PatchDepth::Patch {
                println!(
                    "  [!!] Tried [patch.crates-io] to unify transitive {} versions, but build still failed",
                    base_crate
                );
            }
            // Print blocking crates advice for !!! cases
            print_blocking_crates_advice(row, base_crate, version);
        }
    }

    // Test regressions (less critical than build regressions)
    if !test_regressions.is_empty() {
        for row in &test_regressions {
            let version = row.offered.as_ref().map(|o| o.version.as_str()).unwrap_or("?");
            let patch_depth = row.offered.as_ref().map(|o| o.patch_depth).unwrap_or_default();
            let marker = patch_depth.marker();
            let depth_marker = if !marker.is_empty() { format!(" [{}]", marker) } else { String::new() };

            let baseline_info = format!("{}:{} ({})", base_crate, baseline_version, baseline_spec);
            println!(
                "REGRESSION: {} with {}:{}{} - tests failed (baseline {} passed)",
                dep, base_crate, version, depth_marker, baseline_info
            );
            // Print first error line
            if let Some(error) = first_error_line(row) {
                println!("  {}", error);
            }
            // Explain what patching was attempted for [!!] cases
            if patch_depth == PatchDepth::Patch {
                println!(
                    "  [!!] Used [patch.crates-io] to unify transitive {} versions, but tests still failed",
                    base_crate
                );
            }
            // Print blocking crates advice for !!! cases
            print_blocking_crates_advice(row, base_crate, version);
        }
    }

    // Report passed versions
    if !passed_versions.is_empty() {
        println!("OK: {} - passed with {}", dep, passed_versions.join(", "));
        // Explain what patching was needed for [!!] and [!!!] cases
        for (version, depth) in &patch_explanations {
            match depth {
                PatchDepth::Patch => {
                    println!(
                        "  [!!] {}:{} needed [patch.crates-io] to unify transitive {} versions",
                        base_crate, version, base_crate
                    );
                }
                PatchDepth::DeepPatch => {
                    println!(
                        "  [!!!] {}:{} has deep transitive conflicts even with [patch.crates-io]",
                        base_crate, version
                    );
                }
                _ => {}
            }
        }
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

/// Get a human-readable description of the first failed step.
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

/// Get the first error line from a failed row (for --simple output).
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
                        let display =
                            if trimmed.len() > 100 { format!("{}...", &trimmed[..100]) } else { trimmed.to_string() };
                        return Some(display);
                    }
                }
            }
        }
    }
    None
}

/// Print blocking crates advice for DeepPatch (!!!) cases.
///
/// Shows which transitive dependencies are preventing version unification.
fn print_blocking_crates_advice(row: &OfferedRow, base_crate: &str, base_version: &str) {
    // Check if this is a DeepPatch case
    let patch_depth = row.offered.as_ref().map(|o| o.patch_depth).unwrap_or_default();
    if patch_depth != PatchDepth::DeepPatch {
        return;
    }

    // Extract blocking crates from transitive deps with conflicting specs
    if row.transitive.is_empty() {
        // Try to extract from error message if no transitive data
        for cmd in &row.test.commands {
            if !cmd.result.passed {
                for failure in &cmd.result.failures {
                    // Look for "two different versions of crate X" pattern
                    if failure.error_message.contains("two different versions of crate")
                        || failure.error_message.contains("multiple different versions of crate")
                    {
                        // Extract major.minor for recommendation
                        let parts: Vec<&str> = base_version.split('.').collect();
                        let major_minor = if parts.len() >= 2 {
                            format!("{}.{}", parts[0], parts[1])
                        } else {
                            base_version.to_string()
                        };

                        println!("  BLOCKING TRANSITIVE DEPS (need semver-compatible {} specs):", base_crate);
                        println!("    Recommend: Change restrictive specs (like =X.Y.Z) to ^{}", major_minor);
                        println!("    For forward compat: Use >={} instead of exact version pins", major_minor);
                        return;
                    }
                }
            }
        }
        return;
    }

    // We have transitive dep info - show which crates are blocking
    println!("  BLOCKING TRANSITIVE DEPS:");
    for transitive in &row.transitive {
        let spec = &transitive.dependency.spec;
        let resolved = &transitive.dependency.resolved_version;
        let dep_name = &transitive.dependency.dependent_name;

        // Check if this spec is restrictive (exact pin, incompatible range)
        let is_restrictive = spec.starts_with('=') || spec.starts_with('<');

        if is_restrictive || transitive.dependency.resolved_version != base_version {
            // Extract major.minor from base_version
            let parts: Vec<&str> = base_version.split('.').collect();
            let major_minor =
                if parts.len() >= 2 { format!("{}.{}", parts[0], parts[1]) } else { base_version.to_string() };

            let recommendation = if spec.starts_with('=') {
                format!("^{} (for backward compat) or >={} (for forward compat)", major_minor, major_minor)
            } else if spec.starts_with('~') {
                format!("^{} (allows more flexibility)", major_minor)
            } else if spec.starts_with('<') {
                format!(">={} with adjusted upper bound", major_minor)
            } else {
                format!("^{}", major_minor)
            };

            println!(
                "    {} requires {} {} → resolved {} (recommend: {})",
                dep_name, base_crate, spec, resolved, recommendation
            );
        }
    }
}

/// Print simple summary at end.
pub fn print_simple_summary(rows: &[OfferedRow], report_dir: &Path, base_crate: &str, combined_log_path: &Path) {
    // Group results by version
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
                failed_step == "fetch" || failed_step == "build"
            } else {
                false // Not a regression, baseline was already broken at this step
            };

            if is_regression {
                entry.0.push(dep); // regressed
            }
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
