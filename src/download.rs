/// Download and crate archive handling
///
/// This module handles:
/// - HTTP requests to crates.io
/// - Downloading .crate files
/// - Extracting crate archives
/// - Caching downloaded crates

use flate2::read::GzDecoder;
use log::debug;
use semver::Version;
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use tar::Archive;

use crate::cli::default_cache_dir;

const USER_AGENT: &str = "cargo-copter/0.1.1 (https://github.com/imazen/cargo-copter)";

/// Get the crate cache directory
fn crate_cache_dir() -> PathBuf {
    default_cache_dir().join("crate-cache")
}

/// Build a crates.io API URL
pub fn crate_url(krate: &str, call: Option<&str>) -> String {
    crate_url_with_parms(krate, call, &[])
}

/// Build a crates.io API URL with query parameters
pub fn crate_url_with_parms(krate: &str, call: Option<&str>, parms: &[(&str, &str)]) -> String {
    let url = format!("https://crates.io/api/v1/crates/{}", krate);
    let s = match call {
        Some(c) => format!("{}/{}", url, c),
        None => url,
    };

    if !parms.is_empty() {
        let parms: Vec<String> = parms.iter().map(|&(k, v)| format!("{}={}", k, v)).collect();
        let parms: String = parms.join("&");
        format!("{}?{}", s, parms)
    } else {
        s
    }
}

/// Download data from a URL using HTTP GET
pub fn http_get_bytes(url: &str) -> Result<Vec<u8>, ureq::Error> {
    let resp = ureq::get(url).set("User-Agent", USER_AGENT).call()?;
    let len = resp.header("Content-Length").and_then(|s| s.parse::<usize>().ok()).unwrap_or(0);
    let mut data: Vec<u8> = Vec::with_capacity(len);
    resp.into_reader().read_to_end(&mut data)?;
    Ok(data)
}

/// Handle to a downloaded .crate file
pub struct CrateHandle(PathBuf);

impl CrateHandle {
    /// Unpack the crate source to a directory
    pub fn unpack_source_to(&self, path: &Path) -> std::io::Result<()> {
        debug!("unpacking {:?} to {:?}", self.0, path);
        extract_crate_archive(&self.0, path)
    }

    /// Get the path to the .crate file
    pub fn path(&self) -> &Path {
        &self.0
    }
}

/// Download a crate file (with caching)
pub fn get_crate_handle(crate_name: &str, version: &Version) -> std::io::Result<CrateHandle> {
    let cache_path = crate_cache_dir();
    let crate_dir = cache_path.join(crate_name);
    fs::create_dir_all(&crate_dir)?;

    let crate_file = crate_dir.join(format!("{}-{}.crate", crate_name, version));

    // Check if file exists
    if !crate_file.exists() {
        let url = crate_url(crate_name, Some(&format!("{}/download", version)));
        let body = http_get_bytes(&url)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))?;

        // Write atomically: write to temp file, then rename
        let temp_file = crate_dir.join(format!("{}-{}.crate.tmp", crate_name, version));
        let mut file = File::create(&temp_file)?;
        file.write_all(&body)?;
        file.flush()?;
        drop(file);

        fs::rename(&temp_file, &crate_file)?;
    }

    Ok(CrateHandle(crate_file))
}

/// Download and unpack a specific version of a crate for patching
/// Returns the path to the unpacked source
pub fn download_and_unpack_crate(
    crate_name: &str,
    version: &str,
    staging_dir: &Path,
) -> std::io::Result<PathBuf> {
    debug!("Downloading and unpacking {} version {}", crate_name, version);

    // Parse version
    let vers = Version::parse(version)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidInput, e.to_string()))?;

    // Download the crate
    let crate_handle = get_crate_handle(crate_name, &vers)?;

    // Unpack to staging directory
    let unpack_path = staging_dir.join(format!("base-{}-{}", crate_name, version));
    if !unpack_path.exists() {
        fs::create_dir_all(&unpack_path)?;
        crate_handle.unpack_source_to(&unpack_path)?;
        debug!("Unpacked {} {} to {:?}", crate_name, version, unpack_path);
    } else {
        debug!("Using cached base crate at {:?}", unpack_path);
    }

    Ok(unpack_path)
}

/// Extract all files from a .crate file (gzipped tar) with --strip-components=1 behavior
pub fn extract_crate_archive(crate_file: &Path, dest_dir: &Path) -> std::io::Result<()> {
    let file = File::open(crate_file)?;
    let decoder = GzDecoder::new(file);
    let mut archive = Archive::new(decoder);

    for entry in archive.entries()? {
        let mut entry = entry?;
        let path = entry.path()?;

        // Strip the first path component (equivalent to --strip-components=1)
        let stripped_pathbuf = path.components().skip(1).collect::<PathBuf>();
        if stripped_pathbuf.as_os_str().is_empty() {
            continue; // Skip entries with no path after stripping
        }

        let dest_path = dest_dir.join(&stripped_pathbuf);

        // Ensure parent directory exists
        if let Some(parent) = dest_path.parent() {
            fs::create_dir_all(parent)?;
        }

        // Extract the entry
        entry.unpack(&dest_path)?;
    }

    Ok(())
}

/// Extract only Cargo.toml from a .crate file with --strip-components=1 behavior
pub fn extract_cargo_toml(crate_file: &Path, dest_dir: &Path) -> std::io::Result<()> {
    let file = File::open(crate_file)?;
    let decoder = GzDecoder::new(file);
    let mut archive = Archive::new(decoder);

    for entry in archive.entries()? {
        let mut entry = entry?;
        let path = entry.path()?;

        // Check if this is a Cargo.toml file
        if path.file_name() != Some(std::ffi::OsStr::new("Cargo.toml")) {
            continue;
        }

        // Strip the first path component (equivalent to --strip-components=1)
        let stripped_pathbuf = path.components().skip(1).collect::<PathBuf>();
        if stripped_pathbuf.as_os_str().is_empty() {
            continue;
        }

        let dest_path = dest_dir.join(&stripped_pathbuf);

        // Ensure parent directory exists
        if let Some(parent) = dest_path.parent() {
            fs::create_dir_all(parent)?;
        }

        // Extract the entry
        entry.unpack(&dest_path)?;
    }

    Ok(())
}
