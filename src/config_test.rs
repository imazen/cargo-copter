/// Tests for config module
#[cfg(test)]
mod tests {
    use crate::cli::CliArgs;
    use crate::config::build_test_matrix;
    use crate::types::{OverrideMode, Version};

    #[test]
    fn test_baseline_flag_is_set() {
        // Create minimal args for testing
        let args = CliArgs {
            crate_name: Some("test-crate".to_string()),
            path: None,
            test_versions: vec!["0.1.0".to_string(), "0.2.0".to_string()],
            force_versions: vec![],
            dependents: vec!["dep1".to_string()],
            dependent_paths: vec![],
            top_dependents: 0,
            staging_dir: None,
            clean: false,
            only_fetch: false,
            only_check: false,
            skip_normal_testing: false,
            error_lines: 10,
            json: false,
            output: std::path::PathBuf::from("copter-report.html"),
            console_width: None,
            docker: false,
            patch_transitive: false,
            simple: false,
        };

        let matrix = build_test_matrix(&args).expect("Should build matrix");

        // Verify exactly one baseline
        let baseline_count = matrix.base_versions.iter().filter(|v| v.is_baseline).count();
        assert_eq!(baseline_count, 1, "Should have exactly one baseline version");

        // Verify first version is baseline
        assert!(matrix.base_versions[0].is_baseline, "First version should be marked as baseline");

        // Verify other versions are not baseline
        for version in matrix.base_versions.iter().skip(1) {
            assert!(!version.is_baseline, "Non-first versions should not be baseline");
        }
    }

    #[test]
    fn test_baseline_has_no_override() {
        let args = CliArgs {
            crate_name: Some("test-crate".to_string()),
            path: None,
            test_versions: vec!["0.1.0".to_string()],
            force_versions: vec![],
            dependents: vec!["dep1".to_string()],
            dependent_paths: vec![],
            top_dependents: 0,
            staging_dir: None,
            clean: false,
            only_fetch: false,
            only_check: false,
            skip_normal_testing: false,
            error_lines: 10,
            json: false,
            output: std::path::PathBuf::from("copter-report.html"),
            console_width: None,
            docker: false,
            patch_transitive: false,
            simple: false,
        };

        let matrix = build_test_matrix(&args).expect("Should build matrix");

        // Find baseline version
        let baseline = matrix.base_versions.iter().find(|v| v.is_baseline).expect("Should have baseline");

        // Baseline should have OverrideMode::None or Patch (not Force)
        assert_ne!(baseline.override_mode, OverrideMode::Force, "Baseline should not be forced");
    }

    #[test]
    fn test_multiple_versions_only_one_baseline() {
        let args = CliArgs {
            crate_name: Some("test-crate".to_string()),
            path: None,
            test_versions: vec!["0.1.0".to_string(), "0.2.0".to_string(), "0.3.0".to_string()],
            force_versions: vec![],
            dependents: vec!["dep1".to_string()],
            dependent_paths: vec![],
            top_dependents: 0,
            staging_dir: None,
            clean: false,
            only_fetch: false,
            only_check: false,
            skip_normal_testing: false,
            error_lines: 10,
            json: false,
            output: std::path::PathBuf::from("copter-report.html"),
            console_width: None,
            docker: false,
            patch_transitive: false,
            simple: false,
        };

        let matrix = build_test_matrix(&args).expect("Should build matrix");

        // Count baselines
        let baseline_count = matrix.base_versions.iter().filter(|v| v.is_baseline).count();

        assert_eq!(baseline_count, 1, "Should have exactly one baseline, found {}", baseline_count);
    }

    #[test]
    fn test_dependents_have_baseline_flag() {
        let args = CliArgs {
            crate_name: Some("test-crate".to_string()),
            path: None,
            test_versions: vec!["0.1.0".to_string()],
            force_versions: vec![],
            dependents: vec!["dep1".to_string(), "dep2".to_string()],
            dependent_paths: vec![],
            top_dependents: 0,
            staging_dir: None,
            clean: false,
            only_fetch: false,
            only_check: false,
            skip_normal_testing: false,
            error_lines: 10,
            json: false,
            output: std::path::PathBuf::from("copter-report.html"),
            console_width: None,
            docker: false,
            patch_transitive: false,
            simple: false,
        };

        let matrix = build_test_matrix(&args).expect("Should build matrix");

        // First dependent should be baseline
        assert!(matrix.dependents[0].is_baseline, "First dependent should be marked as baseline");

        // Other dependents should not be baseline
        for dep in matrix.dependents.iter().skip(1) {
            assert!(!dep.is_baseline, "Non-first dependents should not be baseline");
        }
    }

    #[test]
    fn test_multi_version_mode_has_baseline_and_override() {
        // When --test-versions is specified, should create baseline + test versions
        let args = CliArgs {
            crate_name: Some("test-crate".to_string()),
            path: None,
            test_versions: vec!["0.1.0".to_string(), "0.2.0".to_string()],
            force_versions: vec![],
            dependents: vec!["dep1".to_string()],
            dependent_paths: vec![],
            top_dependents: 0,
            staging_dir: None,
            clean: false,
            only_fetch: false,
            only_check: false,
            skip_normal_testing: false,
            error_lines: 10,
            json: false,
            output: std::path::PathBuf::from("copter-report.html"),
            console_width: None,
            docker: false,
            patch_transitive: false,
            simple: false,
        };

        let matrix = build_test_matrix(&args).expect("Should build matrix");

        // Should have at least 2 versions (baseline + test versions)
        assert!(matrix.base_versions.len() >= 2, "Should have baseline + test versions");

        // First should be baseline with no override
        assert!(matrix.base_versions[0].is_baseline, "First version should be baseline");
        assert_eq!(matrix.base_versions[0].override_mode, OverrideMode::None, "Baseline should have no override");

        // Others should not be baseline
        for v in matrix.base_versions.iter().skip(1) {
            assert!(!v.is_baseline, "Non-first versions should not be baseline");
        }
    }
}
