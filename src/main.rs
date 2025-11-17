// Copyright 2015 The Rust Project Developers. See the COPYRIGHT
// file at the top-level directory of this distribution and at
// http://rust-lang.org/COPYRIGHT.
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

mod api;
mod cli;
mod compile;
mod console_tables;
mod error_extract;
mod report;

use semver::Version;
use std::env;
use std::error::Error as StdError;
use std::fmt;
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::{PathBuf, Path};
use std::process::Command;
use std::string::FromUtf8Error;
use std::sync::Mutex;
use std::sync::mpsc::{self, Sender, Receiver, RecvError};
use std::time::Duration;
use threadpool::ThreadPool;
use tempfile::TempDir;
use crates_io_api::SyncClient;

use lazy_static::lazy_static;
use log::debug;

const USER_AGENT: &str = "cargo-copter/0.1.1 (https://github.com/imazen/cargo-copter)";

lazy_static! {
    static ref CRATES_IO_CLIENT: SyncClient = {
        SyncClient::new(USER_AGENT, Duration::from_millis(1000))
            .expect("Failed to create crates.io API client")
    };
}

fn main() {
    env_logger::init();

    // Parse CLI arguments
    let args = cli::CliArgs::parse_args();

    // Validate arguments
    if let Err(e) = args.validate() {
        eprintln!("Error: {}", e);
        std::process::exit(1);
    }

    // Get config
    let config = match get_config(&args) {
        Ok(c) => c,
        Err(e) => {
            report_error(e);
            return;
        }
    };

    // Run tests and report results
    let results = run(args.clone(), config.clone());
    report_results(results, &args, &config);
}

/// Parse dependent spec in "name" or "name:version" format
fn parse_dependent_spec(spec: &str) -> (String, Option<String>) {
    match spec.split_once(':') {
        Some((name, version)) => (name.to_string(), Some(version.to_string())),
        None => (spec.to_string(), None),
    }
}

/// Print a compact test plan showing what will be tested
fn print_test_plan(
    rev_deps: &[(String, Option<String>)],
    versions: &[compile::VersionSource],
    force_versions: &[String],
    force_local: bool,
    config: &Config,
) {
    // Format dependents list
    let deps_display: Vec<String> = rev_deps.iter()
        .map(|(name, ver)| {
            if let Some(v) = ver {
                format!("{}:{}", name, v)
            } else {
                name.clone()
            }
        })
        .collect();

    // Format versions list with force indicators (deduplicate versions)
    let mut versions_display = Vec::new();
    let mut seen_versions = std::collections::HashSet::new();
    versions_display.push("baseline".to_string()); // baseline is always tested first

    for version in versions {
        let (version_str, is_forced) = match version {
            compile::VersionSource::Published(v) => {
                (v.clone(), force_versions.contains(v))
            }
            compile::VersionSource::Local(_) => {
                ("this".to_string(), force_local)
            }
        };

        // Skip if we've already seen this version (dedup)
        if seen_versions.contains(&version_str) {
            continue;
        }
        seen_versions.insert(version_str.clone());

        if is_forced {
            versions_display.push(format!("{} [!]", version_str));
        } else {
            versions_display.push(version_str);
        }
    }

    // Print compact plan
    println!("\nTest Plan:");
    println!("  {} × {} = {} tests",
        rev_deps.len(),
        versions_display.len(),
        rev_deps.len() * versions_display.len()
    );

    // Show dependents (compact, comma-separated, max 80 chars per line)
    print!("  Dependents: ");
    let deps_str = deps_display.join(", ");
    if deps_str.len() <= 70 {
        println!("{}", deps_str);
    } else {
        // Wrap at reasonable points
        let mut line = String::new();
        for (i, dep) in deps_display.iter().enumerate() {
            if i > 0 {
                line.push_str(", ");
            }
            if line.len() + dep.len() > 70 && !line.is_empty() {
                println!("{}", line);
                print!("              ");
                line.clear();
            }
            line.push_str(dep);
        }
        if !line.is_empty() {
            println!("{}", line);
        }
    }

    // Show versions (compact, comma-separated)
    println!("  Versions: {}", versions_display.join(", "));
    println!();
}

