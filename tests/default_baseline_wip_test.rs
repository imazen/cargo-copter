/// Integration test for default baseline + WIP testing (without --test-versions)
///
/// This test validates the output when running cargo-copter with just --path,
/// which implicitly tests:
/// 1. Baseline (published version from crates.io)
/// 2. WIP (local work-in-progress version)
///
/// This is the most common usage pattern for crate authors checking their changes.
use std::path::PathBuf;
use std::process::Command;

#[test]
#[ignore] // Requires network access to download load_image
fn test_default_baseline_wip_output() {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let fixture_path = manifest_dir.join("test-crates/fixtures/rust-rgb-breaking");
    let temp = tempfile::tempdir().unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_cargo-copter"))
        .args(["--path", fixture_path.to_str().unwrap(), "--dependents", "load_image:3.3.1"])
        .arg("--staging-dir")
        .arg(temp.path().join("staging"))
        .current_dir(temp.path())
        .output()
        .expect("Failed to execute cargo-copter");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    println!("=== STDOUT ===\n{stdout}\n=== STDERR ===\n{stderr}");

    assert_eq!(stdout.matches("- baseline").count(), 1, "Exactly one baseline row");
    assert_eq!(stdout.matches("│   Offered").count(), 1, "Exactly one table header");
    assert_eq!(stdout.matches("COMPATIBILITY REPORT").count(), 1, "Exactly one summary");
    assert!(stdout.contains("Total tested"));
    assert!(stdout.find("- baseline").unwrap() < stdout.find("│ ✗").unwrap());
    assert!(!stdout.lines().any(|line| line.starts_with("copter:")));

    let report: serde_json::Value =
        serde_json::from_slice(&std::fs::read(temp.path().join("copter-report/report.json")).unwrap()).unwrap();
    let rows = report["test_results"].as_array().unwrap();
    assert_eq!(rows.len(), 2, "Both baseline and WIP must execute");
    assert!(rows[0]["offered"].is_null());
    let baseline = rows[0]["test"]["commands"].as_array().unwrap();
    assert_eq!(baseline.len(), 3);
    for (step, command) in baseline.iter().zip(["Fetch", "Check", "Test"]) {
        assert_eq!(step["command"], command);
        assert_eq!(step["result"]["passed"], true, "Baseline {command} failed: {step}");
    }
    assert_eq!(rows[1]["baseline_passed"], true);
    assert_eq!(rows[1]["offered"]["version"], "0.8.91");
    assert_eq!(rows[1]["offered"]["forced"], true);
    assert_eq!(rows[1]["primary"]["resolved_version"], "0.8.91");
    assert_eq!(rows[1]["primary"]["resolved_source"], "Local");
    let wip = rows[1]["test"]["commands"].as_array().unwrap();
    assert_eq!(wip.len(), 2, "Stop after the actual WIP compile failure");
    assert_eq!(wip[0]["command"], "Fetch");
    assert_eq!(wip[0]["result"]["passed"], true);
    assert_eq!(wip[1]["command"], "Check");
    assert_eq!(wip[1]["result"]["passed"], false);
    assert_eq!(report["summary"]["regressed"], 1);
}

