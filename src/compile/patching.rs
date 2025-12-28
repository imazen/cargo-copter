//! Cargo.toml manipulation for dependency patching.
//!
//! This module handles all modifications to Cargo.toml files, including:
//! - Backup and restore of original files
//! - Adding `[patch.crates-io]` sections
//! - Modifying dependency specifications
//!
//! # Safety
//!
//! All modifications use a backup/restore pattern:
//! 1. Create backup of original Cargo.toml
//! 2. Modify the file
//! 3. Run the test
//! 4. Restore from backup (even on failure)

use log::debug;
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::Path;

/// Extension for backup files
const BACKUP_EXTENSION: &str = ".copter-backup";

/// Create a backup of a file.
///
/// Returns the path to the backup file.
pub fn backup_file(path: &Path) -> std::io::Result<std::path::PathBuf> {
    let backup_path = path.with_extension(format!(
        "{}{}",
        path.extension().map(|e| e.to_string_lossy()).unwrap_or_default(),
        BACKUP_EXTENSION
    ));

    // If file doesn't exist, return the backup path without creating
    if !path.exists() {
        return Ok(backup_path);
    }

    fs::copy(path, &backup_path)?;
    debug!("Created backup: {:?}", backup_path);
    Ok(backup_path)
}

/// Restore a file from its backup.
///
/// Removes the backup file after restoration.
pub fn restore_file(path: &Path) -> std::io::Result<()> {
    let backup_path = path.with_extension(format!(
        "{}{}",
        path.extension().map(|e| e.to_string_lossy()).unwrap_or_default(),
        BACKUP_EXTENSION
    ));

    if backup_path.exists() {
        fs::copy(&backup_path, path)?;
        fs::remove_file(&backup_path)?;
        debug!("Restored from backup: {:?}", path);
    }
    Ok(())
}

/// Add a `[patch.crates-io]` section to a Cargo.toml file.
///
/// This unifies all versions of the specified crate across the dependency tree.
///
/// # Arguments
/// * `cargo_toml_path` - Path to the Cargo.toml to modify
/// * `crate_name` - Name of the crate to patch
/// * `patch_path` - Local path to use for the patch
///
/// # Example
///
/// Before:
/// ```toml
/// [dependencies]
/// rgb = "0.8"
/// ```
///
/// After:
/// ```toml
/// [dependencies]
/// rgb = "0.8"
///
/// [patch.crates-io]
/// rgb = { path = "/path/to/local/rgb" }
/// ```
pub fn apply_patch_crates_io(cargo_toml_path: &Path, crate_name: &str, patch_path: &Path) -> std::io::Result<()> {
    debug!("Applying [patch.crates-io] for {} with path {:?}", crate_name, patch_path);

    let mut content = String::new();
    File::open(cargo_toml_path)?.read_to_string(&mut content)?;

    // Check if there's already a [patch.crates-io] section
    let patch_section = format!("\n[patch.crates-io]\n{} = {{ path = \"{}\" }}\n", crate_name, patch_path.display());

    if content.contains("[patch.crates-io]") {
        // Section exists - append to it
        // Find the section and add our entry
        if let Some(idx) = content.find("[patch.crates-io]") {
            let section_end = idx + "[patch.crates-io]".len();
            let entry = format!("\n{} = {{ path = \"{}\" }}", crate_name, patch_path.display());

            // Check if this crate is already in the patch section
            if !content[section_end..].contains(&format!("{} =", crate_name)) {
                let mut new_content = content[..section_end].to_string();
                new_content.push_str(&entry);
                new_content.push_str(&content[section_end..]);
                content = new_content;
            }
        }
    } else {
        // No existing section - append new one
        content.push_str(&patch_section);
    }

    let mut file = File::create(cargo_toml_path)?;
    file.write_all(content.as_bytes())?;
    file.flush()?;

    debug!("Applied patch to {:?}", cargo_toml_path);
    Ok(())
}

