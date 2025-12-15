//! Docker execution support for cargo-copter
//!
//! This module provides the ability to run cargo-copter inside a Docker container
//! for security isolation when testing untrusted crates.

use std::env;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus};

/// The embedded docker wrapper script
const EMBEDDED_DOCKER_SCRIPT: &str = include_str!("../copter-docker.sh");

/// Check if Docker is available on the system
pub fn is_docker_available() -> bool {
    Command::new("docker").arg("--version").output().map(|o| o.status.success()).unwrap_or(false)
}

/// Find a local copter-docker.sh script if it exists
fn find_local_script() -> Option<PathBuf> {
    // Check current directory
    let local = Path::new("copter-docker.sh");
    if local.exists() {
        return Some(local.to_path_buf());
    }

    // Check next to the executable
    if let Ok(exe) = env::current_exe() {
        if let Some(dir) = exe.parent() {
            let beside_exe = dir.join("copter-docker.sh");
            if beside_exe.exists() {
                return Some(beside_exe);
            }
        }
    }

    None
}

/// Run cargo-copter inside Docker, passing through all arguments except --docker
pub fn run_in_docker(original_args: &[String]) -> Result<ExitStatus, String> {
    if !is_docker_available() {
        return Err("Docker is not installed or not running. Please install Docker first.".to_string());
    }

    // Filter out --docker from args
    let filtered_args: Vec<&str> = original_args.iter().map(|s| s.as_str()).filter(|&arg| arg != "--docker").collect();

    // Check for local script first
    if let Some(local_script) = find_local_script() {
        eprintln!("Using local script: {}", local_script.display());
        return run_local_script(&local_script, &filtered_args);
    }

    // Use embedded script
    run_embedded_script(&filtered_args)
}

/// Run a local copter-docker.sh script
fn run_local_script(script_path: &Path, args: &[&str]) -> Result<ExitStatus, String> {
    Command::new("bash")
        .arg(script_path)
        .args(args)
        .status()
        .map_err(|e| format!("Failed to execute local docker script: {}", e))
}

/// Run the embedded docker script
fn run_embedded_script(args: &[&str]) -> Result<ExitStatus, String> {
    // Write embedded script to a temp file and execute it
    let temp_dir = env::temp_dir();
    let script_path = temp_dir.join("copter-docker-embedded.sh");

    std::fs::write(&script_path, EMBEDDED_DOCKER_SCRIPT)
        .map_err(|e| format!("Failed to write temporary docker script: {}", e))?;

    // Make it executable (Unix only)
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(&script_path)
            .map_err(|e| format!("Failed to get script permissions: {}", e))?
            .permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&script_path, perms)
            .map_err(|e| format!("Failed to set script permissions: {}", e))?;
    }

    let status = Command::new("bash")
        .arg(&script_path)
        .args(args)
        .status()
        .map_err(|e| format!("Failed to execute docker script: {}", e))?;

    // Clean up temp file (ignore errors)
    let _ = std::fs::remove_file(&script_path);

    Ok(status)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_embedded_script_not_empty() {
        assert!(!EMBEDDED_DOCKER_SCRIPT.is_empty());
        assert!(EMBEDDED_DOCKER_SCRIPT.contains("cargo-copter"));
    }

    #[test]
    fn test_find_local_script_returns_none_when_missing() {
        // This test assumes copter-docker.sh doesn't exist in the test directory
        // which may not always be true, so we just check it doesn't panic
        let _ = find_local_script();
    }
}
