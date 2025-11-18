/// Tests for runner module
#[cfg(test)]
mod tests {
    use crate::types::*;
    use std::path::PathBuf;

    /// Helper to create a minimal test matrix
    fn create_test_matrix() -> TestMatrix {
        TestMatrix {
            base_crate: "test-crate".to_string(),
            base_versions: vec![
                VersionSpec {
                    crate_ref: VersionedCrate::from_registry("test-crate", "0.1.0"),
                    override_mode: OverrideMode::None,
                    is_baseline: true, // First version is baseline
                },
                VersionSpec {
                    crate_ref: VersionedCrate::from_registry("test-crate", "0.2.0"),
                    override_mode: OverrideMode::Patch,
                    is_baseline: false,
                },
            ],
            dependents: vec![VersionSpec {
                crate_ref: VersionedCrate::from_registry("dep1", "1.0.0"),
                override_mode: OverrideMode::None,
                is_baseline: true,
            }],
            staging_dir: PathBuf::from(".copter/staging"),
            skip_check: false,
            skip_test: false,
            error_lines: 10,
        }
    }

    #[test]
    fn test_matrix_has_baseline() {
        let matrix = create_test_matrix();

        // Verify baseline exists
        let baseline = matrix.base_versions.iter().find(|v| v.is_baseline);
        assert!(baseline.is_some(), "Matrix should have a baseline version");

        // Verify exactly one baseline
        let baseline_count = matrix.base_versions.iter().filter(|v| v.is_baseline).count();
        assert_eq!(baseline_count, 1, "Should have exactly one baseline");
    }

    #[test]
    fn test_baseline_is_first() {
        let matrix = create_test_matrix();

        // First version should be baseline
        assert!(
            matrix.base_versions[0].is_baseline,
            "First version should be marked as baseline"
        );
    }

    #[test]
    fn test_baseline_has_no_override() {
        let matrix = create_test_matrix();

        let baseline = matrix.base_versions.iter().find(|v| v.is_baseline).unwrap();

        assert_eq!(
            baseline.override_mode,
            OverrideMode::None,
            "Baseline should have OverrideMode::None"
        );
    }

    #[test]
    fn test_non_baseline_has_override() {
        let matrix = create_test_matrix();

        let non_baseline = matrix.base_versions.iter().find(|v| !v.is_baseline).unwrap();

        assert_ne!(
            non_baseline.override_mode,
            OverrideMode::None,
            "Non-baseline should have an override mode"
        );
    }

    #[test]
    fn test_test_result_baseline_field() {
        // Simulate a baseline result
        let baseline_result = TestResult {
            base_version: VersionedCrate::from_registry("test-crate", "0.1.0"),
            dependent: VersionedCrate::from_registry("dep1", "1.0.0"),
            execution: crate::compile::ThreeStepResult {
                fetch: crate::compile::CompileResult {
                    step: crate::compile::CompileStep::Fetch,
                    success: true,
                    stdout: String::new(),
                    stderr: String::new(),
                    duration: std::time::Duration::from_secs(1),
                    diagnostics: vec![],
                },
                check: None,
                test: None,
                actual_version: Some("0.1.0".to_string()),
                expected_version: Some("0.1.0".to_string()),
                forced_version: false,
                original_requirement: None,
                all_crate_versions: vec![],
            },
            baseline: None, // Baseline has no comparison
        };

        assert!(
            baseline_result.is_baseline(),
            "Result with baseline=None should be identified as baseline"
        );
        assert_eq!(
            baseline_result.status(),
            TestStatus::Baseline { passed: true },
            "Should return Baseline status"
        );
    }

    #[test]
    fn test_test_result_with_baseline_comparison() {
        // Simulate a non-baseline result
        let result = TestResult {
            base_version: VersionedCrate::from_registry("test-crate", "0.2.0"),
            dependent: VersionedCrate::from_registry("dep1", "1.0.0"),
            execution: crate::compile::ThreeStepResult {
                fetch: crate::compile::CompileResult {
                    step: crate::compile::CompileStep::Fetch,
                    success: true,
                    stdout: String::new(),
                    stderr: String::new(),
                    duration: std::time::Duration::from_secs(1),
                    diagnostics: vec![],
                },
                check: None,
                test: None,
                actual_version: Some("0.2.0".to_string()),
                expected_version: Some("0.2.0".to_string()),
                forced_version: false,
                original_requirement: None,
                all_crate_versions: vec![],
            },
            baseline: Some(BaselineComparison {
                baseline_passed: true,
                baseline_version: "0.1.0".to_string(),
            }),
        };

        assert!(!result.is_baseline(), "Result with baseline comparison should not be baseline");
        assert_eq!(result.status(), TestStatus::Passed, "Should return Passed status");
    }

    #[test]
    fn test_test_result_regression() {
        let result = TestResult {
            base_version: VersionedCrate::from_registry("test-crate", "0.2.0"),
            dependent: VersionedCrate::from_registry("dep1", "1.0.0"),
            execution: crate::compile::ThreeStepResult {
                fetch: crate::compile::CompileResult {
                    step: crate::compile::CompileStep::Fetch,
                    success: false, // Failed!
                    stdout: String::new(),
                    stderr: String::new(),
                    duration: std::time::Duration::from_secs(1),
                    diagnostics: vec![],
                },
                check: None,
                test: None,
                actual_version: Some("0.2.0".to_string()),
                expected_version: Some("0.2.0".to_string()),
                forced_version: false,
                original_requirement: None,
                all_crate_versions: vec![],
            },
            baseline: Some(BaselineComparison {
                baseline_passed: true, // Baseline passed
                baseline_version: "0.1.0".to_string(),
            }),
        };

        assert_eq!(
            result.status(),
            TestStatus::Regressed,
            "Should return Regressed status when baseline passed but current failed"
        );
    }

    #[test]
    fn test_test_count() {
        let matrix = create_test_matrix();

        let expected_count = matrix.base_versions.len() * matrix.dependents.len();
        assert_eq!(matrix.test_count(), expected_count, "test_count should return correct number");
    }
}
