/// Cargo.toml manifest parsing utilities
///
/// This module handles:
/// - Reading and parsing Cargo.toml files
/// - Extracting crate name and version
/// - Finding dependency requirements

use crate::download;
use crate::toml_helpers;
use log::debug;
use semver::Version;
use std::fs::{self, File};
use std::io::Read;
use std::path::Path;
use tempfile::TempDir;

/// Extract crate name and version from a Cargo.toml manifest
pub fn get_crate_info(manifest_path: &Path) -> Result<(String, String), String> {
    let toml_str = load_string(manifest_path)?;
    let value: toml::Value = toml::from_str(&toml_str)
        .map_err(|e| format!("Failed to parse TOML: {}", e))?;

    match value.get("package") {
        Some(toml::Value::Table(t)) => {
            let name = match t.get("name") {
                Some(toml::Value::String(s)) => s.clone(),
                _ => return Err("Missing or invalid 'name' in [package]".to_string()),
            };

            let version = match t.get("version") {
                Some(toml::Value::String(s)) => s.clone(),
                _ => "0.0.0".to_string(), // Default if no version
            };

            Ok((name, version))
        }
        _ => Err("Missing [package] section in Cargo.toml".to_string()),
    }
}

/// Load a file's contents as a string
pub fn load_string(path: &Path) -> Result<String, String> {
    let mut file = File::open(path)
        .map_err(|e| format!("Failed to open file: {}", e))?;
    let mut s = String::new();
    file.read_to_string(&mut s)
        .map_err(|e| format!("Failed to read file: {}", e))?;
    Ok(s)
}

/// Reverse dependency (dependent crate) information
#[derive(Debug, Clone)]
pub struct RevDep {
    pub name: String,
    pub vers: Version,
    pub resolved_version: Option<String>,
}

/// Parse dependent specification in "name:version" format
///
/// Returns (name, optional_version)
pub fn parse_dependent_spec(spec: &str) -> (String, Option<String>) {
    match spec.split_once(':') {
        Some((name, version)) => (name.to_string(), Some(version.to_string())),
        None => (spec.to_string(), None),
    }
}

/// Extract the dependency requirement string from a dependent's Cargo.toml
///
/// Downloads the dependent crate, extracts its Cargo.toml, and finds the
/// requirement string for the specified dependency.
///
/// Returns the requirement string (e.g., "^0.8.52") if found
pub fn extract_dependency_requirement(rev_dep: &RevDep, crate_name: &str) -> Option<String> {
    debug!("Extracting dependency requirement for {} from {}", crate_name, rev_dep.name);

    // Download and cache the dependent's .crate file
    let crate_handle = match download::get_crate_handle(&rev_dep.name, &rev_dep.vers) {
        Ok(h) => h,
        Err(e) => {
            debug!("Failed to get crate handle for {}: {}", rev_dep.name, e);
            return None;
        }
    };

    // Create temp directory to extract Cargo.toml
    let temp_dir = match TempDir::new() {
        Ok(d) => d,
        Err(e) => {
            debug!("Failed to create temp dir: {}", e);
            return None;
        }
    };

    let extract_dir = temp_dir.path().join("extracted");
    if fs::create_dir(&extract_dir).is_err() {
        return None;
    }

    // Extract just the Cargo.toml
    if let Err(e) = download::extract_cargo_toml(crate_handle.path(), &extract_dir) {
        debug!("Failed to extract Cargo.toml: {}", e);
        return None;
    }

    // Read and parse Cargo.toml
    let toml_path = extract_dir.join("Cargo.toml");
    let value = match toml_helpers::load_cargo_toml(&toml_path) {
        Ok(v) => v,
        Err(e) => {
            debug!("{}", e);
            return None;
        }
    };

    // Use helper to find dependency requirement
    let result = toml_helpers::find_dependency_requirement(&value, crate_name);
    if let Some(ref req) = result {
        debug!("Found requirement: {}", req);
    } else {
        debug!("No requirement found for {} in {}'s Cargo.toml", crate_name, rev_dep.name);
    }
    result
}
