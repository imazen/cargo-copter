use std::fs::File;
use std::io::Read;
/// TOML parsing helpers for extracting dependency requirements
///
/// This module consolidates repeated TOML parsing logic for dependency
/// requirement extraction from Cargo.toml files.
use std::path::Path;

/// Extract the version requirement string from a toml dependency value
pub fn extract_requirement_string(req: &toml::Value) -> String {
    match req {
        toml::Value::String(s) => s.clone(),
        toml::Value::Table(t) => {
            // Handle { version = "1.0", features = [...] } format
            t.get("version").and_then(|v| v.as_str()).unwrap_or("*").to_string()
        }
        _ => "*".to_string(),
    }
}

/// Find dependency requirement in a Cargo.toml value
///
/// Searches through [dependencies], [dev-dependencies], and [build-dependencies]
/// sections for the specified crate name.
pub fn find_dependency_requirement(toml_content: &toml::Value, dep_name: &str) -> Option<String> {
    // Check [dependencies]
    if let Some(deps) = toml_content.get("dependencies").and_then(|v| v.as_table()) {
        if let Some(req) = deps.get(dep_name) {
            return Some(extract_requirement_string(req));
        }
    }

    // Check [dev-dependencies]
    if let Some(deps) = toml_content.get("dev-dependencies").and_then(|v| v.as_table()) {
        if let Some(req) = deps.get(dep_name) {
            return Some(extract_requirement_string(req));
        }
    }

    // Check [build-dependencies]
    if let Some(deps) = toml_content.get("build-dependencies").and_then(|v| v.as_table()) {
        if let Some(req) = deps.get(dep_name) {
            return Some(extract_requirement_string(req));
        }
    }

    None
}

/// Load and parse a Cargo.toml file
pub fn load_cargo_toml(path: &Path) -> Result<toml::Value, String> {
    let mut file = File::open(path).map_err(|e| format!("Failed to open {:?}: {}", path, e))?;
    let mut s = String::new();
    file.read_to_string(&mut s).map_err(|e| format!("Failed to read {:?}: {}", path, e))?;
    toml::from_str(&s).map_err(|e| format!("Failed to parse TOML in {:?}: {}", path, e))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_requirement_string_simple() {
        let req = toml::Value::String("^1.0.0".to_string());
        assert_eq!(extract_requirement_string(&req), "^1.0.0");
    }

    #[test]
    fn test_extract_requirement_string_table() {
        use toml::map::Map;
        let mut table = Map::new();
        table.insert("version".to_string(), toml::Value::String("^1.0.0".to_string()));
        table.insert("features".to_string(), toml::Value::Array(vec![]));
        let req = toml::Value::Table(table);
        assert_eq!(extract_requirement_string(&req), "^1.0.0");
    }

    #[test]
    fn test_extract_requirement_string_table_no_version() {
        use toml::map::Map;
        let mut table = Map::new();
        table.insert("path".to_string(), toml::Value::String("../local".to_string()));
        let req = toml::Value::Table(table);
        assert_eq!(extract_requirement_string(&req), "*");
    }

    #[test]
    fn test_find_dependency_requirement() {
        let toml_str = r#"
[package]
name = "test"

[dependencies]
serde = "1.0"

[dev-dependencies]
tokio = { version = "1.0", features = ["full"] }

[build-dependencies]
cc = "1.0"
"#;
        let toml_value: toml::Value = toml::from_str(toml_str).unwrap();

        assert_eq!(find_dependency_requirement(&toml_value, "serde"), Some("1.0".to_string()));
        assert_eq!(find_dependency_requirement(&toml_value, "tokio"), Some("1.0".to_string()));
        assert_eq!(find_dependency_requirement(&toml_value, "cc"), Some("1.0".to_string()));
        assert_eq!(find_dependency_requirement(&toml_value, "nonexistent"), None);
    }
}
