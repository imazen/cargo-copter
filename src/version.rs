/// Version resolution and compatibility checking
///
/// This module handles:
/// - Resolving version keywords ("this", "latest", "latest-preview")
/// - Checking semver compatibility
/// - Resolving latest versions from crates.io
/// - Determining if WIP versions satisfy dependent requirements
use crate::api;
use crate::compile;
use crate::manifest;
use log::debug;
use semver::Version;
use std::path::PathBuf;

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
            Version::parse(version_str).map_err(|e| format!("Invalid version '{}': {}", version_str, e))?;

            // Literal version string (supports hyphens like "0.8.2-alpha2")
            Ok(Some(compile::VersionSource::Published { version: version_str.to_string(), forced: false }))
        }
    }
}

/// Resolve 'latest' or 'latest-preview' keyword to actual version
pub fn resolve_latest_version(crate_name: &str, include_prerelease: bool) -> Result<String, String> {
    debug!("Resolving latest version for {} (prerelease={})", crate_name, include_prerelease);

    let krate = api::get_client().get_crate(crate_name).map_err(|e| format!("Failed to fetch crate info: {}", e))?;

    select_latest_version(krate.versions.iter().map(|r| (r.num.as_str(), r.yanked)), include_prerelease)
}

fn select_latest_version<'a>(
    versions: impl IntoIterator<Item = (&'a str, bool)>,
    include_prerelease: bool,
) -> Result<String, String> {
    versions
        .into_iter()
        .filter(|(_, yanked)| !yanked)
        .filter_map(|(num, _)| Version::parse(num).ok())
        .filter(|v| include_prerelease || v.pre.is_empty())
        .max()
        .map(|v| v.to_string())
        .ok_or_else(|| "No non-yanked versions found matching the requested release channel".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn latest_uses_semver_order_and_excludes_yanked_releases() {
        let versions = [
            ("0.6.9", false),
            ("0.7.0", true),
            ("0.6.12", false),
            ("0.8.0-alpha.1", false),
            ("0.9.0-beta.1", true),
            ("invalid", false),
        ];
        assert_eq!(select_latest_version(versions, false).unwrap(), "0.6.12");
        assert_eq!(select_latest_version(versions, true).unwrap(), "0.8.0-alpha.1");
    }

    #[test]
    fn latest_does_not_fall_back_to_yanked_or_prerelease() {
        assert!(select_latest_version([("1.0.0", true)], true).is_err());
        assert!(select_latest_version([("1.0.0-beta.1", false)], false).is_err());
        assert!(select_latest_version([], false).is_err());
    }
}
