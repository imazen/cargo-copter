/// Configuration resolution module
///
/// This module handles:
/// - Building a TestMatrix from CLI arguments
/// - Resolving version keywords ("this", "latest", etc.)
/// - Validating and resolving all paths
/// - Determining baseline versions
use crate::api;
use crate::cli::CliArgs;
use crate::compile;
use crate::manifest;
use crate::types::*;
use crate::version;
use log::debug;
use std::env;
use std::path::PathBuf;

/// Build a complete TestMatrix from CLI arguments
///
/// This resolves all configuration upfront, ensuring the runner receives
/// a fully validated, immutable test specification.
pub fn build_test_matrix(args: &CliArgs) -> Result<TestMatrix, String> {
    debug!("Building test matrix from CLI args");

    // Step 1: Determine the base crate name and get version info
    let (base_crate_name, base_crate_version, local_manifest) = resolve_base_crate_info(args)?;

    debug!("Base crate: {} version {}", base_crate_name, base_crate_version);

    // Step 2: Build list of base crate versions to test
    let base_versions = resolve_base_versions(args, &base_crate_name, &base_crate_version, &local_manifest)?;

    debug!("Resolved {} base versions to test", base_versions.len());

    // Step 3: Build list of dependents to test
    let mut dependents = resolve_dependents(args, &base_crate_name)?;

    debug!("Resolved {} dependents to test", dependents.len());

    // Step 4: Expand with additional versions if --top-versions is specified
    if let Some(budget) = args.top_versions {
        let extra = resolve_top_versions(&dependents, budget)?;
        if !extra.is_empty() {
            debug!("Adding {} additional (dependent, version) pairs from --top-versions", extra.len());
            dependents.extend(extra);
        }
    }

    // Step 5: Ensure baseline versions are resolved for each dependent
    // (This happens during test execution when we need the actual resolved versions)

    // Deprecation warning for --patch-transitive
    if args.patch_transitive {
        eprintln!(
            "⚠️  DEPRECATED: --patch-transitive is no longer needed.\n\
             Auto-retry now handles this automatically. When --force-versions\n\
             encounters a 'multiple versions of crate' error, it automatically\n\
             retries with [patch.crates-io] applied. Look for the '!!' marker in output.\n"
        );
    }

    Ok(TestMatrix {
        base_crate: base_crate_name,
        base_versions,
        dependents,
        staging_dir: args.get_staging_dir(),
        skip_check: args.should_skip_check(),
        skip_test: args.should_skip_test(),
        error_lines: args.error_lines,
        patch_transitive: args.patch_transitive,
    })
}

/// Resolve base crate name, version, and optional local manifest path
///
/// Returns: (crate_name, version, local_manifest_path)
fn resolve_base_crate_info(args: &CliArgs) -> Result<(String, String, Option<PathBuf>), String> {
    if let Some(ref crate_name) = args.crate_name {
        // --crate specified: use that name
        debug!("Using crate name from --crate: {}", crate_name);

        // Check if --path is also specified (for "this" version)
        if let Some(ref path) = args.path {
            let manifest = if path.is_dir() { path.join("Cargo.toml") } else { path.clone() };
            debug!("Using --path for 'this' version: {:?}", manifest);

            // Extract version from the manifest
            let (manifest_crate_name, manifest_version) =
                manifest::get_crate_info(&manifest).map_err(|e| format!("Failed to read manifest: {}", e))?;

            // Verify crate names match
            if manifest_crate_name != *crate_name {
                return Err(format!(
                    "Crate name mismatch: --crate specifies '{}' but {} contains '{}'",
                    crate_name,
                    manifest.display(),
                    manifest_crate_name
                ));
            }

            Ok((crate_name.clone(), manifest_version, Some(manifest)))
        } else {
            // No --path, fetch latest version from crates.io
            debug!("No --path specified, fetching latest version from crates.io");
            let latest_version =
                version::resolve_latest_version(crate_name, false).unwrap_or_else(|_| "0.0.0".to_string());
            Ok((crate_name.clone(), latest_version, None))
        }
    } else {
        // No --crate, use --path or ./Cargo.toml
        let manifest = if let Some(ref path) = args.path {
            if path.is_dir() { path.join("Cargo.toml") } else { path.clone() }
        } else {
            let env_manifest = env::var("COPTER_MANIFEST");
            PathBuf::from(env_manifest.unwrap_or_else(|_| "./Cargo.toml".to_string()))
        };

        debug!("Using manifest {:?}", manifest);

        let (crate_name, version) =
            manifest::get_crate_info(&manifest).map_err(|e| format!("Failed to read manifest: {}", e))?;

        Ok((crate_name, version, Some(manifest)))
    }
}

