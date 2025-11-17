/// API module for interacting with crates.io
///
/// This module provides functions for fetching reverse dependencies,
/// resolving versions, and downloading crate files.

use crates_io_api::SyncClient;
use semver::Version;
use std::io::Read;
use std::time::Duration;
use log::debug;

const USER_AGENT: &str = "cargo-copter/0.1.1 (https://github.com/imazen/cargo-copter)";

lazy_static::lazy_static! {
    static ref CRATES_IO_CLIENT: SyncClient = {
        SyncClient::new(USER_AGENT, Duration::from_millis(1000))
            .expect("Failed to create crates.io API client")
    };
}

/// A reverse dependency (crate that depends on our crate)
#[derive(Debug, Clone)]
pub struct ReverseDependency {
    pub name: String,
    pub downloads: u64,
}

/// Get reverse dependencies with pagination and optional limiting
///
/// This uses the paginated API to avoid downloading all reverse deps at once.
/// Results are sorted by download count descending and limited to the requested amount.
///
/// # Arguments
/// * `crate_name` - The crate to find reverse dependencies for
/// * `limit` - Maximum number of dependents to return (default: all)
pub fn get_reverse_dependencies(
    crate_name: &str,
    limit: Option<usize>,
) -> Result<Vec<ReverseDependency>, String> {
    debug!("fetching reverse dependencies for {}", crate_name);

    let mut all_deps = Vec::new();

    // The API returns 100 items per page by default
    let per_page = 100;

    // Determine how many pages we need
    let max_pages = match limit {
        Some(lim) => ((lim + per_page - 1) / per_page), // Round up
        None => 100, // Safety limit: don't fetch more than 10,000 deps
    };

    for page in 1..=max_pages {
        debug!("fetching page {} of reverse dependencies", page);

        let deps = CRATES_IO_CLIENT
            .crate_reverse_dependencies_page(crate_name, page as u64)
            .map_err(|e| format!("Failed to fetch reverse dependencies: {}", e))?;

        let page_size = deps.dependencies.len();
        debug!("got {} dependencies on page {}", page_size, page);

        // Extract dependency info
        for dep in deps.dependencies {
            all_deps.push(ReverseDependency {
                name: dep.crate_version.crate_name.clone(),
                downloads: dep.crate_version.downloads,
            });
        }

        // If we got less than expected, we've reached the end
        if page_size < per_page {
            break;
        }

        // If we have enough, stop
        if let Some(lim) = limit {
            if all_deps.len() >= lim {
                break;
            }
        }
    }

    // Sort by downloads descending
    all_deps.sort_by_key(|d| std::cmp::Reverse(d.downloads));

    // Apply limit
    if let Some(lim) = limit {
        all_deps.truncate(lim);
    }

    debug!(
        "found {} reverse dependencies for {}",
        all_deps.len(),
        crate_name
    );

    Ok(all_deps)
}

/// Get top N reverse dependencies sorted by download count
///
/// # Arguments
/// * `crate_name` - The crate to find reverse dependencies for
/// * `limit` - Number of top dependents to return
pub fn get_top_dependents(
    crate_name: &str,
    limit: usize,
) -> Result<Vec<ReverseDependency>, String> {
    get_reverse_dependencies(crate_name, Some(limit))
}

/// Resolve the latest version of a crate from crates.io
///
/// # Arguments
/// * `crate_name` - The crate name to resolve
///
/// # Returns
/// The latest version of the crate
pub fn resolve_latest_version(crate_name: &str) -> Result<Version, String> {
    debug!("resolving latest version for {}", crate_name);

    let krate = CRATES_IO_CLIENT
        .get_crate(crate_name)
        .map_err(|e| format!("Failed to get crate info: {}", e))?;

    // Pull out the version numbers and sort them
    let versions: Vec<Version> = krate
        .versions
        .iter()
        .filter_map(|v| Version::parse(&v.num).ok())
        .collect();

    if versions.is_empty() {
        return Err(format!("No versions found for crate {}", crate_name));
    }

    let mut sorted_versions = versions;
    sorted_versions.sort();

    Ok(sorted_versions
        .pop()
        .expect("versions vec should not be empty"))
}

/// Download a .crate file from crates.io
///
/// # Arguments
/// * `crate_name` - Name of the crate
/// * `version` - Version string
///
/// # Returns
/// The raw bytes of the .crate file
pub fn download_crate(crate_name: &str, version: &str) -> Result<Vec<u8>, String> {
    debug!("downloading {}-{}.crate", crate_name, version);

    let url = format!(
        "https://crates.io/api/v1/crates/{}/{}/download",
        crate_name, version
    );

    let resp = ureq::get(&url)
        .set("User-Agent", USER_AGENT)
        .call()
        .map_err(|e| format!("HTTP request failed: {}", e))?;

    let len = resp
        .header("Content-Length")
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(0);

    let mut data: Vec<u8> = Vec::with_capacity(len);
    resp.into_reader()
        .read_to_end(&mut data)
        .map_err(|e| format!("Failed to read response: {}", e))?;

    debug!(
        "downloaded {} bytes for {}-{}",
        data.len(),
        crate_name,
        version
    );

    Ok(data)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Note: These tests require network access and hit the real crates.io API
    // They are here to verify the API works but should not be run in CI

    #[test]
    #[ignore] // Requires network access
    fn test_resolve_latest_version() {
        let version = resolve_latest_version("serde").unwrap();
        assert!(version >= Version::parse("1.0.0").unwrap());
    }

    #[test]
    #[ignore] // Requires network access
    fn test_get_top_dependents() {
        let deps = get_top_dependents("serde", 5).unwrap();
        assert_eq!(deps.len(), 5);

        // Should be sorted by downloads descending
        for i in 1..deps.len() {
            assert!(deps[i - 1].downloads >= deps[i].downloads);
        }
    }

    #[test]
    #[ignore] // Requires network access
    fn test_get_reverse_dependencies_with_limit() {
        let deps = get_reverse_dependencies("log", Some(10)).unwrap();
        assert_eq!(deps.len(), 10);
    }

    #[test]
    fn test_reverse_dependency_structure() {
        let dep = ReverseDependency {
            name: "test-crate".to_string(),
            downloads: 1000,
        };
        assert_eq!(dep.name, "test-crate");
        assert_eq!(dep.downloads, 1000);
    }
}
