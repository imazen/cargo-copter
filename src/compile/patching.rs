//! Cargo.toml manipulation for dependency patching.
//!
//! This module handles all modifications to Cargo.toml files, including:
//! - Backup and restore of original files
//! - Adding `[patch.crates-io]` sections
//! - Modifying dependency specifications (force mode)
//!
//! # Safety
//!
//! All modifications use a backup/restore pattern:
//! 1. Create backup of original Cargo.toml
//! 2. Modify the file
//! 3. Run the test
//! 4. Restore from backup (even on failure)

use log::debug;
use std::env;
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

/// Apply a dependency override to Cargo.toml - Force mode.
///
/// This replaces the dependency specification directly with a path override,
/// bypassing semver requirements. Unlike `apply_patch_crates_io`, this modifies
/// the direct dependency entry rather than adding a patch section.
///
/// # Arguments
/// * `cargo_toml_path` - Path to the Cargo.toml to modify
/// * `dep_name` - Name of the dependency to override
/// * `override_path` - Local path to use for the override
///
/// # Preserved Fields
/// The following fields are preserved from the original dependency:
/// - `optional`
/// - `default-features`
/// - `features`
/// - `package`
pub fn apply_dependency_override(cargo_toml_path: &Path, dep_name: &str, override_path: &Path) -> Result<(), String> {
    // Convert to absolute path
    let override_path = if override_path.is_absolute() {
        override_path.to_path_buf()
    } else {
        env::current_dir().map_err(|e| format!("Failed to get current dir: {}", e))?.join(override_path)
    };

    let mut content = String::new();

    // Read original Cargo.toml
    let mut file = fs::File::open(cargo_toml_path).map_err(|e| format!("Failed to open Cargo.toml: {}", e))?;
    std::io::Read::read_to_string(&mut file, &mut content).map_err(|e| format!("Failed to read Cargo.toml: {}", e))?;
    drop(file);

    // Parse as TOML
    let mut doc: toml_edit::DocumentMut = content.parse().map_err(|e| format!("Failed to parse Cargo.toml: {}", e))?;

    // Update dependency in all sections (force mode - replaces the spec entirely)
    let sections = vec!["dependencies", "dev-dependencies", "build-dependencies"];

    for section in sections {
        if let Some(deps) = doc.get_mut(section).and_then(|s| s.as_table_mut())
            && let Some(dep) = deps.get_mut(dep_name)
        {
            debug!("Force-replacing {} in [{}] with path {:?}", dep_name, section, override_path);

            // Preserve existing fields (optional, default-features, features, etc.)
            let mut new_dep = toml_edit::InlineTable::new();
            new_dep.insert("path", override_path.display().to_string().into());

            // Copy fields from original dependency if it's a table
            if let Some(old_table) = dep.as_inline_table() {
                // Preserve important fields
                for key in ["optional", "default-features", "features", "package"] {
                    if let Some(value) = old_table.get(key) {
                        new_dep.insert(key, value.clone());
                        debug!("Preserving field '{}' = {:?}", key, value);
                    }
                }
            } else if let Some(old_table) = dep.as_table_like() {
                // Handle table-like dependencies
                for key in ["optional", "default-features", "features", "package"] {
                    if let Some(value) = old_table.get(key)
                        && let Some(v) = value.as_value()
                    {
                        new_dep.insert(key, v.clone());
                        debug!("Preserving field '{}' = {:?}", key, v);
                    }
                }
            }

            *dep = toml_edit::Item::Value(toml_edit::Value::InlineTable(new_dep));
        }
    }

    debug!("Force-replaced {} dependency spec with path: {}", dep_name, override_path.display());

    // Write back
    let mut file = fs::File::create(cargo_toml_path).map_err(|e| format!("Failed to create Cargo.toml: {}", e))?;
    std::io::Write::write_all(&mut file, doc.to_string().as_bytes())
        .map_err(|e| format!("Failed to write Cargo.toml: {}", e))?;

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

    #[test]
    fn test_apply_dependency_override() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("Cargo.toml");

        // Create Cargo.toml with a simple dependency
        let original = r#"[package]
name = "test"
version = "0.1.0"

[dependencies]
rgb = "0.8"
"#;
        fs::write(&file_path, original).unwrap();

        // Apply dependency override
        apply_dependency_override(&file_path, "rgb", Path::new("/path/to/rgb")).unwrap();

        // Verify the dependency was replaced with a path
        let modified = fs::read_to_string(&file_path).unwrap();
        assert!(modified.contains("rgb = { path ="));
        assert!(modified.contains("/path/to/rgb"));
        // Should NOT have a patch section
        assert!(!modified.contains("[patch.crates-io]"));
    }

    #[test]
    fn test_apply_dependency_override_preserves_fields() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("Cargo.toml");

        // Create Cargo.toml with a dependency that has extra fields
        let original = r#"[package]
name = "test"
version = "0.1.0"

[dependencies]
rgb = { version = "0.8", optional = true, default-features = false }
"#;
        fs::write(&file_path, original).unwrap();

        // Apply dependency override
        apply_dependency_override(&file_path, "rgb", Path::new("/path/to/rgb")).unwrap();

        // Verify the dependency was replaced but optional and default-features preserved
        let modified = fs::read_to_string(&file_path).unwrap();
        assert!(modified.contains("path ="));
        assert!(modified.contains("optional"));
        assert!(modified.contains("default-features"));
    }
}