/// Resolve all base crate versions to test
///
/// Returns a list of VersionSpec with the baseline first
fn resolve_base_versions(
    args: &CliArgs,
    crate_name: &str,
    local_version: &str,
    local_manifest: &Option<PathBuf>,
) -> Result<Vec<VersionSpec>, String> {
    let mut versions = Vec::new();

    // Determine if we're in multi-version mode
    let use_multi_version = !args.test_versions.is_empty() || !args.force_versions.is_empty();

    if use_multi_version {
        // Add specified versions from --test-versions
        for ver_str in &args.test_versions {
            if let Some(version_source) = version::resolve_version_keyword(ver_str, crate_name, local_manifest.as_ref())
                .map_err(|e| format!("Failed to resolve version '{}': {}", ver_str, e))?
            {
                let version_spec = version_source_to_spec(version_source, crate_name, false)?;
                versions.push(version_spec);
            }
        }

        // Add versions from --force-versions and mark them as forced
        for ver_str in &args.force_versions {
            if let Some(version_source) = version::resolve_version_keyword(ver_str, crate_name, local_manifest.as_ref())
                .map_err(|e| format!("Failed to resolve forced version '{}': {}", ver_str, e))?
            {
                let mut version_spec = version_source_to_spec(version_source, crate_name, true)?;
                version_spec.override_mode = OverrideMode::Force;
                versions.push(version_spec);
            }
        }

        // Auto-insert non-forced variants for each forced version (unless --skip-normal-testing)
        if !args.skip_normal_testing {
            let forced_versions: Vec<VersionSpec> =
                versions.iter().filter(|v| v.override_mode == OverrideMode::Force).cloned().collect();

            for forced_ver in forced_versions {
                // Check if a non-forced variant already exists
                let has_non_forced = versions.iter().any(|v| {
                    v.crate_ref.version == forced_ver.crate_ref.version
                        && v.crate_ref.source == forced_ver.crate_ref.source
                        && v.override_mode != OverrideMode::Force
                });

                if !has_non_forced {
                    // Insert non-forced variant
                    let mut non_forced = forced_ver.clone();
                    non_forced.override_mode = OverrideMode::Patch;
                    debug!("Auto-inserting non-forced test for version {}", non_forced.crate_ref.version.display());
                    versions.push(non_forced);
                }
            }

            // Sort: non-forced before forced for same version
            versions.sort_by(|a, b| {
                use std::cmp::Ordering;
                let version_cmp = a.crate_ref.version.display().cmp(&b.crate_ref.version.display());
                if version_cmp == Ordering::Equal {
                    (a.override_mode == OverrideMode::Force).cmp(&(b.override_mode == OverrideMode::Force))
                } else {
                    version_cmp
                }
            });
        }

        // Auto-add "this" (local WIP) in forced mode if not already specified
        if let Some(manifest_path) = local_manifest {
            // Check if "this" is already in the list
            let this_already_added = versions.iter().any(|v| matches!(v.crate_ref.source, CrateSource::Local { .. }));

            if !this_already_added {
                debug!("Auto-adding 'this' version from {:?} (forced by default)", manifest_path);
                let this_version = VersionSpec {
                    crate_ref: VersionedCrate::from_local(crate_name, local_version, manifest_path.clone()),
                    override_mode: OverrideMode::Force,
                    is_baseline: false,
                };
                versions.push(this_version);
            }
        } else {
            // No local version (only --crate), add "latest" as final version if not already present
            match version::resolve_latest_version(crate_name, false) {
                Ok(ver) => {
                    let already_present =
                        versions.iter().any(|v| matches!(&v.crate_ref.version, Version::Semver(s) if s == &ver));

                    if !already_present {
                        debug!("No local version, adding latest: {}", ver);
                        versions.push(VersionSpec::with_patch(VersionedCrate::from_registry(crate_name, ver)));
                    }
                }
                Err(e) => {
                    debug!("Warning: Failed to resolve latest version: {}", e);
                }
            }
        }
    } else {
        // Default behavior: baseline + WIP
        if let Some(manifest_path) = local_manifest {
            // Add baseline first (latest from registry)
            if let Ok(latest_ver) = version::resolve_latest_version(crate_name, false) {
                versions.push(VersionSpec {
                    crate_ref: VersionedCrate::from_registry(crate_name, latest_ver),
                    override_mode: OverrideMode::None,
                    is_baseline: true,
                });
            }

            // Then add WIP (local version)
            versions.push(VersionSpec {
                crate_ref: VersionedCrate::from_local(crate_name, local_version, manifest_path.clone()),
                override_mode: OverrideMode::Force,
                is_baseline: false,
            });
        } else {
            // No local version, use latest as baseline
            if let Ok(ver) = version::resolve_latest_version(crate_name, false) {
                versions.push(VersionSpec {
                    crate_ref: VersionedCrate::from_registry(crate_name, ver),
                    override_mode: OverrideMode::None,
                    is_baseline: true,
                });
            }
        }
    }

    if versions.is_empty() {
        return Err("No versions to test".to_string());
    }

    // Ensure exactly one baseline is marked with OverrideMode::None
    // (In default mode, baseline is already set. In multi-version mode, mark first)
    let baseline_count = versions.iter().filter(|v| v.is_baseline).count();
    if baseline_count == 0
        && let Some(first) = versions.first_mut()
    {
        first.is_baseline = true;
        first.override_mode = OverrideMode::None; // CRITICAL: baseline must have no override!
    }

    Ok(versions)
}

