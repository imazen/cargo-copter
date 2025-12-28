//! Cargo command execution for the ICT pipeline.
//!
//! This module handles the execution of Cargo commands (fetch, check, test)
//! with proper output parsing and error handling.

use super::types::{CompileResult, CompileStep};
use crate::error_extract;
use log::debug;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// Default timeout for Cargo commands (10 minutes)
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(600);

/// Run a single Cargo step (fetch, check, or test).
///
/// # Arguments
/// * `step` - Which step to run (Fetch, Check, or Test)
/// * `crate_path` - Path to the crate directory
/// * `features` - Additional features to enable
///
/// # Returns
/// A `CompileResult` with the outcome of the command.
pub fn run_cargo_step(step: CompileStep, crate_path: &Path, features: &[String]) -> CompileResult {
    debug!("Running cargo {} in {:?}", step.as_str(), crate_path);

    let start = Instant::now();

    // Build the command
    let mut cmd = Command::new("cargo");
    cmd.arg(step.cargo_subcommand());

    // Add JSON output for structured error parsing (fetch doesn't support this)
    if step != CompileStep::Fetch {
        cmd.arg("--message-format=json");
    }

    // Add features if specified
    if !features.is_empty() {
        cmd.arg("--features");
        cmd.arg(features.join(","));
    }

    // For test, add --no-fail-fast to run all tests
    if step == CompileStep::Test {
        cmd.arg("--no-fail-fast");
    }

    // Set working directory and capture output
    cmd.current_dir(crate_path);
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());

    // Execute the command
    let output = match cmd.output() {
        Ok(output) => output,
        Err(e) => {
            return CompileResult::failure(step, start.elapsed(), format!("Failed to execute cargo: {}", e), vec![]);
        }
    };

    let duration = start.elapsed();
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();

    // Parse diagnostics from JSON output
    let diagnostics = error_extract::parse_cargo_json(&stdout);

    if output.status.success() {
        debug!("cargo {} succeeded in {:?}", step.as_str(), duration);
        CompileResult { step, success: true, duration, stdout, stderr, diagnostics }
    } else {
        debug!("cargo {} failed in {:?}", step.as_str(), duration);
        CompileResult { step, success: false, duration, stdout, stderr, diagnostics }
    }
}

/// Run a Cargo step with a specific environment variable set.
///
/// This is used for setting `CARGO_TARGET_DIR` or other env vars.
pub fn run_cargo_step_with_env(
    step: CompileStep,
    crate_path: &Path,
    features: &[String],
    env_vars: &[(&str, &str)],
) -> CompileResult {
    debug!("Running cargo {} in {:?} with env {:?}", step.as_str(), crate_path, env_vars);

    let start = Instant::now();

    // Build the command
    let mut cmd = Command::new("cargo");
    cmd.arg(step.cargo_subcommand());

    // Add JSON output (fetch doesn't support this)
    if step != CompileStep::Fetch {
        cmd.arg("--message-format=json");
    }

    // Add features
    if !features.is_empty() {
        cmd.arg("--features");
        cmd.arg(features.join(","));
    }

    // For test, add --no-fail-fast
    if step == CompileStep::Test {
        cmd.arg("--no-fail-fast");
    }

    // Set environment variables
    for (key, value) in env_vars {
        cmd.env(key, value);
    }

    // Set working directory and capture output
    cmd.current_dir(crate_path);
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());

    // Execute
    let output = match cmd.output() {
        Ok(output) => output,
        Err(e) => {
            return CompileResult::failure(step, start.elapsed(), format!("Failed to execute cargo: {}", e), vec![]);
        }
    };

    let duration = start.elapsed();
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();

    let diagnostics = error_extract::parse_cargo_json(&stdout);

    CompileResult { step, success: output.status.success(), duration, stdout, stderr, diagnostics }
}

/// Run `cargo metadata` to get dependency information.
///
/// Returns the raw JSON output from cargo metadata.
pub fn run_cargo_metadata(crate_path: &Path) -> Result<String, String> {
    debug!("Running cargo metadata in {:?}", crate_path);

    let output = Command::new("cargo")
        .arg("metadata")
        .arg("--format-version=1")
        .current_dir(crate_path)
        .output()
        .map_err(|e| format!("Failed to run cargo metadata: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("cargo metadata failed: {}", stderr));
    }

    String::from_utf8(output.stdout).map_err(|e| format!("Invalid UTF-8 in cargo metadata output: {}", e))
}

/// Run `cargo fetch` with config override for version forcing.
///
/// This uses `--config` to override the dependency version without modifying Cargo.toml.
pub fn run_cargo_fetch_with_config(crate_path: &Path, crate_name: &str, override_path: &Path) -> CompileResult {
    debug!("Running cargo fetch with config override for {} -> {:?}", crate_name, override_path);

    let start = Instant::now();

    // Build config string for path override
    let config =
        format!("patch.crates-io.{}.path=\"{}\"", crate_name, override_path.display().to_string().replace('\\', "/"));

    let output = Command::new("cargo")
        .arg("fetch")
        .arg("--config")
        .arg(&config)
        .current_dir(crate_path)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output();

    let output = match output {
        Ok(o) => o,
        Err(e) => {
            return CompileResult::failure(
                CompileStep::Fetch,
                start.elapsed(),
                format!("Failed to execute cargo: {}", e),
                vec![],
            );
        }
    };

    let duration = start.elapsed();
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();

    let diagnostics = error_extract::parse_cargo_json(&stdout);

    CompileResult { step: CompileStep::Fetch, success: output.status.success(), duration, stdout, stderr, diagnostics }
}

/// Check if a crate path has a valid Cargo.toml.
pub fn has_cargo_toml(path: &Path) -> bool {
    path.join("Cargo.toml").exists()
}

/// Get the crate name from a Cargo.toml.
pub fn get_crate_name(cargo_toml_path: &Path) -> Result<String, String> {
    let content = std::fs::read_to_string(cargo_toml_path).map_err(|e| format!("Failed to read Cargo.toml: {}", e))?;

    // Simple parsing - look for name = "..."
    for line in content.lines() {
        let line = line.trim();
        if line.starts_with("name")
            && line.contains('=')
            && let Some(name) = line.split('=').nth(1)
        {
            let name = name.trim().trim_matches('"').trim_matches('\'');
            return Ok(name.to_string());
        }
    }

    Err("Could not find crate name in Cargo.toml".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compile_step_cargo_subcommand() {
        assert_eq!(CompileStep::Fetch.cargo_subcommand(), "fetch");
        assert_eq!(CompileStep::Check.cargo_subcommand(), "check");
        assert_eq!(CompileStep::Test.cargo_subcommand(), "test");
    }

    #[test]
    fn test_has_cargo_toml() {
        // Current directory should have Cargo.toml (we're in cargo-copter)
        assert!(has_cargo_toml(Path::new(".")));

        // Random path shouldn't
        assert!(!has_cargo_toml(Path::new("/nonexistent/path")));
    }
}