fn run(args: cli::CliArgs, config: Config) -> Result<Vec<TestResult>, Error> {
    // Initialize failure log
    let log_path = std::env::current_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join("copter-failures.log");
    compile::init_failure_log(log_path.clone());
    debug!("Failure log initialized at: {:?}", log_path);

    // Phase 5: Check if we're doing multi-version testing
    let use_multi_version = !args.test_versions.is_empty() || !args.force_versions.is_empty();

    // Track which "this" versions are forced
    // Auto-added WIP is forced by default, explicitly specified ones check the list
    let mut force_local = args.force_versions.iter().any(|v| v == "this");

    // Build list of versions to test (Phase 5)
    let test_versions: Option<Vec<compile::VersionSource>> = if use_multi_version {
        let mut versions = Vec::new();

        // Add specified versions from --test-versions, resolving keywords
        for ver_str in &args.test_versions {
            let version_source = match ver_str.as_str() {
                "this" => {
                    // User explicitly requested WIP in patch mode (non-forced)
                    if let CrateOverride::Source(ref manifest_path) = config.next_override {
                        debug!("Resolved 'this' to local WIP (non-forced)");
                        compile::VersionSource::Local(manifest_path.clone())
                    } else {
                        status("Warning: 'this' specified but no local source available (--path or --crate)");
                        continue;
                    }
                }
                "latest" => {
                    // Resolve to latest stable version
                    match resolve_latest_version(&config.crate_name, false) {
                        Ok(ver) => {
                            debug!("Resolved 'latest' to {}", ver);
                            compile::VersionSource::Published(ver)
                        }
                        Err(e) => {
                            status(&format!("Warning: Failed to resolve 'latest': {}", e));
                            continue;
                        }
                    }
                }
                "latest-preview" | "latest-prerelease" => {
                    // Resolve to latest version including pre-releases
                    match resolve_latest_version(&config.crate_name, true) {
                        Ok(ver) => {
                            debug!("Resolved 'latest-preview' to {}", ver);
                            compile::VersionSource::Published(ver)
                        }
                        Err(e) => {
                            status(&format!("Warning: Failed to resolve 'latest-preview': {}", e));
                            continue;
                        }
                    }
                }
                _ => {
                    // Validate it's a concrete version, not a version requirement
                    if ver_str.starts_with('^') || ver_str.starts_with('~') || ver_str.starts_with('=') {
                        return Err(Error::InvalidVersion(format!(
                            "Version requirement '{}' not allowed in --test-versions. Use concrete versions like '0.8.52'",
                            ver_str
                        )));
                    }

                    // Validate it's a valid semver version
                    if let Err(e) = Version::parse(ver_str) {
                        return Err(Error::SemverError(e));
                    }

                    // Literal version string (supports hyphens like "0.8.2-alpha2")
                    compile::VersionSource::Published(ver_str.clone())
                }
            };
            versions.push(version_source);
        }

        // Add versions from --force-versions (these will be marked as forced in run_multi_version_test)
        for ver_str in &args.force_versions {
            let version_source = match ver_str.as_str() {
                "this" => {
                    // User explicitly requested WIP in force mode
                    if let CrateOverride::Source(ref manifest_path) = config.next_override {
                        debug!("Resolved 'this' to local WIP (forced)");
                        compile::VersionSource::Local(manifest_path.clone())
                    } else {
                        status("Warning: 'this' specified but no local source available (--path or --crate)");
                        continue;
                    }
                }
                "latest" => {
                    match resolve_latest_version(&config.crate_name, false) {
                        Ok(ver) => {
                            debug!("Resolved 'latest' to {}", ver);
                            compile::VersionSource::Published(ver)
                        }
                        Err(e) => {
                            status(&format!("Warning: Failed to resolve 'latest': {}", e));
                            continue;
                        }
                    }
                }
                "latest-preview" | "latest-prerelease" => {
                    match resolve_latest_version(&config.crate_name, true) {
                        Ok(ver) => {
                            debug!("Resolved 'latest-preview' to {}", ver);
                            compile::VersionSource::Published(ver)
                        }
                        Err(e) => {
                            status(&format!("Warning: Failed to resolve 'latest-preview': {}", e));
                            continue;
                        }
                    }
                }
                _ => {
                    // Validate it's a concrete version, not a version requirement
                    if ver_str.starts_with('^') || ver_str.starts_with('~') || ver_str.starts_with('=') {
                        return Err(Error::InvalidVersion(format!(
                            "Version requirement '{}' not allowed in --force-versions. Use concrete versions like '0.8.52'",
                            ver_str
                        )));
                    }

                    // Validate it's a valid semver version
                    if let Err(e) = Version::parse(ver_str) {
                        return Err(Error::SemverError(e));
                    }

                    compile::VersionSource::Published(ver_str.clone())
                }
            };
            versions.push(version_source);
        }

        // Auto-add "this" (local WIP) in forced mode if not already specified
        // Default: --test-versions baseline --force-versions this
        if let CrateOverride::Source(ref manifest_path) = config.next_override {
            // Check if "this" is already in the list
            let this_already_added = versions.iter().any(|v| {
                matches!(v, compile::VersionSource::Local(_))
            });

            if !this_already_added {
                debug!("Auto-adding 'this' version from {:?} (forced by default)", manifest_path);
                versions.push(compile::VersionSource::Local(manifest_path.clone()));
                // Mark auto-added WIP as forced
                force_local = true;
            }
        } else {
            // No local version (only --crate), add "latest" as final version
            match resolve_latest_version(&config.crate_name, false) {
                Ok(ver) => {
                    debug!("No local version, adding latest: {}", ver);
                    versions.push(compile::VersionSource::Published(ver));
                }
                Err(e) => {
                    status(&format!("Warning: Failed to resolve latest version: {}", e));
                }
            }
        }

        Some(versions)
    } else {
        // No --test-versions or --force-versions specified
        // Default behavior: test baseline (auto-inferred) + this (forced)
        force_local = true;
        None
    };

    // Determine which dependents to test (returns Vec<(name, optional_version)>)
    let rev_deps: Vec<(RevDepName, Option<String>)> = if !args.dependent_paths.is_empty() {
        // Local paths mode - convert to rev dep names (no version spec)
        args.dependent_paths
            .iter()
            .map(|p| {
                p.file_name()
                    .and_then(|n| n.to_str())
                    .map(|s| (s.to_string(), None))
                    .ok_or_else(|| Error::InvalidPath(p.clone()))
            })
            .collect::<Result<Vec<_>, _>>()?
    } else if !args.dependents.is_empty() {
        // Explicit crate names from crates.io (parse name:version syntax)
        args.dependents.iter()
            .map(|spec| parse_dependent_spec(spec))
            .collect()
    } else {
        // Top N by downloads (no version spec)
        let api_deps = api::get_top_dependents(&config.crate_name, args.top_dependents)
            .map_err(|e| Error::CratesIoApiError(e))?;
        api_deps.into_iter().map(|d| (d.name, None)).collect()
    };

    // Run all the tests in a thread pool and create a list of result
    // receivers.
    let mut result_rxs = Vec::new();
    let ref mut pool = ThreadPool::new(args.jobs);

    // Build version list for display (same logic as per-dependent)
    let versions_to_test = test_versions.clone().unwrap_or_else(|| {
        let mut versions = Vec::new();
        if let CrateOverride::Source(ref manifest_path) = config.next_override {
            versions.push(compile::VersionSource::Local(manifest_path.clone()));
        } else {
            if let Ok(ver) = resolve_latest_version(&config.crate_name, false) {
                versions.push(compile::VersionSource::Published(ver));
            }
        }
        versions
    });

    // Print test plan
    print_test_plan(&rev_deps, &versions_to_test, &config.force_versions, force_local, &config);

    for (rev_dep, version) in rev_deps {
        // Always use multi-version testing (legacy path removed)
        // If --test-versions not specified, build vec with just "this" - baseline will be auto-inferred
        let versions = test_versions.clone().unwrap_or_else(|| {
            let mut versions = Vec::new();
            // Add "this" (local WIP) or "latest" if no local version
            if let CrateOverride::Source(ref manifest_path) = config.next_override {
                versions.push(compile::VersionSource::Local(manifest_path.clone()));
            } else {
                // No local version (only --crate), add "latest" as final version
                if let Ok(ver) = resolve_latest_version(&config.crate_name, false) {
                    versions.push(compile::VersionSource::Published(ver));
                }
            }
            versions
        });

        let result = run_test_multi_version(pool, config.clone(), rev_dep, version, versions, force_local);
        result_rxs.push(result);
    }

    // Print table header for streaming output
    let total = result_rxs.len();
    report::print_table_header(&config.crate_name, &config.display_version(), total);

    // Stream results as they arrive
    let mut all_rows = Vec::new();
    for (i, result_rx) in result_rxs.into_iter().enumerate() {
        let result = result_rx.recv();

        // Status line removed - redundant with table output
        // report_quick_result(i + 1, total, &result);

        // Convert to OfferedRows and stream print
        let rows = result.to_offered_rows();
        for (j, row) in rows.iter().enumerate() {
            let is_last_in_group = j == rows.len() - 1;
            report::print_offered_row(row, is_last_in_group);
        }

        // Print separator after each dependent
        if i < total - 1 {
            report::print_separator_line();
        }

        all_rows.extend(rows);
    }

    // Print table footer
    report::print_table_footer();

    // Print summary
    let summary = report::summarize_offered_rows(&all_rows);
    report::print_summary(&summary);

    // Generate markdown report
    let markdown_path = PathBuf::from("copter-report.md");
    match report::export_markdown_table_report(&all_rows, &markdown_path, &config.crate_name, &config.display_version(), total) {
        Ok(_) => {
            println!("Markdown report: {}", markdown_path.display());
        }
        Err(e) => {
            eprintln!("Warning: Failed to generate markdown report: {}", e);
        }
    }

    // Exit with error code if there were regressions
    if summary.regressed > 0 {
        std::process::exit(-2);
    }

    // For now, still return TestResults for compatibility
    // TODO: Eventually remove this and just work with OfferedRows
    Ok(vec![])
}

#[derive(Clone)]
struct Config {
    crate_name: String,
    version: String,
    git_hash: Option<String>,
    is_dirty: bool,
    staging_dir: PathBuf,
    base_override: CrateOverride,
    next_override: CrateOverride,
    limit: Option<usize>,
    force_versions: Vec<String>,  // List of versions to force (bypass semver)
}

impl Config {
    /// Get formatted version string for display
    /// Examples: "1.0.0 abc123f*", "1.0.0 abc123f", "1.0.0*", "1.0.0"
    fn display_version(&self) -> String {
        match (&self.git_hash, self.is_dirty) {
            (Some(hash), true) => format!("{} {}*", self.version, hash),
            (Some(hash), false) => format!("{} {}", self.version, hash),
            (None, true) => format!("{}*", self.version),
            (None, false) => self.version.clone(),
        }
    }
}

#[derive(Clone)]
enum CrateOverride {
    Default,
    Source(PathBuf)
}

/// Get short git hash (7 chars) if in a git repository
fn get_git_hash() -> Option<String> {
    Command::new("git")
        .args(&["rev-parse", "--short", "HEAD"])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .map(|s| s.trim().to_string())
}

/// Check if git working directory is dirty (has uncommitted changes)
fn is_git_dirty() -> bool {
    Command::new("git")
        .args(&["status", "--porcelain"])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .map(|s| !s.trim().is_empty())
        .unwrap_or(false)
}

fn get_config(args: &cli::CliArgs) -> Result<Config, Error> {
    let limit = env::var("COPTER_LIMIT")
        .ok()
        .and_then(|s| s.parse::<usize>().ok());

    // Determine crate name and version based on --crate and --path
    let (crate_name, version, next_override) = if let Some(ref crate_name) = args.crate_name {
        // --crate specified: use that name
        debug!("Using crate name from --crate: {}", crate_name);

        // Check if --path is also specified (for "this" version)
        let (version, next_override) = if let Some(ref path) = args.path {
            let manifest = if path.is_dir() {
                path.join("Cargo.toml")
            } else {
                path.clone()
            };
            debug!("Using --path for 'this' version: {:?}", manifest);

            // Extract version from the manifest
            let (manifest_crate_name, manifest_version) = get_crate_info(&manifest)?;

            // Verify crate names match
            if manifest_crate_name != *crate_name {
                return Err(Error::ProcessError(format!(
                    "Crate name mismatch: --crate specifies '{}' but {} contains '{}'",
                    crate_name,
                    manifest.display(),
                    manifest_crate_name
                )));
            }

            (manifest_version, CrateOverride::Source(manifest))
        } else {
            // No --path, so there's no "this" version
            // Fetch latest version from crates.io for display purposes
            debug!("No --path specified, fetching latest version from crates.io");
            let latest_version = match resolve_latest_version(crate_name, false) {
                Ok(v) => {
                    debug!("Latest version of {} is {}", crate_name, v);
                    v
                }
                Err(e) => {
                    debug!("Failed to fetch latest version: {}, using 0.0.0", e);
                    "0.0.0".to_string()
                }
            };
            (latest_version, CrateOverride::Default)
        };

        (crate_name.clone(), version, next_override)
    } else {
        // No --crate, use --path or ./Cargo.toml
        let manifest = if let Some(ref path) = args.path {
            if path.is_dir() {
                path.join("Cargo.toml")
            } else {
                path.clone()
            }
        } else {
            let env_manifest = env::var("COPTER_MANIFEST");
            PathBuf::from(env_manifest.unwrap_or_else(|_| "./Cargo.toml".to_string()))
        };
        debug!("Using manifest {:?}", manifest);

        let (crate_name, version) = get_crate_info(&manifest)?;
        (crate_name, version, CrateOverride::Source(manifest))
    };

    // Get git information for display (only if we have a local source)
    let git_hash = get_git_hash();
    let is_dirty = git_hash.is_none() || is_git_dirty();

    Ok(Config {
        crate_name,
        version,
        git_hash,
        is_dirty,
        staging_dir: args.staging_dir.clone(),
        base_override: CrateOverride::Default,
        next_override,
        limit,
        force_versions: args.force_versions.clone(),
    })
}

