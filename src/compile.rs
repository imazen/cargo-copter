use crate::error_extract::{Diagnostic, parse_cargo_json};
use crate::metadata;
use fs2::FileExt;
use lazy_static::lazy_static;
use log::{debug, warn};
use std::env;
use std::fs::{self, OpenOptions};
use std::io::{BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;
use std::time::{Duration, Instant};

// Constants for formatting and limits
const LOG_SEPARATOR_LENGTH: usize = 100;
const MAX_METADATA_LOG_LINES: usize = 100;

// Failure log file path
lazy_static! {
    static ref FAILURE_LOG: Mutex<Option<PathBuf>> = Mutex::new(None);
    static ref BUILD_FAILURE_LOG: Mutex<Option<PathBuf>> = Mutex::new(None);
    // Track last error signature for deduplication
    static ref LAST_ERROR_SIGNATURE: Mutex<Option<String>> = Mutex::new(None);
}

/// Initialize the failure log file
pub fn init_failure_log(log_path: PathBuf) {
    let mut log = FAILURE_LOG.lock().unwrap();
    *log = Some(log_path.clone());

    // Also initialize build-only log
    let build_log_path = log_path.with_file_name("copter-build-failures.log");
    let mut build_log = BUILD_FAILURE_LOG.lock().unwrap();
    *build_log = Some(build_log_path);

    // Clear the error signature when initializing
    let mut sig = LAST_ERROR_SIGNATURE.lock().unwrap();
    *sig = None;
}

/// Log a compilation failure to the failure log file with proper locking
#[allow(clippy::too_many_arguments)]
pub fn log_failure(
    dependent: &str,
    dependent_version: &str,
    base_crate: &str,
    test_label: &str, // "baseline", "WIP", or version number
    command: &str,
    exit_code: Option<i32>,
    stdout: &str,
    stderr: &str,
) {
    log_failure_with_diagnostics(
        dependent,
        dependent_version,
        base_crate,
        test_label,
        command,
        exit_code,
        stdout,
        stderr,
        &[],
    );
}

/// Log a compilation failure with parsed diagnostics for better readability
#[allow(clippy::too_many_arguments)]
pub fn log_failure_with_diagnostics(
    dependent: &str,
    dependent_version: &str,
    base_crate: &str,
    test_label: &str, // "baseline", "WIP", or version number
    command: &str,
    exit_code: Option<i32>,
    stdout: &str,
    stderr: &str,
    diagnostics: &[Diagnostic],
) {
    let (log_path, build_log_path) = {
        let log = FAILURE_LOG.lock().unwrap();
        let build_log = BUILD_FAILURE_LOG.lock().unwrap();
        match (&*log, &*build_log) {
            (Some(path), Some(build_path)) => (path.clone(), Some(build_path.clone())),
            (Some(path), None) => (path.clone(), None),
            _ => return, // Logging not initialized
        }
    };

    // Generate error signature for deduplication
    let current_signature = if !diagnostics.is_empty() {
        let error_text = diagnostics.iter().map(|d| d.rendered.as_str()).collect::<Vec<_>>().join("\n");
        crate::report::error_signature(&error_text)
    } else {
        crate::report::error_signature(stderr)
    };

    // Check if this error matches the previous one
    let is_duplicate = {
        let mut last_sig = LAST_ERROR_SIGNATURE.lock().unwrap();
        let duplicate = last_sig.as_ref().map(|s| s == &current_signature).unwrap_or(false);
        *last_sig = Some(current_signature);
        duplicate
    };

    // Write to main debug log
    write_failure_to_log(
        &log_path,
        "FAILURE",
        dependent,
        dependent_version,
        base_crate,
        test_label,
        command,
        exit_code,
        stderr,
        diagnostics,
        is_duplicate,
    );

    // If this is a build/check failure, also write to build-specific log
    let is_build_failure = command.contains("cargo fetch") || command.contains("cargo check");
    if is_build_failure && let Some(build_path) = build_log_path {
        write_failure_to_log(
            &build_path,
            "BUILD FAILURE",
            dependent,
            dependent_version,
            base_crate,
            test_label,
            command,
            exit_code,
            stderr,
            diagnostics,
            is_duplicate,
        );
    }
}

/// Helper function to write a failure entry to a specific log file
#[allow(clippy::too_many_arguments)]
fn write_failure_to_log(
    log_path: &Path,
    log_type: &str, // "FAILURE" or "BUILD FAILURE"
    dependent: &str,
    dependent_version: &str,
    base_crate: &str,
    test_label: &str,
    command: &str,
    exit_code: Option<i32>,
    stderr: &str,
    diagnostics: &[Diagnostic],
    is_duplicate: bool,
) {
    // Open file with append mode
    let file = match OpenOptions::new().create(true).append(true).open(log_path) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("Failed to open {} log: {}", log_type, e);
            return;
        }
    };

    // Lock the file for exclusive write access
    if let Err(e) = file.lock_exclusive() {
        eprintln!("Failed to lock {} log: {}", log_type, e);
        return;
    }

    let mut writer = BufWriter::new(&file);
    let timestamp = chrono::Local::now().format("%Y-%m-%d %H:%M:%S");
    let exit_str = exit_code.map(|c| c.to_string()).unwrap_or_else(|| "N/A".to_string());

    let _ = writeln!(writer, "\n{}", "=".repeat(LOG_SEPARATOR_LENGTH));
    let _ = writeln!(
        writer,
        "[{}] {}: {} {} testing {} {}",
        timestamp, log_type, dependent, dependent_version, base_crate, test_label
    );
    let _ = writeln!(writer, "{}", "=".repeat(LOG_SEPARATOR_LENGTH));
    let _ = writeln!(writer, "Command: {}", command);
    let _ = writeln!(writer, "Exit code: {}", exit_str);

    if is_duplicate {
        let _ = writeln!(writer, "\n--- SAME FAILURE AS PREVIOUS ---");
    } else if !diagnostics.is_empty() {
        let _ = writeln!(writer, "\n--- ERRORS ---");
        for (idx, diag) in diagnostics.iter().enumerate() {
            let level_str = match diag.level {
                crate::error_extract::DiagnosticLevel::Error => "error",
                crate::error_extract::DiagnosticLevel::Warning => "warning",
                crate::error_extract::DiagnosticLevel::Help => "help",
                crate::error_extract::DiagnosticLevel::Note => "note",
                crate::error_extract::DiagnosticLevel::Other(ref s) => s.as_str(),
            };
            let _ = writeln!(writer, "\n{}. [{}] {}", idx + 1, level_str, diag.message);

            if !diag.rendered.is_empty() {
                let _ = writeln!(writer, "{}", diag.rendered);
            }
        }
    } else {
        let _ = writeln!(writer, "\n--- STDERR (no structured errors) ---");
        for line in stderr.lines() {
            if !line.trim_start().starts_with('{') {
                let _ = writeln!(writer, "{}", line);
            }
        }
    }

    let _ = writeln!(writer, "\n{}", "=".repeat(LOG_SEPARATOR_LENGTH));
    let _ = writer.flush();
    // Unlock is automatic when file goes out of scope
}

