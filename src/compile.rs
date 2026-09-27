use crate::error_extract::{Diagnostic, extract_crates_needing_patch, has_multiple_version_conflict, parse_cargo_json};
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

/// Restore an immutable registry-cache manifest from its original backup.
/// Never use this on a local checkout: its backup may predate user edits.
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
fn verify_dependency_version(
    crate_path: &Path,
    dep_name: &str,
    override_spec: Option<(&str, &Path)>,
) -> Option<String> {
    debug!("Verifying {} version in {:?}", dep_name, crate_path);

    // Try using cargo metadata which works better with path dependencies
    // Don't use --no-deps because we need to see resolved dependencies
    let output = cargo_metadata(crate_path, override_spec).ok()?;

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
        let root = resolve.get("root").and_then(|v| v.as_str())?;
        for node in nodes.iter().filter(|n| n.get("id").and_then(|v| v.as_str()) == Some(root)) {
            if let Some(deps) = node.get("deps").and_then(|d| d.as_array()) {
                for dep in deps {
                    if let Some(pkg) = dep.get("pkg").and_then(|p| p.as_str())
                        && let Some(package) =
                            metadata.get("packages").and_then(|p| p.as_array()).and_then(|packages| {
                                packages.iter().find(|p| p.get("id").and_then(|v| v.as_str()) == Some(pkg))
                            })
                        && package.get("name").and_then(|v| v.as_str()) == Some(dep_name)
                    {
                        return package.get("version").and_then(|v| v.as_str()).map(str::to_owned);
                    }
                }
            }
        }
    }

    debug!("Could not find {} in dependency graph", dep_name);
    None
}

/// Extract the version requirement spec for a dependency using cargo metadata
/// Returns None if the dependency is not found
fn extract_dependency_spec(
    crate_path: &Path,
    dep_name: &str,
    override_spec: Option<(&str, &Path)>,
) -> Result<Option<String>, String> {
    debug!("Extracting spec for '{}' from {:?}", dep_name, crate_path);

    // Run cargo metadata to get dependency specs
    let output = cargo_metadata(crate_path, override_spec)?;

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

    let workspace_manifest = workspace_manifest(crate_path)?;
    let workspace_content = fs::read_to_string(workspace_manifest).map_err(|e| e.to_string())?;
    let workspace_doc: toml_edit::DocumentMut = workspace_content.parse().map_err(|e| format!("{e}"))?;
    let workspace_deps = workspace_doc.get("workspace").and_then(|w| w.get("dependencies"));

    match mode {
        DependencyOverrideMode::Force => {
            override_dependency_sections(doc.as_table_mut(), workspace_deps, dep_name, &override_path)?;
            if let Some(targets) = doc.get_mut("target").and_then(|t| t.as_table_mut()) {
                for (_, target) in targets.iter_mut() {
                    if let Some(table) = target.as_table_mut() {
                        override_dependency_sections(table, workspace_deps, dep_name, &override_path)?;
                    }
                }
            }
        }
    }

    // Write back
    let mut file = fs::File::create(&cargo_toml_path).map_err(|e| format!("Failed to create Cargo.toml: {}", e))?;
    file.write_all(doc.to_string().as_bytes()).map_err(|e| format!("Failed to write Cargo.toml: {}", e))?;

    Ok(())
}

