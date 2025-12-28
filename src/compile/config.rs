//! Test configuration builder for the ICT pipeline.
//!
//! This module provides the `TestConfig` builder pattern for configuring
//! test execution. It encapsulates all the options needed to run a test.

use std::path::{Path, PathBuf};

/// Configuration for running a three-step ICT test.
///
/// Use the builder pattern to construct a `TestConfig`:
///
/// ```ignore
/// let config = TestConfig::new(&dependent_path, "rgb")
///     .with_skip_flags(false, true)  // skip_check, skip_test
///     .with_version_info(Some("0.8.91".to_string()), true, None)
///     .with_override_path(&override_path);
/// ```
#[derive(Debug, Clone)]
pub struct TestConfig {
    /// Path to the dependent crate being tested
    pub dependent_path: PathBuf,

    /// Name of the base crate (the one we're testing impact of)
    pub base_crate: String,

    /// Path to the override crate (for WIP or specific version)
    pub override_path: Option<PathBuf>,

    /// Specific version to test (None = use Cargo's natural resolution)
    pub offered_version: Option<String>,

    /// Whether to force the version (bypass semver requirements)
    pub force_version: bool,

    /// Original version requirement from dependent's Cargo.toml
    /// Used to display "Spec" column in output
    pub original_requirement: Option<String>,

    /// Skip the `cargo check` step
    pub skip_check: bool,

    /// Skip the `cargo test` step
    pub skip_test: bool,

    /// Add `[patch.crates-io]` to unify transitive dependencies
    pub patch_transitive: bool,

    /// Additional Cargo features to enable
    pub features: Vec<String>,
}

impl TestConfig {
    /// Create a new test configuration.
    ///
    /// # Arguments
    /// * `dependent_path` - Path to the dependent crate to test
    /// * `base_crate` - Name of the base crate (e.g., "rgb")
    pub fn new(dependent_path: &Path, base_crate: &str) -> Self {
        Self {
            dependent_path: dependent_path.to_path_buf(),
            base_crate: base_crate.to_string(),
            override_path: None,
            offered_version: None,
            force_version: false,
            original_requirement: None,
            skip_check: false,
            skip_test: false,
            patch_transitive: false,
            features: Vec::new(),
        }
    }

    /// Set the skip flags for check and test steps.
    ///
    /// # Arguments
    /// * `skip_check` - If true, skip `cargo check`
    /// * `skip_test` - If true, skip `cargo test`
    pub fn with_skip_flags(mut self, skip_check: bool, skip_test: bool) -> Self {
        self.skip_check = skip_check;
        self.skip_test = skip_test;
        self
    }

    /// Set version information for the test.
    ///
    /// # Arguments
    /// * `version` - The specific version to test (None for baseline)
    /// * `forced` - Whether to force this version (bypass semver)
    /// * `original_requirement` - The original requirement from Cargo.toml
    pub fn with_version_info(
        mut self,
        version: Option<String>,
        forced: bool,
        original_requirement: Option<String>,
    ) -> Self {
        self.offered_version = version;
        self.force_version = forced;
        self.original_requirement = original_requirement;
        self
    }

    /// Set the path to the override crate.
    ///
    /// This is used when testing a specific version (WIP or downloaded).
    pub fn with_override_path(mut self, path: &Path) -> Self {
        self.override_path = Some(path.to_path_buf());
        self
    }

    /// Enable transitive patching via `[patch.crates-io]`.
    ///
    /// This unifies all versions of the base crate across the dependency tree.
    pub fn with_patch_transitive(mut self, enabled: bool) -> Self {
        self.patch_transitive = enabled;
        self
    }

    /// Add cargo features to enable during the test.
    pub fn with_features(mut self, features: Vec<String>) -> Self {
        self.features = features;
        self
    }

    /// Check if this is a baseline test (no version override).
    pub fn is_baseline(&self) -> bool {
        self.override_path.is_none() && self.offered_version.is_none()
    }

    /// Get a display string for the test configuration.
    pub fn display(&self) -> String {
        if self.is_baseline() {
            format!("{} (baseline)", self.base_crate)
        } else if let Some(ref version) = self.offered_version {
            if self.force_version {
                format!("{} {} [forced]", self.base_crate, version)
            } else {
                format!("{} {}", self.base_crate, version)
            }
        } else if let Some(ref path) = self.override_path {
            format!("{} (local: {})", self.base_crate, path.display())
        } else {
            self.base_crate.clone()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_config() {
        let config = TestConfig::new(Path::new("/tmp/dep"), "rgb");
        assert_eq!(config.dependent_path, PathBuf::from("/tmp/dep"));
        assert_eq!(config.base_crate, "rgb");
        assert!(config.is_baseline());
    }

    #[test]
    fn test_with_skip_flags() {
        let config = TestConfig::new(Path::new("/tmp/dep"), "rgb").with_skip_flags(true, true);
        assert!(config.skip_check);
        assert!(config.skip_test);
    }

    #[test]
    fn test_with_version_info() {
        let config = TestConfig::new(Path::new("/tmp/dep"), "rgb").with_version_info(
            Some("0.8.91".to_string()),
            true,
            Some("^0.8".to_string()),
        );
        assert_eq!(config.offered_version, Some("0.8.91".to_string()));
        assert!(config.force_version);
        assert_eq!(config.original_requirement, Some("^0.8".to_string()));
    }

    #[test]
    fn test_with_override_path() {
        let config = TestConfig::new(Path::new("/tmp/dep"), "rgb").with_override_path(Path::new("/tmp/override"));
        assert_eq!(config.override_path, Some(PathBuf::from("/tmp/override")));
        assert!(!config.is_baseline());
    }

    #[test]
    fn test_display() {
        let baseline = TestConfig::new(Path::new("/tmp/dep"), "rgb");
        assert!(baseline.display().contains("baseline"));

        let versioned = TestConfig::new(Path::new("/tmp/dep"), "rgb")
            .with_version_info(Some("0.8.91".to_string()), false, None)
            .with_override_path(Path::new("/tmp/override"));
        assert!(versioned.display().contains("0.8.91"));

        let forced = TestConfig::new(Path::new("/tmp/dep"), "rgb")
            .with_version_info(Some("0.8.91".to_string()), true, None)
            .with_override_path(Path::new("/tmp/override"));
        assert!(forced.display().contains("forced"));
    }
}