/// Restore Cargo.toml from the original backup before testing
/// This prevents contamination between test runs in the cached staging directory
///
/// CRITICAL: This is idempotent and Ctrl+C safe. If a backup exists from a previous
/// (possibly interrupted) run, we restore from it rather than overwriting it.
pub fn restore_cargo_toml(staging_path: &Path) -> Result<(), String> {
    let cargo_toml = staging_path.join("Cargo.toml");
    let original = staging_path.join("Cargo.toml.original.txt");

    // CRITICAL: Never overwrite existing .original - it might be from an interrupted run
    if !original.exists() {
        if cargo_toml.exists() {
            fs::copy(&cargo_toml, &original).map_err(|e| format!("Failed to save original Cargo.toml: {}", e))?;
            debug!("Saved original Cargo.toml to {:?}", original);
        }
    } else {
        // Restore from existing original (might be from interrupted run)
        fs::copy(&original, &cargo_toml).map_err(|e| format!("Failed to restore Cargo.toml from original: {}", e))?;
        debug!("Restored Cargo.toml from existing original backup in {:?}", staging_path);
    }
    Ok(())
}

/// The type of compilation step being performed
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum CompileStep {
    /// cargo fetch - download dependencies
    Fetch,
    /// cargo check - fast compilation check without code generation
    Check,
    /// cargo test - full test suite execution
    Test,
}

impl CompileStep {
    pub fn as_str(&self) -> &'static str {
        match self {
            CompileStep::Fetch => "fetch",
            CompileStep::Check => "check",
            CompileStep::Test => "test",
        }
    }

    pub fn cargo_subcommand(&self) -> &'static str {
        match self {
            CompileStep::Fetch => "fetch",
            CompileStep::Check => "check",
            CompileStep::Test => "test",
        }
    }
}

/// Result of a compilation step
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CompileResult {
    pub step: CompileStep,
    pub success: bool,
    pub stdout: String,
    pub stderr: String,
    pub duration: Duration,
    pub diagnostics: Vec<Diagnostic>,
}

impl CompileResult {
    pub fn failed(&self) -> bool {
        !self.success
    }
}

/// Verify that the correct version of a dependency is being used
/// Returns the actual version found, or None if not found
fn verify_dependency_version(crate_path: &Path, dep_name: &str) -> Option<String> {
    debug!("Verifying {} version in {:?}", dep_name, crate_path);

    // Try using cargo metadata which works better with path dependencies
    // Don't use --no-deps because we need to see resolved dependencies
    let output =
        Command::new("cargo").args(["metadata", "--format-version=1"]).current_dir(crate_path).output().ok()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        debug!("cargo metadata failed: {}", stderr.trim());
        return None;
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let metadata = match serde_json::from_str::<serde_json::Value>(&stdout) {
        Ok(m) => m,
        Err(e) => {
            debug!("Failed to parse metadata JSON: {}", e);
            return None;
        }
    };

    // First try resolve.nodes to find the actually-used version (handles multiple versions correctly)
    if let Some(resolve) = metadata.get("resolve")
        && let Some(nodes) = resolve.get("nodes").and_then(|n| n.as_array())
    {
        for node in nodes {
            if let Some(deps) = node.get("deps").and_then(|d| d.as_array()) {
                for dep in deps {
                    if let Some(name) = dep.get("name").and_then(|n| n.as_str())
                        && name == dep_name
                        && let Some(pkg) = dep.get("pkg").and_then(|p| p.as_str())
                    {
                        // pkg format: "registry+https://...#crate-name@version" or "path+file://...#crate-name@version"
                        // Extract version by splitting on "#" then "@"
                        if let Some(after_hash) = pkg.split('#').nth(1)
                            && let Some(version) = after_hash.split('@').nth(1)
                        {
                            debug!("✓ Verified {} version: {}", dep_name, version);
                            return Some(version.to_string());
                        }
                    }
                }
            }
        }
    }

    // Fallback: Check packages array for the dependency (may pick wrong version if multiple exist)
    let packages = match metadata.get("packages").and_then(|p| p.as_array()) {
        Some(p) => p,
        None => {
            debug!("No 'packages' in metadata");
            return None;
        }
    };

    // Find the package with matching name
    for pkg in packages {
        if let Some(name) = pkg.get("name").and_then(|n| n.as_str())
            && name == dep_name
            && let Some(version) = pkg.get("version").and_then(|v| v.as_str())
        {
            debug!("✓ Verified {} version: {}", dep_name, version);
            return Some(version.to_string());
        }
    }

    debug!("Could not find {} in dependency graph", dep_name);
    None
}