/// Materialize inherited settings before replacing a dependency's source. Features
/// are additive; default-features comes from the workspace unless enabled locally.
fn override_dependency_sections(
    table: &mut toml_edit::Table,
    workspace_deps: Option<&toml_edit::Item>,
    dep_name: &str,
    override_path: &Path,
) -> Result<(), String> {
    for section in ["dependencies", "dev-dependencies", "build-dependencies"] {
        let Some(deps) = table.get_mut(section).and_then(|d| d.as_table_mut()) else { continue };
        for (key, dep) in deps.iter_mut() {
            let inherited = dep.get("workspace").and_then(|v| v.as_bool()) == Some(true);
            let workspace_dep = if inherited {
                Some(
                    workspace_deps
                        .and_then(|deps| deps.get(key.get()))
                        .ok_or_else(|| format!("Missing workspace dependency `{key}`"))?,
                )
            } else {
                None
            };
            let package = dep
                .get("package")
                .or_else(|| workspace_dep.and_then(|d| d.get("package")))
                .and_then(|v| v.as_str())
                .unwrap_or(key.get());
            if package != dep_name {
                continue;
            }

            let mut replacement = toml_edit::InlineTable::new();
            replacement.insert("path", override_path.display().to_string().into());
            for source in workspace_dep.into_iter().chain(std::iter::once(&*dep)) {
                for field in ["optional", "default-features", "package"] {
                    if let Some(value) = source.get(field).and_then(|v| v.as_value()) {
                        replacement.insert(field, value.clone());
                    }
                }
            }
            if let Some(workspace_dep) = workspace_dep {
                let defaults = workspace_dep.get("default-features").and_then(|v| v.as_bool()).unwrap_or(true)
                    || dep.get("default-features").and_then(|v| v.as_bool()).unwrap_or(false);
                replacement.insert("default-features", defaults.into());
            }
            let mut features = toml_edit::Array::new();
            for source in workspace_dep.into_iter().chain(std::iter::once(&*dep)) {
                if let Some(values) = source.get("features").and_then(|v| v.as_array()) {
                    for feature in values.iter() {
                        if !features.iter().any(|v| v.as_str() == feature.as_str()) {
                            features.push(feature.clone());
                        }
                    }
                }
            }
            if !features.is_empty() {
                replacement.insert("features", features.into());
            }
            *dep = toml_edit::value(replacement);
        }
    }
    Ok(())
}

/// Ask Cargo for the actual root, including explicit package.workspace paths and
/// standalone packages. This does not resolve or fetch dependencies.
fn workspace_manifest(crate_path: &Path) -> Result<PathBuf, String> {
    let output = Command::new("cargo")
        .args(["locate-project", "--workspace", "--message-format=plain"])
        .current_dir(crate_path)
        .output()
        .map_err(|e| format!("Failed to locate workspace: {e}"))?;
    if !output.status.success() {
        return Err(format!("Failed to locate workspace: {}", String::from_utf8_lossy(&output.stderr)));
    }
    Ok(PathBuf::from(String::from_utf8_lossy(&output.stdout).trim()))
}

struct FileSnapshot {
    path: PathBuf,
    contents: Option<Vec<u8>>,
}

impl FileSnapshot {
    fn capture(path: PathBuf) -> Result<Self, String> {
        let contents = match fs::read(&path) {
            Ok(bytes) => Some(bytes),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(e) => return Err(format!("Failed to snapshot {}: {e}", path.display())),
        };
        Ok(Self { path, contents })
    }

    fn restore(&self) -> Result<(), String> {
        let result = match &self.contents {
            Some(bytes) => fs::write(&self.path, bytes),
            None => match fs::remove_file(&self.path) {
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
                result => result,
            },
        };
        result.map_err(|e| format!("Failed to restore {}: {e}", self.path.display()))
    }
}

/// Scope all temporary manifest and lockfile edits to one baseline/offered run.
/// Normal errors propagate restoration failures; Drop also covers unwinding.
struct TestFiles {
    manifest: FileSnapshot,
    lock: FileSnapshot,
    restored: bool,
}

impl TestFiles {
    fn capture(crate_path: &Path) -> Result<Self, String> {
        let root = workspace_manifest(crate_path)?;
        Ok(Self {
            manifest: FileSnapshot::capture(crate_path.join("Cargo.toml"))?,
            lock: FileSnapshot::capture(root.with_file_name("Cargo.lock"))?,
            restored: false,
        })
    }

