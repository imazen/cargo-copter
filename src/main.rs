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
mod metadata;
mod report;
mod toml_helpers;

use flate2::read::GzDecoder;
use lazy_static::lazy_static;
use log::debug;
use semver::Version;
use std::env;
use std::error::Error as StdError;
use std::fmt;
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::string::FromUtf8Error;
use std::sync::Mutex;
use std::time::Duration;
use tar::Archive;
use tempfile::TempDir;

const USER_AGENT: &str = "cargo-copter/0.1.1 (https://github.com/imazen/cargo-copter)";

/// Resolve a version keyword ("this", "latest", "latest-preview") or concrete version
/// to a VersionSource for testing
fn resolve_version_keyword(
    version_str: &str,
    crate_name: &str,
    local_manifest: Option<&PathBuf>,
) -> Result<Option<compile::VersionSource>, Error> {
    match version_str {
        "this" => {
            // User explicitly requested WIP version
            if let Some(manifest_path) = local_manifest {
                debug!("Resolved 'this' to local WIP at {:?}", manifest_path);
                Ok(Some(compile::VersionSource::Local { path: manifest_path.clone(), forced: false }))
            } else {
                status("Warning: 'this' specified but no local source available (--path or --crate)");
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
                    status(&format!("Warning: Failed to resolve 'latest': {}", e));
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
                    status(&format!("Warning: Failed to resolve '{}': {}", version_str, e));
                    Ok(None)
                }
            }
        }
        _ => {
            // Validate it's a concrete version, not a version requirement
            if version_str.starts_with('^') || version_str.starts_with('~') || version_str.starts_with('=') {
                return Err(Error::InvalidVersion(format!(
                    "Version requirement '{}' not allowed. Use concrete versions like '0.8.52'",
                    version_str
                )));
            }

            // Validate it's a valid semver version
            Version::parse(version_str)?;

            // Literal version string (supports hyphens like "0.8.2-alpha2")
            Ok(Some(compile::VersionSource::Published { version: version_str.to_string(), forced: false }))
        }
    }
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

    // Clean staging directory if requested
    if args.clean {
        if args.staging_dir.exists() {
            match fs::remove_dir_all(&args.staging_dir) {
                Ok(_) => {
                    println!("Cleaned staging directory: {}", args.staging_dir.display());
                }
                Err(e) => {
                    eprintln!("Warning: Failed to clean staging directory: {}", e);
                }
            }
        }
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

/// Generate a compact test plan showing what will be tested
fn format_test_plan(
    rev_deps: &[(String, Option<String>)],
    versions: &[compile::VersionSource],
    force_versions: &[String],
    force_local: bool,
    crate_name: &str,
) -> String {
    // Format dependents list
    let deps_display: Vec<String> = rev_deps
        .iter()
        .map(|(name, ver)| if let Some(v) = ver { format!("{}:{}", name, v) } else { name.clone() })
        .collect();

    // Format versions list with force indicators (deduplicate versions)
    let mut versions_display = Vec::new();
    let mut seen_versions = std::collections::HashSet::new();
    versions_display.push("baseline".to_string()); // baseline is always tested first

    for version in versions {
        let (version_str, is_forced) = match version {
            compile::VersionSource::Published { version: v, .. } => (v.clone(), force_versions.contains(v)),
            compile::VersionSource::Local { .. } => ("this".to_string(), force_local),
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

    // Build compact plan string
    let mut output = String::new();

    // Show dependents (compact, comma-separated, max 70 chars per line)
    let deps_str = deps_display.join(", ");
    if deps_str.len() <= 70 {
        output.push_str(&format!("  Dependents: {}\n", deps_str));
    } else {
        // Wrap at reasonable points
        output.push_str("  Dependents: ");
        let mut line = String::new();
        for (i, dep) in deps_display.iter().enumerate() {
            if i > 0 {
                line.push_str(", ");
            }
            if line.len() + dep.len() > 70 && !line.is_empty() {
                output.push_str(&format!("{}\n", line));
                output.push_str("              ");
                line.clear();
            }
            line.push_str(dep);
        }
        if !line.is_empty() {
            output.push_str(&format!("{}\n", line));
        }
    }

    // Show versions (compact, comma-separated) with actual crate name
    output.push_str(&format!("  {} versions: {}\n", crate_name, versions_display.join(", ")));

    // Show test count
    output.push_str(&format!(
        "  {} × {} = {} tests",
        rev_deps.len(),
        versions_display.len(),
        rev_deps.len() * versions_display.len()
    ));

    output
}

fn run(args: cli::CliArgs, config: Config) -> Result<Vec<TestResult>, Error> {
    // Initialize failure log and clear any previous contents
    let log_path = std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")).join("copter-failures.log");

    // Truncate/clear the log file if it exists
    if log_path.exists() {
        let _ = std::fs::write(&log_path, ""); // Clear file contents
    }

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
        let local_manifest = match &config.next_override {
            CrateOverride::Source(path) => Some(path),
            CrateOverride::Default => None,
        };

        for ver_str in &args.test_versions {
            if let Some(version_source) = resolve_version_keyword(ver_str, &config.crate_name, local_manifest)? {
                versions.push(version_source);
            }
        }

        // Add versions from --force-versions and mark them as forced
        for ver_str in &args.force_versions {
            if let Some(mut version_source) = resolve_version_keyword(ver_str, &config.crate_name, local_manifest)? {
                // Mark this version as forced
                match &mut version_source {
                    compile::VersionSource::Published { forced, .. } => *forced = true,
                    compile::VersionSource::Local { forced, .. } => *forced = true,
                }
                versions.push(version_source);
            }
        }

        // Auto-insert non-forced variants for each forced version (unless --skip-normal-testing)
        if !args.skip_normal_testing {
            let forced_versions: Vec<compile::VersionSource> =
                versions.iter().filter(|v| v.is_forced()).cloned().collect();

            for forced_ver in forced_versions {
                // Check if a non-forced variant already exists
                let has_non_forced = match &forced_ver {
                    compile::VersionSource::Published { version: fv, .. } => versions.iter().any(|v| match v {
                        compile::VersionSource::Published { version: v, forced } => v == fv && !forced,
                        _ => false,
                    }),
                    compile::VersionSource::Local { path: fp, .. } => versions.iter().any(|v| match v {
                        compile::VersionSource::Local { path: p, forced } => p == fp && !forced,
                        _ => false,
                    }),
                };

                if !has_non_forced {
                    // Insert non-forced variant
                    let non_forced = match forced_ver {
                        compile::VersionSource::Published { version, .. } => {
                            debug!("Auto-inserting non-forced test for version {}", version);
                            compile::VersionSource::Published { version, forced: false }
                        }
                        compile::VersionSource::Local { path, .. } => {
                            debug!("Auto-inserting non-forced test for 'this'");
                            compile::VersionSource::Local { path, forced: false }
                        }
                    };
                    versions.push(non_forced);
                }
            }

            // Reorder versions so non-forced tests run before forced tests for the same version
            // This provides clearer A/B comparison in the output
            versions.sort_by(|a, b| {
                use std::cmp::Ordering;

                // First, compare by version/path to group same versions together
                let version_cmp = match (a, b) {
                    (
                        compile::VersionSource::Published { version: va, .. },
                        compile::VersionSource::Published { version: vb, .. },
                    ) => va.cmp(vb),
                    (
                        compile::VersionSource::Local { path: pa, .. },
                        compile::VersionSource::Local { path: pb, .. },
                    ) => pa.cmp(pb),
                    // Local versions come after published versions
                    (compile::VersionSource::Published { .. }, compile::VersionSource::Local { .. }) => Ordering::Less,
                    (compile::VersionSource::Local { .. }, compile::VersionSource::Published { .. }) => {
                        Ordering::Greater
                    }
                };

                // If versions are the same, non-forced comes before forced
                if version_cmp == Ordering::Equal {
                    let a_forced = a.is_forced();
                    let b_forced = b.is_forced();
                    a_forced.cmp(&b_forced) // false < true, so non-forced comes first
                } else {
                    version_cmp
                }
            });
        }

        // Auto-add "this" (local WIP) in forced mode if not already specified
        // Default: --test-versions baseline --force-versions this
        if let CrateOverride::Source(ref manifest_path) = config.next_override {
            // Check if "this" is already in the list
            let this_already_added = versions.iter().any(|v| matches!(v, compile::VersionSource::Local { .. }));

            if !this_already_added {
                debug!("Auto-adding 'this' version from {:?} (forced by default)", manifest_path);
                versions.push(compile::VersionSource::Local {
                    path: manifest_path.clone(),
                    forced: true, // Auto-added WIP is forced by default
                });
                // Mark auto-added WIP as forced
                force_local = true;
            }
        } else {
            // No local version (only --crate), add "latest" as final version if not already present
            match resolve_latest_version(&config.crate_name, false) {
                Ok(ver) => {
                    // Check if this version is already in the list
                    let already_present = versions
                        .iter()
                        .any(|v| matches!(v, compile::VersionSource::Published { version, .. } if version == &ver));

                    if !already_present {
                        debug!("No local version, adding latest: {}", ver);
                        versions.push(compile::VersionSource::Published { version: ver, forced: false });
                    } else {
                        debug!("Latest version {} already in test list, skipping auto-add", ver);
                    }
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
        args.dependents.iter().map(|spec| parse_dependent_spec(spec)).collect()
    } else {
        // Top N by downloads (no version spec)
        let api_deps =
            api::get_top_dependents(&config.crate_name, args.top_dependents).map_err(|e| Error::CratesIoApiError(e))?;
        api_deps.into_iter().map(|d| (d.name, None)).collect()
    };

    // Build version list for display (same logic as per-dependent)
    let versions_to_test = test_versions.clone().unwrap_or_else(|| {
        let mut versions = Vec::new();
        if let CrateOverride::Source(ref manifest_path) = config.next_override {
            versions.push(compile::VersionSource::Local { path: manifest_path.clone(), forced: force_local });
        } else {
            if let Ok(ver) = resolve_latest_version(&config.crate_name, false) {
                versions.push(compile::VersionSource::Published { version: ver, forced: false });
            }
        }
        versions
    });

    // Generate test plan string
    let test_plan = format_test_plan(&rev_deps, &versions_to_test, &config.force_versions, force_local, &config.crate_name);

    // Initialize table widths based on versions being tested
    let version_strings: Vec<String> = versions_to_test.iter().map(|v| v.label()).collect();
    report::init_table_widths(&version_strings, &config.display_version(), !config.force_versions.is_empty());

    // Get path for "this" definition (if local source)
    let this_path = match &config.next_override {
        CrateOverride::Source(path) => Some(path.display().to_string()),
        _ => None,
    };

    // Print table header for streaming output with embedded test plan
    let total = rev_deps.len();
    report::print_table_header(&config.crate_name, &config.display_version(), total, Some(&test_plan), this_path.as_deref());

    // Run tests serially and collect results
    let mut all_rows = Vec::new();
    for (i, (rev_dep, version)) in rev_deps.into_iter().enumerate() {
        // Always use multi-version testing (legacy path removed)
        // If --test-versions not specified, build vec with just "this" - baseline will be auto-inferred
        let versions = test_versions.clone().unwrap_or_else(|| {
            let mut versions = Vec::new();
            // Add "this" (local WIP) or "latest" if no local version
            if let CrateOverride::Source(ref manifest_path) = config.next_override {
                versions.push(compile::VersionSource::Local { path: manifest_path.clone(), forced: force_local });
            } else {
                // No local version (only --crate), add "latest" as final version
                if let Ok(ver) = resolve_latest_version(&config.crate_name, false) {
                    versions.push(compile::VersionSource::Published { version: ver, forced: false });
                }
            }
            versions
        });

        let (result, rows) = run_test_multi_version(config.clone(), rev_dep, version, versions, force_local, config.error_lines);

        // rows were already printed during testing, just collect for summary

        // Print separator after each dependent
        if i < total - 1 {
            report::print_separator_line();
        }

        all_rows.extend(rows);
    }

    // Print table footer
    report::print_table_footer();

    // Print comparison table (replaces old summary)
    let comparison_stats = report::generate_comparison_table(&all_rows);
    report::print_comparison_table(&comparison_stats);

    // Generate markdown report
    let markdown_path = PathBuf::from("copter-report.md");
    match report::export_markdown_table_report(
        &all_rows,
        &markdown_path,
        &config.crate_name,
        &config.display_version(),
        total,
    ) {
        Ok(_) => {
            println!("Markdown report: {}", markdown_path.display());
        }
        Err(e) => {
            eprintln!("Warning: Failed to generate markdown report: {}", e);
        }
    }

    // Exit with error code if there were regressions
    let summary = report::summarize_offered_rows(&all_rows);

    // Show failure log path if there were any failures
    if summary.regressed > 0 || summary.broken > 0 {
        println!("\nDetailed failure logs: {}", log_path.display());
    }

    // Check for skipped (non-forced) versions and suggest --force-versions
    let skipped_versions: Vec<String> = all_rows
        .iter()
        .filter(|row| {
            if let Some(offered) = &row.offered { !offered.forced && !row.primary.used_offered_version } else { false }
        })
        .filter_map(|row| row.offered.as_ref().map(|o| o.version.clone()))
        .collect::<std::collections::HashSet<_>>()
        .into_iter()
        .collect();

    if !skipped_versions.is_empty() {
        let versions_list = skipped_versions.join(" ");
        println!("\nℹ️  Some versions were not used by cargo (semver incompatible):");
        println!("   To force-test these versions anyway, use: --force-versions {}", versions_list);
    }

    // Collect versions that caused regressions and suggest cargo-public-api
    if summary.regressed > 0 {
        let regressed_versions: Vec<String> = all_rows
            .iter()
            .filter(|row| {
                if let Some(offered) = &row.offered {
                    let overall_passed = row.test.commands.iter().all(|cmd| cmd.result.passed);
                    row.baseline_passed == Some(true) && !overall_passed
                } else {
                    false
                }
            })
            .filter_map(|row| row.offered.as_ref().map(|o| o.version.clone()))
            .collect::<std::collections::HashSet<_>>()
            .into_iter()
            .collect();

        if !regressed_versions.is_empty() {
            println!("\n💡 To analyze API changes that may have caused regressions:");
            println!("   Install: cargo install cargo-public-api");
            println!();
            for version in &regressed_versions {
                let staging_path = args.staging_dir.join(format!("{}-{}", config.crate_name, version));
                println!("   # Compare baseline vs {}", version);
                println!(
                    "   cargo public-api diff {} {}",
                    args.staging_dir.join(format!("{}-baseline", config.crate_name)).display(),
                    staging_path.display()
                );
                println!();
            }
        }
    }

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
    next_override: CrateOverride,
    force_versions: Vec<String>, // List of versions to force (bypass semver)
    error_lines: usize,          // Maximum lines to show per error (0 = unlimited)
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
    Source(PathBuf),
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
    // Determine crate name and version based on --crate and --path
    let (crate_name, version, next_override) = if let Some(ref crate_name) = args.crate_name {
        // --crate specified: use that name
        debug!("Using crate name from --crate: {}", crate_name);

        // Check if --path is also specified (for "this" version)
        let (version, next_override) = if let Some(ref path) = args.path {
            let manifest = if path.is_dir() { path.join("Cargo.toml") } else { path.clone() };
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
            if path.is_dir() { path.join("Cargo.toml") } else { path.clone() }
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
        next_override,
        force_versions: args.force_versions.clone(),
        error_lines: args.error_lines,
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

fn http_get_bytes(url: &str) -> Result<Vec<u8>, Error> {
    let resp = ureq::get(url).set("User-Agent", USER_AGENT).call()?;
    let len = resp.header("Content-Length").and_then(|s| s.parse::<usize>().ok()).unwrap_or(0);
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
    data: TestResultData,
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
    pub dependent_name: String,         // "image"
    pub dependent_version: String,      // "0.25.8"
    pub spec: String,                   // "^0.8.52" (what they require)
    pub resolved_version: String,       // "0.8.91" (what cargo chose)
    pub resolved_source: VersionSource, // CratesIo | Local | Git
    pub used_offered_version: bool,     // true if resolved == offered
}

/// Version offered for testing
#[derive(Debug, Clone)]
pub struct OfferedVersion {
    pub version: String, // "this(0.8.91)" or "0.8.51"
    pub forced: bool,    // true shows [≠→!] suffix
}

/// Test execution (Install/Check/Test)
#[derive(Debug, Clone)]
pub struct TestExecution {
    pub commands: Vec<TestCommand>, // fetch, check, test
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
    pub failures: Vec<CrateFailure>, // Which crate(s) failed
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

/// Extract error message from diagnostics with stderr fallback
fn extract_error_with_fallback(diagnostics: &[error_extract::Diagnostic], stderr: &str, _max_lines: usize) -> String {
    // Always extract FULL error for storage - truncation happens at display time
    let msg = error_extract::extract_error_summary(diagnostics, 0); // 0 = unlimited
    if !msg.is_empty() {
        msg
    } else {
        // Return full stderr
        stderr.to_string()
    }
}

/// Convert CompileResult to TestCommand for OfferedRow construction
fn compile_result_to_command(
    compile_result: &compile::CompileResult,
    command_type: CommandType,
    crate_name: &str,
    max_error_lines: usize,
) -> TestCommand {
    let failures = if !compile_result.success {
        let error_msg =
            extract_error_with_fallback(&compile_result.diagnostics, &compile_result.stderr, max_error_lines);
        vec![CrateFailure { crate_name: crate_name.to_string(), error_message: error_msg }]
    } else {
        vec![]
    };

    TestCommand {
        command: command_type,
        features: vec![],
        result: CommandResult {
            passed: compile_result.success,
            duration: compile_result.duration.as_secs_f64(),
            failures,
        },
    }
}

impl TestResult {
    /// Convert TestResult to OfferedRows for streaming output
    fn to_offered_rows(&self, max_error_lines: usize) -> Vec<OfferedRow> {
        match &self.data {
            TestResultData::MultiVersion(outcomes) => {
                let mut rows = Vec::new();

                // First outcome is always baseline
                let baseline = outcomes.first();

                for (idx, outcome) in outcomes.iter().enumerate() {
                    let is_baseline = idx == 0;

                    // Determine baseline_passed for this row
                    let baseline_passed = if is_baseline {
                        None // This IS the baseline
                    } else {
                        baseline.map(|b| b.result.is_success())
                    };

                    // Convert compile::VersionSource to main::VersionSource
                    let resolved_source = match &outcome.version_source {
                        compile::VersionSource::Local { .. } => VersionSource::Local,
                        compile::VersionSource::Published { .. } => VersionSource::CratesIo,
                    };

                    // Build primary DependencyRef
                    let primary = DependencyRef {
                        dependent_name: self.rev_dep.name.clone(),
                        dependent_version: self.rev_dep.vers.to_string(),
                        spec: outcome.result.original_requirement.clone().unwrap_or_else(|| "?".to_string()),
                        resolved_version: outcome
                            .result
                            .actual_version
                            .clone()
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
                    commands.push(compile_result_to_command(
                        &outcome.result.fetch,
                        CommandType::Fetch,
                        &self.rev_dep.name,
                        max_error_lines,
                    ));

                    // Check command (if ran)
                    if let Some(ref check) = outcome.result.check {
                        commands.push(compile_result_to_command(
                            check,
                            CommandType::Check,
                            &self.rev_dep.name,
                            max_error_lines,
                        ));
                    }

                    // Test command (if ran)
                    if let Some(ref test) = outcome.result.test {
                        commands.push(compile_result_to_command(
                            test,
                            CommandType::Test,
                            &self.rev_dep.name,
                            max_error_lines,
                        ));
                    }

                    // Convert all_crate_versions to TransitiveTest entries
                    // Filter out the primary dependency (only show subdependencies with different versions)
                    let primary_version = &primary.resolved_version;
                    let main_dependent_name = &self.rev_dep.name;
                    // Normalize name for comparison (cargo uses hyphens, crates.io might use underscores)
                    let normalized_main = main_dependent_name.replace('_', "-");
                    let transitive = outcome
                        .result
                        .all_crate_versions
                        .iter()
                        .filter(|(_, resolved_version, dependent_name)| {
                            let normalized_dep = dependent_name.replace('_', "-");
                            // Exclude: same version as primary, or from main dependent itself
                            resolved_version != primary_version && normalized_dep != normalized_main
                        })
                        .map(|(spec, resolved_version, dependent_name)| {
                            TransitiveTest {
                                dependency: DependencyRef {
                                    dependent_name: dependent_name.clone(),
                                    dependent_version: String::new(), // Not available from cargo tree
                                    spec: spec.clone(),
                                    resolved_version: resolved_version.clone(),
                                    resolved_source: VersionSource::CratesIo, // Assume crates.io for now
                                    used_offered_version: false,              // Determine based on version match
                                },
                                depth: 1, // All are depth 1 for simplicity
                            }
                        })
                        .collect();

                    rows.push(OfferedRow {
                        baseline_passed,
                        primary,
                        offered,
                        test: TestExecution { commands },
                        transitive,
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

    fn skipped(rev_dep: RevDep, reason: String) -> TestResult {
        TestResult { rev_dep, data: TestResultData::Skipped(reason) }
    }

    fn error(rev_dep: RevDep, e: Error) -> TestResult {
        TestResult { rev_dep, data: TestResultData::Error(e) }
    }
}

fn run_test_multi_version(
    config: Config,
    rev_dep: RevDepName,
    version: Option<String>,
    test_versions: Vec<compile::VersionSource>,
    force_local: bool,
    max_error_lines: usize,
) -> (TestResult, Vec<OfferedRow>) {
    run_multi_version_test(&config, rev_dep, version, test_versions, force_local, max_error_lines)
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
            output =
                Command::new("cargo").args(&["metadata", "--format-version=1"]).current_dir(&crate_dir).output()?;
        }

        if output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            debug!("cargo metadata output length: {} bytes", stdout.len());

            // Parse JSON metadata using new metadata module
            match metadata::parse_metadata(&stdout) {
                Ok(parsed) => {
                    debug!("Successfully parsed metadata JSON");

                    // Find all versions of the target crate
                    let all_versions = metadata::find_all_versions(&parsed, crate_name);

                    // Group by version
                    let mut found_versions: std::collections::HashMap<String, Vec<(String, String)>> =
                        std::collections::HashMap::new();

                    for version_info in &all_versions {
                        found_versions.entry(version_info.version.clone())
                            .or_insert_with(Vec::new)
                            .push((version_info.node_id.clone(), version_info.spec.clone()));
                    }

                    // Report if we found multiple versions
                    if found_versions.len() > 1 {
                        println!("\n⚠️  MULTIPLE VERSIONS DETECTED for {} in {} {}:",
                                 crate_name, rev_dep.name, rev_dep.vers);
                        println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");

                        let mut versions: Vec<_> = found_versions.iter().collect();
                        versions.sort_by_key(|(v, _)| *v);

                        for (version, dependents) in versions {
                            println!("  Version {}: used by {} package(s)", version, dependents.len());
                            for (i, (node_id, spec)) in dependents.iter().enumerate() {
                                if i < 5 {
                                    // Parse the package ID to get a cleaner name
                                    if let Some((name, ver)) = metadata::parse_node_id(node_id) {
                                        println!("    ├─ {}@{} (spec: {})", name, ver, spec);
                                    } else {
                                        println!("    ├─ {} (spec: {})", node_id, spec);
                                    }
                                } else if i == 5 {
                                    println!("    └─ ... and {} more", dependents.len() - 5);
                                    break;
                                }
                            }
                        }

                        println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━\n");

                        // Also save full metadata for debugging
                        let debug_file = staging_path.join(format!("metadata-{}.json", crate_name));
                        let _ = fs::write(&debug_file, stdout.as_bytes());
                        println!("📋 Full metadata saved to: {}\n", debug_file.display());
                    }

                    // Return the first found version
                    if let Some(version_info) = all_versions.first() {
                        debug!("Primary resolved {} to version: {}", crate_name, version_info.version);
                        return Ok(version_info.version.clone());
                    }

                    debug!("Could not find {} in metadata", crate_name);
                }
                Err(e) => {
                    debug!("Failed to parse metadata: {}", e);
                }
            }
        } else {
            debug!("cargo metadata failed: {}", String::from_utf8_lossy(&output.stderr));
        }
    } else {
        debug!("Cargo.toml not found in {}", crate_dir.display());
    }

    Err(Error::ProcessError("Failed to extract resolved version via cargo metadata".to_string()))
}

/// Convert a single VersionTestOutcome to an OfferedRow
fn outcome_to_row(
    outcome: &VersionTestOutcome,
    rev_dep: &RevDep,
    baseline: Option<&VersionTestOutcome>,
    max_error_lines: usize,
) -> OfferedRow {
    // Determine if this is the baseline
    let is_baseline = baseline.is_none();

    // Determine baseline_passed for this row
    let baseline_passed = if is_baseline {
        None // This IS the baseline
    } else {
        baseline.map(|b| b.result.is_success())
    };

    // Convert compile::VersionSource to main::VersionSource
    let resolved_source = match &outcome.version_source {
        compile::VersionSource::Local { .. } => VersionSource::Local,
        compile::VersionSource::Published { .. } => VersionSource::CratesIo,
    };

    // Build primary DependencyRef
    let primary = DependencyRef {
        dependent_name: rev_dep.name.clone(),
        dependent_version: rev_dep.vers.to_string(),
        spec: outcome.result.original_requirement.clone().unwrap_or_else(|| "?".to_string()),
        resolved_version: outcome
            .result
            .actual_version
            .clone()
            .or(outcome.result.expected_version.clone())
            .unwrap_or_else(|| "?".to_string()),
        resolved_source,
        used_offered_version: outcome.result.expected_version == outcome.result.actual_version,
    };

    // Build OfferedVersion (None for baseline)
    let offered = if is_baseline {
        None
    } else {
        Some(OfferedVersion { version: outcome.version_source.label(), forced: outcome.result.forced_version })
    };

    // Build TestExecution from ThreeStepResult
    let mut commands = Vec::new();

    // Fetch command
    commands.push(compile_result_to_command(&outcome.result.fetch, CommandType::Fetch, &rev_dep.name, max_error_lines));

    // Check command (if ran)
    if let Some(ref check) = outcome.result.check {
        commands.push(compile_result_to_command(check, CommandType::Check, &rev_dep.name, max_error_lines));
    }

    // Test command (if ran)
    if let Some(ref test) = outcome.result.test {
        commands.push(compile_result_to_command(test, CommandType::Test, &rev_dep.name, max_error_lines));
    }

    // Convert all_crate_versions to TransitiveTest entries
    let primary_version = &primary.resolved_version;
    let main_dependent_name = &rev_dep.name;
    let normalized_main = main_dependent_name.replace('_', "-");
    let transitive = outcome
        .result
        .all_crate_versions
        .iter()
        .filter(|(_, resolved_version, dependent_name)| {
            let normalized_dep = dependent_name.replace('_', "-");
            resolved_version != primary_version && normalized_dep != normalized_main
        })
        .map(|(spec, resolved_version, dependent_name)| TransitiveTest {
            dependency: DependencyRef {
                dependent_name: dependent_name.clone(),
                dependent_version: String::new(),
                spec: spec.clone(),
                resolved_version: resolved_version.clone(),
                resolved_source: VersionSource::CratesIo,
                used_offered_version: false,
            },
            depth: 1,
        })
        .collect();

    OfferedRow { baseline_passed, primary, offered, test: TestExecution { commands }, transitive }
}

/// Run multi-version ICT tests for a dependent crate
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
    force_local: bool, // Whether local "this" versions should be forced
    max_error_lines: usize,
) -> (TestResult, Vec<OfferedRow>) {
    // Status line removed - redundant with table output
    // status(&format!("testing crate {} (multi-version)", rev_dep));

    // Resolve dependent version
    let mut rev_dep = match resolve_rev_dep_version(rev_dep.clone(), dependent_version) {
        Ok(r) => r,
        Err(e) => {
            let rev_dep = RevDep { name: rev_dep, vers: Version::parse("0.0.0").unwrap(), resolved_version: None };
            return (TestResult::error(rev_dep, e), vec![]);
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
            test_versions.insert(0, compile::VersionSource::Published { version: baseline.clone(), forced: false });
            debug!("Inserted baseline {} at position 0 (will be non-forced)", baseline);
        }
    }

    // Check version compatibility
    match check_version_compatibility(&rev_dep, &config) {
        Ok(true) => {} // Compatible
        Ok(false) => {
            let reason =
                format!("Dependent requires version incompatible with {} v{}", config.crate_name, config.version);
            return (TestResult::skipped(rev_dep, reason), vec![]);
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
                    return (TestResult::error(rev_dep, Error::IoError(e)), vec![]);
                }
                if let Err(e) = handle.unpack_source_to(&staging_path) {
                    return (TestResult::error(rev_dep, e), vec![]);
                }
            }
            Err(e) => return (TestResult::error(rev_dep, e), vec![]),
        }
    }

    // Run ICT tests for each version
    let mut outcomes = Vec::new();
    let mut rows = Vec::new();
    let mut baseline_outcome: Option<VersionTestOutcome> = None;
    let mut prev_error: Option<String> = None;

    debug!("Total versions to test: {}", test_versions.len());
    for (idx, version_source) in test_versions.iter().enumerate() {
        debug!(
            "[{}/{}] Testing {} against version {}",
            idx + 1,
            test_versions.len(),
            rev_dep.name,
            version_source.label()
        );

        // Check if this is the baseline (first version and matches baseline_version)
        let is_baseline = idx == 0 && baseline_version.is_some() && {
            if let compile::VersionSource::Published { version: ver, .. } = version_source {
                Some(ver.as_str()) == baseline_version.as_deref()
            } else {
                false
            }
        };
        debug!(
            "Version {}: idx={}, baseline_version={:?}, version_source={}, is_baseline={}",
            idx,
            idx,
            baseline_version,
            version_source.label(),
            is_baseline
        );

        // For baseline: no download, no patch - test as-is
        // For offered versions: download and patch
        let override_path = if is_baseline {
            debug!("Testing baseline version {} without patching", version_source.label());
            None // Let cargo handle baseline naturally
        } else {
            match &version_source {
                compile::VersionSource::Local { path, .. } => {
                    // If path points to Cargo.toml, extract directory
                    let dir_path =
                        if path.ends_with("Cargo.toml") { path.parent().unwrap().to_path_buf() } else { path.clone() };
                    debug!("Using local version path: {:?}", dir_path);
                    Some(dir_path)
                }
                compile::VersionSource::Published { version, .. } => {
                    match download_and_unpack_base_crate_version(&config.crate_name, version, &config.staging_dir) {
                        Ok(path) => Some(path),
                        Err(e) => {
                            status(&format!("Warning: Failed to download {} {}: {}", config.crate_name, version, e));
                            // Create a failed outcome
                            let is_forced = version_source.is_forced();

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
                                all_crate_versions: vec![],
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
        let skip_test = false; // TODO: Get from args

        // Determine expected version for verification and if it's forced
        // IMPORTANT: Baseline is NEVER forced, even if it's in --force-versions list
        let (expected_version, is_forced) = if is_baseline {
            match &version_source {
                compile::VersionSource::Published { version: v, .. } => (Some(v.clone()), false),
                compile::VersionSource::Local { .. } => (None, false),
            }
        } else {
            match &version_source {
                compile::VersionSource::Published { version: v, .. } => {
                    // Use the forced flag from VersionSource
                    (Some(v.clone()), version_source.is_forced())
                }
                compile::VersionSource::Local { .. } => {
                    // Use the forced flag from VersionSource
                    (None, version_source.is_forced())
                }
            }
        };

        // Create label for failure logging
        let test_label = if is_baseline {
            format!("baseline ({})", version_source.label())
        } else {
            match &version_source {
                compile::VersionSource::Published { version: v, .. } => format!("offered ({})", v),
                compile::VersionSource::Local { .. } => "offered (WIP)".to_string(),
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
                if let (Some(expected), Some(actual)) = (&result.expected_version, &result.actual_version) {
                    if actual != expected {
                        debug!("⚠️  VERSION MISMATCH: Expected {} but cargo resolved to {}!", expected, actual);
                    } else {
                        debug!("✓ Version verified: {} = {}", expected, actual);
                    }
                } else if result.expected_version.is_some() && result.actual_version.is_none() {
                    debug!("⚠️  Could not verify version for {} (cargo tree failed)", config.crate_name);
                }

                let outcome = VersionTestOutcome { version_source: version_source.clone(), result };

                // Convert outcome to row and print immediately
                // For idx == 0 (baseline), pass None as the baseline parameter
                let row = outcome_to_row(&outcome, &rev_dep, baseline_outcome.as_ref(), max_error_lines);
                let is_last_in_group = idx == test_versions.len() - 1;

                report::print_offered_row(&row, is_last_in_group, prev_error.as_deref(), max_error_lines);
                prev_error = report::extract_error_text(&row);

                // Store baseline for future row conversions (after printing)
                if idx == 0 {
                    baseline_outcome = Some(outcome.clone());
                }

                rows.push(row);
                outcomes.push(outcome);
            }
            Err(e) => {
                // ICT test failed with error - create a failed outcome
                return (TestResult::error(rev_dep.clone(), Error::ProcessError(e)), rows);
            }
        }
    }

    (TestResult { rev_dep, data: TestResultData::MultiVersion(outcomes) }, rows)
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
    extract_cargo_toml(&crate_handle.0, &extract_dir)?;

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

    let req_str = toml_helpers::extract_requirement_string(req);

    debug!("Checking if version {} satisfies requirement '{}'", wip_version, req_str);

    let version_req = VersionReq::parse(&req_str).map_err(|e| Error::SemverError(e))?;

    Ok(version_req.matches(wip_version))
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
    if let Err(e) = extract_cargo_toml(&crate_handle.0, &extract_dir) {
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

fn resolve_rev_dep_version(name: RevDepName, version: Option<String>) -> Result<RevDep, Error> {
    // If version is provided, use it directly
    if let Some(ver_str) = version {
        debug!("using pinned version {} for {}", ver_str, name);
        let vers = Version::parse(&ver_str).map_err(|e| Error::SemverError(e))?;
        return Ok(RevDep { name: name, vers: vers, resolved_version: None });
    }

    // Otherwise, resolve latest version from crates.io
    debug!("resolving current version for {}", name);

    let krate = api::get_client().get_crate(&name).map_err(|e| Error::CratesIoApiError(e.to_string()))?;

    // Pull out the version numbers and sort them
    let versions = krate.versions.iter().filter_map(|r| Version::parse(&r.num).ok());
    let mut versions = versions.collect::<Vec<_>>();
    versions.sort();

    versions.pop().map(|v| RevDep { name: name, vers: v, resolved_version: None }).ok_or(Error::NoCrateVersions)
}

/// Resolve 'latest' or 'latest-preview' keyword to actual version
fn resolve_latest_version(crate_name: &str, include_prerelease: bool) -> Result<String, Error> {
    debug!("Resolving latest version for {} (prerelease={})", crate_name, include_prerelease);

    let krate = api::get_client().get_crate(crate_name).map_err(|e| Error::CratesIoApiError(e.to_string()))?;

    // Filter and sort versions
    let mut versions: Vec<Version> = krate
        .versions
        .iter()
        .filter_map(|r| Version::parse(&r.num).ok())
        .filter(|v| include_prerelease || v.pre.is_empty()) // Filter pre-releases unless requested
        .collect();

    versions.sort();

    versions.pop().map(|v| v.to_string()).ok_or(Error::NoCrateVersions)
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
        let url = crate_url(&rev_dep.name, Some(&format!("{}/download", rev_dep.vers)));
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
    let vers = Version::parse(version).map_err(|e| Error::SemverError(e))?;
    let pseudo_dep = RevDep { name: RevDepName::from(crate_name.to_string()), vers, resolved_version: None };

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

/// Extract all files from a .crate file (gzipped tar) with --strip-components=1 behavior
fn extract_crate_archive(crate_file: &Path, dest_dir: &Path) -> Result<(), Error> {
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
fn extract_cargo_toml(crate_file: &Path, dest_dir: &Path) -> Result<(), Error> {
    let file = File::open(crate_file)?;
    let decoder = GzDecoder::new(file);
    let mut archive = Archive::new(decoder);

    for entry in archive.entries()? {
        let mut entry = entry?;
        let path = entry.path()?;

        // Check if this is a Cargo.toml file (equivalent to --wildcards */Cargo.toml)
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

impl CrateHandle {
    fn unpack_source_to(&self, path: &Path) -> Result<(), Error> {
        debug!("unpacking {:?} to {:?}", self.0, path);
        extract_crate_archive(&self.0, path)
    }
}

fn status_lock<F>(f: F)
where
    F: FnOnce() -> (),
{
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

    fn really_print_color(s: &str, fg: term::color::Color) -> bool {
        if let Some(ref mut t) = term::stdout() {
            if t.fg(fg).is_err() {
                return false;
            }
            let _ = t.attr(term::Attr::Bold);
            if write!(t, "{}", s).is_err() {
                return false;
            }
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

fn report_results(res: Result<Vec<TestResult>, Error>, _args: &cli::CliArgs, _config: &Config) {
    match res {
        Ok(_results) => {
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
    NoCrateVersions,
    FromUtf8Error(FromUtf8Error),
    ProcessError(String),
    InvalidPath(PathBuf),
    InvalidVersion(String),
}

macro_rules! convert_error {
    ($from:ty, $to:ident) => {
        impl From<$from> for Error {
            fn from(e: $from) -> Error {
                Error::$to(e)
            }
        }
    };
}

convert_error!(semver::Error, SemverError);
convert_error!(io::Error, IoError);
convert_error!(toml::de::Error, TomlError);
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
            Error::FromUtf8Error(ref e) => Some(e),
            _ => None,
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