/// Extract the version requirement spec for a dependency using cargo metadata
/// Returns None if the dependency is not found
fn extract_dependency_spec(crate_path: &Path, dep_name: &str) -> Result<Option<String>, String> {
    debug!("Extracting spec for '{}' from {:?}", dep_name, crate_path);

    // Run cargo metadata to get dependency specs
    let output = Command::new("cargo")
        .args(["metadata", "--format-version=1"])
        .current_dir(crate_path)
        .output()
        .map_err(|e| format!("Failed to run cargo metadata: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("cargo metadata failed: {}", stderr.trim()));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed = metadata::parse_metadata(&stdout)?;

    // Get the root package (the dependent being tested)
    let root_package_id =
        if let Some(resolve) = &parsed.resolve { resolve.get("root").and_then(|r| r.as_str()) } else { None };

    if let Some(root_id) = root_package_id {
        // Use the metadata module to get the spec
        match metadata::get_version_spec(&parsed, root_id, dep_name) {
            Ok(spec) if spec != "?" => {
                debug!("  Extracted spec: {}", spec);
                return Ok(Some(spec));
            }
            Ok(_) => debug!("  Spec is '?', dependency not found in root package"),
            Err(e) => debug!("  Failed to get spec: {}", e),
        }
    }

    Ok(None)
}

/// Extract spec from Cargo.toml directly (fallback when cargo metadata fails)
/// Used for broken packages where fetch fails
fn extract_spec_from_toml(crate_path: &Path, dep_name: &str) -> Result<Option<String>, String> {
    use std::fs;
    use toml_edit::DocumentMut;

    debug!("Extracting spec from Cargo.toml for '{}' in {:?}", dep_name, crate_path);

    let toml_path = crate_path.join("Cargo.toml");
    let content = fs::read_to_string(&toml_path).map_err(|e| format!("Failed to read Cargo.toml: {}", e))?;

    let doc: DocumentMut = content.parse().map_err(|e| format!("Failed to parse Cargo.toml: {}", e))?;

    // Check [dependencies] section
    if let Some(deps) = doc.get("dependencies").and_then(|s| s.as_table_like())
        && let Some(dep_value) = deps.get(dep_name)
    {
        // Handle different formats:
        // 1. String: rgb = "0.8.27"
        if let Some(version_str) = dep_value.as_str() {
            return Ok(Some(version_str.to_string()));
        }

        // 2. Table: [dependencies.rgb] or inline table
        if let Some(table) = dep_value.as_table_like()
            && let Some(version_value) = table.get("version")
            && let Some(version_str) = version_value.as_str()
        {
            return Ok(Some(version_str.to_string()));
        }
    }

    // Not found
    Ok(None)
}

/// How to apply a dependency override
#[derive(Debug, Clone, Copy)]
enum DependencyOverrideMode {
    /// Replace dependency spec directly - bypasses semver requirements
    Force,
}

/// Apply a dependency override to Cargo.toml - Force mode only
fn apply_dependency_override(
    crate_path: &Path,
    dep_name: &str,
    override_path: &Path,
    mode: DependencyOverrideMode,
) -> Result<(), String> {
    use std::io::{Read, Write};

    // Convert to absolute path
    let override_path = if override_path.is_absolute() {
        override_path.to_path_buf()
    } else {
        env::current_dir().map_err(|e| format!("Failed to get current dir: {}", e))?.join(override_path)
    };

    let cargo_toml_path = crate_path.join("Cargo.toml");
    let mut content = String::new();

    // Read original Cargo.toml
    let mut file = fs::File::open(&cargo_toml_path).map_err(|e| format!("Failed to open Cargo.toml: {}", e))?;
    file.read_to_string(&mut content).map_err(|e| format!("Failed to read Cargo.toml: {}", e))?;
    drop(file);

    // Parse as TOML
    let mut doc: toml_edit::DocumentMut = content.parse().map_err(|e| format!("Failed to parse Cargo.toml: {}", e))?;

    match mode {
        DependencyOverrideMode::Force => {
            // Update dependency in all sections (force mode - replaces the spec entirely)
            let sections = vec!["dependencies", "dev-dependencies", "build-dependencies"];

            for section in sections {
                if let Some(deps) = doc.get_mut(section).and_then(|s| s.as_table_mut())
                    && let Some(dep) = deps.get_mut(dep_name)
                {
                    debug!("Force-replacing {} in [{}] with path {:?}", dep_name, section, override_path);

                    // Preserve existing fields (optional, default-features, features, etc.)
                    let mut new_dep = toml_edit::InlineTable::new();
                    new_dep.insert("path", override_path.display().to_string().into());

                    // Copy fields from original dependency if it's a table
                    if let Some(old_table) = dep.as_inline_table() {
                        // Preserve important fields
                        for key in ["optional", "default-features", "features", "package"] {
                            if let Some(value) = old_table.get(key) {
                                new_dep.insert(key, value.clone());
                                debug!("Preserving field '{}' = {:?}", key, value);
                            }
                        }
                    } else if let Some(old_table) = dep.as_table_like() {
                        // Handle table-like dependencies
                        for key in ["optional", "default-features", "features", "package"] {
                            if let Some(value) = old_table.get(key)
                                && let Some(v) = value.as_value()
                            {
                                new_dep.insert(key, v.clone());
                                debug!("Preserving field '{}' = {:?}", key, v);
                            }
                        }
                    }

                    *dep = toml_edit::Item::Value(toml_edit::Value::InlineTable(new_dep));
                }
            }

            debug!("Force-replaced {} dependency spec with path: {}", dep_name, override_path.display());
        }
    }

    // Write back
    let mut file = fs::File::create(&cargo_toml_path).map_err(|e| format!("Failed to create Cargo.toml: {}", e))?;
    file.write_all(doc.to_string().as_bytes()).map_err(|e| format!("Failed to write Cargo.toml: {}", e))?;

    Ok(())
}

/// Apply a [patch.crates-io] section to Cargo.toml to patch ALL transitive dependencies
///
/// This adds or updates the [patch.crates-io] section in the dependent's Cargo.toml,
/// which causes cargo to unify ALL versions of the specified crate across the entire
/// dependency tree (including transitive dependencies).
fn apply_patch_crates_io(crate_path: &Path, crate_name: &str, override_path: &Path) -> Result<(), String> {
    use std::io::{Read, Write};

    // Convert to absolute path
    let override_path = if override_path.is_absolute() {
        override_path.to_path_buf()
    } else {
        env::current_dir().map_err(|e| format!("Failed to get current dir: {}", e))?.join(override_path)
    };

    let cargo_toml_path = crate_path.join("Cargo.toml");
    let mut content = String::new();

    // Read original Cargo.toml
    let mut file = fs::File::open(&cargo_toml_path).map_err(|e| format!("Failed to open Cargo.toml: {}", e))?;
    file.read_to_string(&mut content).map_err(|e| format!("Failed to read Cargo.toml: {}", e))?;
    drop(file);

    // Parse as TOML
    let mut doc: toml_edit::DocumentMut = content.parse().map_err(|e| format!("Failed to parse Cargo.toml: {}", e))?;

    // Get or create [patch.crates-io] section
    if doc.get("patch").is_none() {
        doc["patch"] = toml_edit::Item::Table(toml_edit::Table::new());
    }
    let patch = doc["patch"].as_table_mut().ok_or("Failed to get patch table")?;

    if patch.get("crates-io").is_none() {
        patch["crates-io"] = toml_edit::Item::Table(toml_edit::Table::new());
    }
    let crates_io = patch["crates-io"].as_table_mut().ok_or("Failed to get crates-io table")?;

    // Add the patch entry
    let mut patch_entry = toml_edit::InlineTable::new();
    patch_entry.insert("path", override_path.display().to_string().into());
    crates_io[crate_name] = toml_edit::Item::Value(toml_edit::Value::InlineTable(patch_entry));

    debug!("Applied [patch.crates-io].{} = {{ path = \"{}\" }}", crate_name, override_path.display());

    // Write back
    let mut file = fs::File::create(&cargo_toml_path).map_err(|e| format!("Failed to create Cargo.toml: {}", e))?;
    file.write_all(doc.to_string().as_bytes()).map_err(|e| format!("Failed to write Cargo.toml: {}", e))?;

    Ok(())
}

pub fn compile_crate(
    crate_path: &Path,
    step: CompileStep,
    override_spec: Option<(&str, &Path)>,
) -> Result<CompileResult, String> {
    debug!("compiling {:?} with step {:?}", crate_path, step);

    // Run the cargo command with JSON output for better error extraction
    let start = Instant::now();
    let mut cmd = Command::new("cargo");
    cmd.arg(step.cargo_subcommand());

    // Add --message-format=json for check and test (not fetch)
    if step != CompileStep::Fetch {
        cmd.arg("--message-format=json");
    }

    // If override is provided, use --config flag instead of creating .cargo/config file
    if let Some((crate_name, override_path)) = override_spec {
        // Convert to absolute path if needed
        let override_path = if override_path.is_absolute() {
            override_path.to_path_buf()
        } else {
            env::current_dir().map_err(|e| format!("Failed to get current dir: {}", e))?.join(override_path)
        };

        let config_str = format!("patch.crates-io.{}.path=\"{}\"", crate_name, override_path.display());
        cmd.arg("--config").arg(&config_str);
        debug!("using --config: {}", config_str);
    }

    cmd.current_dir(crate_path);

    debug!("running cargo: {:?}", cmd);
    let output = cmd.output().map_err(|e| format!("Failed to execute cargo: {}", e))?;

    let duration = start.elapsed();
    let success = output.status.success();

    debug!("result: {:?}, duration: {:?}", success, duration);

    // Parse stdout for JSON messages (cargo writes JSON to stdout)
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();

    // Parse diagnostics from JSON output (only for check/test, not fetch)
    let diagnostics = if step != CompileStep::Fetch { parse_cargo_json(&stdout) } else { Vec::new() };

    debug!("parsed {} diagnostics", diagnostics.len());

    Ok(CompileResult { step, success, stdout, stderr, duration, diagnostics })
}

/// Source of a version being tested
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VersionSource {
    /// Published version from crates.io
    Published { version: String, forced: bool },
    /// Local work-in-progress version ("this")
    Local { path: PathBuf, forced: bool },
}

impl VersionSource {
    pub fn label(&self) -> String {
        match self {
            VersionSource::Published { version, .. } => version.clone(),
            VersionSource::Local { .. } => "this".to_string(),
        }
    }

    pub fn is_local(&self) -> bool {
        matches!(self, VersionSource::Local { .. })
    }

    pub fn is_forced(&self) -> bool {
        match self {
            VersionSource::Published { forced, .. } => *forced,
            VersionSource::Local { forced, .. } => *forced,
        }
    }

    pub fn version_string(&self) -> Option<String> {
        match self {
            VersionSource::Published { version, .. } => Some(version.clone()),
            VersionSource::Local { .. } => None,
        }
    }

    pub fn path(&self) -> Option<&PathBuf> {
        match self {
            VersionSource::Published { .. } => None,
            VersionSource::Local { path, .. } => Some(path),
        }
    }
}

/// Three-step ICT (Install/Check/Test) result for a single version
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ThreeStepResult {
    /// Install step (cargo fetch) - always runs
    pub fetch: CompileResult,
    /// Check step (cargo check) - only if fetch succeeds
    pub check: Option<CompileResult>,
    /// Test step (cargo test) - only if check succeeds
    pub test: Option<CompileResult>,
    /// Actual version resolved (from cargo tree), if verification succeeded
    pub actual_version: Option<String>,
    /// Expected version being tested
    pub expected_version: Option<String>,
    /// Whether this version was forced (bypassed semver requirements)
    pub forced_version: bool,
    /// Original requirement from dependent (e.g., "^0.8.52"), if known
    pub original_requirement: Option<String>,
    /// All versions of the tested crate found in the dependency tree (for multi-version scenarios)
    pub all_crate_versions: Vec<(String, String, String)>, // (spec, resolved_version, dependent_name)
}

impl ThreeStepResult {
    /// Determine if all executed steps succeeded
    pub fn is_success(&self) -> bool {
        if !self.fetch.success {
            return false;
        }
        if let Some(ref check) = self.check
            && !check.success
        {
            return false;
        }
        if let Some(ref test) = self.test
            && !test.success
        {
            return false;
        }
        true
    }

    /// Get the first failed step, if any
    pub fn first_failure(&self) -> Option<&CompileResult> {
        if !self.fetch.success {
            return Some(&self.fetch);
        }
        if let Some(ref check) = self.check
            && !check.success
        {
            return Some(check);
        }
        if let Some(ref test) = self.test
            && !test.success
        {
            return Some(test);
        }
        None
    }

    /// Format ICT marks for display (e.g., "✓✓✓", "✓✗-", "✗--")
    /// Shows cumulative failure: after first failure, show dashes
    pub fn format_ict_marks(&self) -> String {
        let fetch_mark = if self.fetch.success { "✓" } else { "✗" };

        if !self.fetch.success {
            return format!("{}--", fetch_mark);
        }

        let check_mark = match &self.check {
            Some(c) if c.success => "✓",
            Some(_) => "✗",
            None => "-",
        };

        if matches!(&self.check, Some(c) if !c.success) {
            return format!("{}{}-", fetch_mark, check_mark);
        }

        let test_mark = match &self.test {
            Some(t) if t.success => "✓",
            Some(_) => "✗",
            None => "-",
        };

        format!("{}{}{}", fetch_mark, check_mark, test_mark)
    }
}

/// Information about the dependent crate for logging
#[derive(Debug, Clone)]
pub struct DependentInfo<'a> {
    pub name: &'a str,
    pub version: &'a str,
}

/// Configuration for three-step ICT testing
#[derive(Debug, Clone)]
pub struct TestConfig<'a> {
    /// Path to the dependent crate being tested
    pub crate_path: &'a Path,
    /// Name of the base crate being overridden (e.g., "rgb")
    pub base_crate_name: &'a str,
    /// Optional path to override the dependency with
    pub override_path: Option<&'a Path>,
    /// Skip cargo check step
    pub skip_check: bool,
    /// Skip cargo test step
    pub skip_test: bool,
    /// Expected version to verify after fetch
    pub expected_version: Option<String>,
    /// Force version (bypass semver requirements)
    pub force_versions: bool,
    /// Original requirement from dependent's Cargo.toml
    pub original_requirement: Option<String>,
    /// Information about the dependent for logging
    pub dependent_info: Option<DependentInfo<'a>>,
    /// Test label for logging ("baseline", "WIP", or version)
    pub test_label: Option<&'a str>,
    /// Use [patch.crates-io] to patch all transitive dependencies
    pub patch_transitive: bool,
}

