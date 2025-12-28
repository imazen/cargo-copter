/// Bridge module - converts between new TestResult and old OfferedRow formats
///
/// This allows us to use the new architecture while keeping existing report generation
/// working during the migration period.
use crate::types::*;

/// Convert TestResult to OfferedRow for existing report generation
pub fn test_result_to_offered_row(result: &TestResult) -> OfferedRow {
    // Determine if this is a baseline row
    let is_baseline = result.baseline.is_none();

    // Extract version strings
    let base_version_str = result.base_version.version.display();
    let dependent_version_str = result.dependent.version.display();

    // Create primary DependencyRef
    // Use "?" for broken packages where spec extraction fails (e.g., fetch fails due to yanked deps)
    let spec = result.execution.original_requirement.clone().unwrap_or_else(|| "?".to_string());

    let primary = DependencyRef {
        dependent_name: result.dependent.name.clone(),
        dependent_version: dependent_version_str.clone(),
        spec,
        resolved_version: result.execution.actual_version.clone().unwrap_or_else(|| base_version_str.clone()),
        resolved_source: match result.base_version.source {
            CrateSource::Registry => VersionSource::CratesIo,
            CrateSource::Local { .. } => VersionSource::Local,
            CrateSource::Git { .. } => VersionSource::Git,
        },
        used_offered_version: result
            .execution
            .actual_version
            .as_ref()
            .map(|actual| actual == &base_version_str)
            .unwrap_or(false),
    };

    // Create offered version (None for baseline)
    let offered = if is_baseline {
        None
    } else {
        Some(OfferedVersion {
            version: base_version_str.clone(),
            forced: result.execution.forced_version,
            patch_depth: result.execution.patch_depth,
        })
    };

    // Get baseline_passed (None for baseline itself)
    let baseline_passed = result.baseline.as_ref().map(|b| b.baseline_passed);

    // Convert ThreeStepResult to TestExecution
    let test = TestExecution { commands: three_step_to_commands(&result.execution) };

    // Convert transitive dependencies
    let transitive = result
        .execution
        .all_crate_versions
        .iter()
        .map(|(spec, resolved, dep_name)| TransitiveTest {
            dependency: DependencyRef {
                dependent_name: dep_name.clone(),
                dependent_version: "?".to_string(), // Not available in TestResult
                spec: spec.clone(),
                resolved_version: resolved.clone(),
                resolved_source: VersionSource::CratesIo, // Assume registry for transitives
                used_offered_version: false,
            },
            depth: 1, // Assume depth 1 for all transitives
        })
        .collect();

    let row = OfferedRow { baseline_passed, primary, offered, test, transitive };

    // INVARIANT: Baseline rows have offered=None and baseline_passed=None
    // Non-baseline rows have offered=Some and baseline_passed=Some
    debug_assert!(
        (row.offered.is_none() && row.baseline_passed.is_none())
            || (row.offered.is_some() && row.baseline_passed.is_some()),
        "Invariant violated: offered/baseline_passed inconsistency. \
         offered.is_some={}, baseline_passed.is_some={}",
        row.offered.is_some(),
        row.baseline_passed.is_some()
    );

    row
}

/// Convert ThreeStepResult to TestCommand list
fn three_step_to_commands(result: &crate::compile::ThreeStepResult) -> Vec<TestCommand> {
    let mut commands = Vec::new();

    // Fetch step
    commands.push(TestCommand {
        command: CommandType::Fetch,
        features: vec![],
        result: CommandResult {
            passed: result.fetch.success,
            duration: result.fetch.duration.as_secs_f64(),
            failures: compile_result_to_failures(&result.fetch),
        },
    });

    // Check step (if present)
    if let Some(ref check) = result.check {
        commands.push(TestCommand {
            command: CommandType::Check,
            features: vec![],
            result: CommandResult {
                passed: check.success,
                duration: check.duration.as_secs_f64(),
                failures: compile_result_to_failures(check),
            },
        });
    }

    // Test step (if present)
    if let Some(ref test) = result.test {
        commands.push(TestCommand {
            command: CommandType::Test,
            features: vec![],
            result: CommandResult {
                passed: test.success,
                duration: test.duration.as_secs_f64(),
                failures: compile_result_to_failures(test),
            },
        });
    }

    commands
}

/// Convert CompileResult to CrateFailure list
fn compile_result_to_failures(result: &crate::compile::CompileResult) -> Vec<CrateFailure> {
    if result.success {
        vec![]
    } else {
        vec![CrateFailure {
            crate_name: "dependent".to_string(), // Generic - actual name in context
            error_message: extract_error_with_fallback(&result.diagnostics, &result.stderr, 0),
        }]
    }
}

#[cfg(test)]
#[path = "bridge_test.rs"]
mod bridge_test;
