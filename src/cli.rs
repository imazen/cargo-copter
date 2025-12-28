use clap::Parser;
use std::path::PathBuf;

/// Get the default cache directory for cargo-copter
/// Uses platform-specific cache directories:
/// - Linux: ~/.cache/cargo-copter
/// - macOS: ~/Library/Caches/cargo-copter
/// - Windows: %LOCALAPPDATA%/cargo-copter
pub fn default_cache_dir() -> PathBuf {
    dirs::cache_dir().map(|p| p.join("cargo-copter")).unwrap_or_else(|| PathBuf::from(".copter"))
}

#[derive(Parser, Debug, Clone)]
#[command(name = "cargo-copter")]
#[command(about = "Test the downstream impact of crate changes before publishing")]
#[command(version)]
pub struct CliArgs {
    /// Path to the crate to test (directory or Cargo.toml file)
    #[arg(long, short = 'p', value_name = "PATH")]
    pub path: Option<PathBuf>,

    /// Name of the crate to test (for testing published crates without local source)
    #[arg(long = "crate", visible_alias = "crate-name", short = 'c', value_name = "CRATE")]
    pub crate_name: Option<String>,

    /// Test top N reverse dependencies by download count
    #[arg(long, default_value = "5")]
    pub top_dependents: usize,

    /// Explicitly test these crates from crates.io (supports "name:version" syntax)
    /// Examples: "image", "image:0.25.8"
    /// Can specify multiple: --dependents image serde tokio
    #[arg(long, value_name = "CRATE[:VERSION]", num_args = 1.., value_delimiter = ' ')]
    pub dependents: Vec<String>,

    /// Test local crates at these paths
    /// Can specify multiple: --dependent-paths ./crate1 ./crate2
    #[arg(long, value_name = "PATH", num_args = 1..)]
    pub dependent_paths: Vec<PathBuf>,

    /// Test against specific versions of the base crate (e.g., "0.3.0 4.1.1")
    /// When specified with --path, includes "this" (WIP version) automatically
    /// Supports versions with hyphens: "0.8.0 1.0.0-rc.1 1.0.0-alpha.2"
    #[arg(long, value_name = "VERSION", num_args = 1..)]
    pub test_versions: Vec<String>,

    /// HTML report output path
    #[arg(long = "output-html", default_value = "copter-report.html")]
    pub output: PathBuf,

    /// Directory for staging unpacked crates (enables caching across runs)
    /// Default: ~/.cache/cargo-copter/staging (Linux), ~/Library/Caches/cargo-copter/staging (macOS)
    #[arg(long)]
    pub staging_dir: Option<PathBuf>,

    /// Only fetch dependencies (skip check and test)
    #[arg(long)]
    pub only_fetch: bool,

    /// Only fetch and check (skip tests)
    #[arg(long)]
    pub only_check: bool,

    /// Output results as JSON
    #[arg(long)]
    pub json: bool,

    /// Force testing specific versions, bypassing semver requirements
    /// Accepts multiple versions like --test-versions (e.g., "0.7.0 1.0.0-rc.1")
    /// These versions are tested even if they don't satisfy dependent's requirements
    #[arg(long, value_name = "VERSION", num_args = 0..)]
    pub force_versions: Vec<String>,

    /// Clean staging directory before running tests (purges all cached builds)
    #[arg(long)]
    pub clean: bool,

    /// Number of error lines to show for compilation failures (default: 10)
    #[arg(long, default_value = "10")]
    pub error_lines: usize,

    /// Skip auto-inserting normal (non-forced) tests for force-versions
    /// By default, each forced version is also tested in normal patch mode
    #[arg(long)]
    pub skip_normal_testing: bool,

    /// Override console width for testing (default: auto-detect)
    #[arg(long, value_name = "COLUMNS")]
    pub console_width: Option<usize>,

    /// Run inside a Docker container for security isolation (Linux only)
    /// This protects your system from potentially malicious code in dependencies
    #[arg(long)]
    pub docker: bool,

