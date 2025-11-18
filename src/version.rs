/// Version resolution and compatibility checking
///
/// This module handles:
/// - Resolving version keywords ("this", "latest", "latest-preview")
/// - Checking semver compatibility
/// - Resolving latest versions from crates.io
/// - Determining if WIP versions satisfy dependent requirements

use crate::api;
use crate::compile;
use crate::download;
use crate::manifest::{self, RevDep};
use crate::toml_helpers;
use log::debug;
use semver::{Version, VersionReq};
use std::fs;
use std::path::PathBuf;
use tempfile::TempDir;

/// Resolve a version keyword ("this", "latest", "latest-preview") or concrete version
///
/// Returns None if resolution fails (with warning printed to UI)
pub fn resolve_version_keyword(
    version_str: &str,
    crate_name: &str,
    local_manifest: Option<&PathBuf>,
) -> Result<Option<compile::VersionSource>, String> {
    match version_str {
        "this" => {
            // User explicitly requested WIP version
            if let Some(manifest_path) = local_manifest {
                debug!("Resolved 'this' to local WIP at {:?}", manifest_path);
                Ok(Some(compile::VersionSource::Local { path: manifest_path.clone(), forced: false }))
            } else {
                crate::ui::status("Warning: 'this' specified but no local source available (--path or --crate)");
                Ok(None)
            }
        }
        "latest" => {
            // Resolve to latest stable version
            match resolve_latest_version(crate_name, false) {
                Ok(ver) => {
                    debug!("Resolved 'latest' to {}", ver);
                    Ok(Some(compile::VersionSource::Published { version: ver, forced: false }))
                }
                Err(e) => {
                    crate::ui::status(&format!("Warning: Failed to resolve 'latest': {}", e));
                    Ok(None)
                }
            }
        }
        "latest-preview" | "latest-prerelease" => {
            // Resolve to latest version including pre-releases
            match resolve_latest_version(crate_name, true) {
                Ok(ver) => {
                    debug!("Resolved '{}' to {}", version_str, ver);
                    Ok(Some(compile::VersionSource::Published { version: ver, forced: false }))
                }
                Err(e) => {
                    crate::ui::status(&format!("Warning: Failed to resolve '{}': {}", version_str, e));
                    Ok(None)
                }
            }
        }
        _ => {
            // Validate it's a concrete version, not a version requirement
            if version_str.starts_with('^') || version_str.starts_with('~') || version_str.starts_with('=') {
                return Err(format!(
                    "Version requirement '{}' not allowed. Use concrete versions like '0.8.52'",
                    version_str
                ));
            }

            // Validate it's a valid semver version
            Version::parse(version_str)
                .map_err(|e| format!("Invalid version '{}': {}", version_str, e))?;

            // Literal version string (supports hyphens like "0.8.2-alpha2")
            Ok(Some(compile::VersionSource::Published { version: version_str.to_string(), forced: false }))
        }
    }
}

/// Resolve 'latest' or 'latest-preview' keyword to actual version
pub fn resolve_latest_version(crate_name: &str, include_prerelease: bool) -> Result<String, String> {
    debug!("Resolving latest version for {} (prerelease={})", crate_name, include_prerelease);

    let krate = api::get_client().get_crate(crate_name)
        .map_err(|e| format!("Failed to fetch crate info: {}", e))?;

    // Filter and sort versions
    let mut versions: Vec<Version> = krate
        .versions
        .iter()
        .filter_map(|r| Version::parse(&r.num).ok())
        .filter(|v| include_prerelease || v.pre.is_empty()) // Filter pre-releases unless requested
        .collect();

    versions.sort();

    versions.pop().map(|v| v.to_string()).ok_or_else(|| "No versions found".to_string())
}