    fn reset_for_retry(&self) -> Result<(), String> {
        self.manifest.restore()?;
        match fs::remove_file(&self.lock.path) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            result => result.map_err(|e| format!("Failed to clear temporary lockfile: {e}")),
        }
    }

    fn restore(&mut self) -> Result<(), String> {
        // Attempt both restorations even if one fails.
        let manifest = self.manifest.restore();
        let lock = self.lock.restore();
        manifest.and(lock)?;
        self.restored = true;
        Ok(())
    }
}

impl Drop for TestFiles {
    fn drop(&mut self) {
        if !self.restored
            && let Err(e) = self.restore()
        {
            eprintln!("{e}");
        }
    }
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

/// Recursively discover the local path-dependency siblings of a crate — the
/// workspace crates it (transitively) path-depends on. Returns `(crate_name,
/// absolute_dir)` pairs, never including `base_crate_dir` itself.
///
/// When the base crate under test path-depends on a sibling (e.g. `magetypes`
/// path-depends on `archmage`), patching only the base leaves the sibling split
/// between the dependent's crates.io copy and the base's workspace copy →
/// `error[E0308]: ... multiple versions of crate archmage`. Patching every
/// sibling too unifies the whole local workspace for the dependent.
fn discover_path_dep_siblings(base_crate_dir: &Path) -> Vec<(String, std::path::PathBuf)> {
    let mut out = Vec::new();
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut stack = vec![base_crate_dir.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(content) = fs::read_to_string(dir.join("Cargo.toml")) else {
            continue;
        };
        let Ok(doc) = content.parse::<toml_edit::DocumentMut>() else {
            continue;
        };
        for table_name in ["dependencies", "dev-dependencies", "build-dependencies"] {
            let Some(deps) = doc.get(table_name).and_then(|d| d.as_table()) else {
                continue;
            };
            for (key, item) in deps.iter() {
                // Only path-dependencies are local workspace siblings.
                let Some(path_str) = item.get("path").and_then(|p| p.as_str()) else {
                    continue;
                };
                // Honor `package = "..."` renames; otherwise the dep name is the key.
                let name = item.get("package").and_then(|p| p.as_str()).unwrap_or(key).to_string();
                let abs = dir.join(path_str);
                let abs = abs.canonicalize().unwrap_or(abs);
                if seen.insert(name.clone()) {
                    out.push((name, abs.clone()));
                    stack.push(abs); // follow the sibling's own path-deps
                }
            }
        }
    }
    out
}

fn configure_override(cmd: &mut Command, override_spec: Option<(&str, &Path)>) -> Result<(), String> {
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

        // Also patch the base crate's local workspace siblings, so a dependent
        // that ALSO depends on one of them (e.g. magetypes path-depends on
        // archmage, and the dependent depends on both) resolves a single unified
        // copy instead of "multiple versions of crate X" (E0308). Unused patches
        // are harmless (cargo just warns), so this is safe to apply unconditionally.
        for (sib_name, sib_path) in discover_path_dep_siblings(&override_path) {
            if sib_name == crate_name {
                continue;
            }
            let sib_config = format!("patch.crates-io.{}.path=\"{}\"", sib_name, sib_path.display());
            cmd.arg("--config").arg(&sib_config);
            debug!("using --config (sibling): {}", sib_config);
        }
    }

    Ok(())
}