/// Remove a `[patch.crates-io]` entry for a specific crate.
///
/// This is used to clean up after patching.
pub fn remove_patch_entry(cargo_toml_path: &Path, crate_name: &str) -> std::io::Result<()> {
    let mut content = String::new();
    File::open(cargo_toml_path)?.read_to_string(&mut content)?;

    // Simple removal: find and remove the line
    let pattern = format!("{} = {{ path =", crate_name);
    let lines: Vec<&str> = content.lines().filter(|line| !line.contains(&pattern)).collect();
    let new_content = lines.join("\n");

    let mut file = File::create(cargo_toml_path)?;
    file.write_all(new_content.as_bytes())?;
    file.flush()?;

    Ok(())
}

/// Check if a Cargo.toml has a `[patch.crates-io]` section.
pub fn has_patch_section(cargo_toml_path: &Path) -> std::io::Result<bool> {
    let mut content = String::new();
    File::open(cargo_toml_path)?.read_to_string(&mut content)?;
    Ok(content.contains("[patch.crates-io]"))
}

/// Guard that restores a file from backup when dropped.
///
/// This ensures the backup is restored even if the test panics.
pub struct BackupGuard {
    path: std::path::PathBuf,
    restored: bool,
}

impl BackupGuard {
    /// Create a backup and return a guard.
    pub fn new(path: &Path) -> std::io::Result<Self> {
        backup_file(path)?;
        Ok(Self { path: path.to_path_buf(), restored: false })
    }

    /// Restore the backup manually.
    ///
    /// This is useful when you want to restore before the guard is dropped.
    pub fn restore(&mut self) -> std::io::Result<()> {
        if !self.restored {
            restore_file(&self.path)?;
            self.restored = true;
        }
        Ok(())
    }
}

impl Drop for BackupGuard {
    fn drop(&mut self) {
        if !self.restored {
            let _ = restore_file(&self.path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::TempDir;

    #[test]
    fn test_backup_and_restore() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("Cargo.toml");

        // Create original file
        let original_content = "[package]\nname = \"test\"\n";
        fs::write(&file_path, original_content).unwrap();

        // Create backup
        let backup_path = backup_file(&file_path).unwrap();
        assert!(backup_path.exists());

        // Modify original
        fs::write(&file_path, "[package]\nname = \"modified\"\n").unwrap();

        // Restore
        restore_file(&file_path).unwrap();

        // Verify restored
        let restored = fs::read_to_string(&file_path).unwrap();
        assert_eq!(restored, original_content);
        assert!(!backup_path.exists());
    }

    #[test]
    fn test_apply_patch_crates_io() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("Cargo.toml");

        // Create original file
        let original = "[dependencies]\nrgb = \"0.8\"\n";
        fs::write(&file_path, original).unwrap();

        // Apply patch
        apply_patch_crates_io(&file_path, "rgb", Path::new("/path/to/rgb")).unwrap();

        // Verify
        let patched = fs::read_to_string(&file_path).unwrap();
        assert!(patched.contains("[patch.crates-io]"));
        assert!(patched.contains("rgb = { path = \"/path/to/rgb\" }"));
    }

    #[test]
    fn test_apply_patch_crates_io_preserves_existing_content() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("Cargo.toml");

        // Create original with existing content
        let original =
            "[package]\nname = \"myapp\"\nversion = \"1.0.0\"\n\n[dependencies]\nrgb = \"0.8\"\nserde = \"1.0\"\n";
        fs::write(&file_path, original).unwrap();

        // Apply patch
        apply_patch_crates_io(&file_path, "rgb", Path::new("/path/to/rgb")).unwrap();

        // Verify original content preserved
        let patched = fs::read_to_string(&file_path).unwrap();
        assert!(patched.contains("[package]"));
        assert!(patched.contains("name = \"myapp\""));
        assert!(patched.contains("[dependencies]"));
        assert!(patched.contains("serde = \"1.0\""));
        assert!(patched.contains("[patch.crates-io]"));
    }

    #[test]
    fn test_backup_guard() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("Cargo.toml");

        let original_content = "[package]\nname = \"test\"\n";
        fs::write(&file_path, original_content).unwrap();

        {
            let _guard = BackupGuard::new(&file_path).unwrap();
            fs::write(&file_path, "[package]\nname = \"modified\"\n").unwrap();
            // Guard drops here and restores
        }

        let restored = fs::read_to_string(&file_path).unwrap();
        assert_eq!(restored, original_content);
    }
}
