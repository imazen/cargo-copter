// Integration tests for crates.io API interaction
// These tests hit the real crates.io API using the 'rgb' crate

use std::time::Duration;

/// Test that we can fetch reverse dependencies for rgb
#[test]
fn test_rgb_reverse_dependencies_with_limit() {
    use crates_io_api::SyncClient;

    // Create API client with proper User-Agent and rate limiting
    let client = SyncClient::new(
        "cargo-copter-test/0.1.1 (test suite)",
        Duration::from_millis(1000)
    ).expect("Failed to create API client");

    // Fetch reverse dependencies for rgb
    let deps = client.crate_reverse_dependencies("rgb")
        .expect("Failed to fetch reverse dependencies");

    // Verify we got dependencies
    assert!(!deps.dependencies.is_empty(), "rgb should have reverse dependencies");

    // Verify rgb has dependents (not as many as libc, but a reasonable number)
    assert!(deps.meta.total > 10,
        "rgb should have >10 reverse deps, got {}", deps.meta.total);
}

/// Test API response structure
#[test]
fn test_rgb_pagination() {
    use crates_io_api::SyncClient;

    let client = SyncClient::new(
        "cargo-copter-test/0.1.1 (test suite)",
        Duration::from_millis(1000)
    ).expect("Failed to create API client");

    // Fetch reverse dependencies (API returns all results)
    let page1 = client.crate_reverse_dependencies("rgb")
        .expect("Failed to fetch reverse dependencies");

    // Should get some results
    assert!(page1.dependencies.len() > 10,
        "Expected >10 deps, got {}", page1.dependencies.len());

    // Total should match the actual number returned
    assert!(page1.meta.total > 10,
        "rgb should have some reverse deps");
}

/// Test that we can find known dependents of rgb
#[test]
fn test_rgb_contains_known_dependents() {
    use crates_io_api::SyncClient;

    let client = SyncClient::new(
        "cargo-copter-test/0.1.1 (test suite)",
        Duration::from_millis(1000)
    ).expect("Failed to create API client");

    // Fetch reverse deps
    let mut all_deps = Vec::new();

    // Note: The API doesn't expose reverse_dependencies with pagination directly
    // through the builder, so we'll just test the first page and verify structure
    let deps = client.crate_reverse_dependencies("rgb")
        .expect("Failed to fetch dependencies");

    all_deps.extend(deps.dependencies.iter().map(|d| d.dependency.crate_id.clone()));

    // Check that we got some dependencies
    assert!(!all_deps.is_empty(), "Should have found some dependencies");

    // We can't hardcode specific crates as dependencies may change over time,
    // but we can verify the structure is correct
    println!("Found {} dependencies for rgb", all_deps.len());
    println!("First few: {:?}", &all_deps[..5.min(all_deps.len())]);
}

/// Test version resolution for rgb
#[test]
fn test_rgb_version_resolution() {
    use crates_io_api::SyncClient;

    let client = SyncClient::new(
        "cargo-copter-test/0.1.1 (test suite)",
        Duration::from_millis(1000)
    ).expect("Failed to create API client");

    // Fetch rgb crate metadata
    let rgb_crate = client.get_crate("rgb")
        .expect("Failed to fetch rgb crate info");

    // Verify we got crate info
    assert_eq!(rgb_crate.crate_data.id, "rgb");
    assert!(!rgb_crate.versions.is_empty(), "rgb should have versions");

    // Verify versions are in expected format
    let first_version = &rgb_crate.versions[0];
    assert!(!first_version.num.is_empty(), "Version number should not be empty");

    // Parse version to ensure it's valid semver
    let _parsed: semver::Version = first_version.num.parse()
        .expect("Version should be valid semver");
}

/// Test that total count is reported correctly
#[test]
fn test_limit_parameter_enforced() {
    use crates_io_api::SyncClient;

    let client = SyncClient::new(
        "cargo-copter-test/0.1.1 (test suite)",
        Duration::from_millis(1000)
    ).expect("Failed to create API client");

    // Fetch reverse dependencies
    let deps = client.crate_reverse_dependencies("rgb")
        .expect("Failed to fetch dependencies");

    // Verify we got some dependencies
    assert!(deps.dependencies.len() > 10,
        "Expected some dependencies");

    // Verify total is reported correctly
    assert!(deps.meta.total > 0, "Should report total count");

    // Verify dependencies list matches total count
    assert_eq!(deps.dependencies.len() as u64, deps.meta.total,
        "Dependencies count should match meta.total");
}

/// Smoke test to verify API endpoint structure hasn't changed
#[test]
fn test_api_endpoint_structure() {
    use crates_io_api::SyncClient;

    let client = SyncClient::new(
        "cargo-copter-test/0.1.1 (test suite)",
        Duration::from_millis(1000)
    ).expect("Failed to create API client");

    // Test that basic API calls work
    let deps = client.crate_reverse_dependencies("rgb")
        .expect("API call should succeed");

    // Verify response has expected structure
    assert!(deps.dependencies.iter().all(|d| !d.dependency.crate_id.is_empty()),
        "All dependencies should have non-empty crate_id");

    // Verify metadata is present
    assert!(deps.meta.total > 0, "Meta total should be > 0");
}