fn get_crate_info(manifest_path: &Path) -> Result<(String, String), Error> {
    let toml_str = load_string(manifest_path)?;
    let value: toml::Value = toml::from_str(&toml_str)?;

    match value.get("package") {
        Some(toml::Value::Table(t)) => {
            let name = match t.get("name") {
                Some(toml::Value::String(s)) => s.clone(),
                _ => return Err(Error::ManifestName),
            };

            let version = match t.get("version") {
                Some(toml::Value::String(s)) => s.clone(),
                _ => "0.0.0".to_string(), // Default if no version
            };

            Ok((name, version))
        }
        _ => Err(Error::ManifestName),
    }
}

// Legacy function for compatibility
fn get_crate_name(manifest_path: &Path) -> Result<String, Error> {
    get_crate_info(manifest_path).map(|(name, _)| name)
}

fn load_string(path: &Path) -> Result<String, Error> {
    let mut file = File::open(path)?;
    let mut s = String::new();
    (file.read_to_string(&mut s)?);
    Ok(s)
}

type RevDepName = String;

fn crate_url(krate: &str, call: Option<&str>) -> String {
    crate_url_with_parms(krate, call, &[])
}

fn crate_url_with_parms(krate: &str, call: Option<&str>, parms: &[(&str, &str)]) -> String {
    let url = format!("https://crates.io/api/v1/crates/{}", krate);
    let s = match call {
        Some(c) => format!("{}/{}", url, c),
        None => url
    };

    if !parms.is_empty() {
        let parms: Vec<String> = parms.iter().map(|&(k, v)| format!("{}={}", k, v)).collect();
        let parms: String = parms.join("&");
        format!("{}?{}", s, parms)
    } else {
        s
    }
}

fn get_rev_deps(crate_name: &str, limit: Option<usize>) -> Result<Vec<RevDepName>, Error> {
    status(&format!("downloading reverse deps for {}", crate_name));

    let deps = CRATES_IO_CLIENT.crate_reverse_dependencies(crate_name)
        .map_err(|e| Error::CratesIoApiError(e.to_string()))?;

    let mut all_deps: Vec<String> = deps.dependencies
        .into_iter()
        .map(|d| d.dependency.crate_id)
        .collect();

    // Apply limit if specified
    if let Some(lim) = limit {
        all_deps.truncate(lim);
    }

    status(&format!("{} reverse deps", all_deps.len()));

    Ok(all_deps)
}

fn http_get_bytes(url: &str) -> Result<Vec<u8>, Error> {
    let resp = ureq::get(url)
        .set("User-Agent", USER_AGENT)
        .call()?;
    let len = resp.header("Content-Length")
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(0);
    let mut data: Vec<u8> = Vec::with_capacity(len);
    resp.into_reader().read_to_end(&mut data)?;
    Ok(data)
}

#[derive(Debug, Clone)]
struct RevDep {
    name: RevDepName,
    vers: Version,
    resolved_version: Option<String>, // Exact version from dependent's Cargo.lock
}

#[derive(Debug)]
struct TestResult {
    rev_dep: RevDep,
    data: TestResultData
}

#[derive(Debug)]
enum TestResultData {
    Skipped(String), // Skipped with reason (e.g., version incompatibility)
    Error(Error),
    // Phase 5: Multi-version result
    MultiVersion(Vec<VersionTestOutcome>),
}

/// Result of testing a dependent against a single version
#[derive(Debug, Clone)]
pub struct VersionTestOutcome {
    pub version_source: compile::VersionSource,
    pub result: compile::ThreeStepResult,
}

