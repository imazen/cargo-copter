//! Failure logging for test results.
//!
//! This module handles writing detailed failure logs to disk for later analysis.

use super::types::{CompileStep, ThreeStepResult};
use log::debug;
use std::fs::{self, File};
use std::io::Write;
use std::path::Path;

/// Write a detailed failure log for a test that failed.
///
/// Creates a file in the staging directory with the full error output.
///
/// # Arguments
/// * `staging_dir` - Directory to write logs to
/// * `dependent_name` - Name of the dependent crate
/// * `dependent_version` - Version of the dependent crate
/// * `base_version` - Version of the base crate being tested
/// * `result` - The three-step result containing error details
pub fn log_failure(
    staging_dir: &Path,
    dependent_name: &str,
    dependent_version: &str,
    base_version: &str,
    result: &ThreeStepResult,
) -> std::io::Result<()> {
    // Create logs directory
    let logs_dir = staging_dir.join("logs");
    fs::create_dir_all(&logs_dir)?;

    // Create log filename
    let filename =
        format!("{}_{}_vs_{}.log", dependent_name, dependent_version, base_version).replace(['/', '\\'], "_");
    let log_path = logs_dir.join(&filename);

    debug!("Writing failure log to {:?}", log_path);

    let mut file = File::create(&log_path)?;

    // Write header
    writeln!(file, "=== Failure Log ===")?;
    writeln!(file, "Dependent: {} v{}", dependent_name, dependent_version)?;
    writeln!(file, "Base crate version: {}", base_version)?;
    writeln!(file, "Patch depth: {:?}", result.patch_depth)?;
    writeln!(file, "Forced version: {}", result.forced_version)?;
    writeln!(file)?;

    // Determine which step failed
    let failed_step = result.first_failure();

    if let Some(step) = failed_step {
        writeln!(file, "Failed at step: {:?}", step)?;
        writeln!(file)?;

        // Write the appropriate error output
        match step {
            CompileStep::Fetch => {
                writeln!(file, "=== Fetch Error ===")?;
                write_compile_result(&mut file, &result.fetch)?;
            }
            CompileStep::Check => {
                if let Some(ref check) = result.check {
                    writeln!(file, "=== Check Error ===")?;
                    write_compile_result(&mut file, check)?;
                }
            }
            CompileStep::Test => {
                if let Some(ref test) = result.test {
                    writeln!(file, "=== Test Error ===")?;
                    write_compile_result(&mut file, test)?;
                }
            }
        }
    }

    // Write blocking crates info if present
    if !result.blocking_crates.is_empty() {
        writeln!(file)?;
        writeln!(file, "=== Blocking Crates ===")?;
        for crate_name in &result.blocking_crates {
            writeln!(file, "  - {}", crate_name)?;
        }
    }

    // Write all crate versions if present
    if !result.all_crate_versions.is_empty() {
        writeln!(file)?;
        writeln!(file, "=== All Base Crate Versions in Tree ===")?;
        for (spec, resolved, dep_name) in &result.all_crate_versions {
            writeln!(file, "  {} requires {} -> {}", dep_name, spec, resolved)?;
        }
    }

    file.flush()?;
    Ok(())
}

/// Write a compile result's details to a file.
fn write_compile_result(file: &mut File, result: &super::types::CompileResult) -> std::io::Result<()> {
    writeln!(file, "Duration: {:?}", result.duration)?;
    writeln!(file)?;

    if !result.stderr.is_empty() {
        writeln!(file, "--- stderr ---")?;
        writeln!(file, "{}", result.stderr)?;
    }

    if !result.stdout.is_empty() {
        writeln!(file, "--- stdout ---")?;
        writeln!(file, "{}", result.stdout)?;
    }

    // Write parsed diagnostics
    if !result.diagnostics.is_empty() {
        writeln!(file)?;
        writeln!(file, "--- Parsed Diagnostics ---")?;
        for diag in &result.diagnostics {
            writeln!(file, "[{:?}] {}", diag.level, diag.message)?;
            if let Some(ref span) = diag.primary_span {
                writeln!(file, "  at {}:{}:{}", span.file_name, span.line, span.column)?;
            }
        }
    }

    Ok(())
}

/// Get the path where a failure log would be written.
pub fn failure_log_path(
    staging_dir: &Path,
    dependent_name: &str,
    dependent_version: &str,
    base_version: &str,
) -> std::path::PathBuf {
    let filename =
        format!("{}_{}_vs_{}.log", dependent_name, dependent_version, base_version).replace(['/', '\\'], "_");
    staging_dir.join("logs").join(filename)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compile::types::{CompileResult, PatchDepth};
    use std::time::Duration;
    use tempfile::TempDir;

    #[test]
    fn test_log_failure_creates_file() {
        let temp_dir = TempDir::new().unwrap();

        let result = ThreeStepResult {
            fetch: CompileResult::failure(
                crate::compile::CompileStep::Fetch,
                Duration::from_secs(1),
                "fetch error".to_string(),
                vec![],
            ),
            check: None,
            test: None,
            forced_version: false,
            patch_depth: PatchDepth::None,
            actual_version: None,
            expected_version: None,
            original_requirement: None,
            all_crate_versions: vec![],
            blocking_crates: vec![],
        };

        log_failure(temp_dir.path(), "image", "0.25.8", "0.8.91", &result).unwrap();

        let log_path = failure_log_path(temp_dir.path(), "image", "0.25.8", "0.8.91");
        assert!(log_path.exists());

        let content = fs::read_to_string(log_path).unwrap();
        assert!(content.contains("image"));
        assert!(content.contains("0.8.91"));
        assert!(content.contains("fetch error"));
    }

    #[test]
    fn test_failure_log_path() {
        let path = failure_log_path(Path::new("/tmp/staging"), "image", "0.25.8", "0.8.91");
        assert!(path.to_string_lossy().contains("image_0.25.8_vs_0.8.91.log"));
    }
}
