use crate::compile;
use crate::download;
use crate::types::*;
use crate::ui;
use crate::version;
use log::debug;
use semver::Version as SemverVersion;

/// Run all tests specified in the matrix
///
/// This is the main entry point for test execution.
/// The callback is invoked for each completed test result.
pub fn run_tests<F>(mut matrix: TestMatrix, mut on_result: F) -> Result<Vec<TestResult>, String>
where
    F: FnMut(&TestResult),
{
    debug!("Starting test execution for {} test pairs", matrix.test_count());

    // Step 1: Resolve base version Latest entries (just a few, so do upfront)
    for base_spec in &mut matrix.base_versions {
        if let Version::Latest = base_spec.crate_ref.version {
            let latest = version::resolve_latest_version(&base_spec.crate_ref.name, false)
                .map_err(|e| format!("Failed to resolve latest version for {}: {}", base_spec.crate_ref.name, e))?;
            base_spec.crate_ref.version = Version::Semver(latest);
        }
    }

    // Step 2: Execute all test pairs
    // IMPORTANT: Must iterate dependents × base_versions (outer × inner)
    // This ensures baseline is tested first for each dependent
    let mut results = Vec::new();

    // Use indices to allow lazy resolution per dependent (enables streaming)
    for idx in 0..matrix.dependents.len() {
        // Resolve this specific dependent's version lazily (just before testing it)
        if let Version::Latest = matrix.dependents[idx].crate_ref.version {
            let name = matrix.dependents[idx].crate_ref.name.clone();
            let latest = version::resolve_latest_version(&name, false)
                .map_err(|e| format!("Failed to resolve latest version for {}: {}", name, e))?;
            matrix.dependents[idx].crate_ref.version = Version::Semver(latest);
        }

        let dependent_spec = &matrix.dependents[idx];
        // Get the dependent version (now guaranteed to be resolved)
        let dependent = &dependent_spec.crate_ref;

        // Test baseline first, then other versions
        let baseline_result = {
            let baseline_spec = matrix
                .base_versions
                .iter()
                .find(|v| v.is_baseline)
                .ok_or_else(|| "No baseline version found".to_string())?;

            debug!("Testing BASELINE {} against {}", baseline_spec.crate_ref.display(), dependent.display());

            let execution = run_single_test(baseline_spec, dependent_spec, &matrix)?;
            TestResult {
                base_version: baseline_spec.crate_ref.clone(),
                dependent: dependent.clone(),
                execution,
                baseline: None, // Baseline has no comparison
            }
        };

        let baseline_passed = baseline_result.execution.is_success();

        // Extract the spec from baseline for use in offered version tests
        let baseline_spec_requirement = baseline_result.execution.original_requirement.clone();

        on_result(&baseline_result); // Stream the result immediately
        results.push(baseline_result);

        // Then test other versions
        for base_spec in matrix.base_versions.iter().filter(|v| !v.is_baseline) {
            let base_version = &base_spec.crate_ref;

            debug!("Testing {} against {}", base_version.display(), dependent.display());

            // Run the three-step test, passing the baseline spec requirement
            let execution =
                run_single_test_with_spec(base_spec, dependent_spec, &matrix, baseline_spec_requirement.clone())?;

            let result = TestResult {
                base_version: base_version.clone(),
                dependent: dependent.clone(),
                execution,
                baseline: Some(BaselineComparison {
                    baseline_passed,
                    baseline_version: matrix
                        .base_versions
                        .iter()
                        .find(|v| v.is_baseline)
                        .map(|v| v.crate_ref.version.display())
                        .unwrap_or_else(|| "unknown".to_string()),
                }),
            };
            on_result(&result); // Stream the result immediately
            results.push(result);
        }
    }

    Ok(results)
}

/// Run a single test: one (base_version, dependent) pair
fn run_single_test(
    base_spec: &VersionSpec,
    dependent_spec: &VersionSpec,
    matrix: &TestMatrix,
) -> Result<compile::ThreeStepResult, String> {
    run_single_test_with_spec(base_spec, dependent_spec, matrix, None)
}

