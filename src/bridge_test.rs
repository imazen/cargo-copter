/// Tests for bridge module
#[cfg(test)]
mod tests {
    use crate::bridge::test_result_to_offered_row;
    use crate::compile::{CompileResult, CompileStep, PatchDepth, ThreeStepResult};
    use crate::types::*;
    use std::time::Duration;

    /// Test that spec values are never "?" when original_requirement is provided
    #[test]
    fn test_spec_never_question_mark_when_requirement_provided() {
        let result = create_test_result_with_requirement("^0.8.0");
        let row = test_result_to_offered_row(&result);

        assert_ne!(row.primary.spec, "?", "Spec should not be '?' when original_requirement is provided");
        assert_eq!(row.primary.spec, "^0.8.0");
    }

    /// Test that spec defaults to "?" when original_requirement is None (for broken packages)
    #[test]
    fn test_spec_defaults_to_question_mark_when_none() {
        let result = create_test_result_with_requirement_none();
        let row = test_result_to_offered_row(&result);

        assert_eq!(
            row.primary.spec, "?",
            "Spec should default to '?' when original_requirement is None (broken packages)"
        );
    }

    /// Test that baseline rows have no offered version
    #[test]
    fn test_baseline_has_no_offered_version() {
        let result = create_baseline_result();
        let row = test_result_to_offered_row(&result);

        assert!(row.offered.is_none(), "Baseline row should have no offered version");
        assert!(row.baseline_passed.is_none(), "Baseline row should have no baseline_passed field");
    }

    /// Test that non-baseline rows have offered version
    #[test]
    fn test_non_baseline_has_offered_version() {
        let result = create_non_baseline_result();
        let row = test_result_to_offered_row(&result);

        assert!(row.offered.is_some(), "Non-baseline row should have offered version");
        assert!(row.baseline_passed.is_some(), "Non-baseline row should have baseline_passed field");
    }

    // Helper functions

    fn create_test_result_with_requirement(req: &str) -> TestResult {
        TestResult {
            base_version: VersionedCrate::from_registry("rgb", "0.8.50"),
            dependent: VersionedCrate::from_registry("load_image", "3.3.1"),
            execution: ThreeStepResult {
                fetch: CompileResult {
                    step: CompileStep::Fetch,
                    success: true,
                    stdout: String::new(),
                    stderr: String::new(),
                    duration: Duration::from_secs(1),
                    diagnostics: Vec::new(),
                },
                check: None,
                test: None,
                actual_version: Some("0.8.50".to_string()),
                expected_version: Some("0.8.50".to_string()),
                forced_version: false,
                original_requirement: Some(req.to_string()),
                all_crate_versions: vec![],
                patch_depth: PatchDepth::None,
                blocking_crates: vec![],
            },
            baseline: None, // This IS the baseline
        }
    }

    fn create_test_result_with_requirement_none() -> TestResult {
        TestResult {
            base_version: VersionedCrate::from_registry("rgb", "0.8.50"),
            dependent: VersionedCrate::from_registry("load_image", "3.3.1"),
            execution: ThreeStepResult {
                fetch: CompileResult {
                    step: CompileStep::Fetch,
                    success: true,
                    stdout: String::new(),
                    stderr: String::new(),
                    duration: Duration::from_secs(1),
                    diagnostics: Vec::new(),
                },
                check: None,
                test: None,
                actual_version: Some("0.8.50".to_string()),
                expected_version: Some("0.8.50".to_string()),
                forced_version: false,
                original_requirement: None, // No requirement provided
                all_crate_versions: vec![],
                patch_depth: PatchDepth::None,
                blocking_crates: vec![],
            },
            baseline: None,
        }
    }

    fn create_baseline_result() -> TestResult {
        TestResult {
            base_version: VersionedCrate::from_registry("rgb", "0.8.50"),
            dependent: VersionedCrate::from_registry("load_image", "3.3.1"),
            execution: ThreeStepResult {
                fetch: CompileResult {
                    step: CompileStep::Fetch,
                    success: true,
                    stdout: String::new(),
                    stderr: String::new(),
                    duration: Duration::from_secs(1),
                    diagnostics: Vec::new(),
                },
                check: None,
                test: None,
                actual_version: Some("0.8.50".to_string()),
                expected_version: Some("0.8.50".to_string()),
                forced_version: false,
                original_requirement: Some("^0.8.0".to_string()),
                all_crate_versions: vec![],
                patch_depth: PatchDepth::None,
                blocking_crates: vec![],
            },
            baseline: None, // No baseline comparison = this IS the baseline
        }
    }

    fn create_non_baseline_result() -> TestResult {
        TestResult {
            base_version: VersionedCrate::from_registry("rgb", "0.8.51"),
            dependent: VersionedCrate::from_registry("load_image", "3.3.1"),
            execution: ThreeStepResult {
                fetch: CompileResult {
                    step: CompileStep::Fetch,
                    success: true,
                    stdout: String::new(),
                    stderr: String::new(),
                    duration: Duration::from_secs(1),
                    diagnostics: Vec::new(),
                },
                check: None,
                test: None,
                actual_version: Some("0.8.51".to_string()),
                expected_version: Some("0.8.51".to_string()),
                forced_version: false,
                original_requirement: Some("^0.8.0".to_string()),
                all_crate_versions: vec![],
                patch_depth: PatchDepth::None,
                blocking_crates: vec![],
            },
            baseline: Some(BaselineComparison {
                baseline_passed: true,
                baseline_version: "0.8.50".to_string(),
                baseline_fetch_passed: true,
                baseline_check_passed: None,
                baseline_test_passed: None,
            }),
        }
    }
}
