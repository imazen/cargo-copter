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
                // `version.workspace = true` — inherit from the workspace root's
                // [workspace.package].version (cargo's inheritance). Without this
                // a workspace member reads as "0.0.0" and is needlessly FORCE-tested.
                Some(toml::Value::Table(tbl)) if tbl.get("workspace").and_then(|w| w.as_bool()) == Some(true) => {
                    resolve_workspace_version(manifest_path).unwrap_or_else(|| "0.0.0".to_string())
                }
                _ => "0.0.0".to_string(), // Default if no version
            };

            Ok((name, version))
        }
        _ => Err("Missing [package] section in Cargo.toml".to_string()),
    }
}

/// Resolve a `version.workspace = true` member by walking up to the workspace
/// root and reading `[workspace.package].version`.
fn resolve_workspace_version(manifest_path: &Path) -> Option<String> {
    let mut dir = manifest_path.parent()?.to_path_buf();
    loop {
        if let Ok(s) = load_string(&dir.join("Cargo.toml")) {
            if let Ok(v) = toml::from_str::<toml::Value>(&s) {
                if let Some(ver) = v
                    .get("workspace")
                    .and_then(|w| w.get("package"))
                    .and_then(|p| p.get("version"))
                    .and_then(|ver| ver.as_str())
                {
                    return Some(ver.to_string());
                }
            }
        }
        dir = dir.parent()?.to_path_buf();
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

/// Check if a Cargo.toml depends on the given crate name
///
/// Checks [dependencies], [dev-dependencies], and [build-dependencies].
pub fn depends_on(manifest_path: &Path, crate_name: &str) -> Result<bool, String> {
    let toml_str = load_string(manifest_path)?;
    let value: toml::Value = toml::from_str(&toml_str).map_err(|e| format!("Failed to parse TOML: {}", e))?;

    for section in &["dependencies", "dev-dependencies", "build-dependencies"] {
        if let Some(toml::Value::Table(deps)) = value.get(section)
            && deps.contains_key(crate_name)
        {
            return Ok(true);
        }
    }

    // Also check target-specific dependencies: [target.'cfg(...)'.dependencies]
    if let Some(toml::Value::Table(targets)) = value.get("target") {
        for (_target, target_val) in targets {
            if let toml::Value::Table(target_table) = target_val {
                for section in &["dependencies", "dev-dependencies", "build-dependencies"] {
                    if let Some(toml::Value::Table(deps)) = target_table.get(*section)
                        && deps.contains_key(crate_name)
                    {
                        return Ok(true);
                    }
                }
            }
        }
    }

    Ok(false)
}
