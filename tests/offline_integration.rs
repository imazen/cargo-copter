/// Offline integration tests for cargo-copter
///
/// These tests use local test fixtures to verify all result states
/// without requiring network access to crates.io

use std::path::{Path, PathBuf};

// Helper to get the test fixtures directory
fn fixtures_dir() -> PathBuf {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    Path::new(manifest_dir).join("test-crates/integration-fixtures")
}

// Note: These tests will be implemented once we expose the compile module's
// public API. For now, we create placeholder tests.

#[test]
fn test_fixtures_exist() {
    let fixtures = fixtures_dir();
    assert!(fixtures.exists(), "fixtures directory should exist");

    // Verify all test fixtures are present
    assert!(fixtures.join("base-crate-v1").exists());
    assert!(fixtures.join("base-crate-v2").exists());
    assert!(fixtures.join("dependent-passing").exists());
    assert!(fixtures.join("dependent-regressed").exists());
    assert!(fixtures.join("dependent-broken").exists());
    assert!(fixtures.join("dependent-test-passing").exists());
    assert!(fixtures.join("dependent-test-failing").exists());
}

#[test]
fn test_base_crate_v1_compiles() {
    use std::process::Command;

    let base_v1 = fixtures_dir().join("base-crate-v1");

    let output = Command::new("cargo")
        .arg("check")
        .current_dir(&base_v1)
        .output()
        .expect("Failed to run cargo check");

    assert!(output.status.success(),
            "base-crate-v1 should compile successfully");
}

#[test]
fn test_base_crate_v2_compiles() {
    use std::process::Command;

    let base_v2 = fixtures_dir().join("base-crate-v2");

    let output = Command::new("cargo")
        .arg("check")
        .current_dir(&base_v2)
        .output()
        .expect("Failed to run cargo check");

    assert!(output.status.success(),
            "base-crate-v2 should compile successfully");
}

#[test]
fn test_dependent_passing_with_v1() {
    use std::process::Command;

    let dependent = fixtures_dir().join("dependent-passing");

    // Should compile with v1 (default dependency)
    let output = Command::new("cargo")
        .arg("check")
        .current_dir(&dependent)
        .output()
        .expect("Failed to run cargo check");

    assert!(output.status.success(),
            "dependent-passing should compile with base-crate-v1");
}

#[test]
fn test_dependent_passing_tests_with_v1() {
    use std::process::Command;

    let dependent = fixtures_dir().join("dependent-passing");

    // Tests should pass with v1
    let output = Command::new("cargo")
        .arg("test")
        .current_dir(&dependent)
        .output()
        .expect("Failed to run cargo test");

    assert!(output.status.success(),
            "dependent-passing tests should pass with base-crate-v1");
}

#[test]
fn test_dependent_regressed_with_v1() {
    use std::process::Command;

    let dependent = fixtures_dir().join("dependent-regressed");

    // Should compile with v1
    let output = Command::new("cargo")
        .arg("check")
        .current_dir(&dependent)
        .output()
        .expect("Failed to run cargo check");

    assert!(output.status.success(),
            "dependent-regressed should compile with base-crate-v1");
}

#[test]
fn test_dependent_broken_fails() {
    use std::process::Command;

    let dependent = fixtures_dir().join("dependent-broken");

    // Should fail to compile (has type error)
    let output = Command::new("cargo")
        .arg("check")
        .current_dir(&dependent)
        .output()
        .expect("Failed to run cargo check");

    assert!(!output.status.success(),
            "dependent-broken should fail to compile");
}

#[test]
fn test_dependent_test_failing_with_v1() {
    use std::process::Command;

    let dependent = fixtures_dir().join("dependent-test-failing");

    // Check should pass with v1
    let output = Command::new("cargo")
        .arg("check")
        .current_dir(&dependent)
        .output()
        .expect("Failed to run cargo check");

    assert!(output.status.success(),
            "dependent-test-failing should check successfully with base-crate-v1");

    // Tests should pass with v1
    let output = Command::new("cargo")
        .arg("test")
        .current_dir(&dependent)
        .output()
        .expect("Failed to run cargo test");

    assert!(output.status.success(),
            "dependent-test-failing tests should pass with base-crate-v1");
}

// TODO: Add tests that use cargo's path override to test with base-crate-v2
// These require setting up .cargo/config.toml which is done in the compile module

#[test]
fn test_compile_with_override_scenario() {
    // TODO: This test will verify the 4-step compilation flow:
    // 1. baseline check
    // 2. baseline test
    // 3. override check
    // 4. override test
    //
    // We'll use dependent-passing with v1 as baseline and v2 as override
    // Expected: All 4 steps pass (PASSED state)
}

