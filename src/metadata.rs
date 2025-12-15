use serde_json::Value;
use std::collections::HashMap;

#[derive(Debug)]
pub struct ParsedMetadata {
    pub packages: HashMap<String, Value>,
    pub resolve: Option<Value>,
}

#[derive(Debug, Clone)]
pub struct VersionInfo {
    pub version: String,
    pub spec: String,
    pub node_id: String,
}

/// Parse cargo metadata JSON and build a package lookup map
pub fn parse_metadata(metadata_json: &str) -> Result<ParsedMetadata, String> {
    let metadata: Value =
        serde_json::from_str(metadata_json).map_err(|e| format!("Failed to parse metadata JSON: {}", e))?;

    let mut packages = HashMap::new();

    if let Some(packages_array) = metadata.get("packages").and_then(|p| p.as_array()) {
        for package in packages_array {
            if let Some(id) = package.get("id").and_then(|i| i.as_str()) {
                packages.insert(id.to_string(), package.clone());
            }
        }
    }

    let resolve = metadata.get("resolve").cloned();

    Ok(ParsedMetadata { packages, resolve })
}

/// Get the version spec for a specific crate dependency
/// Returns "?" if the spec cannot be determined
/// Returns an error if the package is not found in metadata
pub fn get_version_spec(parsed: &ParsedMetadata, node_id: &str, crate_name: &str) -> Result<String, String> {
    let package = parsed
        .packages
        .get(node_id)
        .ok_or_else(|| format!("Package not found in metadata for node_id: {}", node_id))?;

    if let Some(dependencies) = package.get("dependencies").and_then(|d| d.as_array()) {
        // Find dependency by matching simple crate name
        for dep in dependencies {
            if let Some(dep_name) = dep.get("name").and_then(|n| n.as_str())
                && dep_name == crate_name
            {
                return Ok(dep.get("req").and_then(|r| r.as_str()).unwrap_or("?").to_string());
            }
        }
    }

    // No matching dependency found - this is expected for transitive deps
    Ok("?".to_string())
}

/// Find all versions of a specific crate in the dependency tree
/// Returns a vector of (version, spec, node_id) tuples
pub fn find_all_versions(parsed: &ParsedMetadata, crate_name: &str) -> Vec<VersionInfo> {
    let mut versions = Vec::new();

    if let Some(resolve) = &parsed.resolve
        && let Some(nodes) = resolve.get("nodes").and_then(|n| n.as_array())
    {
        for node in nodes {
            let node_id = node.get("id").and_then(|i| i.as_str()).unwrap_or("");

            if let Some(deps) = node.get("deps").and_then(|d| d.as_array()) {
                for dep in deps {
                    let pkg = dep.get("pkg").and_then(|p| p.as_str()).unwrap_or("");

                    // Extract crate name and version from package ID
                    // Format: "source#name@version" (e.g., "registry+https://...#rgb@0.8.52")
                    if let Some(hash_pos) = pkg.find('#') {
                        let after_hash = &pkg[hash_pos + 1..];
                        if let Some(at_pos) = after_hash.find('@') {
                            let dep_crate_name = &after_hash[..at_pos];
                            let version = &after_hash[at_pos + 1..];

                            if dep_crate_name == crate_name {
                                // Get the version spec from the dependent package
                                let spec =
                                    get_version_spec(parsed, node_id, crate_name).unwrap_or_else(|_| "?".to_string());

                                versions.push(VersionInfo {
                                    version: version.to_string(),
                                    spec,
                                    node_id: node_id.to_string(),
                                });
                            }
                        }
                    }
                }
            }
        }
    }

    versions
}

/// Get the dependent's name and version from a node ID
/// Returns (name, version) or None if parsing fails
pub fn parse_node_id(node_id: &str) -> Option<(String, String)> {
    // Format: "source#name@version" (e.g., "registry+https://...#rgb@0.8.52")
    if let Some(hash_pos) = node_id.find('#') {
        let after_hash = &node_id[hash_pos + 1..];
        if let Some(at_pos) = after_hash.find('@') {
            let name = &after_hash[..at_pos];
            let version = &after_hash[at_pos + 1..];
            return Some((name.to_string(), version.to_string()));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_node_id() {
        let node_id = "registry+https://github.com/rust-lang/crates.io-index#rgb@0.8.52";
        let result = parse_node_id(node_id);
        assert_eq!(result, Some(("rgb".to_string(), "0.8.52".to_string())));
    }

    #[test]
    fn test_parse_node_id_invalid() {
        assert_eq!(parse_node_id("invalid"), None);
        assert_eq!(parse_node_id("no-version (registry)"), None);
    }

    #[test]
    fn test_parse_empty_metadata() {
        let json = r#"{"packages": [], "resolve": {"nodes": []}}"#;
        let result = parse_metadata(json);
        assert!(result.is_ok());
    }

    #[test]
    fn test_get_version_spec_not_found() {
        let json = r#"{"packages": [], "resolve": {"nodes": []}}"#;
        let parsed = parse_metadata(json).unwrap();
        let result = get_version_spec(&parsed, "fake-id", "rgb");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Package not found"));
    }
}
