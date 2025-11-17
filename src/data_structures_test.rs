/// Tests for data structures serialization
///
/// These tests ensure OfferedRow and related structures can be
/// properly serialized to JSON for export and testing.

#[cfg(test)]
mod tests {
    use crate::*;

    #[test]
    fn test_version_source_serialization() {
        let sources = vec![
            VersionSource::CratesIo,
            VersionSource::Local,
            VersionSource::Git,
        ];

        for source in sources {
            let json = serde_json::to_string(&source).unwrap();
            let deserialized: VersionSource = serde_json::from_str(&json).unwrap();
            assert_eq!(source, deserialized);
        }
    }

    #[test]
    fn test_command_type_serialization() {
        let commands = vec![
            CommandType::Fetch,
            CommandType::Check,
            CommandType::Test,
        ];

        for cmd in commands {
            let json = serde_json::to_string(&cmd).unwrap();
            let deserialized: CommandType = serde_json::from_str(&json).unwrap();
            assert_eq!(cmd, deserialized);
        }
    }

    #[test]
    fn test_crate_failure_serialization() {
        let failure = CrateFailure {
            crate_name: "test-crate".to_string(),
            error_message: "error[E0432]: unresolved import".to_string(),
        };

        let json = serde_json::to_string(&failure).unwrap();
        let deserialized: CrateFailure = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.crate_name, "test-crate");
        assert_eq!(deserialized.error_message, "error[E0432]: unresolved import");
    }

    #[test]
    fn test_command_result_serialization() {
        let result = CommandResult {
            passed: false,
            duration: 1.23,
            failures: vec![
                CrateFailure {
                    crate_name: "dep1".to_string(),
                    error_message: "build failed".to_string(),
                },
            ],
        };

        let json = serde_json::to_string(&result).unwrap();
        let deserialized: CommandResult = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.passed, false);
        assert_eq!(deserialized.duration, 1.23);
        assert_eq!(deserialized.failures.len(), 1);
        assert_eq!(deserialized.failures[0].crate_name, "dep1");
    }

    #[test]
    fn test_test_command_serialization() {
        let cmd = TestCommand {
            command: CommandType::Check,
            features: vec!["serde".to_string()],
            result: CommandResult {
                passed: true,
                duration: 0.5,
                failures: vec![],
            },
        };

        let json = serde_json::to_string(&cmd).unwrap();
        let deserialized: TestCommand = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.command, CommandType::Check);
        assert_eq!(deserialized.features, vec!["serde"]);
        assert_eq!(deserialized.result.passed, true);
    }

    #[test]
    fn test_test_execution_serialization() {
        let execution = TestExecution {
            commands: vec![
                TestCommand {
                    command: CommandType::Fetch,
                    features: vec![],
                    result: CommandResult {
                        passed: true,
                        duration: 0.1,
                        failures: vec![],
                    },
                },
                TestCommand {
                    command: CommandType::Check,
                    features: vec![],
                    result: CommandResult {
                        passed: true,
                        duration: 0.2,
                        failures: vec![],
                    },
                },
                TestCommand {
                    command: CommandType::Test,
                    features: vec![],
                    result: CommandResult {
                        passed: false,
                        duration: 1.0,
                        failures: vec![
                            CrateFailure {
                                crate_name: "test-dep".to_string(),
                                error_message: "test failed".to_string(),
                            },
                        ],
                    },
                },
            ],
        };

        let json = serde_json::to_string(&execution).unwrap();
        let deserialized: TestExecution = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.commands.len(), 3);
        assert_eq!(deserialized.commands[0].command, CommandType::Fetch);
        assert_eq!(deserialized.commands[1].command, CommandType::Check);
        assert_eq!(deserialized.commands[2].command, CommandType::Test);
        assert_eq!(deserialized.commands[2].result.passed, false);
    }

    #[test]
    fn test_offered_version_serialization() {
        let offered = OfferedVersion {
            version: "0.8.52".to_string(),
            forced: true,
        };

        let json = serde_json::to_string(&offered).unwrap();
        let deserialized: OfferedVersion = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.version, "0.8.52");
        assert_eq!(deserialized.forced, true);
    }

    #[test]
    fn test_dependency_ref_serialization() {
        let dep = DependencyRef {
            dependent_name: "image".to_string(),
            dependent_version: "0.25.0".to_string(),
            spec: "^0.8".to_string(),
            resolved_version: "0.8.52".to_string(),
            resolved_source: VersionSource::CratesIo,
            used_offered_version: true,
        };

        let json = serde_json::to_string(&dep).unwrap();
        let deserialized: DependencyRef = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.dependent_name, "image");
        assert_eq!(deserialized.spec, "^0.8");
        assert_eq!(deserialized.resolved_version, "0.8.52");
        assert_eq!(deserialized.used_offered_version, true);
    }

    #[test]
    fn test_transitive_test_serialization() {
        let transitive = TransitiveTest {
            dependency: DependencyRef {
                dependent_name: "sub-dep".to_string(),
                dependent_version: "1.0.0".to_string(),
                spec: "^0.8".to_string(),
                resolved_version: "0.8.51".to_string(),
                resolved_source: VersionSource::CratesIo,
                used_offered_version: false,
            },
            depth: 2,
        };

        let json = serde_json::to_string(&transitive).unwrap();
        let deserialized: TransitiveTest = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.depth, 2);
        assert_eq!(deserialized.dependency.dependent_name, "sub-dep");
    }

    #[test]
    fn test_offered_row_serialization_baseline() {
        // Baseline row (no offered version)
        let row = OfferedRow {
            baseline_passed: None,
            primary: DependencyRef {
                dependent_name: "test-dep".to_string(),
                dependent_version: "1.0.0".to_string(),
                spec: "^0.8".to_string(),
                resolved_version: "0.8.52".to_string(),
                resolved_source: VersionSource::CratesIo,
                used_offered_version: true,
            },
            offered: None,
            test: TestExecution {
                commands: vec![],
            },
            transitive: vec![],
        };

        let json = serde_json::to_string(&row).unwrap();
        let deserialized: OfferedRow = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.baseline_passed, None);
        assert_eq!(deserialized.offered, None);
        assert_eq!(deserialized.primary.dependent_name, "test-dep");
    }

    #[test]
    fn test_offered_row_serialization_with_offered() {
        // Row with offered version
        let row = OfferedRow {
            baseline_passed: Some(true),
            primary: DependencyRef {
                dependent_name: "test-dep".to_string(),
                dependent_version: "1.0.0".to_string(),
                spec: "^0.8".to_string(),
                resolved_version: "0.8.52".to_string(),
                resolved_source: VersionSource::CratesIo,
                used_offered_version: true,
            },
            offered: Some(OfferedVersion {
                version: "0.8.52".to_string(),
                forced: false,
            }),
            test: TestExecution {
                commands: vec![
                    TestCommand {
                        command: CommandType::Fetch,
                        features: vec![],
                        result: CommandResult {
                            passed: true,
                            duration: 0.5,
                            failures: vec![],
                        },
                    },
                ],
            },
            transitive: vec![],
        };

        let json = serde_json::to_string(&row).unwrap();
        let deserialized: OfferedRow = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.baseline_passed, Some(true));
        assert!(deserialized.offered.is_some());
        assert_eq!(deserialized.offered.unwrap().version, "0.8.52");
    }

    #[test]
    fn test_offered_row_json_round_trip() {
        // Complex row with everything
        let row = OfferedRow {
            baseline_passed: Some(true),
            primary: DependencyRef {
                dependent_name: "complex-dep".to_string(),
                dependent_version: "2.1.0".to_string(),
                spec: "^0.8.50".to_string(),
                resolved_version: "0.8.52".to_string(),
                resolved_source: VersionSource::Local,
                used_offered_version: true,
            },
            offered: Some(OfferedVersion {
                version: "0.8.52".to_string(),
                forced: true,
            }),
            test: TestExecution {
                commands: vec![
                    TestCommand {
                        command: CommandType::Fetch,
                        features: vec!["feature1".to_string()],
                        result: CommandResult {
                            passed: true,
                            duration: 1.1,
                            failures: vec![],
                        },
                    },
                    TestCommand {
                        command: CommandType::Check,
                        features: vec!["feature1".to_string()],
                        result: CommandResult {
                            passed: false,
                            duration: 2.3,
                            failures: vec![
                                CrateFailure {
                                    crate_name: "complex-dep".to_string(),
                                    error_message: "error[E0308]: type mismatch".to_string(),
                                },
                            ],
                        },
                    },
                ],
            },
            transitive: vec![
                TransitiveTest {
                    dependency: DependencyRef {
                        dependent_name: "transitive-1".to_string(),
                        dependent_version: "0.5.0".to_string(),
                        spec: "0.8.51".to_string(),
                        resolved_version: "0.8.51".to_string(),
                        resolved_source: VersionSource::CratesIo,
                        used_offered_version: false,
                    },
                    depth: 1,
                },
            ],
        };

        // Serialize to JSON
        let json = serde_json::to_string_pretty(&row).unwrap();

        // Deserialize back
        let deserialized: OfferedRow = serde_json::from_str(&json).unwrap();

        // Verify all fields
        assert_eq!(deserialized.baseline_passed, Some(true));
        assert_eq!(deserialized.primary.dependent_name, "complex-dep");
        assert_eq!(deserialized.offered.as_ref().unwrap().forced, true);
        assert_eq!(deserialized.test.commands.len(), 2);
        assert_eq!(deserialized.test.commands[1].result.passed, false);
        assert_eq!(deserialized.transitive.len(), 1);
        assert_eq!(deserialized.transitive[0].depth, 1);
    }
}