impl VersionTestOutcome {
    /// Classify this version test as PASSED, REGRESSED, BROKEN, or ERROR
    fn classify(&self, baseline_outcome: Option<&VersionTestOutcome>) -> VersionStatus {
        if self.result.is_success() {
            VersionStatus::Passed
        } else {
            // Failed - determine if REGRESSED or BROKEN
            if let Some(baseline) = baseline_outcome {
                if baseline.result.is_success() {
                    VersionStatus::Regressed
                } else {
                    VersionStatus::Broken
                }
            } else {
                // No baseline to compare - treat as BROKEN
                VersionStatus::Broken
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum VersionStatus {
    Passed,
    Broken,
    Regressed,
}

// ============================================================================
// Five-Column Console Table Data Structures (Phase 5+)
// ============================================================================

/// A single row in the five-column console table output
#[derive(Debug, Clone)]
pub struct OfferedRow {
    /// Baseline test result: None = this IS baseline, Some(bool) = baseline exists and passed/failed
    pub baseline_passed: Option<bool>,

    /// Primary dependency being tested (depth 0)
    pub primary: DependencyRef,

    /// Version offered for testing (None for baseline rows)
    pub offered: Option<OfferedVersion>,

    /// Test execution results for primary dependency
    pub test: TestExecution,

    /// Transitive dependencies using different versions (depth > 0)
    pub transitive: Vec<TransitiveTest>,
}

/// Reference to a dependency (primary or transitive)
#[derive(Debug, Clone)]
pub struct DependencyRef {
    pub dependent_name: String,       // "image"
    pub dependent_version: String,    // "0.25.8"
    pub spec: String,                 // "^0.8.52" (what they require)
    pub resolved_version: String,     // "0.8.91" (what cargo chose)
    pub resolved_source: VersionSource,  // CratesIo | Local | Git
    pub used_offered_version: bool,   // true if resolved == offered
}

/// Version offered for testing
#[derive(Debug, Clone)]
pub struct OfferedVersion {
    pub version: String,  // "this(0.8.91)" or "0.8.51"
    pub forced: bool,     // true shows [≠→!] suffix
}

/// Test execution (Install/Check/Test)
#[derive(Debug, Clone)]
pub struct TestExecution {
    pub commands: Vec<TestCommand>,  // fetch, check, test
}

/// A single test command (fetch, check, or test)
#[derive(Debug, Clone)]
pub struct TestCommand {
    pub command: CommandType,
    pub features: Vec<String>,
    pub result: CommandResult,
}

/// Type of command executed
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandType {
    Fetch,
    Check,
    Test,
}

/// Result of executing a command
#[derive(Debug, Clone)]
pub struct CommandResult {
    pub passed: bool,
    pub duration: f64,
    pub failures: Vec<CrateFailure>,  // Which crate(s) failed
}

/// A crate that failed during testing
#[derive(Debug, Clone)]
pub struct CrateFailure {
    pub crate_name: String,
    pub error_message: String,
}

/// Transitive dependency test (depth > 0)
#[derive(Debug, Clone)]
pub struct TransitiveTest {
    pub dependency: DependencyRef,
    pub depth: usize,
}

/// Source of a version (crates.io, local, or git)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VersionSource {
    CratesIo,
    Local,
    Git,
}

impl TestResult {
    // TODO: Remove - FourStepResult no longer exists, using MultiVersion instead
    /*
    fn from_four_step(rev_dep: RevDep, result: compile::FourStepResult) -> TestResult {
        let data = if result.is_broken() {
            TestResultData::Broken(result)
        } else if result.is_regressed() {
            TestResultData::Regressed(result)
        } else {
            TestResultData::Passed(result)
        };

        TestResult { rev_dep, data }
    }
    */

    /// Convert TestResult to OfferedRows for streaming output
    fn to_offered_rows(&self) -> Vec<OfferedRow> {
        match &self.data {
            TestResultData::MultiVersion(outcomes) => {
                let mut rows = Vec::new();

                // First outcome is always baseline
                let baseline = outcomes.first();

                for (idx, outcome) in outcomes.iter().enumerate() {
                    let is_baseline = idx == 0;

                    // Determine baseline_passed for this row
                    let baseline_passed = if is_baseline {
                        None  // This IS the baseline
                    } else {
                        baseline.map(|b| b.result.is_success())
                    };

                    // Convert compile::VersionSource to main::VersionSource
                    let resolved_source = match &outcome.version_source {
                        compile::VersionSource::Local(_) => VersionSource::Local,
                        compile::VersionSource::Published(_) => VersionSource::CratesIo,
                    };

                    // Build primary DependencyRef
                    let primary = DependencyRef {
                        dependent_name: self.rev_dep.name.clone(),
                        dependent_version: self.rev_dep.vers.to_string(),
                        spec: outcome.result.original_requirement.clone().unwrap_or_else(|| "?".to_string()),
                        resolved_version: outcome.result.actual_version.clone()
                            .or(outcome.result.expected_version.clone())
                            .unwrap_or_else(|| "?".to_string()),
                        resolved_source,
                        used_offered_version: outcome.result.expected_version == outcome.result.actual_version,
                    };

                    // Build OfferedVersion (None for baseline)
                    let offered = if is_baseline {
                        None
                    } else {
                        Some(OfferedVersion {
                            version: outcome.version_source.label(),
                            forced: outcome.result.forced_version,
                        })
                    };

                    // Build TestExecution from ThreeStepResult
                    let mut commands = Vec::new();

                    // Fetch command
                    commands.push(TestCommand {
                        command: CommandType::Fetch,
                        features: vec![],  // TODO: track features
                        result: CommandResult {
                            passed: outcome.result.fetch.success,
                            duration: outcome.result.fetch.duration.as_secs_f64(),
                            failures: if !outcome.result.fetch.success {
                                let error_msg = error_extract::extract_error_summary(&outcome.result.fetch.diagnostics);
                                let error_msg = if !error_msg.is_empty() {
                                    error_msg
                                } else {
                                    // Fallback to stderr if no error diagnostics
                                    outcome.result.fetch.stderr.clone()
                                };
                                vec![CrateFailure {
                                    crate_name: self.rev_dep.name.clone(),
                                    error_message: error_msg,
                                }]
                            } else {
                                vec![]
                            },
                        },
                    });

                    // Check command (if ran)
                    if let Some(ref check) = outcome.result.check {
                        commands.push(TestCommand {
                            command: CommandType::Check,
                            features: vec![],
                            result: CommandResult {
                                passed: check.success,
                                duration: check.duration.as_secs_f64(),
                                failures: if !check.success {
                                    let error_msg = error_extract::extract_error_summary(&check.diagnostics);
                                    let error_msg = if !error_msg.is_empty() {
                                        error_msg
                                    } else {
                                        // Fallback to stderr if no error diagnostics
                                        check.stderr.clone()
                                    };
                                    vec![CrateFailure {
                                        crate_name: self.rev_dep.name.clone(),
                                        error_message: error_msg,
                                    }]
                                } else {
                                    vec![]
                                },
                            },
                        });
                    }

                    // Test command (if ran)
                    if let Some(ref test) = outcome.result.test {
                        commands.push(TestCommand {
                            command: CommandType::Test,
                            features: vec![],
                            result: CommandResult {
                                passed: test.success,
                                duration: test.duration.as_secs_f64(),
                                failures: if !test.success {
                                    let error_msg = error_extract::extract_error_summary(&test.diagnostics);
                                    let error_msg = if !error_msg.is_empty() {
                                        error_msg
                                    } else {
                                        // Fallback to stderr if no error diagnostics
                                        test.stderr.clone()
                                    };
                                    vec![CrateFailure {
                                        crate_name: self.rev_dep.name.clone(),
                                        error_message: error_msg,
                                    }]
                                } else {
                                    vec![]
                                },
                            },
                        });
                    }

                    rows.push(OfferedRow {
                        baseline_passed,
                        primary,
                        offered,
                        test: TestExecution { commands },
                        transitive: vec![],  // TODO: extract from cargo tree
                    });
                }

                rows
            }
            TestResultData::Error(msg) => {
                // Create a single failed row for errors
                vec![OfferedRow {
                    baseline_passed: None,
                    primary: DependencyRef {
                        dependent_name: self.rev_dep.name.clone(),
                        dependent_version: self.rev_dep.vers.to_string(),
                        spec: "ERROR".to_string(),
                        resolved_version: "ERROR".to_string(),
                        resolved_source: VersionSource::CratesIo,
                        used_offered_version: false,
                    },
                    offered: None,
                    test: TestExecution {
                        commands: vec![TestCommand {
                            command: CommandType::Fetch,
                            features: vec![],
                            result: CommandResult {
                                passed: false,
                                duration: 0.0,
                                failures: vec![CrateFailure {
                                    crate_name: self.rev_dep.name.clone(),
                                    error_message: msg.to_string(),
                                }],
                            },
                        }],
                    },
                    transitive: vec![],
                }]
            }
            TestResultData::Skipped(reason) => {
                // Create a single row for skipped
                vec![OfferedRow {
                    baseline_passed: None,
                    primary: DependencyRef {
                        dependent_name: self.rev_dep.name.clone(),
                        dependent_version: self.rev_dep.vers.to_string(),
                        spec: "SKIPPED".to_string(),
                        resolved_version: reason.clone(),
                        resolved_source: VersionSource::CratesIo,
                        used_offered_version: false,
                    },
                    offered: None,
                    test: TestExecution { commands: vec![] },
                    transitive: vec![],
                }]
            }
        }
    }

    // Legacy constructors removed (passed, regressed, broken) - only used by deleted run_test_local()
    // Kept: skipped() and error() - still used by multi-version path

    fn skipped(rev_dep: RevDep, reason: String) -> TestResult {
        TestResult {
            rev_dep,
            data: TestResultData::Skipped(reason)
        }
    }

    fn error(rev_dep: RevDep, e: Error) -> TestResult {
        TestResult {
            rev_dep,
            data: TestResultData::Error(e)
        }
    }

    fn quick_str(&self) -> &'static str {
        match self.data {
            TestResultData::Skipped(_) => "skipped",
            TestResultData::Error(_) => "error",
            TestResultData::MultiVersion(ref outcomes) => {
                // For multi-version, return worst status
                let has_regressed = outcomes.iter().any(|o| {
                    matches!(o.classify(None), VersionStatus::Regressed)
                });
                if has_regressed {
                    "regressed"
                } else if outcomes.iter().any(|o| !o.result.is_success()) {
                    "broken"
                } else {
                    "passed"
                }
            }
        }
    }

    fn html_class(&self) -> &'static str {
        self.quick_str()
    }

    fn html_anchor(&self) -> String {
        sanitize_link(&format!("{}-{}", self.rev_dep.name, self.rev_dep.vers))
    }
}

fn sanitize_link(s: &str) -> String {
    s.chars().map(|c| {
        let c = c.to_lowercase().collect::<Vec<_>>()[0];
        if c != '-' && (c < 'a' || c > 'z')
            && (c < '0' || c > '9') {
            '_'
        } else {
            c
        }
    }).collect()
}

struct TestResultReceiver {
    rev_dep: RevDepName,
    rx: Receiver<TestResult>
}

impl TestResultReceiver {
    fn recv(self) -> TestResult {
        match self.rx.recv() {
            Ok(r) => r,
            Err(e) => {
                let r = RevDep {
                    name: self.rev_dep,
                    vers: Version::parse("0.0.0").unwrap(),
                    resolved_version: None,
                };
                TestResult::error(r, Error::from(e))
            }
        }
    }
}

fn new_result_receiver(rev_dep: RevDepName) -> (Sender<TestResult>, TestResultReceiver) {
    let (tx, rx) = mpsc::channel();

    let fut = TestResultReceiver {
        rev_dep: rev_dep,
        rx: rx
    };

    (tx, fut)
}

// Legacy run_test() removed - now always use run_test_multi_version()

fn run_test_multi_version(
    pool: &mut ThreadPool,
    config: Config,
    rev_dep: RevDepName,
    version: Option<String>,
    test_versions: Vec<compile::VersionSource>,
    force_local: bool,
) -> TestResultReceiver {
    let (result_tx, result_rx) = new_result_receiver(rev_dep.clone());
    pool.execute(move || {
        let res = run_multi_version_test(&config, rev_dep, version, test_versions, force_local);
        result_tx.send(res).unwrap();
    });

    return result_rx;
}

/// Extract the resolved version of a dependency using cargo metadata
/// Caches unpacked crates in staging_dir for reuse across runs
fn extract_resolved_version(rev_dep: &RevDep, crate_name: &str, staging_dir: &Path) -> Result<String, Error> {
    // Create staging directory if it doesn't exist
    fs::create_dir_all(staging_dir)?;

    // Staging path: staging_dir/{crate-name}-{version}/
    let staging_path = staging_dir.join(format!("{}-{}", rev_dep.name, rev_dep.vers));

    // Check if already unpacked
    if !staging_path.exists() {
        debug!("Unpacking {} to staging dir", rev_dep.name);
        let crate_handle = get_crate_handle(rev_dep)?;
        fs::create_dir_all(&staging_path)?;
        crate_handle.unpack_source_to(&staging_path)?;
    } else {
        debug!("Using cached staging dir for {}", rev_dep.name);
    }

    // The crate is unpacked directly into staging_path (--strip-components=1)
    let crate_dir = &staging_path;

    // Verify Cargo.toml exists
    if crate_dir.join("Cargo.toml").exists() {

        // IMPORTANT: Restore Cargo.toml from backup to ensure clean state
        // Previous runs may have modified it with force-versions
        let cargo_toml = crate_dir.join("Cargo.toml");
        let backup = crate_dir.join(".Cargo.toml.backup");
        if backup.exists() {
            debug!("Restoring Cargo.toml from backup before extracting baseline version");
            fs::copy(&backup, &cargo_toml)?;
        }

        // Run cargo metadata to get resolved dependencies
        // Try with --locked first, fallback to generating Cargo.lock if needed
        let mut output = Command::new("cargo")
            .args(&["metadata", "--format-version=1", "--locked"])
            .current_dir(&crate_dir)
            .output()?;

        if !output.status.success() {
            // If --locked failed (no Cargo.lock), try without it to generate one
            debug!("cargo metadata --locked failed, trying without --locked");
            output = Command::new("cargo")
                .args(&["metadata", "--format-version=1"])
                .current_dir(&crate_dir)
                .output()?;
        }

        if output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            debug!("cargo metadata output length: {} bytes", stdout.len());

            // Parse JSON metadata
            if let Ok(metadata) = serde_json::from_str::<serde_json::Value>(&stdout) {
                debug!("Successfully parsed metadata JSON");
                // Look through resolve.nodes for our dependency
                if let Some(resolve) = metadata.get("resolve") {
                    debug!("Found resolve section");
                    if let Some(nodes) = resolve.get("nodes").and_then(|n| n.as_array()) {
                        debug!("Found {} nodes in resolve", nodes.len());
                        for node in nodes {
                            if let Some(deps) = node.get("deps").and_then(|d| d.as_array()) {
                                for dep in deps {
                                    if let Some(name) = dep.get("name").and_then(|n| n.as_str()) {
                                        if name == crate_name {
                                            debug!("Found {} in resolve.nodes!", crate_name);
                                            if let Some(pkg) = dep.get("pkg").and_then(|p| p.as_str()) {
                                                // pkg format: "SOURCE#crate-name@version"
                                                // Examples:
                                                //   "registry+https://github.com/rust-lang/crates.io-index#rgb@0.8.52"
                                                //   "path+file:///home/user/crate#rgb@0.8.91"
                                                debug!("pkg field: {}", pkg);

                                                // Extract version from after @
                                                if let Some(at_pos) = pkg.rfind('@') {
                                                    let resolved_version = pkg[at_pos + 1..].to_string();
                                                    debug!("Resolved {} to version: {}", crate_name, resolved_version);
                                                    return Ok(resolved_version);
                                                } else {
                                                    debug!("No '@' found in pkg field");
                                                }
                                            } else {
                                                debug!("No 'pkg' field in dep");
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    } else {
                        debug!("No nodes array in resolve");
                    }
                } else {
                    debug!("No resolve section in metadata");
                }

                // Fallback: check packages array for version requirement
                if let Some(packages) = metadata.get("packages").and_then(|p| p.as_array()) {
                    debug!("Checking {} packages for {}", packages.len(), crate_name);
                    for package in packages {
                        if let Some(pkg_name) = package.get("name").and_then(|n| n.as_str()) {
                            debug!("Checking package: {}", pkg_name);
                        }
                        if let Some(deps) = package.get("dependencies").and_then(|d| d.as_array()) {
                            for dep in deps {
                                if let Some(name) = dep.get("name").and_then(|n| n.as_str()) {
                                    if name == crate_name {
                                        debug!("Found {} in dependencies!", crate_name);
                                        if let Some(req) = dep.get("req").and_then(|r| r.as_str()) {
                                            debug!("Version requirement: {}", req);
                                            return Ok(req.to_string());
                                        }
                                    }
                                }
                            }
                        }
                    }
                }

                debug!("Could not find {} in metadata", crate_name);
            }
        } else {
            debug!("cargo metadata failed: {}", String::from_utf8_lossy(&output.stderr));
        }
    } else {
        debug!("Cargo.toml not found in {}", crate_dir.display());
    }

    Err(Error::ProcessError("Failed to extract resolved version via cargo metadata".to_string()))
}

// Legacy run_test_local() removed - now always use run_multi_version_test()

/// Run multi-version ICT tests for a dependent crate (Phase 5)
///
/// Tests the dependent against multiple versions of the base crate and returns
/// a MultiVersion result containing outcomes for each version.
///
/// # Version Ordering
/// 1. Baseline (what the dependent naturally resolves to)
/// 2. Additional versions from --test-versions
/// 3. "this" (local WIP) or "latest" (if no local source)
fn run_multi_version_test(
    config: &Config,
    rev_dep: RevDepName,
    dependent_version: Option<String>,
    mut test_versions: Vec<compile::VersionSource>,
    force_local: bool,  // Whether local "this" versions should be forced
) -> TestResult {
    // Status line removed - redundant with table output
    // status(&format!("testing crate {} (multi-version)", rev_dep));

    // Resolve dependent version
    let mut rev_dep = match resolve_rev_dep_version(rev_dep.clone(), dependent_version) {
        Ok(r) => r,
        Err(e) => {
            let rev_dep = RevDep {
                name: rev_dep,
                vers: Version::parse("0.0.0").unwrap(),
                resolved_version: None,
            };
            return TestResult::error(rev_dep, e);
        }
    };

    // Extract resolved baseline version for this specific dependent
    let baseline_version = match extract_resolved_version(&rev_dep, &config.crate_name, &config.staging_dir) {
        Ok(resolved) => {
            debug!("Baseline version for {} -> {}: {}", rev_dep.name, config.crate_name, resolved);
            rev_dep.resolved_version = Some(resolved.clone());
            Some(resolved)
        }
        Err(e) => {
            debug!("Failed to extract resolved version for {}: {}", rev_dep.name, e);
            None
        }
    };

    // Extract the original requirement spec from the dependent's Cargo.toml
    let original_requirement = extract_dependency_requirement(&rev_dep, &config.crate_name);

    // Add baseline at the front (always non-forced)
    // IMPORTANT: Don't remove duplicates - if user specified same version in --force-versions,
    // we want to test it twice: once as baseline (non-forced), once as forced
    if let Some(ref baseline) = baseline_version {
        // Skip wildcard or star baselines
        if baseline != "*" && !baseline.is_empty() {
            // Always insert baseline at position 0
            // Even if the same version appears later in the list (from --force-versions),
            // this baseline will be tested in non-forced mode
            test_versions.insert(0, compile::VersionSource::Published(baseline.clone()));
            debug!("Inserted baseline {} at position 0 (will be non-forced)", baseline);
        }
    }

    // Check version compatibility
    match check_version_compatibility(&rev_dep, &config) {
        Ok(true) => {}, // Compatible
        Ok(false) => {
            let reason = format!(
                "Dependent requires version incompatible with {} v{}",
                config.crate_name, config.version
            );
            return TestResult::skipped(rev_dep, reason);
        }
        Err(e) => {
            debug!("Failed to check version compatibility: {}, testing anyway", e);
        }
    }

    // Unpack the dependent crate once (cached)
    let staging_path = config.staging_dir.join(format!("{}-{}", rev_dep.name, rev_dep.vers));
    if !staging_path.exists() {
        debug!("Unpacking {} to staging for multi-version test", rev_dep.name);
        match get_crate_handle(&rev_dep) {
            Ok(handle) => {
                if let Err(e) = fs::create_dir_all(&staging_path) {
                    return TestResult::error(rev_dep, Error::IoError(e));
                }
                if let Err(e) = handle.unpack_source_to(&staging_path) {
                    return TestResult::error(rev_dep, e);
                }
            }
            Err(e) => return TestResult::error(rev_dep, e),
        }
    }

    // Run ICT tests for each version
    let mut outcomes = Vec::new();
    debug!("Total versions to test: {}", test_versions.len());
    for (idx, version_source) in test_versions.iter().enumerate() {
        debug!("[{}/{}] Testing {} against version {}", idx + 1, test_versions.len(), rev_dep.name, version_source.label());

        // Check if this is the baseline (first version and matches baseline_version)
        let is_baseline = idx == 0 && baseline_version.is_some() && {
            if let compile::VersionSource::Published(ref ver) = version_source {
                Some(ver.as_str()) == baseline_version.as_deref()
            } else {
                false
            }
        };
        debug!("Version {}: idx={}, baseline_version={:?}, version_source={}, is_baseline={}",
               idx, idx, baseline_version, version_source.label(), is_baseline);

        // For baseline: no download, no patch - test as-is
        // For offered versions: download and patch
        let override_path = if is_baseline {
            debug!("Testing baseline version {} without patching", version_source.label());
            None  // Let cargo handle baseline naturally
        } else {
            match &version_source {
                compile::VersionSource::Local(path) => {
                    // If path points to Cargo.toml, extract directory
                    let dir_path = if path.ends_with("Cargo.toml") {
                        path.parent().unwrap().to_path_buf()
                    } else {
                        path.clone()
                    };
                    debug!("Using local version path: {:?}", dir_path);
                    Some(dir_path)
                }
                compile::VersionSource::Published(version) => {
                    match download_and_unpack_base_crate_version(
                    &config.crate_name,
                    version,
                    &config.staging_dir,
                ) {
                    Ok(path) => Some(path),
                    Err(e) => {
                        status(&format!("Warning: Failed to download {} {}: {}", config.crate_name, version, e));
                        // Create a failed outcome
                        // version is already validated as concrete semver at input time
                        let is_forced = config.force_versions.contains(version);

                        let failed_result = compile::ThreeStepResult {
                            fetch: compile::CompileResult {
                                step: compile::CompileStep::Fetch,
                                success: false,
                                stdout: String::new(),
                                stderr: format!("Failed to download base crate: {}", e),
                                duration: Duration::from_secs(0),
                                diagnostics: Vec::new(),
                            },
                            check: None,
                            test: None,
                            actual_version: None,
                            expected_version: Some(version.to_string()),
                            forced_version: is_forced,
                            original_requirement: original_requirement.clone(),
                        };
                        outcomes.push(VersionTestOutcome {
                            version_source: version_source.clone(),
                            result: failed_result,
                        });
                        continue;
                    }
                }
                }
            }
        };

        let skip_check = false; // TODO: Get from args
        let skip_test = false;  // TODO: Get from args

        // Determine expected version for verification and if it's forced
        // IMPORTANT: Baseline is NEVER forced, even if it's in --force-versions list
        let (expected_version, is_forced) = if is_baseline {
            match &version_source {
                compile::VersionSource::Published(v) => (Some(v.clone()), false),
                compile::VersionSource::Local(_) => (None, false),
            }
        } else {
            match &version_source {
                compile::VersionSource::Published(v) => {
                    // v is already validated as concrete semver at input time
                    let forced = config.force_versions.contains(v);
                    (Some(v.clone()), forced)
                }
                compile::VersionSource::Local(_) => {
                    // Local WIP: use force_local flag
                    // true if: "this" in --force-versions OR auto-added (default)
                    // false if: "this" in --test-versions (explicitly non-forced)
                    (None, force_local)
                }
            }
        };

        // Create label for failure logging
        let test_label = if is_baseline {
            format!("baseline ({})", version_source.label())
        } else {
            match &version_source {
                compile::VersionSource::Published(v) => format!("offered ({})", v),
                compile::VersionSource::Local(_) => "offered (WIP)".to_string(),
            }
        };

        match compile::run_three_step_ict(
            &staging_path,
            &config.crate_name,
            override_path.as_deref(),
            skip_check,
            skip_test,
            expected_version,
            is_forced,
            original_requirement.clone(),
            Some(&rev_dep.name),
            Some(&rev_dep.vers.to_string()),
            Some(&test_label),
        ) {
            Ok(result) => {
                // Version mismatch is shown in table with [≠→!] suffix, no need for separate warning
                if let (Some(ref expected), Some(ref actual)) = (&result.expected_version, &result.actual_version) {
                    if actual != expected {
                        debug!("⚠️  VERSION MISMATCH: Expected {} but cargo resolved to {}!", expected, actual);
                    } else {
                        debug!("✓ Version verified: {} = {}", expected, actual);
                    }
                } else if result.expected_version.is_some() && result.actual_version.is_none() {
                    debug!("⚠️  Could not verify version for {} (cargo tree failed)", config.crate_name);
                }

                outcomes.push(VersionTestOutcome {
                    version_source: version_source.clone(),
                    result,
                });
            }
            Err(e) => {
                // ICT test failed with error - create a failed outcome
                return TestResult::error(rev_dep, Error::ProcessError(e));
            }
        }
    }

    TestResult {
        rev_dep,
        data: TestResultData::MultiVersion(outcomes),
    }
}

fn check_version_compatibility(rev_dep: &RevDep, config: &Config) -> Result<bool, Error> {
    debug!("checking version compatibility for {} {}", rev_dep.name, rev_dep.vers);

    // Download and cache the dependent's .crate file
    let crate_handle = get_crate_handle(rev_dep)?;

    // Create temp directory to extract Cargo.toml
    let temp_dir = TempDir::new()?;
    let extract_dir = temp_dir.path().join("extracted");
    fs::create_dir(&extract_dir)?;

    // Extract just the Cargo.toml
    let mut cmd = Command::new("tar");
    let cmd = cmd
        .arg("xzf")
        .arg(&crate_handle.0)
        .arg("--strip-components=1")
        .arg("-C")
        .arg(&extract_dir)
        .arg("--wildcards")
        .arg("*/Cargo.toml");

    let output = cmd.output()?;
    if !output.status.success() {
        return Err(Error::ProcessError("Failed to extract Cargo.toml".to_string()));
    }

    // Read and parse Cargo.toml
    let toml_path = extract_dir.join("Cargo.toml");
    let toml_str = load_string(&toml_path)?;
    let value: toml::Value = toml::from_str(&toml_str)?;

    // Look for our crate in dependencies
    let our_crate = &config.crate_name;
    let wip_version = Version::parse(&config.version)?;

    // Check [dependencies]
    if let Some(deps) = value.get("dependencies").and_then(|v| v.as_table()) {
        if let Some(req) = deps.get(our_crate) {
            return check_requirement(req, &wip_version);
        }
    }

    // Check [dev-dependencies]
    if let Some(deps) = value.get("dev-dependencies").and_then(|v| v.as_table()) {
        if let Some(req) = deps.get(our_crate) {
            return check_requirement(req, &wip_version);
        }
    }

    // Check [build-dependencies]
    if let Some(deps) = value.get("build-dependencies").and_then(|v| v.as_table()) {
        if let Some(req) = deps.get(our_crate) {
            return check_requirement(req, &wip_version);
        }
    }

    // Crate not found in dependencies (shouldn't happen for reverse deps)
    debug!("Warning: {} not found in {}'s dependencies", our_crate, rev_dep.name);
    Ok(true) // Test anyway
}

fn check_requirement(req: &toml::Value, wip_version: &Version) -> Result<bool, Error> {
    use semver::VersionReq;

    let req_str = extract_requirement_string(req);

    debug!("Checking if version {} satisfies requirement '{}'", wip_version, req_str);

    let version_req = VersionReq::parse(&req_str)
        .map_err(|e| Error::SemverError(e))?;

    Ok(version_req.matches(wip_version))
}

/// Extract the version requirement string from a toml dependency value
fn extract_requirement_string(req: &toml::Value) -> String {
    match req {
        toml::Value::String(s) => s.clone(),
        toml::Value::Table(t) => {
            // Handle { version = "1.0", features = [...] } format
            t.get("version")
                .and_then(|v| v.as_str())
                .unwrap_or("*")
                .to_string()
        }
        _ => "*".to_string(),
    }
}

/// Extract the original requirement spec for our crate from a dependent's Cargo.toml
/// Returns the requirement string (e.g., "^0.8.52") if found
fn extract_dependency_requirement(rev_dep: &RevDep, crate_name: &str) -> Option<String> {
    debug!("Extracting dependency requirement for {} from {}", crate_name, rev_dep.name);

    // Download and cache the dependent's .crate file
    let crate_handle = match get_crate_handle(rev_dep) {
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
    let mut cmd = Command::new("tar");
    let cmd = cmd
        .arg("xzf")
        .arg(&crate_handle.0)
        .arg("--strip-components=1")
        .arg("-C")
        .arg(&extract_dir)
        .arg("--wildcards")
        .arg("*/Cargo.toml");

    let output = match cmd.output() {
        Ok(o) => o,
        Err(e) => {
            debug!("Failed to run tar command: {}", e);
            return None;
        }
    };

    if !output.status.success() {
        debug!("tar command failed for {}", rev_dep.name);
        return None;
    }

    // Read and parse Cargo.toml
    let toml_path = extract_dir.join("Cargo.toml");
    let toml_str = match load_string(&toml_path) {
        Ok(s) => s,
        Err(e) => {
            debug!("Failed to read Cargo.toml: {}", e);
            return None;
        }
    };

    let value: toml::Value = match toml::from_str(&toml_str) {
        Ok(v) => v,
        Err(e) => {
            debug!("Failed to parse Cargo.toml: {}", e);
            return None;
        }
    };

    // Check [dependencies]
    if let Some(deps) = value.get("dependencies").and_then(|v| v.as_table()) {
        if let Some(req) = deps.get(crate_name) {
            let req_str = extract_requirement_string(req);
            debug!("Found requirement in [dependencies]: {}", req_str);
            return Some(req_str);
        }
    }

    // Check [dev-dependencies]
    if let Some(deps) = value.get("dev-dependencies").and_then(|v| v.as_table()) {
        if let Some(req) = deps.get(crate_name) {
            let req_str = extract_requirement_string(req);
            debug!("Found requirement in [dev-dependencies]: {}", req_str);
            return Some(req_str);
        }
    }

    // Check [build-dependencies]
    if let Some(deps) = value.get("build-dependencies").and_then(|v| v.as_table()) {
        if let Some(req) = deps.get(crate_name) {
            let req_str = extract_requirement_string(req);
            debug!("Found requirement in [build-dependencies]: {}", req_str);
            return Some(req_str);
        }
    }

    debug!("No requirement found for {} in {}'s Cargo.toml", crate_name, rev_dep.name);
    None
}

fn resolve_rev_dep_version(name: RevDepName, version: Option<String>) -> Result<RevDep, Error> {
    // If version is provided, use it directly
    if let Some(ver_str) = version {
        debug!("using pinned version {} for {}", ver_str, name);
        let vers = Version::parse(&ver_str)
            .map_err(|e| Error::SemverError(e))?;
        return Ok(RevDep {
            name: name,
            vers: vers,
            resolved_version: None,
        });
    }

    // Otherwise, resolve latest version from crates.io
    debug!("resolving current version for {}", name);

    let krate = CRATES_IO_CLIENT.get_crate(&name)
        .map_err(|e| Error::CratesIoApiError(e.to_string()))?;

    // Pull out the version numbers and sort them
    let versions = krate.versions.iter()
        .filter_map(|r| Version::parse(&r.num).ok());
    let mut versions = versions.collect::<Vec<_>>();
    versions.sort();

    versions.pop().map(|v| {
        RevDep {
            name: name,
            vers: v,
            resolved_version: None,
        }
    }).ok_or(Error::NoCrateVersions)
}

/// Resolve 'latest' or 'latest-preview' keyword to actual version
fn resolve_latest_version(crate_name: &str, include_prerelease: bool) -> Result<String, Error> {
    debug!("Resolving latest version for {} (prerelease={})", crate_name, include_prerelease);

    let krate = CRATES_IO_CLIENT.get_crate(crate_name)
        .map_err(|e| Error::CratesIoApiError(e.to_string()))?;

    // Filter and sort versions
    let mut versions: Vec<Version> = krate.versions.iter()
        .filter_map(|r| Version::parse(&r.num).ok())
        .filter(|v| include_prerelease || v.pre.is_empty()) // Filter pre-releases unless requested
        .collect();

    versions.sort();

    versions.pop()
        .map(|v| v.to_string())
        .ok_or(Error::NoCrateVersions)
}


// CompileResult is now in compile module
type CompileResult = compile::CompileResult;

fn compile_with_custom_dep(
    rev_dep: &RevDep,
    krate: &CrateOverride,
    crate_name: &str,
    staging_dir: &Path
) -> Result<CompileResult, Error> {
    // Use staging directory instead of temp dir to cache build artifacts
    fs::create_dir_all(staging_dir)?;
    let staging_path = staging_dir.join(format!("{}-{}", rev_dep.name, rev_dep.vers));

    // Check if already unpacked, if not unpack it
    if !staging_path.exists() {
        debug!("Unpacking {} to staging for compilation", rev_dep.name);
        let crate_handle = get_crate_handle(rev_dep)?;
        fs::create_dir_all(&staging_path)?;
        crate_handle.unpack_source_to(&staging_path)?;
    } else {
        debug!("Using cached staging dir for compilation of {}", rev_dep.name);
    }

    let source_dir = &staging_path;

    // Restore Cargo.toml from original backup to prevent contamination
    restore_cargo_toml(&staging_path)?;

    // Clean up any existing .cargo/config from previous runs (old system)
    let cargo_dir = source_dir.join(".cargo");
    if cargo_dir.exists() {
        fs::remove_dir_all(&cargo_dir).ok(); // Ignore errors
    }

    // Build override spec for new --config system
    let override_spec = match krate {
        CrateOverride::Default => None,
        CrateOverride::Source(ref path) => {
            // Extract directory from Cargo.toml path
            let override_dir = if path.ends_with("Cargo.toml") {
                path.parent().unwrap()
            } else {
                path.as_path()
            };
            Some((crate_name, override_dir))
        }
    };

    // Use cargo build with --config flag (legacy: still using build instead of check)
    let start = std::time::Instant::now();
    let mut cmd = Command::new("cargo");
    cmd.arg("build").current_dir(source_dir);

    if let Some((name, path)) = override_spec {
        // Convert to absolute path
        let abs_path = if path.is_absolute() {
            path.to_path_buf()
        } else {
            env::current_dir()?.join(path)
        };

        let config_str = format!("patch.crates-io.{}.path=\"{}\"", name, abs_path.display());
        cmd.arg("--config").arg(&config_str);
        debug!("using --config: {}", config_str);
    }

    debug!("running cargo: {:?}", cmd);
    let r = cmd.output()?;

    let duration = start.elapsed();
    let success = r.status.success();

    debug!("result: {:?}", success);

    Ok(CompileResult {
        step: compile::CompileStep::Check, // Legacy: using Check for old build command
        success,
        stdout: String::from_utf8(r.stdout)?,
        stderr: String::from_utf8(r.stderr)?,
        duration,
        diagnostics: Vec::new(), // Legacy path doesn't use JSON parsing
    })
}

struct CrateHandle(PathBuf);

fn get_crate_handle(rev_dep: &RevDep) -> Result<CrateHandle, Error> {
    let cache_path = Path::new("./.copter/crate-cache");
    let ref crate_dir = cache_path.join(&rev_dep.name);
    (fs::create_dir_all(crate_dir)?);
    let crate_file = crate_dir.join(format!("{}-{}.crate", rev_dep.name, rev_dep.vers));
    // FIXME: Path::exists() is unstable so just opening the file
    let crate_file_exists = File::open(&crate_file).is_ok();
    if !crate_file_exists {
        let url = crate_url(&rev_dep.name,
                            Some(&format!("{}/download", rev_dep.vers)));
        let body = http_get_bytes(&url)?;
        // FIXME: Should move this into place atomically
        let mut file = File::create(&crate_file)?;
        (file.write_all(&body)?);
        (file.flush()?);
    }

    return Ok(CrateHandle(crate_file));
}

/// Download and unpack a specific version of the base crate for patching
/// Returns the path to the unpacked source
fn download_and_unpack_base_crate_version(
    crate_name: &str,
    version: &str,
    staging_dir: &Path,
) -> Result<PathBuf, Error> {
    debug!("Downloading and unpacking {} version {}", crate_name, version);

    // version is already validated as concrete semver at input time
    // Create a pseudo-RevDep for downloading
    let vers = Version::parse(version)
        .map_err(|e| Error::SemverError(e))?;
    let pseudo_dep = RevDep {
        name: RevDepName::from(crate_name.to_string()),
        vers,
        resolved_version: None,
    };

    // Download the crate
    let crate_handle = get_crate_handle(&pseudo_dep)?;

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

impl CrateHandle {
    fn unpack_source_to(&self, path: &Path) -> Result<(), Error> {
        debug!("unpackng {:?} to {:?}", self.0, path);
        let mut cmd = Command::new("tar");
        let cmd = cmd
            .arg("xzf")
            .arg(self.0.to_str().unwrap().to_owned())
            .arg("--strip-components=1")
            .arg("-C")
            .arg(path.to_str().unwrap().to_owned());
        let r = cmd.output()?;
        if r.status.success() {
            // Save original Cargo.toml if this is first unpack
            save_original_cargo_toml(path)?;
            Ok(())
        } else {
            // FIXME: Want to put r in this value but
            // process::Output doesn't implement Debug
            let s = String::from_utf8_lossy(&r.stderr).into_owned();
            Err(Error::ProcessError(s))
        }
    }
}

/// Save a backup of Cargo.toml as Cargo.toml.original.txt (only if not already saved)
fn save_original_cargo_toml(staging_path: &Path) -> Result<(), Error> {
    let cargo_toml = staging_path.join("Cargo.toml");
    let original = staging_path.join("Cargo.toml.original.txt");

    // Only save if original doesn't exist yet (first unpack)
    if !original.exists() && cargo_toml.exists() {
        fs::copy(&cargo_toml, &original)?;
        debug!("Saved original Cargo.toml to {:?}", original);
    }
    Ok(())
}

/// Restore Cargo.toml from the original backup before testing
fn restore_cargo_toml(staging_path: &Path) -> Result<(), Error> {
    let cargo_toml = staging_path.join("Cargo.toml");
    let original = staging_path.join("Cargo.toml.original.txt");

    if original.exists() {
        fs::copy(&original, &cargo_toml)?;
        debug!("Restored Cargo.toml from original backup in {:?}", staging_path);
    }
    Ok(())
}


fn status_lock<F>(f: F) where F: FnOnce() -> () {
   lazy_static! {
        static ref LOCK: Mutex<()> = Mutex::new(());
    }
    let _guard = LOCK.lock();
    f();
}

fn print_status_header() {
    print!("copter: ");
}

fn print_color(s: &str, fg: term::color::Color) {
    if !really_print_color(s, fg) {
        print!("{}", s);
    }

    fn really_print_color(s: &str,
                          fg: term::color::Color) -> bool {
        if let Some(ref mut t) = term::stdout() {
            if t.fg(fg).is_err() { return false }
            let _ = t.attr(term::Attr::Bold);
            if write!(t, "{}", s).is_err() { return false }
            let _ = t.reset();
        }

        true
    }
}

fn status(s: &str) {
    status_lock(|| {
        print_status_header();
        println!("{}", s);
    });
}

fn report_quick_result(current_num: usize, total: usize, result: &TestResult) {
    status_lock(|| {
        print_status_header();
        print!("result {} of {}, {} {}: ",
               current_num,
               total,
               result.rev_dep.name,
               result.rev_dep.vers
               );
        let color = match result.data {
            TestResultData::Skipped(_) => term::color::BRIGHT_CYAN,
            TestResultData::Error(_) => term::color::BRIGHT_MAGENTA,
            TestResultData::MultiVersion(_) => term::color::BRIGHT_GREEN, // TODO: Compute worst status
        };
        print_color(&format!("{}", result.quick_str()), color);
        println!("");

        // Print detailed error output immediately for failures
        // TODO: Migrate to OfferedRow-based failure reporting
        if matches!(result.data, TestResultData::Error(_)) {
            report::print_immediate_failure(result);
        }
    });
}

fn report_results(res: Result<Vec<TestResult>, Error>, args: &cli::CliArgs, config: &Config) {
    match res {
        Ok(results) => {
            // Console table is already printed in streaming mode during run_tests()
            // No need to print again here

            // Note: Markdown report generation disabled for now
            // The table is already printed to console in streaming mode
            // TODO: Capture console output and write to markdown file
        }
        Err(e) => {
            report_error(e);
        }
    }
}

fn report_error(e: Error) {
    println!("");
    print_color("error", term::color::BRIGHT_RED);
    println!(": {}", e);
    println!("");

    std::process::exit(-1);
}

// Report generation functions moved to src/report.rs

#[derive(Debug)]
enum Error {
    ManifestName,
    SemverError(semver::Error),
    TomlError(toml::de::Error),
    IoError(io::Error),
    UreqError(Box<ureq::Error>),
    CratesIoApiError(String),
    RecvError(RecvError),
    NoCrateVersions,
    FromUtf8Error(FromUtf8Error),
    ProcessError(String),
    InvalidPath(PathBuf),
    InvalidVersion(String),
}

macro_rules! convert_error {
    ($from:ty, $to:ident) => (
        impl From<$from> for Error {
            fn from(e: $from) -> Error {
                Error::$to(e)
            }
        }
    )
}

convert_error!(semver::Error, SemverError);
convert_error!(io::Error, IoError);
convert_error!(toml::de::Error, TomlError);
convert_error!(RecvError, RecvError);
convert_error!(FromUtf8Error, FromUtf8Error);

impl From<ureq::Error> for Error {
    fn from(e: ureq::Error) -> Error {
        Error::UreqError(Box::new(e))
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> Result<(), fmt::Error> {
        match *self {
            Error::ManifestName => write!(f, "error extracting crate name from manifest"),
            Error::SemverError(ref e) => write!(f, "semver error: {}", e),
            Error::TomlError(ref e) => write!(f, "TOML parse error: {}", e),
            Error::IoError(ref e) => write!(f, "IO error: {}", e),
            Error::UreqError(ref e) => write!(f, "HTTP error: {}", e),
            Error::CratesIoApiError(ref e) => write!(f, "crates.io API error: {}", e),
            Error::RecvError(ref e) => write!(f, "receive error: {}", e),
            Error::NoCrateVersions => write!(f, "crate has no published versions"),
            Error::FromUtf8Error(ref e) => write!(f, "UTF-8 conversion error: {}", e),
            Error::ProcessError(ref s) => write!(f, "process error: {}", s),
            Error::InvalidPath(ref p) => write!(f, "invalid path: {}", p.display()),
            Error::InvalidVersion(ref s) => write!(f, "{}", s),
        }
    }
}

impl StdError for Error {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        match *self {
            Error::SemverError(ref e) => Some(e),
            Error::TomlError(ref e) => Some(e),
            Error::IoError(ref e) => Some(e),
            Error::UreqError(ref e) => Some(e.as_ref()),
            Error::RecvError(ref e) => Some(e),
            Error::FromUtf8Error(ref e) => Some(e),
            _ => None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use semver::Version;

    #[test]
    fn test_check_requirement_string_exact_version() {
        let req = toml::Value::String("0.2.0".to_string());
        let version = Version::parse("0.2.0").unwrap();

        assert!(check_requirement(&req, &version).unwrap());
    }

    #[test]
    fn test_check_requirement_string_caret() {
        let req = toml::Value::String("^0.1.0".to_string());
        let version_compatible = Version::parse("0.1.5").unwrap();
        let version_incompatible = Version::parse("0.2.0").unwrap();

        assert!(check_requirement(&req, &version_compatible).unwrap());
        assert!(!check_requirement(&req, &version_incompatible).unwrap());
    }

    #[test]
    fn test_check_requirement_string_tilde() {
        let req = toml::Value::String("~0.1.0".to_string());
        let version_compatible = Version::parse("0.1.9").unwrap();
        let version_incompatible = Version::parse("0.2.0").unwrap();

        assert!(check_requirement(&req, &version_compatible).unwrap());
        assert!(!check_requirement(&req, &version_incompatible).unwrap());
    }

    #[test]
    fn test_check_requirement_wildcard() {
        let req = toml::Value::String("*".to_string());
        let version = Version::parse("999.999.999").unwrap();

        assert!(check_requirement(&req, &version).unwrap());
    }

    #[test]
    fn test_check_requirement_table_with_version() {
        use toml::map::Map;

        let mut table = Map::new();
        table.insert("version".to_string(), toml::Value::String("^0.1.0".to_string()));
        table.insert("features".to_string(), toml::Value::Array(vec![]));
        let req = toml::Value::Table(table);

        let version_compatible = Version::parse("0.1.5").unwrap();
        let version_incompatible = Version::parse("0.2.0").unwrap();

        assert!(check_requirement(&req, &version_compatible).unwrap());
        assert!(!check_requirement(&req, &version_incompatible).unwrap());
    }

    #[test]
    fn test_check_requirement_table_without_version() {
        use toml::map::Map;

        let mut table = Map::new();
        table.insert("path".to_string(), toml::Value::String("../local".to_string()));
        let req = toml::Value::Table(table);

        // Table without version field should default to "*" (wildcard)
        let version = Version::parse("999.999.999").unwrap();
        assert!(check_requirement(&req, &version).unwrap());
    }

    #[test]
    fn test_check_requirement_gte_operator() {
        let req = toml::Value::String(">=0.1.0".to_string());
        let version_compatible = Version::parse("0.2.0").unwrap();
        let version_incompatible = Version::parse("0.0.9").unwrap();

        assert!(check_requirement(&req, &version_compatible).unwrap());
        assert!(!check_requirement(&req, &version_incompatible).unwrap());
    }

    #[test]
    fn test_check_requirement_complex_range() {
        let req = toml::Value::String(">=0.1.0, <0.3.0".to_string());
        let version_compatible1 = Version::parse("0.1.5").unwrap();
        let version_compatible2 = Version::parse("0.2.9").unwrap();
        let version_incompatible = Version::parse("0.3.0").unwrap();

        assert!(check_requirement(&req, &version_compatible1).unwrap());
        assert!(check_requirement(&req, &version_compatible2).unwrap());
        assert!(!check_requirement(&req, &version_incompatible).unwrap());
    }
}