#[test]
fn test_regression_scenario() {
    // TODO: This test will verify regression detection:
    // - dependent-regressed compiles with v1
    // - dependent-regressed fails with v2
    // Expected: REGRESSED state
}

#[test]
fn test_broken_scenario() {
    // TODO: This test will verify broken detection:
    // - dependent-broken fails with v1
    // - v2 not tested
    // Expected: BROKEN state
}

#[test]
fn test_test_regression_scenario() {
    // TODO: This test will verify test-time regression:
    // - dependent-test-failing check passes with both
    // - dependent-test-failing tests pass with v1
    // - dependent-test-failing tests fail with v2
    // Expected: REGRESSED state
}

#[test]
fn test_staging_directory_creates_on_first_use() {
    use tempfile::TempDir;
    use std::fs;

    let temp_dir = TempDir::new().unwrap();
    let staging_dir = temp_dir.path().join("staging");

    // Staging dir should not exist yet
    assert!(!staging_dir.exists());

    // Create it
    fs::create_dir_all(&staging_dir).unwrap();

    // Now it should exist
    assert!(staging_dir.exists());
}

#[test]
fn test_staging_directory_structure() {
    use tempfile::TempDir;
    use std::fs;

    let temp_dir = TempDir::new().unwrap();
    let staging_dir = temp_dir.path().join("staging");
    fs::create_dir_all(&staging_dir).unwrap();

    // Simulate creating a staging directory for a crate
    let crate_staging = staging_dir.join("serde-1.0.0");
    fs::create_dir_all(&crate_staging).unwrap();

    assert!(crate_staging.exists());
    assert!(crate_staging.is_dir());
}

#[test]
fn test_staging_directory_caching_check() {
    use tempfile::TempDir;
    use std::fs;
    use std::time::{SystemTime, Duration};

    let temp_dir = TempDir::new().unwrap();
    let staging_dir = temp_dir.path().join("staging");
    fs::create_dir_all(&staging_dir).unwrap();

    let crate_staging = staging_dir.join("test-crate-0.1.0");
    fs::create_dir_all(&crate_staging).unwrap();

    // Write a marker file
    let marker = crate_staging.join("marker.txt");
    fs::write(&marker, "cached").unwrap();

    // Simulate checking if already unpacked
    if crate_staging.exists() {
        // Should use cached version
        let content = fs::read_to_string(&marker).unwrap();
        assert_eq!(content, "cached");
    } else {
        panic!("Staging directory should exist");
    }
}

#[test]
fn test_cargo_metadata_works_on_fixture() {
    use std::process::Command;

    let fixtures = fixtures_dir();
    let dependent = fixtures.join("dependent-passing");

    // Verify cargo metadata works on one of our fixtures
    let output = Command::new("cargo")
        .args(&["metadata", "--format-version=1", "--no-deps"])
        .current_dir(&dependent)
        .output()
        .expect("Failed to run cargo metadata");

    assert!(output.status.success(), "cargo metadata should succeed");

    // Parse JSON to verify structure
    let stdout = String::from_utf8_lossy(&output.stdout);
    let metadata: serde_json::Value = serde_json::from_str(&stdout)
        .expect("Should parse metadata JSON");

    // Verify expected fields exist
    assert!(metadata.get("packages").is_some());
    assert!(metadata.get("workspace_root").is_some());
}

#[test]
fn test_cargo_metadata_shows_base_crate_dependency() {
    use std::process::Command;

    let fixtures = fixtures_dir();
    let dependent = fixtures.join("dependent-passing");

    let output = Command::new("cargo")
        .args(&["metadata", "--format-version=1", "--no-deps"])
        .current_dir(&dependent)
        .output()
        .expect("Failed to run cargo metadata");

    assert!(output.status.success());

    let stdout = String::from_utf8_lossy(&output.stdout);
    let metadata: serde_json::Value = serde_json::from_str(&stdout).unwrap();

    // Look for base-crate in dependencies
    if let Some(packages) = metadata.get("packages").and_then(|p| p.as_array()) {
        for package in packages {
            if let Some(deps) = package.get("dependencies").and_then(|d| d.as_array()) {
                let has_base_crate = deps.iter().any(|dep| {
                    dep.get("name")
                        .and_then(|n| n.as_str())
                        .map(|name| name == "base-crate")
                        .unwrap_or(false)
                });

                if has_base_crate {
                    // Found it! This is what extract_resolved_version uses
                    return;
                }
            }
        }
    }

    panic!("Should find base-crate in dependent-passing's dependencies");
}