/// Convert compile::VersionSource to VersionSpec
fn version_source_to_spec(
    source: compile::VersionSource,
    crate_name: &str,
    forced: bool,
) -> Result<VersionSpec, String> {
    let override_mode = if forced { OverrideMode::Force } else { OverrideMode::Patch };

    match source {
        compile::VersionSource::Published { version, .. } => Ok(VersionSpec {
            crate_ref: VersionedCrate::from_registry(crate_name, version),
            override_mode,
            is_baseline: false,
        }),
        compile::VersionSource::Local { path, .. } => {
            // Extract version from Cargo.toml
            let manifest = if path.ends_with("Cargo.toml") { path } else { path.join("Cargo.toml") };
            let (_, local_version) =
                manifest::get_crate_info(&manifest).map_err(|e| format!("Failed to read local manifest: {}", e))?;

            Ok(VersionSpec {
                crate_ref: VersionedCrate::from_local(crate_name, local_version, manifest),
                override_mode,
                is_baseline: false,
            })
        }
    }
}

/// Resolve all dependents to test
/// Expand --dependent-glob and --dependent-dir into concrete paths
fn expand_dependent_discovery(args: &CliArgs, base_crate_name: &str) -> Result<Vec<PathBuf>, String> {
    let mut discovered = Vec::new();

    // Expand --dependent-glob patterns
    for pattern in &args.dependent_glob {
        // Expand ~ to home directory
        let expanded = if pattern.starts_with('~') {
            if let Some(home) = dirs::home_dir() {
                pattern.replacen('~', &home.display().to_string(), 1)
            } else {
                pattern.clone()
            }
        } else {
            pattern.clone()
        };

        let entries = glob::glob(&expanded).map_err(|e| format!("Invalid glob pattern '{}': {}", pattern, e))?;

        for entry in entries {
            let path = entry.map_err(|e| format!("Glob error: {}", e))?;
            if path.file_name().map(|n| n == "Cargo.toml").unwrap_or(false) {
                match manifest::depends_on(&path, base_crate_name) {
                    Ok(true) => {
                        let dir = path.parent().unwrap().to_path_buf();
                        debug!("Glob discovered dependent: {}", dir.display());
                        discovered.push(dir);
                    }
                    Ok(false) => {
                        debug!("Glob skipping {} (does not depend on {})", path.display(), base_crate_name);
                    }
                    Err(e) => {
                        debug!("Glob skipping {} ({})", path.display(), e);
                    }
                }
            }
        }
    }

    // Expand --dependent-dir (search one level deep for Cargo.toml)
    for dir in &args.dependent_dir {
        if !dir.is_dir() {
            return Err(format!("--dependent-dir path is not a directory: {}", dir.display()));
        }

        let entries =
            std::fs::read_dir(dir).map_err(|e| format!("Failed to read directory {}: {}", dir.display(), e))?;

        for entry in entries {
            let entry = entry.map_err(|e| format!("Directory read error: {}", e))?;
            let child = entry.path();
            if child.is_dir() {
                let manifest = child.join("Cargo.toml");
                if manifest.exists() {
                    match manifest::depends_on(&manifest, base_crate_name) {
                        Ok(true) => {
                            debug!("Dir discovered dependent: {}", child.display());
                            discovered.push(child);
                        }
                        Ok(false) => {
                            debug!("Dir skipping {} (does not depend on {})", child.display(), base_crate_name);
                        }
                        Err(e) => {
                            debug!("Dir skipping {} ({})", child.display(), e);
                        }
                    }
                }
            }
        }
    }

    // Deduplicate by canonical path
    let mut seen = std::collections::HashSet::new();
    discovered.retain(|p| {
        let canonical = p.canonicalize().unwrap_or_else(|_| p.clone());
        seen.insert(canonical)
    });

    if !discovered.is_empty() {
        eprintln!("Discovered {} local dependent(s) of {}", discovered.len(), base_crate_name);
    }

    Ok(discovered)
}

