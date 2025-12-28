//! Compile and test execution module.
//!
//! This module orchestrates the three-step Install/Check/Test (ICT) pipeline
//! for testing crate compatibility.
//!
//! # Architecture
//!
//! The module is organized into focused sub-modules:
//!
//! - [`types`] - Core types: `PatchDepth`, `CompileResult`, `ThreeStepResult`
//! - [`config`] - Test configuration builder: `TestConfig`
//! - [`executor`] - Cargo command execution
//! - [`patching`] - Cargo.toml manipulation for version overrides
//! - [`retry`] - Auto-retry logic for multi-version conflicts
//! - [`logging`] - Failure log writing
//!
//! # The Three-Step Pipeline
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────────┐
//! │                    run_three_step_ict()                         │
//! └─────────────────────────────────────────────────────────────────┘
//!                              │
//!          ┌───────────────────┼───────────────────┐
//!          │                   │                   │
//!          ▼                   ▼                   ▼
//!   ┌─────────────┐     ┌─────────────┐     ┌─────────────┐
//!   │ cargo fetch │────►│ cargo check │────►│ cargo test  │
//!   └─────────────┘     └─────────────┘     └─────────────┘
//!          │                   │                   │
//!          │ (early stop       │ (early stop       │
//!          │  on failure)      │  on failure)      │
//!          ▼                   ▼                   ▼
//!   [Multi-version    [Compile error?]    [Test failure?]
//!    conflict?]
//!          │
//!          ▼
//!   [Auto-retry with
//!    patch.crates-io]
//! ```
//!
//! # Patching Strategies
//!
//! See [`types::PatchDepth`] for the different patching strategies used to
//! force specific versions.

pub mod config;
pub mod executor;
pub mod logging;
pub mod patching;
pub mod retry;
pub mod types;

// Re-export main types for convenience
pub use config::TestConfig;
pub use types::{CompileResult, CompileStep, PatchDepth, ThreeStepResult, VersionSource};

use crate::metadata;
use log::debug;
use std::fs;
use std::path::Path;
use std::time::Duration;