/// Resolve reverse dependency version (use provided version or latest from crates.io)
pub fn resolve_rev_dep_version(name: String, version: Option<String>) -> Result<RevDep, String> {
    // If version is provided, use it directly
    if let Some(ver_str) = version {
        debug!("using pinned version {} for {}", ver_str, name);
        let vers = Version::parse(&ver_str)
            .map_err(|e| format!("Invalid version: {}", e))?;
        return Ok(RevDep { name, vers, resolved_version: None });
    }

    // Otherwise, resolve latest version from crates.io
    debug!("resolving current version for {}", name);

    let krate = api::get_client().get_crate(&name)
        .map_err(|e| format!("Failed to fetch crate: {}", e))?;

    // Pull out the version numbers and sort them
    let versions = krate.versions.iter().filter_map(|r| Version::parse(&r.num).ok());
    let mut versions = versions.collect::<Vec<_>>();
    versions.sort();

    versions.pop()
        .map(|v| RevDep { name, vers: v, resolved_version: None })
        .ok_or_else(|| "No versions found".to_string())
}

/// Check if a WIP version is compatible with a dependent's semver requirement
///
/// Downloads the dependent's Cargo.toml and checks if the WIP version
/// satisfies the version requirement specified for our crate.
pub fn check_version_compatibility(
    rev_dep: &RevDep,
    crate_name: &str,
    wip_version: &str,
) -> Result<bool, String> {
    debug!("checking version compatibility for {} {}", rev_dep.name, rev_dep.vers);

    // Download and cache the dependent's .crate file
    let crate_handle = download::get_crate_handle(&rev_dep.name, &rev_dep.vers)
        .map_err(|e| format!("Failed to download crate: {}", e))?;

    // Create temp directory to extract Cargo.toml
    let temp_dir = TempDir::new()
        .map_err(|e| format!("Failed to create temp dir: {}", e))?;
    let extract_dir = temp_dir.path().join("extracted");
    fs::create_dir(&extract_dir)
        .map_err(|e| format!("Failed to create extract dir: {}", e))?;

    // Extract just the Cargo.toml
    download::extract_cargo_toml(crate_handle.path(), &extract_dir)
        .map_err(|e| format!("Failed to extract Cargo.toml: {}", e))?;

    // Read and parse Cargo.toml
    let toml_path = extract_dir.join("Cargo.toml");
    let toml_str = manifest::load_string(&toml_path)?;
    let value: toml::Value = toml::from_str(&toml_str)
        .map_err(|e| format!("Failed to parse TOML: {}", e))?;

    // Look for our crate in dependencies
    let wip_ver = Version::parse(wip_version)
        .map_err(|e| format!("Invalid WIP version: {}", e))?;

    // Check [dependencies]
    if let Some(deps) = value.get("dependencies").and_then(|v| v.as_table()) {
        if let Some(req) = deps.get(crate_name) {
            return check_requirement(req, &wip_ver);
        }
    }

    // Check [dev-dependencies]
    if let Some(deps) = value.get("dev-dependencies").and_then(|v| v.as_table()) {
        if let Some(req) = deps.get(crate_name) {
            return check_requirement(req, &wip_ver);
        }
    }

    // Check [build-dependencies]
    if let Some(deps) = value.get("build-dependencies").and_then(|v| v.as_table()) {
        if let Some(req) = deps.get(crate_name) {
            return check_requirement(req, &wip_ver);
        }
    }

    // Crate not found in dependencies (shouldn't happen for reverse deps)
    debug!("Warning: {} not found in {}'s dependencies", crate_name, rev_dep.name);
    Ok(true) // Test anyway
}

/// Check if a version satisfies a TOML dependency requirement
fn check_requirement(req: &toml::Value, wip_version: &Version) -> Result<bool, String> {
    let req_str = toml_helpers::extract_requirement_string(req);

    debug!("Checking if version {} satisfies requirement '{}'", wip_version, req_str);

    let version_req = VersionReq::parse(&req_str)
        .map_err(|e| format!("Invalid version requirement: {}", e))?;

    Ok(version_req.matches(wip_version))
}