fn cargo_metadata(crate_path: &Path, override_spec: Option<(&str, &Path)>) -> Result<std::process::Output, String> {
    let mut cmd = Command::new("cargo");
    cmd.args(["metadata", "--format-version=1"]).current_dir(crate_path);
    configure_override(&mut cmd, override_spec)?;
    cmd.output().map_err(|e| format!("Failed to execute cargo metadata: {e}"))
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

    configure_override(&mut cmd, override_spec)?;

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

/// Depth of patching applied to resolve version conflicts
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum PatchDepth {
    /// No patching - natural resolution or simple force mode
    #[default]
    None,
    /// Force mode only - direct dependency spec replaced (!)
    Force,
    /// Patch retry - [patch.crates-io] added after multi-version error (!!)
    Patch,
    /// Deep patch - recursive transitive patching after Patch still failed (!!!)
    DeepPatch,
}

impl PatchDepth {
    /// Get marker suffix for display
    pub fn marker(&self) -> &'static str {
        match self {
            PatchDepth::None => "",
            PatchDepth::Force => "!",
            PatchDepth::Patch => "!!",
            PatchDepth::DeepPatch => "!!!",
        }
    }

    /// Check if any patching was applied
    pub fn is_patched(&self) -> bool {
        !matches!(self, PatchDepth::None)
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
    /// Depth of patching applied to resolve version conflicts
    pub patch_depth: PatchDepth,
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

    /// Validate internal consistency of the result (debug builds only).
    ///
    /// Panics if fetch failed but check/test is Some, or check failed but test is Some.
    #[inline]
    pub fn debug_assert_consistent(&self) {
        debug_assert!(
            self.fetch.success || (self.check.is_none() && self.test.is_none()),
            "Invariant violated: fetch failed but check/test is Some. \
             fetch.success={}, check.is_some={}, test.is_some={}",
            self.fetch.success,
            self.check.is_some(),
            self.test.is_some()
        );
        if let Some(ref check) = self.check {
            debug_assert!(
                check.success || self.test.is_none(),
                "Invariant violated: check failed but test is Some. \
                 check.success={}, test.is_some={}",
                check.success,
                self.test.is_some()
            );
        }
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
    let mut files = TestFiles::capture(config.crate_path)?;
    let result = run_three_step_ict_inner(config, &files);
    match (result, files.restore()) {
        (result, Ok(())) => result,
        (Ok(_), Err(cleanup)) => Err(cleanup),
        (Err(error), Err(cleanup)) => Err(format!("{error}; {cleanup}")),
    }
}

fn run_three_step_ict_inner(config: TestConfig, files: &TestFiles) -> Result<ThreeStepResult, String> {
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
        "running three-step ICT for {:?} (force={}, expected_version={:?}, patch_transitive={}, has_override_path={})",
        crate_path,
        force_versions,
        expected_version,
        patch_transitive,
        override_path.is_some()
    );

    // Sanity check: baseline should NOT have an override_path
    if override_path.is_some() && !force_versions {
        debug!("PATCH MODE: will use --config for patching (override_path={:?})", override_path);
    } else if override_path.is_none() {
        debug!("BASELINE MODE: no override, testing natural resolution");
    }

    // Start from the current manifest, never a stale backup from an earlier run.
    // Fresh resolution uses the workspace lockfile, restored on every exit.
    files.reset_for_retry()?;

    // Setup: Choose patching strategy based on mode
    // For FORCE mode: Modify Cargo.toml to bypass semver (direct dependency)
    //   - If patch_transitive is also enabled, add [patch.crates-io] for transitive deps
    // For PATCH mode (non-force): Use --config flag (clean, no file modifications)
    // For BASELINE: No override at all
    //
    // IMPORTANT: patch_transitive ONLY applies with force mode, because:
    // 1. Baseline should never be modified
    // 2. Non-forced versions use --config which doesn't modify files
    let override_path_buf = if let Some(override_path) = override_path {
        if force_versions {
            // FORCE MODE: bypass semver on the dependent's DIRECT dep by
            // rewriting its manifest spec to the WIP path.
            apply_dependency_override(crate_path, base_crate_name, override_path, DependencyOverrideMode::Force)?;

            // The direct override does NOT reach copies of the base crate (or its
            // workspace siblings) that the dependent pulls TRANSITIVELY via other
            // deps — those resolve to crates.io and collide with the WIP ("multiple
            // versions of crate X", E0308). A manifest [patch.crates-io] can't fix
            // that either: cargo only honors [patch] in the WORKSPACE ROOT, so for
            // a dependent that is itself a workspace member it lands in the wrong
            // manifest and is silently ignored. Route base + siblings through
            // `--config patch.crates-io` instead (returning Some hands them to
            // compile_crate's --config path below), which cargo applies at the
            // build-root level regardless of workspace layout. The WIP version
            // satisfies the transitive requirement, so the patch applies cleanly.
            let _ = patch_transitive; // subsumed: --config is always transitive
            let abs_path = if override_path.is_absolute() {
                override_path.to_path_buf()
            } else {
                env::current_dir().map_err(|e| format!("Failed to get current directory: {}", e))?.join(override_path)
            };
            Some(abs_path)
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
    let actual_version =
        if fetch.success { verify_dependency_version(crate_path, base_crate_name, override_spec) } else { None };

    // Extract original requirement spec from metadata if not provided
    let original_requirement = if original_requirement.is_none() {
        if fetch.success {
            // Fetch succeeded - extract from metadata
            let extracted = extract_dependency_spec(crate_path, base_crate_name, override_spec).ok().flatten();
            debug!("Extracted spec (fetch succeeded): {:?} (force={})", extracted, force_versions);
            if extracted.is_none() && !force_versions {
                // The fetched dependent declares no dependency on the base crate —
                // e.g. a historical version released before that dependency was
                // added. There is nothing to patch or test, so signal a skip
                // rather than panicking and aborting the whole run.
                return Err(format!("`{base_crate_name}` is not a dependency of this version — skipping"));
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
            patch_depth: if force_versions { PatchDepth::Force } else { PatchDepth::None },
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

            // Check failed - try auto-retry with [patch.crates-io] if it's a multi-version conflict
            let combined_output = format!("{}\n{}", result.stdout, result.stderr);
            if force_versions
                && (has_multiple_version_conflict(&combined_output)
                    || has_multiple_resolved_versions(crate_path, base_crate_name, override_spec))
            {
                debug!("Multi-version conflict detected, attempting auto-retry with [patch.crates-io]");

                // Restore Cargo.toml and apply both force AND patch.crates-io
                files.reset_for_retry()?;

                // Apply force override
                if let Some(override_path) = override_path {
                    apply_dependency_override(
                        crate_path,
                        base_crate_name,
                        override_path,
                        DependencyOverrideMode::Force,
                    )?;
                    // Also apply [patch.crates-io] for transitive deps
                    apply_patch_crates_io(crate_path, base_crate_name, override_path)?;
                    debug!("Applied FORCE + [patch.crates-io] for auto-retry");
                }

                // Retry fetch and check
                let retry_fetch = compile_crate(crate_path, CompileStep::Fetch, override_spec)?;
                if retry_fetch.success {
                    let retry_check = compile_crate(crate_path, CompileStep::Check, override_spec)?;
                    if retry_check.success {
                        // Auto-retry succeeded! Continue with test step
                        debug!("Auto-retry with [patch.crates-io] succeeded!");

                        // Run test if not skipped
                        let test = if !skip_test {
                            Some(compile_crate(crate_path, CompileStep::Test, override_spec)?)
                        } else {
                            None
                        };

                        // Log test failure if needed
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

                        // Collect metadata before the outer wrapper restores the files.
                        let all_crate_versions = extract_all_crate_versions(crate_path, base_crate_name, override_spec);

                        return Ok(ThreeStepResult {
                            fetch: retry_fetch,
                            check: Some(retry_check),
                            test,
                            actual_version: verify_dependency_version(crate_path, base_crate_name, override_spec),
                            expected_version: expected_version.clone(),
                            forced_version: true,
                            original_requirement: original_requirement.clone(),
                            all_crate_versions,
                            patch_depth: PatchDepth::Patch, // !! marker
                        });
                    }
                    // Retry check also failed - check if still multi-version conflict
                    let retry_output = format!("{}\n{}", retry_check.stdout, retry_check.stderr);
                    let still_multi_version = has_multiple_version_conflict(&retry_output);

                    // Extract blocking crates for !!! case
                    let blocking_crates = if still_multi_version {
                        let crates = extract_crates_needing_patch(&retry_output, base_crate_name);
                        debug!("Auto-retry still has multi-version conflict - blocking crates: {:?}", crates);
                        // Convert to all_crate_versions format: (spec, version, crate_name)
                        crates.into_iter().map(|c| ("blocking".to_string(), "?".to_string(), c)).collect()
                    } else {
                        debug!("Auto-retry check failed with different error");
                        vec![]
                    };

                    return Ok(ThreeStepResult {
                        fetch: retry_fetch,
                        check: Some(retry_check),
                        test: None,
                        actual_version: actual_version.clone(),
                        expected_version: expected_version.clone(),
                        forced_version: true,
                        original_requirement: original_requirement.clone(),
                        all_crate_versions: blocking_crates,
                        // !!! if still multi-version (deep transitive issue), !! otherwise
                        patch_depth: if still_multi_version { PatchDepth::DeepPatch } else { PatchDepth::Patch },
                    });
                }
                // Retry fetch failed - return original failure
                debug!("Auto-retry fetch failed");
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
                patch_depth: if force_versions { PatchDepth::Force } else { PatchDepth::None },
            });
        }
        Some(result)
    } else {
        None
    };

    // Step 3: Test (only if check succeeded or was skipped, and not skip_test)
    // If test fails with force_versions, check for multi-version conflicts in the dep tree
    // and retry with [patch.crates-io] (mirrors the check-step auto-retry logic).
    // This catches cases where dev-dependencies (only compiled during tests) bring in
    // a second version of the base crate, causing trait mismatches that the compiler
    // doesn't always annotate with "multiple different versions of crate".
    let (test, _test_patch_depth): (Option<CompileResult>, Option<PatchDepth>) = if !skip_test {
        let should_run = match &check {
            Some(c) => c.success,
            None => true, // check was skipped, proceed
        };

        if should_run {
            let result = compile_crate(crate_path, CompileStep::Test, override_spec)?;
            if result.failed() && force_versions {
                // Check if there are multiple resolved versions in the dep tree
                let multi_version_in_tree = has_multiple_resolved_versions(crate_path, base_crate_name, override_spec);
                let combined_output = format!("{}\n{}", result.stdout, result.stderr);
                let multi_version_in_output = has_multiple_version_conflict(&combined_output);

                if multi_version_in_tree || multi_version_in_output {
                    debug!(
                        "Test failed with multi-version conflict (tree={}, output={}), attempting auto-retry with [patch.crates-io]",
                        multi_version_in_tree, multi_version_in_output
                    );

                    // Restore Cargo.toml and apply both force AND patch.crates-io
                    files.reset_for_retry()?;

                    if let Some(op) = override_path {
                        apply_dependency_override(crate_path, base_crate_name, op, DependencyOverrideMode::Force)?;
                        apply_patch_crates_io(crate_path, base_crate_name, op)?;
                        debug!("Applied FORCE + [patch.crates-io] for test auto-retry");
                    }

                    // Retry fetch + check + test
                    let retry_fetch = compile_crate(crate_path, CompileStep::Fetch, override_spec)?;
                    if retry_fetch.success {
                        let retry_check = compile_crate(crate_path, CompileStep::Check, override_spec)?;
                        if retry_check.success {
                            let retry_test = compile_crate(crate_path, CompileStep::Test, override_spec)?;

                            if let (Some(dep_info), Some(label)) = (dependent_info.as_ref(), test_label)
                                && retry_test.failed()
                            {
                                log_failure_with_diagnostics(
                                    dep_info.name,
                                    dep_info.version,
                                    base_crate_name,
                                    label,
                                    "cargo test",
                                    None,
                                    &retry_test.stdout,
                                    &retry_test.stderr,
                                    &retry_test.diagnostics,
                                );
                            }

                            let all_crate_versions =
                                extract_all_crate_versions(crate_path, base_crate_name, override_spec);

                            return Ok(ThreeStepResult {
                                fetch: retry_fetch,
                                check: Some(retry_check),
                                test: Some(retry_test),
                                actual_version: verify_dependency_version(crate_path, base_crate_name, override_spec),
                                expected_version: expected_version.clone(),
                                forced_version: true,
                                original_requirement: original_requirement.clone(),
                                all_crate_versions,
                                patch_depth: PatchDepth::Patch, // !! marker
                            });
                        }
                    }

                    // Retry failed; the outer wrapper restores files after collecting the result.
                    debug!("Test auto-retry with [patch.crates-io] failed");
                }

                // Log original failure
                if let (Some(dep_info), Some(label)) = (dependent_info.as_ref(), test_label) {
                    log_failure_with_diagnostics(
                        dep_info.name,
                        dep_info.version,
                        base_crate_name,
                        label,
                        "cargo test",
                        None,
                        &result.stdout,
                        &result.stderr,
                        &result.diagnostics,
                    );
                }
                (Some(result), None)
            } else {
                if result.failed()
                    && let (Some(dep_info), Some(label)) = (dependent_info.as_ref(), test_label)
                {
                    log_failure_with_diagnostics(
                        dep_info.name,
                        dep_info.version,
                        base_crate_name,
                        label,
                        "cargo test",
                        None,
                        &result.stdout,
                        &result.stderr,
                        &result.diagnostics,
                    );
                }
                (Some(result), None)
            }
        } else {
            (None, None)
        }
    } else {
        (None, None)
    };

    // Extract all versions of the base crate from the dependency tree (if fetch succeeded)
    let all_crate_versions =
        if fetch.success { extract_all_crate_versions(crate_path, base_crate_name, override_spec) } else { vec![] };

    // Determine patch depth based on mode
    let patch_depth = if force_versions && patch_transitive {
        PatchDepth::Patch // Force + explicit patch_transitive = !!
    } else if force_versions {
        PatchDepth::Force // Force only = !
    } else {
        PatchDepth::None // Natural resolution
    };

    Ok(ThreeStepResult {
        fetch,
        check,
        test,
        actual_version,
        expected_version,
        forced_version: force_versions,
        original_requirement,
        all_crate_versions,
        patch_depth,
    })
}

/// Check if the dependency tree has multiple distinct resolved versions of a crate.
/// This detects multi-version conflicts even when the compiler error message
/// doesn't explicitly mention "multiple different versions of crate".
fn has_multiple_resolved_versions(crate_dir: &Path, crate_name: &str, override_spec: Option<(&str, &Path)>) -> bool {
    let all_versions = extract_all_crate_versions(crate_dir, crate_name, override_spec);
    let unique_versions: std::collections::HashSet<&String> =
        all_versions.iter().map(|(_, resolved, _)| resolved).collect();
    let result = unique_versions.len() > 1;
    if result {
        debug!(
            "Detected {} distinct resolved versions of '{}': {:?}",
            unique_versions.len(),
            crate_name,
            unique_versions
        );
    }
    result
}

/// Extract ALL versions of a crate from cargo metadata (for multi-version scenarios)
/// Returns Vec<(spec, resolved_version, dependent_name)>
fn extract_all_crate_versions(
    crate_dir: &Path,
    crate_name: &str,
    override_spec: Option<(&str, &Path)>,
) -> Vec<(String, String, String)> {
    let mut all_versions = Vec::new();

    debug!("extracting all versions of '{}' from cargo metadata", crate_name);

    // Run cargo metadata to get resolved dependencies
    let output = match cargo_metadata(crate_dir, override_spec) {
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

#[cfg(test)]
#[path = "compile_test.rs"]
mod regression_tests;