impl<'a> TestConfig<'a> {
    /// Create a new test configuration
    pub fn new(crate_path: &'a Path, base_crate_name: &'a str) -> Self {
        Self {
            crate_path,
            base_crate_name,
            override_path: None,
            skip_check: false,
            skip_test: false,
            expected_version: None,
            force_versions: false,
            original_requirement: None,
            dependent_info: None,
            test_label: None,
            patch_transitive: false,
        }
    }

    /// Set patch_transitive flag (builder pattern)
    pub fn with_patch_transitive(mut self, patch_transitive: bool) -> Self {
        self.patch_transitive = patch_transitive;
        self
    }

    /// Set the override path (builder pattern)
    pub fn with_override_path(mut self, path: &'a Path) -> Self {
        self.override_path = Some(path);
        self
    }

    /// Set skip flags (builder pattern)
    pub fn with_skip_flags(mut self, skip_check: bool, skip_test: bool) -> Self {
        self.skip_check = skip_check;
        self.skip_test = skip_test;
        self
    }

    /// Set version information (builder pattern)
    pub fn with_version_info(
        mut self,
        expected_version: Option<String>,
        force_versions: bool,
        original_requirement: Option<String>,
    ) -> Self {
        self.expected_version = expected_version;
        self.force_versions = force_versions;
        self.original_requirement = original_requirement;
        self
    }

