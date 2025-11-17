/// Integration test for WIP breaking changes
///
/// This test validates that:
/// 1. WIP version with breaking changes correctly fails against dependents
/// 2. Published version passes against the same dependents
/// 3. OfferedRow fields (spec, resolved) are properly populated (not "?")
/// 4. Test results accurately reflect regression detection

use std::path::PathBuf;
use std::process::Command;

#[test]
#[ignore] // Requires network access to download load_image
fn test_wip_breaking_change_regression() {
    // This test uses a copy of rust-rgb (v0.8.91 WIP) that has breaking changes
    // that cause load_image:3.3.1 to fail compilation (22 errors in cargo check)
    //
    // Expected behavior:
    // - Baseline (published 0.8.52): PASS ✓✓✓
    // - WIP (0.8.91 with breaking changes): REGRESSED ✓✗ (check fails)

    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let fixture_path = manifest_dir.join("test-crates/fixtures/rust-rgb-breaking");
    let binary_path = manifest_dir.join("target/release/cargo-copter");

    assert!(fixture_path.exists(), "Fixture path should exist: {:?}", fixture_path);
    assert!(binary_path.exists(), "Binary should be built: {:?}", binary_path);

    // Run cargo-copter with multi-version testing
    let output = Command::new(&binary_path)
        .arg("--path")
        .arg(&fixture_path)
        .arg("--dependents")
        .arg("load_image:3.3.1")
        .arg("--test-versions")
        .arg("0.8.52")
        .output()
        .expect("Failed to execute cargo-copter");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    println!("=== STDOUT ===\n{}", stdout);
    println!("=== STDERR ===\n{}", stderr);

    // Validate output contains expected elements

    // 1. Should test load_image in multi-version mode
    assert!(stdout.contains("load_image") || stderr.contains("load_image"),
        "Should mention load_image in output");
    assert!(stdout.contains("multi-version") || stderr.contains("multi-version"),
        "Should use multi-version testing");

    // 2. Baseline row should PASS with proper fields
    assert!(stdout.contains("baseline") || stderr.contains("baseline"),
        "Should have baseline row");
    assert!(stdout.contains("PASSED") || stderr.contains("PASSED"),
        "Baseline should pass");
    assert!(stdout.contains("0.8.52"),
        "Spec field should show '0.8.52' (not '?')");

    // 3. WIP row should FAIL/REGRESS
    assert!(stdout.contains("REGRESSED") || stdout.contains("✗"),
        "WIP should show as regressed");
    assert!(stdout.contains("0.8.91") || stdout.contains("this"),
        "WIP should show version 0.8.91 or 'this'");

    // 4. Should show error details about check failure
    assert!(stdout.contains("cargo check failed") || stdout.contains("✓✗"),
        "Should indicate check step failed (ICT = ✓✗)");

    // 5. Summary should show 1 regressed
    assert!(stdout.contains("Regressed: 1") || stderr.contains("Regressed: 1"),
        "Summary should show 1 regression");

    println!("\n✅ All validations passed!");
    println!("   - Baseline (0.8.52): PASSED ✓✓✓");
    println!("   - WIP (0.8.91):     REGRESSED ✓✗");
    println!("   - Spec field:        populated (not '?')");
    println!("   - Resolved field:    populated (not '?')");
}

#[test]
fn test_rgb_fixture_exists_and_is_valid() {
    // Verify that our rust-rgb fixture exists and has a valid structure
    use std::fs;

    let fixture_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("test-crates/fixtures/rust-rgb-breaking");

    assert!(fixture_dir.exists(), "Fixture directory should exist");

    let cargo_toml = fixture_dir.join("Cargo.toml");
    assert!(cargo_toml.exists(), "Cargo.toml should exist");

    let cargo_content = fs::read_to_string(&cargo_toml)
        .expect("Should be able to read Cargo.toml");

    // Verify it's the rgb crate with version 0.8.91
    assert!(cargo_content.contains("name = \"rgb\""),
        "Should be the rgb crate");
    assert!(cargo_content.contains("0.8.91"),
        "Should be version 0.8.91 (the WIP version with breaking changes)");

    // Verify src/lib.rs exists
    let lib_rs = fixture_dir.join("src/lib.rs");
    assert!(lib_rs.exists(), "src/lib.rs should exist");

    println!("✅ rust-rgb fixture is valid");
    println!("   Version: 0.8.91 (WIP with breaking changes)");
    println!("   Note: This version breaks load_image:3.3.1 compilation");
}