/// Run a single test with an optional pre-extracted spec requirement
fn run_single_test_with_spec(
    base_spec: &VersionSpec,
    dependent_spec: &VersionSpec,
    matrix: &TestMatrix,
    original_requirement: Option<String>,
) -> Result<compile::ThreeStepResult, String> {
    let base_version = &base_spec.crate_ref;
    let dependent = &dependent_spec.crate_ref;

    // Get version strings
    let base_version_str = match &base_version.version {
        Version::Semver(v) => v.clone(),
        _ => return Err("Version not resolved".to_string()),
    };

    let dependent_version_str = match &dependent.version {
        Version::Semver(v) => v.clone(),
        _ => return Err("Dependent version not resolved".to_string()),
    };

    // Get dependent path or download it
    let dependent_path = match &dependent.source {
        CrateSource::Local { path } => path.clone(),
        CrateSource::Registry => {
            // Download and unpack
            let vers = SemverVersion::parse(&dependent_version_str).map_err(|e| format!("Invalid semver: {}", e))?;
            let crate_handle = download::get_crate_handle(&dependent.name, &vers)
                .map_err(|e| format!("Failed to download {}: {}", dependent.name, e))?;

            let dest = matrix.staging_dir.join(format!("{}-{}", dependent.name, dependent_version_str));
            if !dest.exists() {
                std::fs::create_dir_all(&dest).map_err(|e| format!("Failed to create staging dir: {}", e))?;
                crate_handle
                    .unpack_source_to(&dest)
                    .map_err(|e| format!("Failed to unpack {}: {}", dependent.name, e))?;
            }

            dest
        }
        CrateSource::Git { .. } => {
            return Err("Git sources not yet implemented".to_string());
        }
    };

    // Build the TestConfig using the builder pattern
    let test_config = compile::TestConfig::new(dependent_path.as_path(), &matrix.base_crate)
        .with_skip_flags(matrix.skip_check, matrix.skip_test)
        .with_version_info(
            Some(base_version_str.clone()),
            base_spec.override_mode == OverrideMode::Force,
            original_requirement, // Use provided spec from baseline test (if any)
        )
        .with_patch_transitive(matrix.patch_transitive);

    // Prepare override path if needed (download registry versions)
    let override_path = if base_spec.override_mode != OverrideMode::None {
        match &base_version.source {
            CrateSource::Local { path } => {
                // If path points to Cargo.toml, extract directory
                let dir_path =
                    if path.ends_with("Cargo.toml") { path.parent().unwrap().to_path_buf() } else { path.clone() };
                Some(dir_path)
            }
            CrateSource::Registry => {
                // Download the registry version to use as override path
                let base_vers =
                    SemverVersion::parse(&base_version_str).map_err(|e| format!("Invalid semver for base: {}", e))?;
                let crate_handle = download::get_crate_handle(&base_version.name, &base_vers)
                    .map_err(|e| format!("Failed to download {}: {}", base_version.name, e))?;

                let dest = matrix.staging_dir.join(format!("{}-{}", base_version.name, base_version_str));
                if !dest.exists() {
                    std::fs::create_dir_all(&dest).map_err(|e| format!("Failed to create staging dir: {}", e))?;
                    crate_handle
                        .unpack_source_to(&dest)
                        .map_err(|e| format!("Failed to unpack {}: {}", base_version.name, e))?;
                }

                Some(dest)
            }
            CrateSource::Git { .. } => {
                return Err("Git sources not yet implemented".to_string());
            }
        }
    } else {
        None
    };

    // Apply override if we have a path
    let test_config = if let Some(ref path) = override_path {
        test_config.with_override_path(path)
    } else {
        // Baseline: no override, test naturally resolved version
        test_config
    };

    // Execute the test
    let result = compile::run_three_step_ict(test_config).map_err(|e| format!("Test execution failed: {}", e))?;

    Ok(result)
}

#[cfg(test)]
#[path = "runner_test.rs"]
mod runner_test;