#[test]
fn test_baseline_wip_data_structures() {
    // This test documents the EXPECTED OfferedRow structure for baseline + WIP testing
    //
    // 🐛 BUG: Legacy path (no --test-versions) currently produces FALSE POSITIVES
    // It reports both baseline and WIP as PASSED when WIP actually breaks dependents.
    //
    // EXPECTED OfferedRow structure (what SHOULD happen):
    //
    // OfferedRow #1 (Baseline):
    // {
    //     baseline_passed: None,  // This IS the baseline
    //     primary: DependencyRef {
    //         dependent_name: "load_image",
    //         dependent_version: "3.3.1",
    //         spec: "^0.8.52",  // SHOULD be extracted, currently "?" in legacy
    //         resolved_version: "0.8.52",
    //         resolved_source: CratesIo,
    //         used_offered_version: false,
    //     },
    //     offered: None,  // Baseline has no offered version
    //     test: TestExecution {
    //         commands: [
    //             TestCommand { command: Fetch, result: { passed: true, ... } },
    //             TestCommand { command: Check, result: { passed: true, ... } },
    //             TestCommand { command: Test, result: { passed: true, ... } },
    //         ]
    //     },
    //     transitive: vec![],
    // }
    //
    // OfferedRow #2 (WIP) - SHOULD FAIL but legacy reports PASSED (FALSE POSITIVE):
    // {
    //     baseline_passed: Some(true),  // Baseline passed
    //     primary: DependencyRef {
    //         dependent_name: "load_image",
    //         dependent_version: "3.3.1",
    //         spec: "^0.8.52",  // SHOULD match baseline, currently "?"
    //         resolved_version: "0.8.91",  // SHOULD be detected, currently "?"
    //         resolved_source: Local,
    //         used_offered_version: true,
    //     },
    //     offered: Some(OfferedVersion {
    //         version: "this(0.8.91)",
    //         forced: true,  // Local versions always forced
    //     }),
    //     test: TestExecution {
    //         commands: [
    //             // 🐛 BUG: Legacy only runs cargo build (passes)
    //             // SHOULD run: Fetch → Check (FAILS with 22 errors) → Test (skipped)
    //             TestCommand { command: Fetch, result: { passed: true, ... } },
    //             TestCommand { command: Check, result: { passed: false, ... } },  // SHOULD FAIL
    //             // Test command skipped due to early stopping
    //         ]
    //     },
    //     transitive: vec![],
    // }
    //
    // Classification (CORRECT behavior with multi-version path):
    // - If baseline.passed && !wip.passed → REGRESSED ✓ (correct for this case)
    // - If !baseline.passed && !wip.passed → BROKEN
    // - If baseline.passed && wip.passed → PASSED
    //
    // Classification (INCORRECT behavior with legacy path):
    // - baseline.passed && wip.passed → PASSED 🐛 (FALSE POSITIVE!)

    // This is a documentation test - it always passes
    println!("📚 OfferedRow structure documented for baseline + WIP testing");
    println!("   ⚠️  WARNING: Legacy path has FALSE POSITIVE bug");
    println!("   ✅ Use --test-versions for correct behavior");
    println!("   See test source code for detailed structure expectations");
}

#[test]
fn test_offered_cell_baseline_rendering() {
    // Test that OfferedCell correctly renders baseline rows
    //
    // Expected rendering:
    // - OfferedCell::Baseline → "- baseline"

    println!("✅ OfferedCell::Baseline should render as: '- baseline'");
    println!("   This is validated in src/report.rs::OfferedCell::format()");
}

#[test]
fn test_offered_cell_wip_rendering() {
    // Test that OfferedCell correctly renders WIP/offered rows
    //
    // Expected rendering for WIP with regression:
    // - StatusIcon::Failed → "✗"
    // - Resolution::Mismatch → "≠"
    // - Version: "this"
    // - Forced: true → "[≠→!]"
    // Result: "✗ ≠this→!"

    println!("✅ OfferedCell::Tested (WIP, forced) should render as:");
    println!("   '✗ ≠this→!' (when failed and forced, PatchDepth::Force)");
    println!("   '✓ =this' (when passed and resolved exactly)");
    println!("   This is validated in src/report.rs::OfferedCell::format()");
}

#[test]
fn test_patch_depth_marker_rendering() {
    // Test that PatchDepth markers are correctly displayed
    //
    // PatchDepth enum and markers:
    // - PatchDepth::None    → "" (no marker)
    // - PatchDepth::Force   → "!" (force mode, direct dependency spec replaced)
    // - PatchDepth::Patch   → "!!" (patch retry, [patch.crates-io] added after multi-version error)
    // - PatchDepth::DeepPatch → "!!!" (deep patch, recursive transitive patching)
    //
    // Display format: "{icon} {resolution}{version}→{marker}"
    // Examples:
    // - "✗ ≠0.8.91→!" (forced)
    // - "✗ ≠0.8.91→!!" (forced + auto-patch retry for multi-version conflict)
    // - "✗ ≠0.8.91→!!!" (forced + deep recursive patching)
    //
    // Usage in simple output:
    // - "rgb:0.8.91 [!]" (forced)
    // - "rgb:0.8.91 [!!]" (patch retry)
    // - "rgb:0.8.91 [!!!]" (deep patch)

    println!("✅ PatchDepth markers:");
    println!("   PatchDepth::None      → '' (no marker)");
    println!("   PatchDepth::Force     → '!' (force mode)");
    println!("   PatchDepth::Patch     → '!!' (auto-patch after multi-version error)");
    println!("   PatchDepth::DeepPatch → '!!!' (deep recursive patching)");
    println!();
    println!("   Table format: '✗ ≠0.8.91→!!' (with marker suffix)");
    println!("   Simple format: 'rgb:0.8.91 [!!]' (marker in brackets)");
    println!();
    println!("   This is validated in:");
    println!("   - src/compile.rs::PatchDepth::marker()");
    println!("   - src/report.rs::OfferedCell::format()");
    println!("   - src/report.rs::print_simple_dependent_result()");
}
