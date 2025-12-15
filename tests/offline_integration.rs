/// Offline integration tests for cargo-copter
///
/// These tests use local test fixtures to verify all result states
/// without requiring network access to crates.io
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

// Helper to get the test fixtures directory
fn fixtures_dir() -> PathBuf {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    Path::new(manifest_dir).join("test-crates/integration-fixtures")
}

// Helper to run cargo commands in a directory
fn run_cargo(args: &[&str], cwd: &Path) -> Output {
    Command::new("cargo")
        .args(args)
        .current_dir(cwd)
        .output()
        .unwrap_or_else(|e| panic!("Failed to run cargo {}: {}", args.join(" "), e))
}

// Helper to assert cargo command succeeded
fn assert_cargo_success(output: &Output, context: &str) {
    assert!(
        output.status.success(),
        "{} failed with status: {:?}\nstderr: {}",
        context,
        output.status.code(),
        String::from_utf8_lossy(&output.stderr)
    );
}

// Helper to assert cargo command failed
fn assert_cargo_failure(output: &Output, context: &str) {
    assert!(!output.status.success(), "{} should have failed but succeeded", context);
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
    let base_v1 = fixtures_dir().join("base-crate-v1");
    let output = run_cargo(&["check"], &base_v1);
    assert_cargo_success(&output, "base-crate-v1 check");
}

#[test]
fn test_base_crate_v2_compiles() {
    let base_v2 = fixtures_dir().join("base-crate-v2");
    let output = run_cargo(&["check"], &base_v2);
    assert_cargo_success(&output, "base-crate-v2 check");
}

#[test]
fn test_dependent_passing_with_v1() {
    let dependent = fixtures_dir().join("dependent-passing");
    let output = run_cargo(&["check"], &dependent);
    assert_cargo_success(&output, "dependent-passing check with v1");
}

#[test]
fn test_dependent_passing_tests_with_v1() {
    let dependent = fixtures_dir().join("dependent-passing");
    let output = run_cargo(&["test"], &dependent);
    assert_cargo_success(&output, "dependent-passing tests with v1");
}

#[test]
fn test_dependent_regressed_with_v1() {
    let dependent = fixtures_dir().join("dependent-regressed");
    let output = run_cargo(&["check"], &dependent);
    assert_cargo_success(&output, "dependent-regressed check with v1");
}

#[test]
fn test_dependent_broken_fails() {
    let dependent = fixtures_dir().join("dependent-broken");
    let output = run_cargo(&["check"], &dependent);
    assert_cargo_failure(&output, "dependent-broken check");
}

#[test]
fn test_dependent_test_failing_with_v1() {
    let dependent = fixtures_dir().join("dependent-test-failing");

    let check_output = run_cargo(&["check"], &dependent);
    assert_cargo_success(&check_output, "dependent-test-failing check with v1");

    let test_output = run_cargo(&["test"], &dependent);
    assert_cargo_success(&test_output, "dependent-test-failing tests with v1");
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
    use std::fs;
    use tempfile::TempDir;

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
    use std::fs;
    use tempfile::TempDir;

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
    use std::fs;
    use tempfile::TempDir;

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
    let dependent = fixtures_dir().join("dependent-passing");
    let output = run_cargo(&["metadata", "--format-version=1", "--no-deps"], &dependent);
    assert_cargo_success(&output, "cargo metadata");

    // Parse JSON to verify structure
    let stdout = String::from_utf8_lossy(&output.stdout);
    let metadata: serde_json::Value = serde_json::from_str(&stdout).expect("Should parse metadata JSON");

    // Verify expected fields exist
    assert!(metadata.get("packages").is_some());
    assert!(metadata.get("workspace_root").is_some());
}

#[test]
fn test_cargo_metadata_shows_base_crate_dependency() {
    let dependent = fixtures_dir().join("dependent-passing");
    let output = run_cargo(&["metadata", "--format-version=1", "--no-deps"], &dependent);
    assert_cargo_success(&output, "cargo metadata");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let metadata: serde_json::Value = serde_json::from_str(&stdout).unwrap();

    // Look for base-crate in dependencies
    if let Some(packages) = metadata.get("packages").and_then(|p| p.as_array()) {
        for package in packages {
            if let Some(deps) = package.get("dependencies").and_then(|d| d.as_array()) {
                let has_base_crate = deps.iter().any(|dep| {
                    dep.get("name").and_then(|n| n.as_str()).map(|name| name == "base-crate").unwrap_or(false)
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
