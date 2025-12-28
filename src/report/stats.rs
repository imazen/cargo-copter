//! Statistics and summary generation for test results.
//!
//! This module handles aggregating test results into summary statistics
//! and comparison tables.

use super::types::TestSummary;
use crate::console_format::ComparisonStats;
use crate::types::{CommandType, OfferedRow};
use std::collections::{HashMap, HashSet};

/// Calculate summary statistics from OfferedRows.
///
/// # Arguments
/// * `rows` - All test result rows
///
/// # Returns
/// A `TestSummary` with counts of passed, regressed, and broken tests.
pub fn summarize_offered_rows(rows: &[OfferedRow]) -> TestSummary {
    let mut passed = 0;
    let mut regressed = 0;
    let mut broken = 0;

    for row in rows {
        // Only count non-baseline rows
        if row.offered.is_some() {
            let overall_passed = row.test.commands.iter().all(|cmd| cmd.result.passed);

            // Use baseline_check_passed to determine if truly broken (check fails)
            // Only consider baseline "broken" if check fails - test failures don't count
            let baseline_compiles = row
                .baseline_check_passed
                .unwrap_or_else(|| row.baseline_passed.unwrap_or(false));

            match (baseline_compiles, row.baseline_passed, overall_passed) {
                // Baseline doesn't compile = truly broken
                (false, _, _) => broken += 1,
                // Baseline compiles AND passed overall, this passed = PASSED
                (true, Some(true), true) => passed += 1,
                // Baseline compiles AND passed overall, this failed = REGRESSED
                (true, Some(true), false) => regressed += 1,
                // Baseline compiles but test failed, this passed = PASSED
                (true, Some(false), true) => passed += 1,
                // Baseline compiles but test failed, this also failed = not a regression
                // (test was already failing, count as passed since compilation works)
                (true, Some(false), false) => passed += 1,
                // No baseline comparison data, this passed = PASSED
                (true, None, true) => passed += 1,
                // No baseline comparison data, this failed = BROKEN (can't determine)
                (true, None, false) => broken += 1,
            }
        }
    }

    TestSummary { passed, regressed, broken, total: passed + regressed + broken }
}

/// Generate comparison table statistics.
///
/// Creates statistics for each tested version, showing how many
/// dependents passed at each step (fetch, check, test).
///
/// # Arguments
/// * `rows` - All test result rows
///
/// # Returns
/// A list of `ComparisonStats` for the baseline and each tested version.
pub fn generate_comparison_table(rows: &[OfferedRow]) -> Vec<ComparisonStats> {
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

#[cfg(test)]
mod tests {
    use super::*;

    // Note: These tests require constructing OfferedRow which is complex.
    // In a real test, we'd use fixtures or mocks.

    #[test]
    fn test_empty_summary() {
        let summary = summarize_offered_rows(&[]);
        assert_eq!(summary.passed, 0);
        assert_eq!(summary.regressed, 0);
        assert_eq!(summary.broken, 0);
        assert_eq!(summary.total, 0);
    }

    #[test]
    fn test_empty_comparison_table() {
        let stats = generate_comparison_table(&[]);
        // Should have at least baseline stats
        assert!(!stats.is_empty());
        assert_eq!(stats[0].version_label, "Default");
        assert_eq!(stats[0].total_tested, 0);
    }
}