    /// Set logging information (builder pattern)
    pub fn with_logging_info(mut self, dependent_info: Option<DependentInfo<'a>>, test_label: Option<&'a str>) -> Self {
        self.dependent_info = dependent_info;
        self.test_label = test_label;
        self
    }
}

/// Run three-step ICT (Install/Check/Test) test with early stopping
///
/// # Returns
/// ThreeStepResult with cumulative early stopping:
/// - Fetch always runs
/// - Check only runs if fetch succeeds (and !skip_check)
/// - Test only runs if check succeeds (and !skip_test)
pub fn run_three_step_ict(config: TestConfig) -> Result<ThreeStepResult, String> {
    let TestConfig {
        crate_path,
        base_crate_name,
        override_path,
        skip_check,
        skip_test,
        expected_version,
        force_versions,
        original_requirement,
        dependent_info,
        test_label,
        patch_transitive,
    } = config;
    debug!(
        "running three-step ICT for {:?} (force={}, expected_version={:?}, patch_transitive={})",
        crate_path, force_versions, expected_version, patch_transitive
    );

    // Always restore Cargo.toml from original backup to prevent contamination
    restore_cargo_toml(crate_path)?;

    // Always delete Cargo.lock to force fresh dependency resolution
    let lock_file = crate_path.join("Cargo.lock");
    if lock_file.exists() {
        debug!("Deleting Cargo.lock to force dependency resolution");
        fs::remove_file(&lock_file).map_err(|e| format!("Failed to remove Cargo.lock: {}", e))?;
    }

    // Setup: Choose patching strategy based on mode
    // For FORCE mode: Modify Cargo.toml to bypass semver (direct dependency)
    // For PATCH_TRANSITIVE mode: Add [patch.crates-io] section to patch ALL transitive deps
    // For PATCH mode: Use --config flag (only patches direct dep)
    //
    // IMPORTANT: When both force AND patch_transitive are enabled,
    // we apply BOTH: force for direct dep + [patch.crates-io] for transitive deps
    let override_path_buf = if let Some(override_path) = override_path {
        if force_versions {
            // FORCE MODE: Must modify Cargo.toml to bypass semver
            // No backup needed - restore_cargo_toml already has .original saved

            // Replace dependency spec directly (bypasses semver)
            apply_dependency_override(crate_path, base_crate_name, override_path, DependencyOverrideMode::Force)?;

            // If patch_transitive is also enabled, add [patch.crates-io] for transitive deps
            if patch_transitive {
                apply_patch_crates_io(crate_path, base_crate_name, override_path)?;
                debug!("Applied BOTH force override AND [patch.crates-io] for transitive patching");
            }

            None // Don't use --config when we modified Cargo.toml
        } else if patch_transitive {
            // PATCH_TRANSITIVE MODE (without force): Add [patch.crates-io] section to Cargo.toml
            // This unifies ALL versions of the crate across the entire dependency tree
            apply_patch_crates_io(crate_path, base_crate_name, override_path)?;

            debug!("Applied [patch.crates-io] for transitive patching");
            None // Don't use --config when we modified Cargo.toml
        } else {
            // PATCH MODE: Use --config flag (clean, no file modifications)
            let abs_path = if override_path.is_absolute() {
                override_path.to_path_buf()
            } else {
                env::current_dir().map_err(|e| format!("Failed to get current directory: {}", e))?.join(override_path)
            };

            debug!("Using --config for patch mode with override_path={:?}, abs_path={:?}", override_path, abs_path);
            Some(abs_path) // Use --config, no file modifications
        }
    } else {
        None // No override (baseline test)
    };

    // Build override_spec for compile_crate calls (only used in regular patch mode)
    let override_spec = override_path_buf.as_ref().map(|path| (base_crate_name, path.as_path()));

    // Step 1: Fetch (always runs)
    let fetch = compile_crate(crate_path, CompileStep::Fetch, override_spec)?;

    // Verify the actual version after fetch
    let actual_version = if fetch.success { verify_dependency_version(crate_path, base_crate_name) } else { None };

    // Extract original requirement spec from metadata if not provided
    let original_requirement = if original_requirement.is_none() {
        if fetch.success {
            // Fetch succeeded - extract from metadata
            let extracted = extract_dependency_spec(crate_path, base_crate_name).ok().flatten();
            debug!("Extracted spec (fetch succeeded): {:?} (force={})", extracted, force_versions);
            if extracted.is_none() && !force_versions {
                panic!(
                    "Failed to extract dependency spec for '{}' from {:?}. \
                    This should never happen if fetch succeeded in non-force mode. \
                    The dependency must exist in the manifest.",
                    base_crate_name, crate_path
                );
            }
            extracted
        } else {
            // Fetch failed - try to extract from Cargo.toml directly (fallback for broken dependents)
            let extracted = extract_spec_from_toml(crate_path, base_crate_name).ok().flatten();
            debug!("Extracted spec (fetch failed, from Cargo.toml): {:?}", extracted);
            extracted
        }
    } else {
        original_requirement.clone()
    };

    if fetch.failed() {
        // Log failure with diagnostics
        if let (Some(dep_info), Some(label)) = (dependent_info.as_ref(), test_label) {
            log_failure_with_diagnostics(
                dep_info.name,
                dep_info.version,
                base_crate_name,
                label,
                "cargo fetch",
                None,
                &fetch.stdout,
                &fetch.stderr,
                &fetch.diagnostics,
            );
        }

        // Fetch failed - stop here with dashes for remaining steps
        return Ok(ThreeStepResult {
            fetch,
            check: None,
            test: None,
            actual_version,
            expected_version,
            forced_version: force_versions,
            original_requirement,
            all_crate_versions: vec![],
        });
    }

    // Step 2: Check (only if fetch succeeded and not skipped)
    let check = if !skip_check {
        let result = compile_crate(crate_path, CompileStep::Check, override_spec)?;
        if result.failed() {
            // Log failure with diagnostics
            if let (Some(dep_info), Some(label)) = (dependent_info.as_ref(), test_label) {
                log_failure_with_diagnostics(
                    dep_info.name,
                    dep_info.version,
                    base_crate_name,
                    label,
                    "cargo check",
                    None,
                    &result.stdout,
                    &result.stderr,
                    &result.diagnostics,
                );
            }

            // Check failed - stop here with dash for test
            return Ok(ThreeStepResult {
                fetch,
                check: Some(result),
                test: None,
                actual_version: actual_version.clone(),
                expected_version: expected_version.clone(),
                forced_version: force_versions,
                original_requirement: original_requirement.clone(),
                all_crate_versions: vec![],
            });
        }
        Some(result)
    } else {
        None
    };

    // Step 3: Test (only if check succeeded or was skipped, and not skip_test)
    let test = if !skip_test {
        let should_run = match &check {
            Some(c) => c.success,
            None => true, // check was skipped, proceed
        };

        if should_run { Some(compile_crate(crate_path, CompileStep::Test, override_spec)?) } else { None }
    } else {
        None
    };

    // Log test failure if test failed
    if let Some(ref test_result) = test
        && test_result.failed()
        && let (Some(dep_info), Some(label)) = (dependent_info.as_ref(), test_label)
    {
        log_failure_with_diagnostics(
            dep_info.name,
            dep_info.version,
            base_crate_name,
            label,
            "cargo test",
            None,
            &test_result.stdout,
            &test_result.stderr,
            &test_result.diagnostics,
        );
    }

    // Cleanup: Always restore Cargo.toml to original state
    // This handles both FORCE mode (where we modified it) and ensures clean state
    restore_cargo_toml(crate_path).ok(); // Ignore errors on cleanup
    debug!("Restored Cargo.toml to original state");

    // Extract all versions of the base crate from the dependency tree (if fetch succeeded)
    let all_crate_versions =
        if fetch.success { extract_all_crate_versions(crate_path, base_crate_name) } else { vec![] };

    Ok(ThreeStepResult {
        fetch,
        check,
        test,
        actual_version,
        expected_version,
        forced_version: force_versions,
        original_requirement,
        all_crate_versions,
    })
}