/// Run the three-step ICT (Install/Check/Test) pipeline.
///
/// This is the main entry point for testing a dependent crate against
/// a specific version of the base crate.
///
/// # Arguments
/// * `config` - Test configuration specifying what to test
///
/// # Returns
/// A `ThreeStepResult` with the outcome of all steps.
///
/// # Error Handling
///
/// This function handles errors gracefully:
/// - Multi-version conflicts trigger auto-retry with `[patch.crates-io]`
/// - Cargo.toml is always restored from backup after the test
/// - Failures at any step stop the pipeline early
pub fn run_three_step_ict(config: TestConfig) -> Result<ThreeStepResult, String> {
    debug!("Starting three-step ICT for {}", config.display());

    // Determine patching strategy
    let (patch_depth, should_modify_toml) = determine_patch_strategy(&config);

    debug!("Using patch strategy: {:?}, modify_toml: {}", patch_depth, should_modify_toml);

    // Set up backup guard for Cargo.toml if we'll modify it
    let cargo_toml_path = config.dependent_path.join("Cargo.toml");
    let _backup_guard = if should_modify_toml && cargo_toml_path.exists() {
        Some(patching::BackupGuard::new(&cargo_toml_path).map_err(|e| format!("Failed to backup Cargo.toml: {}", e))?)
    } else {
        None
    };

    // Apply patching based on strategy
    if should_modify_toml && let Some(ref override_path) = config.override_path {
        if config.force_version {
            // Force mode: replace dependency spec directly (bypasses semver)
            patching::apply_dependency_override(&cargo_toml_path, &config.base_crate, override_path)
                .map_err(|e| format!("Failed to apply dependency override: {}", e))?;

            // If patch_transitive is also enabled, add [patch.crates-io] for transitive deps
            if config.patch_transitive {
                patching::apply_patch_crates_io(&cargo_toml_path, &config.base_crate, override_path)
                    .map_err(|e| format!("Failed to apply patch: {}", e))?;
                debug!("Applied BOTH force override AND [patch.crates-io] for transitive patching");
            }
        } else {
            // Non-force mode: use [patch.crates-io] only
            patching::apply_patch_crates_io(&cargo_toml_path, &config.base_crate, override_path)
                .map_err(|e| format!("Failed to apply patch: {}", e))?;
        }
    }

    // Build environment variables
    let env_vars: Vec<(&str, &str)> = vec![];

    // Step 1: Fetch
    let fetch_result =
        executor::run_cargo_step_with_env(CompileStep::Fetch, &config.dependent_path, &config.features, &env_vars);

    // Check for multi-version conflict and potentially retry
    let (fetch_result, final_patch_depth) = if !fetch_result.success {
        let analysis = retry::analyze_conflict(&fetch_result.stdout, &fetch_result.stderr, &config.base_crate);

        if retry::should_retry_with_patch(&analysis, patch_depth == PatchDepth::Patch) {
            debug!("Detected multi-version conflict, retrying with [patch.crates-io]");

            // Apply both force override AND [patch.crates-io] for retry
            if let Some(ref override_path) = config.override_path {
                // If we were using force mode, reapply the dependency override
                if config.force_version {
                    patching::apply_dependency_override(&cargo_toml_path, &config.base_crate, override_path)
                        .map_err(|e| format!("Failed to apply dependency override for retry: {}", e))?;
                }
                // Always add [patch.crates-io] for the retry (this is what fixes the conflict)
                patching::apply_patch_crates_io(&cargo_toml_path, &config.base_crate, override_path)
                    .map_err(|e| format!("Failed to apply patch for retry: {}", e))?;
            }

            let retry_result = executor::run_cargo_step_with_env(
                CompileStep::Fetch,
                &config.dependent_path,
                &config.features,
                &env_vars,
            );

            (retry_result, PatchDepth::Patch)
        } else {
            (fetch_result, patch_depth)
        }
    } else {
        (fetch_result, patch_depth)
    };

    // Extract version info from Cargo.lock if fetch succeeded
    let (actual_version, original_requirement, all_crate_versions) = if fetch_result.success {
        extract_version_info(&config.dependent_path, &config.base_crate)
    } else {
        (None, config.original_requirement.clone(), vec![])
    };

    // Early stop if fetch failed
    if !fetch_result.success {
        let result = ThreeStepResult {
            fetch: fetch_result,
            check: None,
            test: None,
            forced_version: config.force_version,
            patch_depth: final_patch_depth,
            actual_version,
            expected_version: config.offered_version.clone(),
            original_requirement,
            all_crate_versions,
            blocking_crates: vec![],
        };
        result.debug_assert_consistent();
        return Ok(result);
    }

    // Step 2: Check (unless skipped)
    let check_result = if config.skip_check {
        None
    } else {
        Some(executor::run_cargo_step_with_env(CompileStep::Check, &config.dependent_path, &config.features, &env_vars))
    };

    // Early stop if check failed
    if let Some(ref check) = check_result
        && !check.success
    {
        let result = ThreeStepResult {
            fetch: fetch_result,
            check: check_result,
            test: None,
            forced_version: config.force_version,
            patch_depth: final_patch_depth,
            actual_version,
            expected_version: config.offered_version.clone(),
            original_requirement,
            all_crate_versions,
            blocking_crates: vec![],
        };
        result.debug_assert_consistent();
        return Ok(result);
    }

    // Step 3: Test (unless skipped)
    let test_result = if config.skip_test {
        None
    } else {
        Some(executor::run_cargo_step_with_env(CompileStep::Test, &config.dependent_path, &config.features, &env_vars))
    };

    let result = ThreeStepResult {
        fetch: fetch_result,
        check: check_result,
        test: test_result,
        forced_version: config.force_version,
        patch_depth: final_patch_depth,
        actual_version,
        expected_version: config.offered_version.clone(),
        original_requirement,
        all_crate_versions,
        blocking_crates: vec![],
    };

    // INVARIANT: Result must be internally consistent
    result.debug_assert_consistent();

    Ok(result)
}

/// Determine the patching strategy based on configuration.
///
/// Returns (PatchDepth, should_modify_toml)
fn determine_patch_strategy(config: &TestConfig) -> (PatchDepth, bool) {
    if config.override_path.is_none() {
        // Baseline test - no patching
        return (PatchDepth::None, false);
    }

    if config.patch_transitive {
        // Explicit transitive patching requested
        (PatchDepth::Patch, true)
    } else if config.force_version {
        // Force mode - start with Force, may escalate to Patch on retry
        (PatchDepth::Force, true)
    } else {
        // Normal override - just patch
        (PatchDepth::Patch, true)
    }
}