fn resolve_dependents(args: &CliArgs, base_crate_name: &str) -> Result<Vec<VersionSpec>, String> {
    let mut dependents = Vec::new();

    // Determine which dependents to test
    // Collect local path dependents separately (they use CrateSource::Local, not Registry)
    let mut local_dependents: Vec<VersionSpec> = Vec::new();

    // Expand --dependent-glob and --dependent-dir into additional paths
    let discovered_paths = expand_dependent_discovery(args, base_crate_name)?;

    // Combine explicit --dependent-paths with discovered paths
    let all_local_paths: Vec<PathBuf> = args.dependent_paths.iter().cloned().chain(discovered_paths).collect();

    let rev_deps: Vec<(String, Option<String>)> = if !all_local_paths.is_empty() {
        // Local paths mode - read Cargo.toml from each path to get crate name and version
        for p in &all_local_paths {
            let manifest_path = if p.ends_with("Cargo.toml") {
                p.clone()
            } else if p.is_dir() {
                p.join("Cargo.toml")
            } else {
                return Err(format!("Invalid dependent path (not a directory or Cargo.toml): {}", p.display()));
            };

            let (name, version) = manifest::get_crate_info(&manifest_path)
                .map_err(|e| format!("Failed to read dependent at {}: {}", manifest_path.display(), e))?;

            let dir_path = if manifest_path.ends_with("Cargo.toml") {
                manifest_path.parent().unwrap().to_path_buf()
            } else {
                p.clone()
            };

            local_dependents.push(VersionSpec {
                crate_ref: VersionedCrate::from_local(&name, &version, dir_path),
                override_mode: OverrideMode::None,
                is_baseline: false, // Will be set below
            });
        }
        // Return empty rev_deps since we handled these directly
        vec![]
    } else if !args.dependents.is_empty() {
        // Explicit crate names from crates.io (parse name:version syntax)
        args.dependents.iter().map(|spec| manifest::parse_dependent_spec(spec)).collect()
    } else {
        // Top N by downloads (no version spec)
        let api_deps = api::get_top_dependents(base_crate_name, args.top_dependents)
            .map_err(|e| format!("Failed to fetch top dependents: {}", e))?;
        api_deps.into_iter().map(|d| (d.name, None)).collect()
    };

    // Add local dependents first (from --dependent-paths)
    for mut local_dep in local_dependents {
        local_dep.is_baseline = dependents.is_empty(); // First is baseline
        dependents.push(local_dep);
    }

    // Add registry dependents (from --dependents or --top-dependents)
    for (name, version) in rev_deps {
        let version_spec = if let Some(ver) = version {
            // Specific version requested
            VersionSpec {
                crate_ref: VersionedCrate::from_registry(name, ver),
                override_mode: OverrideMode::None,
                is_baseline: dependents.is_empty(), // First is baseline
            }
        } else {
            // Use Latest, will be resolved at test time
            VersionSpec {
                crate_ref: VersionedCrate::latest_from_registry(name),
                override_mode: OverrideMode::None,
                is_baseline: dependents.is_empty(), // First is baseline
            }
        };

        dependents.push(version_spec);
    }

    if dependents.is_empty() {
        return Err("No dependents to test".to_string());
    }

    Ok(dependents)
}