/// Extract ALL versions of a crate from cargo metadata (for multi-version scenarios)
/// Returns Vec<(spec, resolved_version, dependent_name)>
fn extract_all_crate_versions(crate_dir: &Path, crate_name: &str) -> Vec<(String, String, String)> {
    let mut all_versions = Vec::new();

    debug!("extracting all versions of '{}' from cargo metadata", crate_name);

    // Run cargo metadata to get resolved dependencies
    let output = match Command::new("cargo").args(["metadata", "--format-version=1"]).current_dir(crate_dir).output() {
        Ok(o) => o,
        Err(e) => {
            debug!("failed to run cargo metadata: {}", e);
            return all_versions;
        }
    };

    if !output.status.success() {
        debug!("cargo metadata exited with error status");
        return all_versions;
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed = match metadata::parse_metadata(&stdout) {
        Ok(p) => p,
        Err(e) => {
            debug!("failed to parse cargo metadata JSON: {}", e);
            return all_versions;
        }
    };

    // Find all versions of the target crate using the metadata module
    let version_infos = metadata::find_all_versions(&parsed, crate_name);
    debug!("processing {} version entries from cargo metadata", version_infos.len());

    for (idx, version_info) in version_infos.iter().enumerate() {
        // Extract the dependent name from the node_id
        let dependent_name = if let Some((name, _ver)) = metadata::parse_node_id(&version_info.node_id) {
            name
        } else {
            version_info.node_id.clone()
        };

        debug!(
            "  [{}]: spec='{}' resolved='{}' dependent='{}'",
            idx, version_info.spec, version_info.version, dependent_name
        );

        all_versions.push((version_info.spec.clone(), version_info.version.clone(), dependent_name));
    }

    debug!("extracted {} total version entries for '{}'", all_versions.len(), crate_name);

    // Check for multiple different resolved versions (version mismatch scenario)
    let unique_versions: std::collections::HashSet<&String> =
        all_versions.iter().map(|(_, resolved, _)| resolved).collect();

    if unique_versions.len() > 1 {
        // Multiple versions detected - log them with metadata context
        warn!("⚠️  Multiple versions of '{}' detected in dependency tree:", crate_name);

        // Log the raw metadata JSON for debugging (just the resolve section to keep it manageable)
        if let Some(resolve) = &parsed.resolve {
            debug!("Metadata resolve section (for debugging multi-version scenario):");
            if let Ok(pretty_json) = serde_json::to_string_pretty(resolve) {
                // Log first MAX_METADATA_LOG_LINES to avoid overwhelming logs
                for (idx, line) in pretty_json.lines().enumerate() {
                    if idx >= MAX_METADATA_LOG_LINES {
                        debug!("  ... ({} more lines truncated)", pretty_json.lines().count() - MAX_METADATA_LOG_LINES);
                        break;
                    }
                    debug!("  {}", line);
                }
            }
        }
        for (spec, resolved, dependent) in &all_versions {
            warn!("  {} requires {} → resolved to {} (via {})", dependent, spec, resolved, crate_name);
        }

        // Log to failure log file if initialized
        if let Some(ref log_path) = *FAILURE_LOG.lock().unwrap()
            && let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open(log_path)
        {
            let _ = writeln!(file, "\n=== Multi-version detection for '{}' ===", crate_name);
            for (spec, resolved, dependent) in &all_versions {
                let _ = writeln!(file, "  {} requires {} → resolved to {}", dependent, spec, resolved);
            }
        }
    }

    all_versions
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compile_step_as_str() {
        assert_eq!(CompileStep::Check.as_str(), "check");
        assert_eq!(CompileStep::Test.as_str(), "test");
    }

    #[test]
    fn test_compile_step_cargo_subcommand() {
        assert_eq!(CompileStep::Check.cargo_subcommand(), "check");
        assert_eq!(CompileStep::Test.cargo_subcommand(), "test");
    }

    #[test]
    fn test_compile_result_failed() {
        let result = CompileResult {
            step: CompileStep::Check,
            success: false,
            stdout: String::new(),
            stderr: String::new(),
            duration: Duration::from_secs(1),
            diagnostics: Vec::new(),
        };
        assert!(result.failed());

        let result = CompileResult {
            step: CompileStep::Check,
            success: true,
            stdout: String::new(),
            stderr: String::new(),
            duration: Duration::from_secs(1),
            diagnostics: Vec::new(),
        };
        assert!(!result.failed());
    }

    #[test]
    fn test_apply_patch_crates_io() {
        use tempfile::TempDir;

        let temp_dir = TempDir::new().unwrap();
        let crate_path = temp_dir.path();

        // Create a basic Cargo.toml
        let cargo_toml = crate_path.join("Cargo.toml");
        fs::write(
            &cargo_toml,
            r#"[package]
name = "test-crate"
version = "0.1.0"

[dependencies]
rgb = "0.8.50"
"#,
        )
        .unwrap();

        // Apply the patch
        let override_path = PathBuf::from("/some/local/path");
        apply_patch_crates_io(crate_path, "rgb", &override_path).unwrap();

        // Verify the result
        let content = fs::read_to_string(&cargo_toml).unwrap();
        assert!(content.contains("[patch.crates-io]"), "Should have [patch.crates-io] section");
        assert!(content.contains("rgb"), "Should have rgb entry");
        assert!(content.contains("/some/local/path"), "Should have the override path");
    }

    #[test]
    fn test_apply_patch_crates_io_preserves_existing_content() {
        use tempfile::TempDir;

        let temp_dir = TempDir::new().unwrap();
        let crate_path = temp_dir.path();

        // Create a Cargo.toml with existing patch section
        let cargo_toml = crate_path.join("Cargo.toml");
        fs::write(
            &cargo_toml,
            r#"[package]
name = "test-crate"
version = "0.1.0"

[dependencies]
rgb = "0.8.50"
serde = "1.0"

[patch.crates-io]
other-crate = { path = "/other/path" }
"#,
        )
        .unwrap();

        // Apply the patch
        let override_path = PathBuf::from("/rgb/path");
        apply_patch_crates_io(crate_path, "rgb", &override_path).unwrap();

        // Verify the result
        let content = fs::read_to_string(&cargo_toml).unwrap();
        assert!(content.contains("other-crate"), "Should preserve existing patches");
        assert!(content.contains("/other/path"), "Should preserve existing patch path");
        assert!(content.contains("/rgb/path"), "Should have new rgb path");
    }
}