/// Version information extracted from Cargo.lock
type VersionInfo = (Option<String>, Option<String>, Vec<(String, String, String)>);

/// Extract version information from the dependent's Cargo.lock.
///
/// Returns (actual_version, original_requirement, all_versions)
fn extract_version_info(dependent_path: &Path, base_crate: &str) -> VersionInfo {
    // Try to run cargo metadata to get version info
    match executor::run_cargo_metadata(dependent_path) {
        Ok(json) => {
            match metadata::parse_metadata(&json) {
                Ok(parsed) => {
                    // Find all versions of the base crate
                    let versions = metadata::find_all_versions(&parsed, base_crate);

                    // Get the first version as the actual version
                    let actual = versions.first().map(|v| v.version.clone());

                    // Get the spec (original requirement)
                    let spec = versions.first().map(|v| v.spec.clone());

                    // Convert to the expected format
                    let all_versions: Vec<_> = versions
                        .iter()
                        .filter_map(|v| {
                            metadata::parse_node_id(&v.node_id)
                                .map(|(name, _)| (v.spec.clone(), v.version.clone(), name))
                        })
                        .collect();

                    (actual, spec, all_versions)
                }
                Err(e) => {
                    debug!("Failed to parse metadata: {}", e);
                    (None, None, vec![])
                }
            }
        }
        Err(e) => {
            debug!("Failed to run cargo metadata: {}", e);
            (None, None, vec![])
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compile_result_failed() {
        let result = CompileResult::failure(CompileStep::Fetch, Duration::from_secs(1), "error".to_string(), vec![]);
        assert!(!result.success);
    }

    #[test]
    fn test_compile_step_as_str() {
        assert_eq!(CompileStep::Fetch.as_str(), "fetch");
        assert_eq!(CompileStep::Check.as_str(), "check");
        assert_eq!(CompileStep::Test.as_str(), "test");
    }

    #[test]
    fn test_compile_step_cargo_subcommand() {
        assert_eq!(CompileStep::Fetch.cargo_subcommand(), "fetch");
        assert_eq!(CompileStep::Check.cargo_subcommand(), "check");
        assert_eq!(CompileStep::Test.cargo_subcommand(), "test");
    }

    #[test]
    fn test_apply_patch_crates_io() {
        use std::io::Write;
        use tempfile::TempDir;

        let temp_dir = TempDir::new().unwrap();
        let cargo_toml = temp_dir.path().join("Cargo.toml");

        // Create a minimal Cargo.toml
        let mut file = fs::File::create(&cargo_toml).unwrap();
        writeln!(file, "[package]").unwrap();
        writeln!(file, "name = \"test\"").unwrap();
        writeln!(file, "version = \"0.1.0\"").unwrap();

        // Apply patch
        patching::apply_patch_crates_io(&cargo_toml, "rgb", Path::new("/path/to/rgb")).unwrap();

        // Verify
        let content = fs::read_to_string(&cargo_toml).unwrap();
        assert!(content.contains("[patch.crates-io]"));
        assert!(content.contains("rgb"));
    }

    #[test]
    fn test_apply_patch_crates_io_preserves_existing_content() {
        use std::io::Write;
        use tempfile::TempDir;

        let temp_dir = TempDir::new().unwrap();
        let cargo_toml = temp_dir.path().join("Cargo.toml");

        // Create Cargo.toml with existing content
        let mut file = fs::File::create(&cargo_toml).unwrap();
        writeln!(file, "[package]").unwrap();
        writeln!(file, "name = \"test\"").unwrap();
        writeln!(file, "version = \"0.1.0\"").unwrap();
        writeln!(file).unwrap();
        writeln!(file, "[dependencies]").unwrap();
        writeln!(file, "serde = \"1.0\"").unwrap();

        // Apply patch
        patching::apply_patch_crates_io(&cargo_toml, "rgb", Path::new("/path/to/rgb")).unwrap();

        // Verify original content is preserved
        let content = fs::read_to_string(&cargo_toml).unwrap();
        assert!(content.contains("[package]"));
        assert!(content.contains("name = \"test\""));
        assert!(content.contains("[dependencies]"));
        assert!(content.contains("serde = \"1.0\""));
        assert!(content.contains("[patch.crates-io]"));
    }
}
