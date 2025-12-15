/// Cargo.toml manifest parsing utilities
///
/// This module handles:
/// - Reading and parsing Cargo.toml files
/// - Extracting crate name and version
/// - Finding dependency requirements
use semver::Version;
use std::fs::File;
use std::io::Read;
use std::path::Path;

/// Extract crate name and version from a Cargo.toml manifest
pub fn get_crate_info(manifest_path: &Path) -> Result<(String, String), String> {
    let toml_str = load_string(manifest_path)?;
    let value: toml::Value = toml::from_str(&toml_str).map_err(|e| format!("Failed to parse TOML: {}", e))?;

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
    let mut file = File::open(path).map_err(|e| format!("Failed to open file: {}", e))?;
    let mut s = String::new();
    file.read_to_string(&mut s).map_err(|e| format!("Failed to read file: {}", e))?;
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
