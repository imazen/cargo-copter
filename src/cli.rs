use clap::Parser;
use std::path::PathBuf;

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
    #[arg(long, value_name = "CRATE[:VERSION]", num_args = 1..)]
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

    /// Number of parallel test jobs
    #[arg(long, short = 'j', default_value = "1")]
    pub jobs: usize,

    /// HTML report output path
    #[arg(long, default_value = "copter-report.html")]
    pub output: PathBuf,

    /// Directory for staging unpacked crates (enables caching across runs)
    #[arg(long, default_value = ".copter/staging")]
    pub staging_dir: PathBuf,

    /// Skip cargo check (only run tests)
    #[arg(long)]
    pub no_check: bool,

    /// Skip cargo test (only run check)
    #[arg(long)]
    pub no_test: bool,

    /// Output results as JSON
    #[arg(long)]
    pub json: bool,

    /// Force testing specific versions, bypassing semver requirements
    /// Accepts multiple versions like --test-versions (e.g., "0.7.0 1.0.0-rc.1")
    /// These versions are tested even if they don't satisfy dependent's requirements
    #[arg(long, value_name = "VERSION", num_args = 0..)]
    pub force_versions: Vec<String>,
}

impl CliArgs {
    /// Parse command-line arguments
    pub fn parse_args() -> Self {
        let mut args = CliArgs::parse();

        // Split test_versions on whitespace to support quoted lists like '0.8.51 0.8.91-alpha.3'
        args.test_versions = args.test_versions
            .iter()
            .flat_map(|s| s.split_whitespace().map(|v| v.to_string()))
            .collect();

        // Split force_versions on whitespace as well
        args.force_versions = args.force_versions
            .iter()
            .flat_map(|s| s.split_whitespace().map(|v| v.to_string()))
            .collect();

        args
    }

    /// Validate argument combinations
    pub fn validate(&self) -> Result<(), String> {
        // Can't skip both check and test
        if self.no_check && self.no_test {
            return Err("Cannot specify both --no-check and --no-test".to_string());
        }

        // Need at least one of: top_dependents, dependents, or dependent_paths
        if self.top_dependents == 0
            && self.dependents.is_empty()
            && self.dependent_paths.is_empty() {
            return Err("Must specify at least one of: --top-dependents, --dependents, or --dependent-paths".to_string());
        }

        // Validate jobs >= 1
        if self.jobs == 0 {
            return Err("--jobs must be at least 1".to_string());
        }

        // Check if we have a way to determine the crate name
        let has_path = self.path.is_some();
        let has_crate = self.crate_name.is_some();
        let has_local_manifest = std::path::Path::new("./Cargo.toml").exists();

        if !has_path && !has_crate && !has_local_manifest {
            return Err(
                "Cannot determine which crate to test. \
                 Please specify --path <PATH>, --crate <NAME>, or run from a crate directory with ./Cargo.toml"
                    .to_string(),
            );
        }

        Ok(())
    }

    /// Check if we're testing local paths only (no network required)
    pub fn is_offline_mode(&self) -> bool {
        self.dependents.is_empty()
            && self.top_dependents == 0
            && !self.dependent_paths.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_both_no_flags_fails() {
        let args = CliArgs {
            path: None,
            crate_name: None,
            top_dependents: 5,
            dependents: vec![],
            dependent_paths: vec![],
            test_versions: vec![],
            force_versions: vec![],
            jobs: 1,
            output: PathBuf::from("report.html"),
            staging_dir: PathBuf::from(".copter/staging"),
            no_check: true,
            no_test: true,
            json: false,
        };
        assert!(args.validate().is_err());
    }

    #[test]
    fn test_validate_zero_jobs_fails() {
        let args = CliArgs {
            path: None,
            crate_name: None,
            top_dependents: 5,
            dependents: vec![],
            dependent_paths: vec![],
            test_versions: vec![],
            force_versions: vec![],
            jobs: 0,
            output: PathBuf::from("report.html"),
            staging_dir: PathBuf::from(".copter/staging"),
            no_check: false,
            no_test: false,
            json: false,
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
            jobs: 1,
            output: PathBuf::from("report.html"),
            staging_dir: PathBuf::from(".copter/staging"),
            no_check: false,
            no_test: false,
            json: false,
        };
        let result = args.validate();
        std::fs::remove_file("./Cargo.toml.test").ok();
        assert!(result.is_ok());
    }

    #[test]
    fn test_is_offline_mode() {
        let args = CliArgs {
            path: None,
            crate_name: None,
            top_dependents: 0,
            dependents: vec![],
            dependent_paths: vec![PathBuf::from("/tmp/crate")],
            test_versions: vec![],
            force_versions: vec![],
            jobs: 1,
            output: PathBuf::from("report.html"),
            staging_dir: PathBuf::from(".copter/staging"),
            no_check: false,
            no_test: false,
            json: false,
        };
        assert!(args.is_offline_mode());
    }

    #[test]
    fn test_not_offline_mode_with_dependents() {
        let args = CliArgs {
            path: None,
            crate_name: None,
            top_dependents: 0,
            dependents: vec!["serde".to_string()],
            dependent_paths: vec![],
            test_versions: vec![],
            force_versions: vec![],
            jobs: 1,
            output: PathBuf::from("report.html"),
            staging_dir: PathBuf::from(".copter/staging"),
            no_check: false,
            no_test: false,
            json: false,
        };
        assert!(!args.is_offline_mode());
    }
}