    /// [DEPRECATED] Patch transitive dependencies when using --force-versions
    ///
    /// DEPRECATED: Auto-retry now handles this automatically. When --force-versions
    /// encounters a "multiple versions of crate" error, it automatically retries
    /// with [patch.crates-io] applied. See the `!!` marker in output.
    ///
    /// This flag is kept for backwards compatibility but is no longer needed.
    #[arg(long, requires = "force_versions", hide = true)]
    pub patch_transitive: bool,

    /// Use simple, verbal output format instead of table
    /// Better for AI parsing and large dependency counts.
    /// Shows clear PASS/FAIL/REGRESSION status for each test.
    #[arg(long)]
    pub simple: bool,
}

impl CliArgs {
    /// Parse command-line arguments
    pub fn parse_args() -> Self {
        let mut args = CliArgs::parse();

        // Split test_versions on whitespace to support quoted lists like '0.8.51 0.8.91-alpha.3'
        args.test_versions =
            args.test_versions.iter().flat_map(|s| s.split_whitespace().map(|v| v.to_string())).collect();

        // Split force_versions on whitespace as well
        args.force_versions =
            args.force_versions.iter().flat_map(|s| s.split_whitespace().map(|v| v.to_string())).collect();

        args
    }

    /// Validate argument combinations
    pub fn validate(&self) -> Result<(), String> {
        // Can't specify both --only-fetch and --only-check
        if self.only_fetch && self.only_check {
            return Err("Cannot specify both --only-fetch and --only-check".to_string());
        }

        // Need at least one of: top_dependents, dependents, or dependent_paths
        if self.top_dependents == 0 && self.dependents.is_empty() && self.dependent_paths.is_empty() {
            return Err(
                "Must specify at least one of: --top-dependents, --dependents, or --dependent-paths".to_string()
            );
        }

        // Check if we have a way to determine the crate name
        let has_path = self.path.is_some();
        let has_crate = self.crate_name.is_some();
        let has_local_manifest = std::path::Path::new("./Cargo.toml").exists();

        if !has_path && !has_crate && !has_local_manifest {
            return Err("Cannot determine which crate to test. \
                 Please specify --path <PATH>, --crate <NAME>, or run from a crate directory with ./Cargo.toml"
                .to_string());
        }

        Ok(())
    }

    /// Should we skip cargo check?
    pub fn should_skip_check(&self) -> bool {
        self.only_fetch
    }

    /// Should we skip cargo test?
    pub fn should_skip_test(&self) -> bool {
        self.only_fetch || self.only_check
    }

    /// Get the staging directory, using the default cache location if not specified
    pub fn get_staging_dir(&self) -> PathBuf {
        self.staging_dir.clone().unwrap_or_else(|| default_cache_dir().join("staging"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_both_only_flags_fails() {
        let args = CliArgs {
            path: None,
            crate_name: None,
            top_dependents: 5,
            dependents: vec![],
            dependent_paths: vec![],
            test_versions: vec![],
            force_versions: vec![],
            output: PathBuf::from("report.html"),
            staging_dir: None,
            only_fetch: true,
            only_check: true,
            json: false,
            clean: false,
            error_lines: 10,
            skip_normal_testing: false,
            console_width: None,
            docker: false,
            patch_transitive: false,
            simple: false,
        };
        assert!(args.validate().is_err());
    }

    #[test]
    fn test_validate_valid_config_succeeds() {
        // Create a temp Cargo.toml so validation passes
        std::fs::write("./Cargo.toml.test", "[package]\nname = \"test\"\nversion = \"0.1.0\"\n").ok();

        let args = CliArgs {
            path: Some(PathBuf::from("./Cargo.toml.test")),
            crate_name: None,
            top_dependents: 5,
            dependents: vec![],
            dependent_paths: vec![],
            test_versions: vec![],
            force_versions: vec![],
            output: PathBuf::from("report.html"),
            staging_dir: None,
            only_fetch: false,
            only_check: false,
            json: false,
            clean: false,
            error_lines: 10,
            skip_normal_testing: false,
            console_width: None,
            docker: false,
            patch_transitive: false,
            simple: false,
        };
        let result = args.validate();
        std::fs::remove_file("./Cargo.toml.test").ok();
        assert!(result.is_ok());
    }
}
