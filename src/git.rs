/// Git repository utilities
///
/// This module handles:
/// - Getting the current git commit hash
/// - Checking for uncommitted changes

use std::process::Command;

/// Get the short git commit hash (7 characters)
pub fn get_git_hash() -> Option<String> {
    Command::new("git")
        .args(&["rev-parse", "--short", "HEAD"])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .map(|s| s.trim().to_string())
}

/// Check if git working directory is dirty (has uncommitted changes)
pub fn is_git_dirty() -> bool {
    Command::new("git")
        .args(&["status", "--porcelain"])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .map(|s| !s.trim().is_empty())
        .unwrap_or(false)
}