/// Resolve additional (dependent, version) pairs for --top-versions budget
///
/// Each dependent already has its latest version in the list. This function
/// fetches version download counts and allocates Q additional slots across
/// all dependents, ranked by downloads.
fn resolve_top_versions(existing_dependents: &[VersionSpec], budget: usize) -> Result<Vec<VersionSpec>, String> {
    if budget == 0 {
        return Ok(vec![]);
    }

    // Only expand registry dependents (local dependents don't have multiple versions on crates.io)
    let registry_deps: Vec<&VersionSpec> =
        existing_dependents.iter().filter(|d| matches!(d.crate_ref.source, CrateSource::Registry)).collect();

    if registry_deps.is_empty() {
        return Ok(vec![]);
    }

    // Collect all (dependent_name, version, downloads) across all dependents
    let mut all_pairs: Vec<(String, String, u64)> = Vec::new();

    for dep in &registry_deps {
        let name = &dep.crate_ref.name;
        match api::get_version_downloads(name) {
            Ok(versions) => {
                // Skip the latest version (already in the list)
                // The latest is the first one that's not yanked and not prerelease,
                // but we need to match what the runner will resolve. Skip the first entry
                // since versions are sorted by downloads (most popular first).
                // Instead, skip versions that match the existing entry.
                let existing_version = match &dep.crate_ref.version {
                    Version::Semver(v) => Some(v.as_str()),
                    _ => None,
                };

                for v in &versions {
                    // Skip the version already selected (latest)
                    if existing_version.is_some_and(|ev| ev == v.version) {
                        continue;
                    }
                    all_pairs.push((name.clone(), v.version.clone(), v.downloads));
                }
            }
            Err(e) => {
                debug!("Warning: Could not fetch versions for {}: {}", name, e);
            }
        }
    }

    // Sort by downloads descending
    all_pairs.sort_by_key(|(_name, _ver, downloads)| std::cmp::Reverse(*downloads));

    // Take top Q
    let selected: Vec<VersionSpec> = all_pairs
        .into_iter()
        .take(budget)
        .map(|(name, version, _downloads)| VersionSpec {
            crate_ref: VersionedCrate::from_registry(name, version),
            override_mode: OverrideMode::None,
            is_baseline: false,
        })
        .collect();

    if !selected.is_empty() {
        eprintln!(
            "Selected {} additional dependent version(s) by download count (--top-versions {})",
            selected.len(),
            budget
        );
    }

    Ok(selected)
}

#[cfg(test)]
#[path = "config_test.rs"]
mod config_test;
